//! Reader for the zipped record-format project file (`.dez`). The archive
//! holds text members; the model member is a header followed by records:
//!
//! ```text
//! RECORD=<table>  '<type name>
//! <TAB>key=value        (first record of a table: named fields)
//! <TAB>value            (later records: values in the same field order)
//! ENDREC
//! ```
//!
//! Numbers are stored as `x<8 hex digits>` (IEEE single, big-endian bit
//! pattern), `L<integer>` (the bit pattern of an IEEE double), or plain text.
//! A blank value means "not entered". Impedances are per unit on the file's
//! base MVA (`fBaseMva`) and the element's bus voltage.

use std::collections::HashMap;

use crate::import_common::*;

/// One record: its type name and named fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub kind: String,
    pub fields: HashMap<String, String>,
}

impl Record {
    pub fn text(&self, key: &str) -> &str {
        self.fields.get(key).map(|s| s.as_str()).unwrap_or("")
    }
    pub fn num(&self, key: &str) -> Option<f64> {
        decode_number(self.text(key))
    }
}

/// Decode a stored number. Blank or unparseable text is None.
pub fn decode_number(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let v = if let Some(hex) = t.strip_prefix('x') {
        // Shortest decimal form of the single, so 12.47 stays 12.47.
        f32::from_bits(u32::from_str_radix(hex, 16).ok()?).to_string().parse::<f64>().ok()?
    } else if let Some(int) = t.strip_prefix('L') {
        f64::from_bits(int.parse::<i64>().ok()? as u64)
    } else {
        t.parse::<f64>().ok()?
    };
    // Very large singles are the program's "not set" sentinel.
    (v.is_finite() && v.abs() < 1e37).then_some(v)
}

/// Read the stored members of a zip archive (stored or deflated).
pub fn zip_members(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let u16_at = |o: usize| -> Result<usize, String> {
        bytes.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(|| "truncated archive".to_string())
    };
    let u32_at = |o: usize| -> Result<usize, String> {
        bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(|| "truncated archive".to_string())
    };
    let end = (0..bytes.len().saturating_sub(21)).rev()
        .find(|&o| bytes[o..o + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("not a zip archive: no end-of-directory record")?;
    let count = u16_at(end + 10)?;
    let mut at = u32_at(end + 16)?;
    let mut out = Vec::new();
    for _ in 0..count {
        if bytes.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02][..]) {
            return Err("bad central directory entry".into());
        }
        let method = u16_at(at + 10)?;
        let comp = u32_at(at + 20)?;
        let (nlen, elen, clen) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?);
        let local = u32_at(at + 42)?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + nlen).ok_or("truncated name")?).to_string();
        at += 46 + nlen + elen + clen;
        let data_at = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let data = bytes.get(data_at..data_at + comp).ok_or("truncated member data")?;
        let body = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(data).map_err(|e| format!("{name}: inflate failed: {e:?}"))?,
            m => return Err(format!("{name}: unsupported compression method {m}")),
        };
        out.push((name, body));
    }
    Ok(out)
}

/// The model text: the largest member that contains records. A file that is
/// not an archive is taken as the model text itself.
pub fn model_text(bytes: &[u8]) -> Result<String, String> {
    let decode = |b: &[u8]| String::from_utf8_lossy(b).trim_start_matches('\u{feff}').replace("\r\n", "\n");
    if !bytes.starts_with(b"PK") {
        return Ok(decode(bytes));
    }
    zip_members(bytes)?.into_iter()
        .map(|(_, b)| decode(&b))
        .filter(|t| t.contains("\nRECORD="))
        .max_by_key(|t| t.len())
        .ok_or_else(|| "archive has no member with records".into())
}

