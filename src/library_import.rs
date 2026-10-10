//! Reader for a binary manufacturer device library.
//!
//! Layout as established from the sample file (little-endian throughout):
//!
//! * The file is a sequence of 64-byte blocks after a 960-byte header.
//! * A record begins in a block whose first u32 equals that block's number.
//!   The block header is `u32 block, u32 continuation, u32 length, u16 tag`,
//!   followed by `length` data bytes. Data that does not fit in the first
//!   block continues in consecutive blocks starting at `continuation` (or at
//!   the next block when `continuation` is 0).
//! * Object data starts `u32 object id, u16 0, u16 class code, u16 version,
//!   u16 0`, then a table of u32 offsets (relative to byte 12) that ends with
//!   an entry equal to the data length minus 12. The bytes between the table
//!   and the first offset are the object's fixed-size fields; each pair of
//!   offsets delimits one variable field.
//! * Variable fields are text (`u16 length, u16 flags, bytes, 0x00, 0xFF`),
//!   arrays (`u32 8, u32 byte length, u32 count, items` with f32 or text
//!   items, possibly several in a row), or reference lists (`u32 count`,
//!   then 14-byte entries `u32 block, u32 object id, u32 0, u16 class`).
//!
//! Only the device structure is interpreted (see `library` for field
//! meanings and their basis). Everything else is reported, not guessed.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::library::*;

const BLOCK: usize = 64;
const HEADER_BLOCKS: usize = 15;
const RECORD_HEADER: usize = 14;

const CLASS_DEVICE: u16 = 126;
const CLASS_RATING: u16 = 122;
const CLASS_FRAME: u16 = 124;
const CLASS_SENSOR: u16 = 123;
const CLASS_TRIP_UNIT: u16 = 125;
const CLASS_FUNCTION: u16 = 121;
const CLASS_POINT_CURVE: u16 = 110;
const CLASS_POINT_BANDS: u16 = 113;
const CLASS_DIAL_FAMILY: u16 = 109;

/// Descriptive family for each element kind code, inferred from element
/// names and data in the sample. Unknown codes are reported by number.
pub fn kind_name(code: u32) -> String {
    match code {
        0 => "pickup_settings",
        1 => "pickup_adjustable",
        2 => "delay_settings",
        3 => "delay_band",
        4 => "relay_equation_curve",
        5 => "relay_curve_set",
        6 => "time_dial_curve_family",
        7 => "point_curve",
        8 => "point_curve_bands",
        9 => "instantaneous_settings",
        10 => "instantaneous_or_fixed",
        11 => "definite_time",
        12 => "labeled_delay_band",
        13 => "fixed_element",
        14 => "relay_element",
        15 => "instantaneous_adjustable",
        16 => "short_time_delay_band",
        18 => "inverse_equation_curve",
        20 => "motor_overload",
        other => return format!("kind_{other}"),
    }
    .to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Text(String),
    Arrays(Vec<Array>),
    Refs(Vec<Ref>),
    Raw(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Array {
    Numbers(Vec<f64>),
    Text(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ref {
    pub object: u32,
    pub class: u16,
}

#[derive(Clone, Debug)]
pub struct Object {
    pub id: u32,
    pub class: u16,
    pub fixed: Vec<u8>,
    pub fields: Vec<Field>,
}

fn u16_at(b: &[u8], p: usize) -> Option<u16> { b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]])) }
fn u32_at(b: &[u8], p: usize) -> Option<u32> { b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) }
fn f32_at(b: &[u8], p: usize) -> Option<f64> { u32_at(b, p).map(|v| f32::from_bits(v) as f64) }

