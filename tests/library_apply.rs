//! Applying a library curve or trip setting onto a project device.
//! Libraries here are built in code. Nothing reads a manufacturer file.

use flashmob::curves::{self, PointTime};
use flashmob::exec::{self, Command};
use flashmob::library::{apply_device, ApplyDevice, DialCurve, Function, FunctionData, Frame, Library, LibraryDevice, PointCurve, Rating, Setting, TripUnit, FORMAT, VERSION};
use flashmob::library_apply::SettingRef;
use flashmob::model::{BandInterpretation, CurveSpec, Device, PointTail, Project};

fn lib(devices: Vec<LibraryDevice>) -> Library {
    Library { format: FORMAT.into(), version: VERSION, devices, ..Default::default() }
}

fn sample_device(category: &str, units: Vec<TripUnit>) -> LibraryDevice {
    LibraryDevice {
        id: 7,
        category: category.into(),
        manufacturer: "Acme".into(),
        device_type: "Class RK1".into(),
        style: "KTU".into(),
        catalog: "KTU-100".into(),
        modified: "2020-01-02".into(),
        header_voltage_v: Some(600.0),
        trip_units: units,
        ..Default::default()
    }
}

fn unit(rating_a: f64, functions: Vec<Function>) -> TripUnit {
    TripUnit { rating_a, functions, ..Default::default() }
}

fn curve_fn(name: &str, kind: &str, current_unit: &str, bands: &[(&[f64], &[f64])]) -> Function {
    Function {
        name: name.into(),
        kind: kind.into(),
        data: FunctionData {
            current_unit: Some(current_unit.into()),
            curves: bands.iter().map(|(current, time_s)| PointCurve { current: current.to_vec(), time_s: time_s.to_vec() }).collect(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn slow_then_fast() -> Function {
    curve_fn("Bands", "point_curve_bands", "A", &[
        (&[100.0, 1000.0], &[10.0, 1.0]),
        (&[100.0, 1000.0], &[1.0, 0.1]),
    ])
}

fn req(device_id: u32) -> ApplyDevice {
    ApplyDevice {
        device_id,
        id: "fuse-1".into(),
        name: String::new(),
        bus: "mcc".into(),
        protected_branch: None,
        rating_a: None,
        current_base_a: None,
        voltage_v: None,
        function_name: None,
        library_name: "test-lib".into(),
        fuse_total_clearing: false,
        dial: None,
        ct_ratio: None,
        tap_a: None,
        sensor_a: None,
        lt_pickup: None,
        lt_delay: None,
        lt_delay_multiple: None,
        st_pickup: None,
        st_delay: None,
        inst: None,
        inst_s: None,
        pickup_base: None,
    }
}

fn apply_one(category: &str, function: Function) -> Result<Device, String> {
    apply_device(&lib(vec![sample_device(category, vec![unit(100.0, vec![function])])]), &req(7))
}

fn points_of(device: &Device) -> (&[[f64; 2]], &Option<Vec<[f64; 2]>>, PointTail, BandInterpretation) {
    match &device.curve {
        CurveSpec::Points { points, max_points, beyond_last, band_interpretation } => (points, max_points, *beyond_last, *band_interpretation),
        other => panic!("expected a point curve, got {other:?}"),
    }
}

#[test]
fn two_ampere_bands_stored_slow_then_fast_are_reordered() {
    let device = apply_one("LV Fuse", slow_then_fast()).unwrap();
    let (points, max_points, tail, interpretation) = points_of(&device);
    assert_eq!(points, [[100.0, 1.0], [1000.0, 0.1]]);
    assert_eq!(max_points.as_deref(), Some([[100.0, 10.0], [1000.0, 1.0]].as_slice()));
    assert_eq!(tail, PointTail::Unavailable);
    assert_eq!(interpretation, BandInterpretation::Assumed);
    assert!(!device.fuse_total_clearing);
    assert!(!device.curve_is_total_clearing);
    assert!(device.breaker_interrupting_s.is_none());
    assert_eq!(device.basis, "assumed");
    assert_eq!(device.name, "Acme KTU KTU-100 100");
    let link = device.library.as_ref().unwrap();
    assert_eq!(link.device_id, 7);
    assert_eq!(link.library, "test-lib");
    assert_eq!(link.manufacturer, "Acme");
    assert_eq!(link.device_type, "Class RK1");
    assert_eq!(link.style, "KTU");
    assert_eq!(link.category, "LV Fuse");
    assert_eq!(link.size, "KTU-100");
    assert_eq!(link.rating_a, Some(100.0));
    assert_eq!(link.voltage_v, Some(600.0));
    assert_eq!(link.modified, "2020-01-02");
    assert_eq!(link.unsupported_elements, vec!["slower band is not marked total clearing".to_string()]);

    let mut named = req(7);
    named.name = "Main fuse".into();
    let named = apply_device(&lib(vec![sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])])]), &named).unwrap();
    assert_eq!(named.name, "Main fuse");

    // Source order is not an identity. Fast stored first still lands in points.
    let reversed = curve_fn("Bands", "point_curve_bands", "A", &[
        (&[100.0, 1000.0], &[1.0, 0.1]),
        (&[100.0, 1000.0], &[10.0, 1.0]),
    ]);
    let reversed = apply_one("LV Fuse", reversed).unwrap();
    let (points, max_points, _, _) = points_of(&reversed);
    assert_eq!(points, [[100.0, 1.0], [1000.0, 0.1]]);
    assert_eq!(max_points.as_deref(), Some([[100.0, 10.0], [1000.0, 1.0]].as_slice()));
}