/// Header settings (`key=value` lines before the first record) and records.
pub fn parse_records(text: &str) -> (HashMap<String, String>, Vec<Record>) {
    let mut header = HashMap::new();
    let mut schema: HashMap<String, Vec<String>> = HashMap::new();
    let mut records = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("RECORD=") {
            let kind = rest.split_once('\'').map(|(_, k)| k.trim()).unwrap_or("").to_string();
            current = Some((kind, Vec::new()));
        } else if line == "ENDREC" {
            let Some((kind, lines)) = current.take() else { continue };
            let names = schema.entry(kind.clone()).or_insert_with(|| {
                lines.iter().map(|l| l.split_once('=').map(|(k, _)| k).unwrap_or(l).to_string()).collect()
            }).clone();
            let named = lines.first().is_some_and(|l| names.first().is_some_and(|n| l.starts_with(&format!("{n}="))));
            let fields = names.into_iter().zip(lines).map(|(n, l)| {
                let v = if named { l.split_once('=').map(|(_, v)| v.to_string()).unwrap_or_default() } else { l };
                (n, v)
            }).collect();
            records.push(Record { kind, fields });
        } else if let Some((_, lines)) = current.as_mut() {
            if let Some(v) = line.strip_prefix('\t') {
                lines.push(v.to_string());
            }
        } else if records.is_empty() {
            if let Some((k, v)) = line.trim().split_once('=') {
                header.entry(k.to_string()).or_insert_with(|| v.to_string());
            }
        }
    }
    (header, records)
}

fn conn(code: Option<f64>) -> WindingConn {
    match code.map(|c| c as i64).unwrap_or(0) {
        0 => WindingConn::Delta,
        1 => WindingConn::Wye,
        2 => WindingConn::WyeGrounded,
        _ => WindingConn::Unknown,
    }
}

fn name(r: &Record) -> String {
    let n = r.text("szName").trim();
    if n.is_empty() { format!("{} {}", r.kind, r.text("nADDR")) } else { n.to_string() }
}