/// Shortest decimal that round-trips the stored f32, so 0.1f32 is 0.1, not 0.10000000149.
fn clean(value: f64) -> f64 {
    let text = format!("{}", value as f32);
    text.parse().unwrap_or(value)
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// Reassemble every record. Returns object data keyed by object id.
pub fn records(bytes: &[u8]) -> Result<(HashMap<u32, Vec<u8>>, Vec<String>), String> {
    if bytes.len() < (HEADER_BLOCKS + 1) * BLOCK || bytes.len() % BLOCK != 0 {
        return Err(format!("{} bytes is not a whole number of {BLOCK}-byte blocks after the header", bytes.len()));
    }
    let blocks = bytes.len() / BLOCK - HEADER_BLOCKS;
    let at = |b: usize| (b + HEADER_BLOCKS) * BLOCK;
    let mut objects = HashMap::new();
    let mut warnings = Vec::new();
    let mut truncated = 0usize;
    for b in 1..blocks {
        let o = at(b);
        if u32_at(bytes, o) != Some(b as u32) { continue; }
        let next = u32_at(bytes, o + 4).unwrap() as usize;
        let len = u32_at(bytes, o + 8).unwrap() as usize;
        let total = RECORD_HEADER + len;
        let mut data = Vec::with_capacity(total);
        data.extend_from_slice(&bytes[o..o + BLOCK]);
        if total > BLOCK {
            let start = if next == 0 { b + 1 } else { next };
            let need = (total - BLOCK).div_ceil(BLOCK);
            if start + need > blocks { truncated += 1; continue; }
            data.extend_from_slice(&bytes[at(start)..at(start) + need * BLOCK]);
        }
        data.truncate(total);
        let data = data.split_off(RECORD_HEADER);
        // Object records carry a zero u16 after the object id; index pages do not.
        if data.len() >= 12 && data[4] == 0 && data[5] == 0 {
            let id = u32_at(&data, 0).unwrap();
            objects.entry(id).or_insert(data);
        }
    }
    if truncated > 0 { warnings.push(format!("{truncated} record(s) ran past the end of the file and were skipped")); }
    Ok((objects, warnings))
}

fn text_at(b: &[u8], p: usize) -> Option<(String, usize)> {
    let len = u16_at(b, p)? as usize;
    let flags = b.get(p + 2..p + 4)?;
    if len < 4 || !(flags == [1, 1] || flags == [0, 0]) { return None; }
    let end = p + 2 + len;
    if b.get(end - 2..end)? != [0, 0xff] { return None; }
    Some((b[p + 4..end - 2].iter().map(|&c| c as char).collect(), end))
}

fn parse_arrays(v: &[u8]) -> Option<Vec<Array>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < v.len() {
        let (h, total, count) = (u32_at(v, p)?, u32_at(v, p + 4)? as usize, u32_at(v, p + 8)? as usize);
        if h != 8 || total < 12 || p + total > v.len() { return None; }
        let body = &v[p + 12..p + total];
        let texts = (count > 0 && body.len() != 4 * count).then(|| {
            let mut q = 0;
            let mut items = Vec::new();
            for _ in 0..count {
                let (s, end) = text_at(body, q)?;
                items.push(s);
                q = end;
            }
            (q == body.len()).then_some(items)
        }).flatten();
        match texts {
            Some(items) => out.push(Array::Text(items)),
            None if body.len() == 4 * count => out.push(Array::Numbers((0..count).map(|i| clean(f32_at(body, 4 * i).unwrap())).collect())),
            None => return None,
        }
        p += total;
    }
    (!out.is_empty()).then_some(out)
}

pub fn parse_field(v: &[u8]) -> Field {
    if let Some((s, end)) = text_at(v, 0) {
        if end == v.len() { return Field::Text(s); }
    }
    if let Some(arrays) = parse_arrays(v) { return Field::Arrays(arrays); }
    if let Some(n) = u32_at(v, 0) {
        let n = n as usize;
        if v.len() == 4 + 14 * n {
            return Field::Refs((0..n).map(|i| Ref { object: u32_at(v, 8 + 14 * i).unwrap(), class: u16_at(v, 16 + 14 * i).unwrap() }).collect());
        }
    }
    Field::Raw(v.to_vec())
}

