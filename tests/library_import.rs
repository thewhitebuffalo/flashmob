//! Device library conversion against a small synthetic library file built
//! with the block and record layout described in `library_import`.

use flashmob::library::{Library, PointCurve, Setting};
use flashmob::library_import::{self, Field};

fn text(s: &str) -> Vec<u8> {
    let mut v = ((s.len() + 4) as u16).to_le_bytes().to_vec();
    v.extend([1, 1]);
    v.extend(s.as_bytes());
    v.extend([0, 0xff]);
    v
}

fn array_header(total: usize, count: usize) -> Vec<u8> {
    [8u32, total as u32, count as u32].iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn numbers(values: &[f32]) -> Vec<u8> {
    let mut v = array_header(12 + 4 * values.len(), values.len());
    for x in values { v.extend(x.to_le_bytes()); }
    v
}

fn texts(items: &[&str]) -> Vec<u8> {
    let body: Vec<u8> = items.iter().flat_map(|s| text(s)).collect();
    let mut v = array_header(12 + body.len(), items.len());
    v.extend(body);
    v
}

fn reference(object: u32, class: u16) -> Vec<u8> {
    let mut v = 0u32.to_le_bytes().to_vec(); // block number; not used by the reader
    v.extend(object.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend(class.to_le_bytes());
    v
}

fn refs(items: &[(u32, u16)]) -> Vec<u8> {
    let mut v = (items.len() as u32).to_le_bytes().to_vec();
    for &(o, c) in items { v.extend(reference(o, c)); }
    v
}

fn f32s(values: &[f32]) -> Vec<u8> { values.iter().flat_map(|x| x.to_le_bytes()).collect() }

fn object(id: u32, class: u16, fixed: &[u8], fields: &[Vec<u8>]) -> Vec<u8> {
    let mut d = id.to_le_bytes().to_vec();
    d.extend(0u16.to_le_bytes());
    d.extend(class.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    let mut offset = 4 * (fields.len() + 1) + fixed.len();
    for f in fields {
        d.extend((offset as u32).to_le_bytes());
        offset += f.len();
    }
    d.extend((offset as u32).to_le_bytes());
    d.extend(fixed);
    for f in fields { d.extend(f); }
    d
}

struct File { blocks: Vec<[u8; 64]> }

impl File {
    fn new(count: usize) -> Self { File { blocks: vec![[0u8; 64]; count] } }

    /// Store a record starting at `first`, continuing at `rest` (contiguous when None).
    fn put(&mut self, first: usize, rest: Option<usize>, data: &[u8]) {
        let mut bytes = (first as u32).to_le_bytes().to_vec();
        bytes.extend((rest.unwrap_or(0) as u32).to_le_bytes());
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend([0x12, 0x54]);
        bytes.extend(data);
        for (i, chunk) in bytes.chunks(64).enumerate() {
            let block = if i == 0 { first } else { rest.map_or(first + i, |r| r + i - 1) };
            self.blocks[block][..chunk.len()].copy_from_slice(chunk);
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8; 15 * 64];
        for b in &self.blocks { out.extend(b); }
        out
    }
}

fn sample_file() -> Vec<u8> {
    let mut device_fixed = 1u32.to_le_bytes().to_vec();
    device_fixed.extend(600f32.to_le_bytes());
    let device = object(100, 126, &device_fixed, &[
        text("key"), text("ACME"), text("100-400A, 3 Pole"), text("Model Q"), text("Q___"), text("TC-1"),
        text("Notes with Vendorname internal text."), text("1/2/2003 4:05 PM"), text("6/7/2008 9:10 AM"),
        text("Fuses - Low Voltage"),
        refs(&[(101, 122)]), refs(&[(102, 124)]), refs(&[(104, 125)]), refs(&[]), refs(&[]), text(""), numbers(&[]),
    ]);
    let rating = object(101, 122, &f32s(&[600.0, 400.0, 200.0, 0.0, 0.0, 249.37, 1.0, 3.0, 5.0, 4.899, 400.0, 0.0]), &[text("Q 400")]);
    let frame = object(102, 124, &f32s(&[400.0]), &[refs(&[(103, 123)])]);
    let sensor = object(103, 123, &f32s(&[250.0]), &[numbers(&[])]);
    let unit = object(104, 125, &f32s(&[400.0, 0.0, 0.0, 0.0]), &[refs(&[(105, 121), (107, 121)]), numbers(&[1.0, 5.0])]);
    let function = |id: u32, kind: u32, target: u32, class: u16, name: &str| {
        let mut fixed = kind.to_le_bytes().to_vec();
        fixed.extend(1u32.to_le_bytes());
        fixed.extend(reference(target, class));
        object(id, 121, &fixed, &[text(name)])
    };
    let mut curve_fixed = 0u32.to_le_bytes().to_vec();
    curve_fixed.extend(10u32.to_le_bytes());
    curve_fixed.extend(f32s(&[10.0, -10.0, 0.01, 0.0, 0.0, 0.0, 0.0]));
    let curve = object(106, 110, &curve_fixed, &[numbers(&[1150.0, 2500.0, 10500.0]), numbers(&[1000.0, 10.0, 0.01])]);
    let mut pickup_fixed = 1u32.to_le_bytes().to_vec();
    pickup_fixed.extend(8u32.to_le_bytes());
    let pickup = object(108, 104, &pickup_fixed, &[texts(&["0.5", "1"]), numbers(&[0.5, 1.0]), text("")]);

    let mut file = File::new(64);
    // The device record starts in block 1 and continues at block 40.
    file.put(1, Some(40), &device);
    file.put(10, None, &rating);
    file.put(13, None, &frame);
    file.put(15, None, &sensor);
    file.put(17, None, &unit);
    file.put(20, None, &function(105, 7, 106, 110, "400 Amps"));
    file.put(22, None, &curve);
    file.put(26, None, &function(107, 0, 108, 104, "LTPU"));
    file.put(28, None, &pickup);
    file.bytes()
}

#[test]
fn converts_device_ratings_frames_and_characteristics() {
    let library = library_import::convert(&sample_file()).unwrap();
    assert_eq!(library.devices.len(), 1);
    let d = &library.devices[0];
    assert_eq!((d.id, d.manufacturer.as_str(), d.device_type.as_str(), d.style.as_str()), (100, "ACME", "100-400A, 3 Pole", "Model Q"));
    assert_eq!((d.catalog.as_str(), d.reference.as_str(), d.category.as_str()), ("Q___", "TC-1", "Fuses - Low Voltage"));
    assert_eq!(d.header_voltage_v, Some(600.0));
    let r = &d.ratings[0];
    assert_eq!((r.label.as_str(), r.voltage_v, r.rating_a, r.interrupting_ka, r.test_xr), ("Q 400", 600.0, 400.0, 200.0, 4.899));
    assert_eq!(d.frames[0].rating_a, 400.0);
    assert_eq!(d.frames[0].sensors_a, vec![250.0]);
    let unit = &d.trip_units[0];
    assert_eq!(unit.arrays, vec![vec![1.0, 5.0]]);
    let curve = &unit.functions[0];
    assert_eq!((curve.name.as_str(), curve.kind.as_str()), ("400 Amps", "point_curve"));
    assert_eq!(curve.data.curves, vec![PointCurve { current: vec![1150.0, 2500.0, 10500.0], time_s: vec![1000.0, 10.0, 0.01] }]);
    assert_eq!(curve.data.current_unit.as_deref(), Some("A"));
    assert_eq!(curve.data.current_tolerance_pct, Some([10.0, -10.0]));
    let pickup = &unit.functions[1];
    assert_eq!(pickup.kind, "pickup_settings");
    assert_eq!(pickup.data.settings, vec![Setting { label: "0.5".into(), value: 0.5 }, Setting { label: "1".into(), value: 1.0 }]);
    assert!(library.summary.warnings.is_empty(), "{:?}", library.summary.warnings);
    assert!(library.summary.skipped_records_by_class.is_empty());
}

#[test]
fn converted_library_round_trips_through_json() {
    let library = library_import::convert(&sample_file()).unwrap();
    let text = serde_json::to_string(&library).unwrap();
    let back = Library::from_json(&text).unwrap();
    assert_eq!(back.devices.len(), 1);
    assert_eq!(back.search(&["acme".into(), "model q".into()]).len(), 1);
    assert!(back.search(&["other".into()]).is_empty());
}

#[test]
fn library_validation_rejects_mismatched_curves_and_wrong_format() {
    let mut library = library_import::convert(&sample_file()).unwrap();
    library.devices[0].trip_units[0].functions[0].data.curves[0].time_s.pop();
    assert!(library.validate().iter().any(|e| e.contains("3 currents and 2 times")));
    library.format = "something-else".into();
    assert!(library.validate().iter().any(|e| e.contains("format")));
}

#[test]
fn redaction_replaces_free_text_and_withholds_identity_matches() {
    let mut library = library_import::convert(&sample_file()).unwrap();
    library_import::redact(&mut library, &["vendorname".into()]);
    assert_eq!(library.devices[0].notes, "Notes with [redacted] internal text.");
    assert_eq!(library.summary.redacted_text_values, 1);
    library_import::redact(&mut library, &["model q".into()]);
    assert!(library.devices.is_empty());
    assert_eq!(library.summary.withheld_device_ids, vec![100]);
    assert_eq!(library.summary.devices, 0);
}

#[test]
fn rejects_files_that_are_not_block_aligned_or_have_no_devices() {
    assert!(library_import::convert(&[0u8; 100]).unwrap_err().contains("blocks"));
    assert!(library_import::convert(&vec![0u8; 64 * 40]).unwrap_err().contains("no device"));
}

#[test]
fn field_parser_distinguishes_text_arrays_and_references() {
    assert_eq!(library_import::parse_field(&text("ABC")), Field::Text("ABC".into()));
    assert!(matches!(library_import::parse_field(&numbers(&[1.0, 2.0])), Field::Arrays(a) if a.len() == 1));
    assert!(matches!(library_import::parse_field(&refs(&[(5, 121)])), Field::Refs(r) if r[0].object == 5 && r[0].class == 121));
}