#[test]
fn non_fuse_two_bands_do_not_set_fuse_total_clearing() {
    let device = apply_one("Molded Case", slow_then_fast()).unwrap();
    assert!(!device.fuse_total_clearing);
    assert!(!device.curve_is_total_clearing);
    let (_, max_points, _, _) = points_of(&device);
    assert!(max_points.is_some());
}

#[test]
fn one_band_does_not_set_fuse_total_clearing() {
    let device = apply_one("LV Fuse", curve_fn("Melt", "point_curve", "A", &[(&[100.0, 1000.0], &[1.0, 0.1])])).unwrap();
    assert!(!device.fuse_total_clearing);
    assert!(!device.curve_is_total_clearing);
    let (points, max_points, tail, interpretation) = points_of(&device);
    assert_eq!(points, [[100.0, 1.0], [1000.0, 0.1]]);
    assert!(max_points.is_none());
    assert_eq!(tail, PointTail::Unavailable);
    assert_eq!(interpretation, BandInterpretation::Assumed);
    assert_eq!(device.basis, "assumed");
}

#[test]
fn multiples_scale_by_the_trip_unit_rating() {
    let device = apply_one("LV Fuse", curve_fn("Melt", "point_curve", "multiple", &[(&[3.0, 30.0], &[4.0, 0.4])])).unwrap();
    let (points, max_points, _, _) = points_of(&device);
    assert_eq!(points, [[300.0, 4.0], [3000.0, 0.4]]);
    assert!(max_points.is_none());
    assert!(!device.fuse_total_clearing);
    let link = device.library.as_ref().unwrap();
    assert_eq!(link.rating_a, Some(100.0));
    assert_eq!(link.unsupported_elements, vec!["current scale 100 A".to_string()]);
    assert_eq!(device.name, "Acme KTU KTU-100 100");
}

#[test]
fn multiples_use_current_base_override() {
    let mut request = req(7);
    request.current_base_a = Some(50.0);
    let device = apply_device(&lib(vec![sample_device("LV Fuse", vec![unit(100.0, vec![
        curve_fn("Melt", "point_curve", "multiple", &[(&[3.0, 30.0], &[4.0, 0.4])]),
    ])])]), &request).unwrap();
    let (points, _, _, _) = points_of(&device);
    assert_eq!(points, [[150.0, 4.0], [1500.0, 0.4]]);
    let link = device.library.as_ref().unwrap();
    assert_eq!(link.rating_a, Some(100.0));
    assert_eq!(link.unsupported_elements, vec!["current scale 50 A".to_string()]);
}