/// Map parsed records to the shared intermediate network.
pub fn network_from_text(text: &str, project_name: &str) -> ImportedNetwork {
    let (header, records) = parse_records(text);
    let base_mva = header.get("fBaseMva").and_then(|v| decode_number(v)).filter(|v| *v > 0.0).unwrap_or(100.0);
    let mut net = ImportedNetwork { name: project_name.to_string(), ..Default::default() };
    let mut bus_kv: HashMap<String, f64> = HashMap::new();
    for r in records.iter().filter(|r| r.kind == "Bus") {
        let key = r.text("nADDR").to_string();
        let kv = r.num("r_base_kv").unwrap_or(0.0);
        bus_kv.insert(key.clone(), kv);
        let dims = |p: &str| -> Option<[f64; 3]> {
            Some([r.num(&format!("{p}_h"))?, r.num(&format!("{p}_w"))?, r.num(&format!("{p}_d"))?])
        };
        let user = r.text("c_afh_encsize_user") == "1";
        let enclosure_mm = if user { dims("r_afh_encsize_user") } else { None }.or_else(|| dims("r_afh_encsize_display"));
        net.buses.push(ImportBus { key, name: name(r), kv, bracing_ka: r.num("r_bracing_ka"), enclosure_mm });
    }
    let zbase = |bus: &str| bus_kv.get(bus).map(|kv| kv * kv / base_mva).unwrap_or(0.0);
    let mut kinds: HashMap<String, usize> = HashMap::new();
    for r in &records {
        let key = r.text("nADDR").to_string();
        let from = r.text("a_from_bus_rec").to_string();
        let to = r.text("a_to_bus_rec").to_string();
        match r.kind.as_str() {
            "Bus" | "General Info" | "Note" => {}
            "Utility" => {
                let mva = r.num("r_3phase_mva").unwrap_or(0.0);
                let slg = r.num("r_slg_mva").filter(|v| *v > 0.0);
                let z0_over_z1 = slg.map(|s| (3.0 * mva / s - 2.0).max(0.0)).unwrap_or(1.0);
                net.sources.push(ImportSource {
                    key, name: name(r), bus: to, kv: r.num("r_kv"), mva_sc: mva,
                    xr: r.num("r_3phase_x_r_ratio").unwrap_or(10.0), z0_over_z1,
                    v_pu: r.num("r_ctrl_kv_pu").unwrap_or(1.0),
                });
            }
            "2W Transformer" => {
                let mva = r.num("r_mva").unwrap_or(0.0);
                // Entered %Z, else recover it from the stored per-unit impedance.
                let z_pu = match (r.num("r_zps_r1_pu"), r.num("r_zps_x1_pu")) {
                    (Some(a), Some(b)) if a.hypot(b) > 0.0 => Some(a.hypot(b) * 100.0 * mva / base_mva),
                    _ => None,
                };
                net.branches.push(ImportBranch {
                    key, name: name(r), from, to,
                    kind: ImportBranchKind::Transformer {
                        kva: mva * 1000.0, z_percent: r.num("r_z_percent").or(z_pu), z0_percent: r.num("r_z0_percent"),
                        xr: r.num("r_x_r_ratio").unwrap_or(1.0),
                        from_kv: r.num("r_from_nom_kv").unwrap_or(0.0), to_kv: r.num("r_to_nom_kv").unwrap_or(0.0),
                        from_conn: conn(r.num("c_from_conn_code")), to_conn: conn(r.num("c_to_conn_code")),
                    },
                });
            }
            "Cable" => {
                let z = zbase(&from);
                let pu = |k: &str| r.num(k).unwrap_or(0.0) * z;
                net.branches.push(ImportBranch {
                    key, name: name(r), from, to,
                    kind: ImportBranchKind::Cable {
                        r_ohm: pu("r_r1_pu"), x_ohm: pu("r_x1_pu"), r0_ohm: pu("r_r0_pu"), x0_ohm: pu("r_x0_pu"),
                        ampacity_a: r.num("l_cable_rating").filter(|a| *a > 0.0),
                    },
                });
            }
            "Motor" => {
                net.motors.push(ImportMotor {
                    key, name: name(r), bus: to, hp: r.num("r_hp").unwrap_or(0.0), kv: r.num("r_kv").unwrap_or(0.0),
                    pf: r.num("r_pfactor").unwrap_or(0.85),
                    efficiency: r.num("r_efficiency").map(|e| if e > 1.0 { e / 100.0 } else { e }),
                    x_subtransient_pu: r.num("r_xppdv").unwrap_or(16.7) / 100.0, xr: r.num("r_x_r_ratio").unwrap_or(10.0),
                });
            }
            "LV Breaker" | "Fuse" => {
                let kind = if r.kind == "Fuse" { ImportDeviceKind::Fuse } else { ImportDeviceKind::Breaker };
                let present = |k: &str| (!k.is_empty() && k != "0").then(|| k.to_string());
                // The device sits at a bus terminal and feeds the record on its other side.
                let (bus, branch) = if bus_kv.contains_key(&from) { (present(&from), present(&to)) } else { (present(&to), present(&from)) };
                let rating = r.num("r_ext_ltpu_amps").or_else(|| r.num("r_cont_current")).filter(|a| *a > 0.0);
                net.devices.push(ImportDevice {
                    key, name: name(r), kind, bus, branch,
                    manufacturer: r.text("s_mfr_id").trim().to_string(), device_type: r.text("s_type_id").trim().to_string(),
                    style: r.text("s_style_id").trim().to_string(), frame: r.text("s_frame_size").trim().to_string(),
                    rating_a: rating, interrupting_ka: r.num("r_int_ka"),
                });
            }
            "Switch" => net.warnings.push(ImportWarning::new("switch_closed_not_modeled", &name(r),
                if r.text("c_normal_state") == "0" { "switch is entered open; imported as closed with zero impedance" } else { "closed switch treated as zero impedance" })),
            other => *kinds.entry(other.to_string()).or_default() += 1,
        }
    }
    // Devices and switches sit in series with a cable: make the cable end at
    // the bus on the device's far side, so every branch joins two buses.
    let device_keys: HashMap<String, (String, String)> = records.iter()
        .filter(|r| matches!(r.kind.as_str(), "LV Breaker" | "Fuse" | "Switch"))
        .map(|r| (r.text("nADDR").to_string(), (r.text("a_from_bus_rec").to_string(), r.text("a_to_bus_rec").to_string())))
        .collect();
    let resolve = |end: &str, branch_key: &str| -> String {
        if bus_kv.contains_key(end) {
            return end.to_string();
        }
        match device_keys.get(end) {
            Some((f, t)) if f == branch_key => t.clone(),
            Some((f, _)) => f.clone(),
            None => end.to_string(),
        }
    };
    for br in &mut net.branches {
        br.from = resolve(&br.from, &br.key);
        br.to = resolve(&br.to, &br.key);
    }
    for (kind, n) in kinds {
        net.warnings.push(ImportWarning::new("record_skipped", &kind, format!("{n} record(s) of this type are not imported")));
    }
    net
}

/// Read a `.dez` file (or its extracted model text) into the shared network.
pub fn read(bytes: &[u8], project_name: &str) -> Result<ImportedNetwork, String> {
    Ok(network_from_text(&model_text(bytes)?, project_name))
}