pub fn parse_object(d: &[u8]) -> Option<Object> {
    let id = u32_at(d, 0)?;
    let class = u16_at(d, 6)?;
    let end = d.len().checked_sub(12)?;
    let mut offsets = Vec::new();
    let mut p = 12;
    loop {
        let v = u32_at(d, p)? as usize;
        p += 4;
        if v > end || offsets.last().is_some_and(|&last| v < last) { return None; }
        offsets.push(v);
        if v == end { break; }
    }
    let first = offsets[0] + 12;
    if first < p { return None; }
    let fixed = d[p..first].to_vec();
    let fields = offsets.windows(2).map(|w| parse_field(&d[12 + w[0]..12 + w[1]])).collect();
    Some(Object { id, class, fixed, fields })
}

struct Reader<'a> {
    raw: &'a HashMap<u32, Vec<u8>>,
    used: HashSet<u32>,
    warnings: Vec<String>,
}

impl Reader<'_> {
    fn get(&mut self, id: u32) -> Option<Object> {
        let object = self.raw.get(&id).and_then(|d| parse_object(d));
        if object.is_some() { self.used.insert(id); }
        object
    }

    fn children(&mut self, field: Option<&Field>, class: u16, owner: u32) -> Vec<Object> {
        let Some(Field::Refs(refs)) = field else { return Vec::new() };
        let mut out = Vec::new();
        for r in refs {
            match self.get(r.object) {
                Some(o) if o.class == class => out.push(o),
                Some(o) => self.warnings.push(format!("object {owner}: child {} has class {} (expected {class})", r.object, o.class)),
                None => self.warnings.push(format!("object {owner}: child {} is missing or unreadable", r.object)),
            }
        }
        out
    }

    fn raw_record(&mut self, o: &Object, depth: usize) -> RawRecord {
        let mut rec = RawRecord { class_code: o.class, fixed_hex: hex(&o.fixed), ..Default::default() };
        for field in &o.fields {
            match field {
                Field::Text(s) => rec.strings.push(s.clone()),
                Field::Arrays(a) => for arr in a {
                    match arr { Array::Numbers(n) => rec.arrays.push(n.clone()), Array::Text(t) => rec.strings.extend(t.iter().cloned()) }
                },
                Field::Refs(refs) if depth < 3 => for r in refs {
                    if let Some(child) = self.get(r.object) { rec.children.push(self.raw_record(&child, depth + 1)); }
                },
                Field::Refs(_) => {}
                Field::Raw(b) => rec.unparsed_hex.push(hex(b)),
            }
        }
        rec
    }

    fn function_data(&mut self, o: &Object) -> FunctionData {
        let mut data = FunctionData {
            class_code: o.class,
            header_codes: [u32_at(&o.fixed, 0).unwrap_or(0), u32_at(&o.fixed, 4).unwrap_or(0)],
            fixed_hex: hex(&o.fixed),
            ..Default::default()
        };
        let mut fields: Vec<Option<&Field>> = o.fields.iter().map(Some).collect();
        let single = |f: Option<&Field>| match f { Some(Field::Arrays(a)) if a.len() == 1 => Some(a[0].clone()), _ => None };
        let numbers = |f: Option<&Field>| match f {
            Some(Field::Arrays(a)) => a.iter().map(|x| match x { Array::Numbers(n) => Some(n.clone()), _ => None }).collect::<Option<Vec<_>>>(),
            _ => None,
        };
        // Point curves: field 0 holds current arrays, field 1 the matching time arrays.
        if matches!(o.class, CLASS_POINT_CURVE | CLASS_POINT_BANDS) {
            if let (Some(currents), Some(times)) = (numbers(fields[0]), numbers(fields.get(1).copied().flatten())) {
                if currents.len() == times.len() && currents.iter().zip(&times).all(|(c, t)| c.len() == t.len()) {
                    data.curves = currents.into_iter().zip(times).map(|(current, time_s)| PointCurve { current, time_s }).collect();
                    data.current_unit = Some(if data.header_codes[1] == 10 { "A" } else { "multiple" }.into());
                    let (plus, minus) = (f32_at(&o.fixed, 8).unwrap_or(0.0), f32_at(&o.fixed, 12).unwrap_or(0.0));
                    if o.class == CLASS_POINT_CURVE { data.current_tolerance_pct = Some([clean(plus), clean(minus)]); }
                    fields[0] = None;
                    fields[1] = None;
                }
            }
        }
        // Settings: text labels followed by the same number of values.
        if fields.len() >= 2 {
            if let (Some(Array::Text(labels)), Some(Array::Numbers(values))) = (single(fields[0]), single(fields[1])) {
                if labels.len() == values.len() {
                    data.settings = labels.into_iter().zip(values).map(|(label, value)| Setting { label, value }).collect();
                    fields[0] = None;
                    fields[1] = None;
                    // Dial families: shared current multiples, then one time array per dial.
                    if o.class == CLASS_DIAL_FAMILY {
                        if let (Some(Array::Numbers(multiples)), Some(curves)) = (single(fields.get(2).copied().flatten()), numbers(fields.get(3).copied().flatten())) {
                            let used: Vec<_> = curves.iter().take(data.settings.len()).collect();
                            if used.len() == data.settings.len() && used.iter().all(|c| c.len() == multiples.len()) && curves[used.len()..].iter().all(|c| c.is_empty()) {
                                data.dial_curves = data.settings.iter().zip(used).map(|(s, t)| DialCurve { dial: s.value, time_s: t.clone() }).collect();
                                data.dial_current_multiples = multiples;
                                fields[2] = None;
                                fields[3] = None;
                            }
                        }
                    }
                }
            }
        }
        for field in fields.into_iter().flatten() {
            match field {
                Field::Text(s) if !s.is_empty() => data.strings.push(s.clone()),
                Field::Text(_) => {}
                Field::Arrays(a) => for arr in a {
                    match arr {
                        Array::Numbers(n) if !n.is_empty() => data.arrays.push(n.clone()),
                        Array::Numbers(_) => {}
                        Array::Text(t) => data.strings.extend(t.iter().cloned()),
                    }
                },
                Field::Refs(refs) => for r in refs {
                    if let Some(child) = self.get(r.object) {
                        let rec = self.raw_record(&child, 1);
                        data.strings.extend(rec.strings);
                        data.arrays.extend(rec.arrays);
                    }
                },
                Field::Raw(b) => data.unparsed_hex.push(hex(b)),
            }
        }
        data
    }

    fn function(&mut self, o: &Object) -> Function {
        let kind_code = u32_at(&o.fixed, 0).unwrap_or(u32::MAX);
        let flag = u32_at(&o.fixed, 4).unwrap_or(0);
        let target = u32_at(&o.fixed, 12);
        let name = match o.fields.first() { Some(Field::Text(s)) => s.clone(), _ => String::new() };
        let data = match target.and_then(|id| self.get(id)) {
            Some(element) => self.function_data(&element),
            None => FunctionData { error: Some(format!("element record {target:?} is missing or unreadable")), fixed_hex: hex(&o.fixed), ..Default::default() },
        };
        Function { name, kind_code, kind: kind_name(kind_code), flag, data }
    }

    fn device(&mut self, o: &Object) -> Result<LibraryDevice, String> {
        if o.fields.len() < 13 { return Err(format!("device {} has {} fields (expected at least 13)", o.id, o.fields.len())); }
        let text = |i: usize| match o.fields.get(i) { Some(Field::Text(s)) => s.clone(), _ => String::new() };
        let mut device = LibraryDevice {
            id: o.id,
            manufacturer: text(1),
            device_type: text(2),
            style: text(3),
            catalog: text(4),
            reference: text(5),
            notes: text(6),
            created: text(7),
            modified: text(8),
            category: text(9),
            header_voltage_v: f32_at(&o.fixed, 4).map(clean),
            ..Default::default()
        };
        for r in self.children(o.fields.get(10), CLASS_RATING, o.id) {
            let v: Vec<f64> = (0..12).map(|i| f32_at(&r.fixed, 4 * i).map(clean).unwrap_or(f64::NAN)).collect();
            let label = match r.fields.first() { Some(Field::Text(s)) => s.clone(), _ => String::new() };
            device.ratings.push(Rating {
                label, voltage_v: v[0], rating_a: v[1], interrupting_ka: v[2], interrupting_asym_ka: v[5], test_xr: v[9],
                unlabeled: [3, 4, 6, 7, 8, 10, 11].iter().map(|&i| v[i]).collect(),
            });
        }
        for f in self.children(o.fields.get(11), CLASS_FRAME, o.id) {
            let sensors = self.children(f.fields.first(), CLASS_SENSOR, f.id);
            device.frames.push(Frame {
                rating_a: f32_at(&f.fixed, 0).map(clean).unwrap_or(f64::NAN),
                sensors_a: sensors.iter().filter_map(|s| f32_at(&s.fixed, 0).map(clean)).collect(),
            });
        }
        for t in self.children(o.fields.get(12), CLASS_TRIP_UNIT, o.id) {
            let functions = self.children(t.fields.first(), CLASS_FUNCTION, t.id).iter().map(|f| self.function(f)).collect();
            let arrays = match t.fields.get(1) {
                Some(Field::Arrays(a)) => a.iter().filter_map(|x| match x { Array::Numbers(n) if !n.is_empty() => Some(n.clone()), _ => None }).collect(),
                _ => Vec::new(),
            };
            device.trip_units.push(TripUnit { rating_a: f32_at(&t.fixed, 0).map(clean).unwrap_or(f64::NAN), functions, arrays });
        }
        for i in 13..o.fields.len() {
            match &o.fields[i] {
                Field::Refs(refs) => for r in refs.clone() {
                    if let Some(child) = self.get(r.object) {
                        let rec = self.raw_record(&child, 1);
                        if !(rec.strings.is_empty() && rec.arrays.is_empty() && rec.children.is_empty() && rec.unparsed_hex.is_empty()) || child.class != 329 {
                            device.other_records.push(rec);
                        }
                    }
                },
                Field::Text(s) if !s.is_empty() => device.other_records.push(RawRecord { class_code: o.class, strings: vec![s.clone()], ..Default::default() }),
                Field::Arrays(a) => {
                    let arrays: Vec<Vec<f64>> = a.iter().filter_map(|x| match x { Array::Numbers(n) if !n.is_empty() => Some(n.clone()), _ => None }).collect();
                    if !arrays.is_empty() { device.other_records.push(RawRecord { class_code: o.class, arrays, ..Default::default() }); }
                }
                _ => {}
            }
        }
        Ok(device)
    }
}