#[test]
fn ampere_curve_rejects_current_base() {
    let mut request = req(7);
    request.current_base_a = Some(50.0);
    let err = apply_device(&lib(vec![sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])])]), &request).unwrap_err();
    assert!(err.contains("current_base_a"), "{err}");
}

#[test]
fn unknown_device_id_errors() {
    let err = apply_device(&lib(vec![sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])])]), &req(99)).unwrap_err();
    assert!(err.contains("99"), "{err}");
}

#[test]
fn rating_a_selects_among_trip_units() {
    let mut device = sample_device("Molded Case", vec![
        unit(100.0, vec![curve_fn("Small", "point_curve", "A", &[(&[10.0, 100.0], &[5.0, 0.5])])]),
        unit(200.0, vec![
            curve_fn("Large", "point_curve", "A", &[(&[20.0, 200.0], &[8.0, 0.4])]),
            Function { name: "Ground".into(), kind: "pickup".into(), ..Default::default() },
        ]),
    ]);
    device.catalog.clear();
    let library = lib(vec![device]);
    let missing = apply_device(&library, &req(7)).unwrap_err();
    assert!(missing.contains("rating_a") && missing.contains("100") && missing.contains("200"), "{missing}");

    let mut request = req(7);
    request.rating_a = Some(200.0);
    let selected = apply_device(&library, &request).unwrap();
    let (points, _, _, _) = points_of(&selected);
    assert_eq!(points, [[20.0, 8.0], [200.0, 0.4]]);
    let link = selected.library.as_ref().unwrap();
    assert_eq!(link.rating_a, Some(200.0));
    assert_eq!(link.size, "200");
    assert_eq!(selected.name, "Acme KTU 200");
    assert!(link.unsupported_elements.iter().any(|entry| entry.contains("trip unit 100 A")), "{:?}", link.unsupported_elements);
    assert!(link.unsupported_elements.iter().any(|entry| entry.contains("Ground")), "{:?}", link.unsupported_elements);

    request.rating_a = Some(200.0 * (1.0 + 0.0001));
    let close = apply_device(&library, &request).unwrap();
    assert_eq!(close.library.as_ref().unwrap().rating_a, Some(200.0));

    request.rating_a = Some(200.0 * (1.0 + 0.001));
    let outside = apply_device(&library, &request).unwrap_err();
    assert!(outside.contains("matches no trip unit"), "{outside}");

    let twins = lib(vec![sample_device("Molded Case", vec![
        unit(100.0, vec![curve_fn("A", "point_curve", "A", &[(&[10.0, 100.0], &[5.0, 0.5])])]),
        unit(100.0, vec![curve_fn("B", "point_curve", "A", &[(&[12.0, 120.0], &[5.0, 0.5])])]),
    ])]);
    let mut request = req(7);
    request.rating_a = Some(100.0);
    let err = apply_device(&twins, &request).unwrap_err();
    assert!(err.contains("matches 2"), "{err}");
}

#[test]
fn crossing_bands_error() {
    let err = apply_one("LV Fuse", curve_fn("Bands", "point_curve_bands", "A", &[
        (&[100.0, 1000.0], &[10.0, 0.1]),
        (&[100.0, 1000.0], &[2.0, 1.0]),
    ])).unwrap_err();
    assert!(err.contains("cross"), "{err}");
}

#[test]
fn bands_that_do_not_overlap_or_are_not_one_or_two_error() {
    let overlap = apply_one("LV Fuse", curve_fn("Bands", "point_curve_bands", "A", &[
        (&[10.0, 20.0], &[5.0, 1.0]),
        (&[100.0, 200.0], &[5.0, 1.0]),
    ])).unwrap_err();
    assert!(overlap.contains("overlap"), "{overlap}");

    let count = apply_one("LV Fuse", curve_fn("Bands", "point_curve_bands", "A", &[
        (&[10.0, 100.0], &[5.0, 1.0]),
        (&[10.0, 100.0], &[4.0, 0.5]),
        (&[10.0, 100.0], &[3.0, 0.2]),
    ])).unwrap_err();
    assert!(count.contains("3 bands"), "{count}");
}

#[test]
fn band_order_is_not_repaired() {
    let decreasing = apply_one("LV Fuse", curve_fn("Melt", "point_curve", "A", &[(&[1000.0, 100.0], &[0.1, 10.0])])).unwrap_err();
    assert!(decreasing.contains("increase strictly"), "{decreasing}");
    let rising = apply_one("LV Fuse", curve_fn("Melt", "point_curve", "A", &[(&[100.0, 1000.0], &[0.1, 10.0])])).unwrap_err();
    assert!(rising.contains("times must not increase"), "{rising}");
    let bad = apply_one("LV Fuse", curve_fn("Melt", "point_curve", "A", &[(&[100.0, -5.0], &[1.0, 0.1])])).unwrap_err();
    assert!(bad.contains("positive"), "{bad}");
}

#[test]
fn function_name_selects_the_only_matching_curve() {
    let device = sample_device("LV Fuse", vec![unit(100.0, vec![
        curve_fn("Melt", "point_curve", "A", &[(&[100.0, 1000.0], &[1.0, 0.1])]),
        curve_fn("Clear", "point_curve", "A", &[(&[100.0, 1000.0], &[2.0, 0.2])]),
        Function { name: "Pickup".into(), kind: "pickup".into(), ..Default::default() },
    ])]);
    let library = lib(vec![device]);
    let missing = apply_device(&library, &req(7)).unwrap_err();
    assert!(missing.contains("function_name"), "{missing}");

    let mut request = req(7);
    request.function_name = Some("CLEAR".into());
    let selected = apply_device(&library, &request).unwrap();
    let (points, _, _, _) = points_of(&selected);
    assert_eq!(points, [[100.0, 2.0], [1000.0, 0.2]]);
    let unsupported = &selected.library.as_ref().unwrap().unsupported_elements;
    assert!(unsupported.iter().any(|entry| entry.contains("Melt")), "{unsupported:?}");
    assert!(unsupported.iter().any(|entry| entry.contains("Pickup")), "{unsupported:?}");
}

#[test]
fn voltage_on_the_link_prefers_the_request() {
    let mut device = sample_device("LV Fuse", vec![unit(100.0, vec![curve_fn("Melt", "point_curve", "A", &[(&[100.0, 1000.0], &[1.0, 0.1])])])]);
    device.ratings = vec![Rating { voltage_v: 240.0, ..Default::default() }];
    let header = apply_device(&lib(vec![device.clone()]), &req(7)).unwrap();
    assert_eq!(header.library.unwrap().voltage_v, Some(600.0));

    let mut request = req(7);
    request.voltage_v = Some(480.0);
    let requested = apply_device(&lib(vec![device.clone()]), &request).unwrap();
    assert_eq!(requested.library.unwrap().voltage_v, Some(480.0));

    device.header_voltage_v = None;
    let from_rating = apply_device(&lib(vec![device.clone()]), &req(7)).unwrap();
    assert_eq!(from_rating.library.unwrap().voltage_v, Some(240.0));

    device.ratings.clear();
    let none = apply_device(&lib(vec![device]), &req(7)).unwrap();
    assert_eq!(none.library.unwrap().voltage_v, None);
}

#[test]
fn applied_two_band_fuse_validates_and_clears_on_the_slow_band() {
    let mut request = req(7);
    request.protected_branch = Some("feeder".into());
    let unmarked = apply_device(&lib(vec![sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])])]), &request).unwrap();
    assert!(curves::clearing_time(&unmarked, 1000.0).is_none());
    request.fuse_total_clearing = true;
    let device = apply_device(&lib(vec![sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])])]), &request).unwrap();
    assert!(device.fuse_total_clearing);
    assert!(!device.library.as_ref().unwrap().unsupported_elements.iter().any(|entry| entry.contains("not marked")));
    let mut project = Project::sample();
    project.devices.push(device.clone());
    let errors = project.validate();
    assert!(errors.is_empty(), "{errors:?}");

    assert!(device.breaker_interrupting_s.is_none());
    assert!((curves::clearing_time(&device, 1000.0).unwrap() - 1.0).abs() < 1e-12);
    let amps = (100.0_f64 * 1000.0).sqrt();
    let slow = [[100.0, 10.0], [1000.0, 1.0]];
    let PointTime::Within(slow_t) = curves::point_time(&slow, amps) else { panic!("slow band has no time at {amps}") };
    let clearing = curves::clearing_time(&device, amps).unwrap();
    assert!((clearing - slow_t).abs() < 1e-12, "{clearing} vs {slow_t}");
    assert_eq!(curves::clearing_time(&device, amps), curves::trip_time(&device.curve, amps));
    let PointTime::Within(fast_t) = curves::point_time(&[[100.0, 1.0], [1000.0, 0.1]], amps) else { panic!("fast") };
    assert!(clearing > fast_t);
}