/// Convert a binary manufacturer device library into a Flashmob library.
pub fn convert(bytes: &[u8]) -> Result<Library, String> {
    let (raw, mut warnings) = records(bytes)?;
    let mut ids: Vec<u32> = raw.iter().filter(|(_, d)| u16_at(d, 6) == Some(CLASS_DEVICE)).map(|(&id, _)| id).collect();
    ids.sort_unstable();
    if ids.is_empty() { return Err("no device records found".into()); }
    let mut reader = Reader { raw: &raw, used: HashSet::new(), warnings: Vec::new() };
    let mut devices = Vec::new();
    for id in ids {
        match reader.get(id) {
            Some(o) => match reader.device(&o) {
                Ok(d) => devices.push(d),
                Err(err) => reader.warnings.push(err),
            },
            None => reader.warnings.push(format!("device {id} could not be parsed")),
        }
    }
    let mut summary = LibrarySummary { devices: devices.len(), ..Default::default() };
    for d in &devices {
        *summary.devices_by_category.entry(d.category.clone()).or_default() += 1;
        for f in d.trip_units.iter().flat_map(|t| &t.functions) {
            *summary.functions_by_kind.entry(f.kind.clone()).or_default() += 1;
        }
    }
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    for (id, d) in &raw {
        if !reader.used.contains(id) {
            *skipped.entry(format!("class_{}", u16_at(d, 6).unwrap_or(0))).or_default() += 1;
        }
    }
    summary.skipped_records_by_class = skipped;
    let shown = reader.warnings.len().min(200);
    warnings.extend(reader.warnings.drain(..shown));
    if !reader.warnings.is_empty() { warnings.push(format!("{} more warning(s) not shown", reader.warnings.len())); }
    summary.warnings = warnings;
    Ok(Library {
        format: FORMAT.into(),
        version: VERSION,
        source: LibrarySource { description: "binary manufacturer device library".into(), bytes: bytes.len() as u64 },
        summary,
        devices,
    })
}