#[test]
fn exec_apply_library_device_adds_once() {
    let library = lib(vec![sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])])]);
    let dir = std::env::temp_dir().join(format!("flashmob-library-apply-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("library.json");
    std::fs::write(&path, serde_json::to_string(&library).unwrap()).unwrap();
    let path_str = path.display().to_string();

    let response = exec::exec_request(&serde_json::json!({
        "commands": [
            {"op": "sample"},
            {"op": "topology"},
            {"op": "apply_library_device", "library_path": path_str, "device_id": 7, "id": "fuse-1", "bus": "mcc"}
        ]
    }).to_string());
    assert!(response.ok, "{:?} {:?}", response.error, response.validation_errors);
    assert!(response.topology.is_none(), "apply_library_device is a model edit");
    assert_eq!(response.project.devices.len(), 3);
    let device = response.project.devices.iter().find(|d| d.id == "fuse-1").unwrap();
    assert!(!device.fuse_total_clearing);
    assert!(device.library.as_ref().unwrap().unsupported_elements.iter().any(|entry| entry.contains("not marked total clearing")));
    assert_eq!(device.library.as_ref().unwrap().library, path_str);
    let (points, max_points, _, interpretation) = points_of(device);
    assert_eq!(points, [[100.0, 1.0], [1000.0, 0.1]]);
    assert_eq!(max_points.as_deref(), Some([[100.0, 10.0], [1000.0, 1.0]].as_slice()));
    assert_eq!(interpretation, BandInterpretation::Assumed);

    let again = exec::exec_request(&serde_json::json!({
        "project": response.project,
        "commands": [{"op": "apply_library_device", "library_path": path_str, "device_id": 7, "id": "fuse-1", "bus": "mcc"}]
    }).to_string());
    assert!(!again.ok);
    assert!(again.error.as_deref().unwrap_or_default().contains("already used"), "{:?}", again.error);
    assert_eq!(again.project.devices.iter().filter(|d| d.id == "fuse-1").count(), 1);

    let missing = exec::apply(Project::sample(), &[apply_cmd(dir.join("missing.json").display().to_string(), "fuse-2")]);
    assert!(!missing.ok);
    assert!(missing.error.as_deref().unwrap_or_default().contains("could not read"), "{:?}", missing.error);
    assert_eq!(missing.project.devices.len(), 2);

    let bad_path = dir.join("bad.json");
    std::fs::write(&bad_path, "{}").unwrap();
    let bad = exec::apply(Project::sample(), &[apply_cmd(bad_path.display().to_string(), "fuse-3")]);
    assert!(!bad.ok);
    assert_eq!(bad.project.devices.len(), 2);

    std::fs::remove_dir_all(&dir).unwrap();
}

fn apply_cmd(library_path: String, id: &str) -> Command {
    Command::ApplyLibraryDevice {
        library_path,
        device_id: 7,
        id: id.into(),
        bus: "mcc".into(),
        name: String::new(),
        rating_a: None,
        current_base_a: None,
        voltage_v: None,
        function_name: None,
        protected_branch: None,
        library_name: String::new(),
        fuse_total_clearing: false,
        dial: None,
        ct_ratio: None,
        tap_a: None,
        sensor_a: None,
        lt_pickup_function: None,
        lt_pickup_label: None,
        lt_delay_function: None,
        lt_delay_label: None,
        lt_delay_multiple: None,
        st_pickup_function: None,
        st_pickup_label: None,
        st_delay_function: None,
        st_delay_label: None,
        inst_function: None,
        inst_label: None,
        inst_s: None,
        pickup_base: None,
    }
}

fn dial_fn(label: &str, dial: f64, multiples: &[f64], times: &[f64]) -> Function {
    Function {
        name: "51".into(),
        kind: "time_dial_curve_family".into(),
        data: FunctionData {
            settings: vec![Setting { label: label.into(), value: dial }],
            dial_current_multiples: multiples.to_vec(),
            dial_curves: vec![DialCurve { dial, time_s: times.to_vec() }],
            ..Default::default()
        },
        ..Default::default()
    }
}

fn setting_fn(name: &str, header_second: u32, settings: &[(&str, f64)], arrays: &[Vec<f64>]) -> Function {
    Function {
        name: name.into(),
        kind: "pickup_settings".into(),
        data: FunctionData {
            header_codes: [0, header_second],
            settings: settings.iter().map(|(label, value)| Setting { label: (*label).into(), value: *value }).collect(),
            arrays: arrays.to_vec(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn breaker(functions: Vec<Function>) -> LibraryDevice {
    let mut device = sample_device("Molded Case", vec![unit(0.0, functions)]);
    device.frames = vec![Frame { rating_a: 600.0, sensors_a: vec![400.0] }];
    device
}

fn trip_req() -> ApplyDevice {
    let mut request = req(7);
    request.sensor_a = Some(400.0);
    request.lt_pickup = Some(SettingRef { function: "LTPU".into(), label: "0.5".into() });
    request.lt_delay = Some(SettingRef { function: "LTD".into(), label: "3".into() });
    request.lt_delay_multiple = Some(6.0);
    request.pickup_base = Some("sensor".into());
    request
}

fn thermal(device: &Device) -> (f64, f64, Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match device.curve {
        CurveSpec::ThermalMagnetic { lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s } => {
            (lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s)
        }
        ref other => panic!("expected a thermal-magnetic curve, got {other:?}"),
    }
}

fn dial_request(tap_a: Option<f64>, ct_ratio: Option<f64>) -> ApplyDevice {
    let mut request = req(7);
    request.dial = Some(5.0);
    request.ct_ratio = ct_ratio;
    request.tap_a = tap_a;
    request
}

#[test]
fn dial_curve_scales_to_primary_amperes() {
    let function = dial_fn("5", 5.0, &[2.0, 10.0], &[1.0, 0.1]);
    let device = apply_device(&lib(vec![sample_device("Relay", vec![unit(5.0, vec![function])])]), &dial_request(Some(5.0), Some(120.0))).unwrap();
    let (points, max_points, tail, interpretation) = points_of(&device);
    assert_eq!(points, [[1200.0, 1.0], [6000.0, 0.1]]);
    assert!(max_points.is_none());
    assert!(!device.fuse_total_clearing);
    assert!(!device.curve_is_total_clearing);
    assert!(device.breaker_interrupting_s.is_none());
    assert!(device.interrupting.is_none());
    assert_eq!(device.basis, "assumed");
    assert_eq!(tail, PointTail::Unavailable);
    assert_eq!(interpretation, BandInterpretation::Assumed);
    let notes = &device.library.as_ref().unwrap().unsupported_elements;
    assert!(notes.iter().any(|note| note == "dial 5 scaled by tap 5 A and CT ratio 120"), "{notes:?}");
}

#[test]
fn dial_curve_requires_ct_ratio() {
    let function = dial_fn("5A", 5.0, &[2.0, 10.0], &[1.0, 0.1]);
    let err = apply_device(&lib(vec![sample_device("Relay", vec![unit(5.0, vec![function])])]), &dial_request(Some(5.0), None)).unwrap_err();
    assert!(err.contains("ct_ratio is required to scale a relay dial curve to primary amperes"), "{err}");
}

#[test]
fn dial_label_without_amperes_requires_tap() {
    let function = dial_fn("5", 5.0, &[2.0, 10.0], &[1.0, 0.1]);
    let err = apply_device(&lib(vec![sample_device("Relay", vec![unit(5.0, vec![function])])]), &dial_request(None, Some(120.0))).unwrap_err();
    assert!(err.contains("tap_a (secondary amperes) is required; ct_ratio alone does not set the pickup"), "{err}");
}

#[test]
fn dial_label_ending_in_a_assumes_secondary_tap() {
    let function = dial_fn("5A", 5.0, &[2.0, 10.0], &[1.0, 0.1]);
    let device = apply_device(&lib(vec![sample_device("Relay", vec![unit(5.0, vec![function])])]), &dial_request(None, Some(120.0))).unwrap();
    let (points, max_points, _, _) = points_of(&device);
    assert_eq!(points, [[1200.0, 1.0], [6000.0, 0.1]]);
    assert!(max_points.is_none());
    assert!(!device.fuse_total_clearing);
    let notes = &device.library.as_ref().unwrap().unsupported_elements;
    assert!(notes.iter().any(|note| note == "tap 5 A taken from the dial label, assumed secondary amperes"), "{notes:?}");
    assert!(notes.iter().any(|note| note == "dial 5 scaled by tap 5 A and CT ratio 120"), "{notes:?}");
}

#[test]
fn sensor_pickup_base_scales_long_time_pickup() {
    let library = lib(vec![breaker(vec![
        setting_fn("LTPU", 1, &[("0.5", 0.5)], &[]),
        setting_fn("LTD", 1, &[("3", 3.0)], &[]),
    ])]);
    let device = apply_device(&library, &trip_req()).unwrap();
    let (lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s) = thermal(&device);
    assert_eq!(lt_pickup_a, 200.0);
    assert_eq!(lt_delay_s, 3.0);
    assert!(st_pickup_a.is_none() && st_delay_s.is_none() && inst_a.is_none() && inst_s.is_none());
    assert!(!device.fuse_total_clearing);
    assert!(device.interrupting.is_none());
    assert_eq!(device.basis, "assumed");
    let notes = &device.library.as_ref().unwrap().unsupported_elements;
    assert!(notes.iter().any(|note| note == "sensor 400 A"), "{notes:?}");
}

#[test]
fn long_time_delay_uses_slower_stored_time_at_six_times() {
    let library = lib(vec![breaker(vec![
        setting_fn("LTPU", 1, &[("0.5", 0.5)], &[]),
        setting_fn("LTD", 1, &[("3", 3.0)], &[vec![4.5]]),
    ])]);
    let device = apply_device(&library, &trip_req()).unwrap();
    let (_, lt_delay_s, _, _, _, _) = thermal(&device);
    assert_eq!(lt_delay_s, 4.5);
    let notes = &device.library.as_ref().unwrap().unsupported_elements;
    assert!(notes.iter().any(|note| note == "long-time delay uses the slower stored time 4.5 s (setting 3 s)"), "{notes:?}");
    assert!(notes.iter().any(|note| note == "long-time delay 4.5 s at 6× converted to 4.5 s at 6×"), "{notes:?}");
}

#[test]
fn long_time_delay_converts_a_three_times_time_to_six_times() {
    let library = lib(vec![breaker(vec![
        setting_fn("LTPU", 1, &[("0.5", 0.5)], &[]),
        setting_fn("LTD", 1, &[("3", 3.0)], &[vec![4.5]]),
    ])]);
    let mut request = trip_req();
    request.lt_delay_multiple = Some(3.0);
    let device = apply_device(&library, &request).unwrap();
    let (_, lt_delay_s, _, _, _, _) = thermal(&device);
    assert!((lt_delay_s - 1.125).abs() < 1e-12, "{lt_delay_s}");
    let notes = &device.library.as_ref().unwrap().unsupported_elements;
    assert!(notes.iter().any(|note| note == "long-time delay 4.5 s at 3× converted to 1.125 s at 6×"), "{notes:?}");
}

#[test]
fn long_time_delay_multiple_is_required() {
    let library = lib(vec![breaker(vec![
        setting_fn("LTPU", 1, &[("0.5", 0.5)], &[]),
        setting_fn("LTD", 1, &[("3", 3.0)], &[]),
    ])]);
    let mut request = trip_req();
    request.lt_delay_multiple = None;
    let err = apply_device(&library, &request).unwrap_err();
    assert!(err.contains("the library does not state the current multiple at which the long-time delay applies; pass lt_delay_multiple (6 when that time is at 6 times long-time pickup)"), "{err}");
}

#[test]
fn i2t_short_time_is_not_copied() {
    let library = lib(vec![breaker(vec![
        setting_fn("LTPU", 1, &[("0.5", 0.5)], &[]),
        setting_fn("LTD", 1, &[("3", 3.0)], &[]),
        setting_fn("STD-I2T", 1, &[("0.2", 0.2)], &[]),
    ])]);
    let mut request = trip_req();
    request.st_delay = Some(SettingRef { function: "STD-I2T".into(), label: "0.2".into() });
    let device = apply_device(&library, &request).unwrap();
    let (lt_pickup_a, _, st_pickup_a, st_delay_s, _, _) = thermal(&device);
    assert_eq!(lt_pickup_a, 200.0);
    assert!(st_pickup_a.is_none());
    assert!(st_delay_s.is_none());
    let notes = &device.library.as_ref().unwrap().unsupported_elements;
    assert!(notes.iter().any(|note| note == "short-time function STD-I2T is I2t; the flat short-time model was not used"), "{notes:?}");
}

#[test]
fn header_code_10_pickup_stays_in_amperes() {
    let library = lib(vec![breaker(vec![
        setting_fn("LTPU", 10, &[("100", 100.0)], &[]),
        setting_fn("LTD", 1, &[("3", 3.0)], &[]),
    ])]);
    let mut request = trip_req();
    request.lt_pickup = Some(SettingRef { function: "LTPU".into(), label: "100".into() });
    let device = apply_device(&library, &request).unwrap();
    let (lt_pickup_a, _, _, _, _, _) = thermal(&device);
    assert_eq!(lt_pickup_a, 100.0);
}

#[test]
fn pickup_without_a_stated_base_errors() {
    let library = lib(vec![breaker(vec![
        setting_fn("LTPU", 1, &[("0.5", 0.5)], &[]),
        setting_fn("LTD", 1, &[("3", 3.0)], &[]),
    ])]);
    let mut request = trip_req();
    request.pickup_base = None;
    let err = apply_device(&library, &request).unwrap_err();
    assert!(err.contains("pickup base is not stated for LTPU; pass pickup_base sensor or long_time"), "{err}");
}

fn duty_rating(voltage_v: f64, rating_a: f64, interrupting_ka: f64, test_xr: f64) -> Rating {
    Rating { voltage_v, rating_a, interrupting_ka, test_xr, ..Default::default() }
}

#[test]
fn one_matching_interrupting_rating_is_copied() {
    let mut device = sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])]);
    device.ratings = vec![
        duty_rating(240.0, 100.0, 65.0, 4.9),
        duty_rating(480.0, 100.0, 42.0, 4.9),
    ];
    let mut request = req(7);
    request.voltage_v = Some(480.0);
    let applied = apply_device(&lib(vec![device]), &request).unwrap();
    let rating = applied.interrupting.unwrap();
    assert_eq!(rating.symmetrical_ka, 42.0);
    assert_eq!(rating.test_xr, 4.9);
    assert_eq!(rating.voltage_v, 480.0);
}

#[test]
fn disagreeing_interrupting_ratings_are_not_copied() {
    let mut device = sample_device("LV Fuse", vec![unit(100.0, vec![slow_then_fast()])]);
    device.ratings = vec![
        duty_rating(240.0, 100.0, 65.0, 4.9),
        duty_rating(480.0, 100.0, 42.0, 4.9),
    ];
    let applied = apply_device(&lib(vec![device]), &req(7)).unwrap();
    assert!(applied.interrupting.is_none());
    let notes = &applied.library.unwrap().unsupported_elements;
    assert!(notes.iter().any(|note| note.starts_with("interrupting rating not selected:")), "{notes:?}");
}