/// Replace every case-insensitive occurrence of each term with `[redacted]`.
/// Returns true when anything changed.
pub fn redact_text(text: &mut String, terms: &[String]) -> bool {
    let mut changed = false;
    for term in terms.iter().filter(|t| !t.is_empty()) {
        let needle = term.to_ascii_lowercase();
        loop {
            let hay = text.to_ascii_lowercase();
            let Some(start) = hay.find(&needle) else { break };
            text.replace_range(start..start + needle.len(), "[redacted]");
            changed = true;
        }
    }
    changed
}

fn contains_term(text: &str, terms: &[String]) -> bool {
    let hay = text.to_ascii_lowercase();
    terms.iter().filter(|t| !t.is_empty()).any(|t| hay.contains(&t.to_ascii_lowercase()))
}

fn redact_record(record: &mut RawRecord, terms: &[String], count: &mut usize) {
    for s in &mut record.strings { if redact_text(s, terms) { *count += 1; } }
    for child in &mut record.children { redact_record(child, terms, count); }
}

/// Remove caller-supplied terms from a converted library. Free text has each
/// term replaced by `[redacted]`. A device whose identifying text contains a
/// term is withheld instead, because rewriting a manufacturer's product or
/// catalog name would change the data; withheld ids are listed in the summary.
pub fn redact(library: &mut Library, terms: &[String]) {
    if terms.iter().all(|t| t.is_empty()) { return; }
    let mut count = 0;
    let mut withheld = Vec::new();
    library.devices.retain(|d| {
        let identity = [&d.manufacturer, &d.category, &d.device_type, &d.style, &d.catalog, &d.reference];
        let keep = !identity.iter().any(|s| contains_term(s, terms)) && !d.ratings.iter().any(|r| contains_term(&r.label, terms));
        if !keep { withheld.push(d.id); }
        keep
    });
    for d in &mut library.devices {
        if redact_text(&mut d.notes, terms) { count += 1; }
        for unit in &mut d.trip_units {
            for f in &mut unit.functions {
                if redact_text(&mut f.name, terms) { count += 1; }
                for s in &mut f.data.strings { if redact_text(s, terms) { count += 1; } }
                for s in &mut f.data.settings { if redact_text(&mut s.label, terms) { count += 1; } }
            }
        }
        for r in &mut d.other_records { redact_record(r, terms, &mut count); }
    }
    let summary = &mut library.summary;
    summary.devices = library.devices.len();
    summary.devices_by_category.clear();
    summary.functions_by_kind.clear();
    for d in &library.devices {
        *summary.devices_by_category.entry(d.category.clone()).or_default() += 1;
        for f in d.trip_units.iter().flat_map(|t| &t.functions) {
            *summary.functions_by_kind.entry(f.kind.clone()).or_default() += 1;
        }
    }
    summary.withheld_device_ids = withheld;
    summary.redacted_text_values += count;
}
