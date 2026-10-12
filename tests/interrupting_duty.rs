//! Interrupting duty against a rating's test X/R. Built from code, not a client project.

use flashmob::duty::{self, DutyInput, DutyStatus};
use flashmob::fault::half_cycle_factor;
use flashmob::model::*;
use flashmob::study::{self, Studies};

fn close(got: f64, want: f64) {
    let scale = got.abs().max(want.abs()).max(1.0);
    assert!((got - want).abs() <= 1e-8 * scale, "{got} vs {want}");
}

fn fault_only() -> Studies {
    Studies { loadflow: false, fault: true, arcflash: false, coordination: false }
}

fn rating(symmetrical_ka: f64, test_xr: f64, voltage_v: f64) -> InterruptingRating {
    InterruptingRating { symmetrical_ka, test_xr, voltage_v, contact_parting_cycles: None }
}

fn one_bus(source_xr: f64, bracing_ka: Option<f64>, bracing_test_xr: Option<f64>, interrupting: InterruptingRating) -> Project {
    let mut project = Project::default();
    project.name = "Duty".into();
    project.buses.push(Bus {
        id: "bus".into(),
        name: "Bus".into(),
        kv: 0.48,
        shunt_kvar: 0.0,
        bracing_ka,
        main_rating_a: None,
        bracing_test_xr,
        equipment_class: None,
    });
    project.sources.push(Source {
        id: "src".into(),
        name: "Source".into(),
        bus: "bus".into(),
        in_service: true,
        v_pu: 1.0,
        angle_deg: 0.0,
        mva_sc: 10.0,
        decrement_curve: Vec::new(),
        xr: source_xr,
        x0_over_x1: 1.0,
        r0_over_r1: 1.0,
        is_slack: true,
        p_mw: 0.0,
        qmin_mvar: None,
        qmax_mvar: None,
        defaulted: Vec::new(),
    });
    project.devices.push(Device {
        id: "brk".into(),
        name: "Breaker".into(),
        bus: "bus".into(),
        curve: CurveSpec::Definite { pickup_a: 100.0, time_s: 0.1 },
        protected_branch: None,
        breaker_interrupting_s: Some(0.05),
        fuse_total_clearing: false,
        curve_is_total_clearing: false,
        basis: String::new(),
        library: None,
        interrupting: Some(interrupting),
        current_limiting: None,
    });
    project
}

fn device_row(out: &study::StudyOutput) -> &study::InterruptingDuty {
    out.interrupting.iter().find(|row| row.kind == "device").expect("device duty")
}

fn bracing_row(out: &study::StudyOutput) -> &study::InterruptingDuty {
    out.interrupting.iter().find(|row| row.kind == "bus_bracing").expect("bracing duty")
}

#[test]
fn half_cycle_factor_matches_the_closed_form() {
    for xr in [0.5, 1.0, 4.9, 8.0, 15.0, 25.0] {
        let want = (1.0 + 2.0 * (-2.0 * std::f64::consts::PI / xr).exp()).sqrt();
        close(half_cycle_factor(Some(xr)).unwrap(), want);
    }
    close(half_cycle_factor(None).unwrap(), 3.0_f64.sqrt());
    close(half_cycle_factor(Some(0.0)).unwrap(), 1.0);
    assert!(half_cycle_factor(Some(-1.0)).is_err());
    assert!(half_cycle_factor(Some(f64::NAN)).is_err());
    assert!(half_cycle_factor(Some(f64::INFINITY)).is_err());
}

#[test]
fn equal_system_and_test_xr_leaves_the_symmetrical_rating_and_a_higher_duty_exceeds() {
    let rated = 5.0;
    let project = one_bus(8.0, None, None, rating(rated, 8.0, 480.0));
    assert!(project.validate().is_empty(), "{:?}", project.validate());
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    assert_eq!(row.status, "exceeds_rating");
    assert_eq!(row.fault, "three_phase");
    close(row.adjusted_symmetrical_ka.unwrap(), rated);
    close(row.rated_half_cycle_rms_ka.unwrap(), rated * half_cycle_factor(Some(8.0)).unwrap());
    let point = out.fault.as_ref().unwrap().buses[0].three_phase.as_ref().unwrap();
    assert!(point.symmetrical_ka > rated);
    close(row.symmetrical_ka, point.symmetrical_ka);
    close(point.half_cycle_rms_ka, point.symmetrical_ka * half_cycle_factor(point.x_over_r).unwrap());
    close(row.half_cycle_rms_ka, row.symmetrical_ka * half_cycle_factor(row.system_xr).unwrap());
    assert!(!row.note.contains("Rating voltage does not match"));
    let warning = out.warnings.iter().find(|w| w.contains("brk")).expect("exceeds warning");
    assert!(warning.contains("exceeds") && warning.contains("test X/R"), "{warning}");
    let report = study::text_report(&project, &out);
    let bus_at = report.find("  Bus\n").expect("bus heading");
    let duty_at = report.find("Interrupting duty").expect("duty line");
    assert!(duty_at > bus_at);
    assert!(report[duty_at..].contains("brk") && report[duty_at..].contains("exceeds_rating"));
}

#[test]
fn higher_system_xr_derates_below_the_nameplate() {
    let rated = 100.0;
    let test_xr = 4.0;
    let project = one_bus(15.0, None, None, rating(rated, test_xr, 4160.0));
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    let adjusted = row.adjusted_symmetrical_ka.unwrap();
    assert!(adjusted < rated, "adjusted {adjusted}, nameplate {rated}");
    let system = row.system_xr.expect("finite system X/R");
    assert!(system > test_xr, "system {system}, test {test_xr}");
    close(adjusted, rated * half_cycle_factor(Some(test_xr)).unwrap() / half_cycle_factor(Some(system)).unwrap());
    assert_eq!(row.status, "within_rating");
    assert!(row.note.contains("Rating voltage does not match the bus nominal voltage."));
    assert!(out.warnings.iter().all(|w| !w.contains("brk")), "within rating is not a warning: {:?}", out.warnings);
}

#[test]
fn infinite_system_xr_uses_sqrt_3() {
    let direct = duty::compare(&DutyInput {
        symmetrical_ka: 10.0,
        system_xr: None,
        rated_symmetrical_ka: 100.0,
        test_xr: Some(15.0),
    }).unwrap();
    close(direct.half_cycle_rms_ka, 10.0 * 3.0_f64.sqrt());
    close(direct.adjusted_symmetrical_ka.unwrap(), 100.0 * half_cycle_factor(Some(15.0)).unwrap() / 3.0_f64.sqrt());
    assert_eq!(direct.status, DutyStatus::WithinRating);

    let project = one_bus(1.0e12, None, None, rating(1000.0, 15.0, 480.0));
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    assert!(row.system_xr.is_none(), "purely reactive source should be infinite X/R, got {:?}", row.system_xr);
    close(row.half_cycle_rms_ka, row.symmetrical_ka * 3.0_f64.sqrt());
    let json = serde_json::to_value(row).unwrap();
    assert!(json["system_xr"].is_null());
    assert_eq!(json["status"], "within_rating");
}

#[test]
fn bracing_without_test_xr_below_the_current_is_not_adjusted() {
    let project = one_bus(8.0, Some(1.0e6), None, rating(1.0e6, 8.0, 480.0));
    let out = study::run(&project, fault_only()).unwrap();
    let row = bracing_row(&out);
    assert_eq!(row.status, "not_adjusted");
    assert_ne!(row.status, "within_rating");
    assert!(row.adjusted_symmetrical_ka.is_none() && row.rated_half_cycle_rms_ka.is_none());
    assert!(row.symmetrical_ka < row.rated_symmetrical_ka);
    let warning = out.warnings.iter().find(|w| w.contains("bus")).expect("not-adjusted warning");
    assert!(warning.contains("without a test X/R") && warning.contains("asymmetrical duty was not checked"), "{warning}");
    assert!(out.warnings.iter().all(|w| !w.contains("within_rating")));
}

#[test]
fn bracing_without_test_xr_above_the_current_exceeds() {
    let project = one_bus(8.0, Some(1.0), None, rating(1.0e6, 8.0, 480.0));
    let out = study::run(&project, fault_only()).unwrap();
    let row = bracing_row(&out);
    assert!(row.symmetrical_ka > row.rated_symmetrical_ka);
    assert_eq!(row.status, "exceeds_rating");
    assert!(row.test_xr.is_none() && row.adjusted_symmetrical_ka.is_none());
    let warning = out.warnings.iter().find(|w| w.contains("bus:") || w.starts_with("bus:")).expect("exceeds warning");
    assert!(warning.contains("exceeds") && warning.contains("rated"), "{warning}");
    assert!(!warning.contains("test X/R"), "{warning}");
}

#[test]
fn arc_flash_only_keeps_interrupting_duty_after_dropping_the_fault() {
    let project = one_bus(8.0, Some(1.0), None, rating(5.0, 8.0, 480.0));
    let out = study::run(&project, Studies { loadflow: false, fault: false, arcflash: true, coordination: false }).unwrap();
    assert!(out.fault.is_none());
    assert!(out.interrupting.iter().any(|row| row.id == "brk" && row.status == "exceeds_rating"));
    assert!(out.interrupting.iter().any(|row| row.kind == "bus_bracing"));
}

#[test]
fn invalid_fault_produces_no_interrupting_rows() {
    let mut project = one_bus(8.0, Some(1.0), Some(8.0), rating(5.0, 8.0, 480.0));
    project.prefault = Prefault::Loadflow;
    project.sources[0].is_slack = false;
    let out = study::run(&project, fault_only()).unwrap();
    assert!(!out.fault.as_ref().unwrap().valid);
    assert!(out.interrupting.is_empty());
}

#[test]
fn protected_branch_uses_the_terminal_current() {
    let mut project = one_bus(8.0, None, None, rating(100.0, 8.0, 480.0));
    project.buses.push(Bus {
        id: "load".into(), name: "Load".into(), kv: 0.48, shunt_kvar: 0.0,
        bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None,
    });
    project.branches.push(Branch {
        id: "feeder".into(), name: "Feeder".into(), from: "bus".into(), to: "load".into(),
        kind: BranchKind::Line {
            r_ohm: 0.02, x_ohm: 0.04, b_siemens: 0.0, r0_ohm: 0.06, x0_ohm: 0.12, ampacity_a: None,
        },
    });
    project.devices[0].protected_branch = Some("feeder".into());
    assert!(project.validate().is_empty(), "{:?}", project.validate());
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    assert!(row.note.contains("bolted three-phase current"));
    assert!(row.note.contains("Line-to-ground current through the device is not stored"));
    assert_eq!(row.fault, "three_phase");
    assert!(!row.note.contains("No protected branch"));
    let fault = out.fault.as_ref().unwrap();
    let at_load = fault.buses.iter().find(|bus| bus.id == "load").unwrap();
    let terminal = at_load.terminal_currents.iter().find(|c| c.branch_id == "feeder").unwrap();
    close(row.symmetrical_ka, terminal.from_a / 1000.0);
    let local = fault.buses.iter().find(|bus| bus.id == "bus").unwrap().three_phase.as_ref().unwrap().symmetrical_ka;
    assert!(row.symmetrical_ka < local * 0.5, "device {} vs bus {}", row.symmetrical_ka, local);
    assert_eq!(row.bus_id, "bus");
}

#[test]
fn remote_multiplier_applies_above_one_kilovolt_and_stays_off_below_it() {
    let mut high = rating(0.50, 80.0, 13800.0);
    high.contact_parting_cycles = Some(3.0);
    let mut project = one_bus(80.0, None, None, high);
    project.buses[0].kv = 13.8;
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    assert_eq!(row.status, "within_rating");
    let factor = row.remote_multiplying_factor.expect("remote factor");
    let asymmetrical = (1.0 + 2.0 * (-4.0 * std::f64::consts::PI * 3.0 / 80.0).exp()).sqrt();
    close(factor, asymmetrical / 1.1);
    close(row.multiplied_symmetrical_ka.unwrap(), row.symmetrical_ka * factor);
    assert_eq!(row.remote_status.as_deref(), Some("exceeds_rating"));
    let nacd = row.nacd.expect("nacd");
    assert!((nacd - 1.0).abs() < 1e-6, "{nacd}");
    let warning = out.warnings.iter().find(|line| line.contains("remote multiplier")).expect("remote warning");
    assert!(warning.contains("Local AC decrement is not applied"), "{warning}");
    let report = study::text_report(&project, &out);
    assert!(report.contains("Remote multiplier"));
    assert!(report.contains("Local AC-decrement curves are not applied"));

    let mut low = rating(0.50, 80.0, 480.0);
    low.contact_parting_cycles = Some(3.0);
    let low_out = study::run(&one_bus(80.0, None, None, low), fault_only()).unwrap();
    let low_row = device_row(&low_out);
    assert!(low_row.remote_status.is_none());
    assert!(low_row.note.contains("not used at or below 1 kV"), "{}", low_row.note);
    assert!(!low_out.warnings.iter().any(|line| line.contains("remote multiplier")));

    let mut early = rating(100.0, 15.0, 13800.0);
    early.contact_parting_cycles = Some(0.5);
    let mut early_project = one_bus(15.0, None, None, early);
    early_project.buses[0].kv = 13.8;
    let early_out = study::run(&early_project, fault_only()).unwrap();
    let early_row = device_row(&early_out);
    assert!(early_row.remote_status.is_none());
    assert!(early_row.note.contains("at least 1 cycle"), "{}", early_row.note);
}

#[test]
fn project_without_the_new_fields_still_validates() {
    let raw = r#"{
        "buses": [{"id": "a", "name": "A", "kv": 0.48}],
        "sources": [{"id": "s", "name": "S", "bus": "a", "mva_sc": 10.0, "xr": 5.0}],
        "devices": [{"id": "d", "name": "D", "bus": "a", "curve": {"type": "definite", "pickup_a": 100.0, "time_s": 0.1}}]
    }"#;
    let project: Project = serde_json::from_str(raw).unwrap();
    assert!(project.validate().is_empty(), "{:?}", project.validate());
    assert!(project.buses.iter().all(|bus| bus.bracing_test_xr.is_none()));
    assert!(project.devices.iter().all(|device| device.interrupting.is_none()));

    let mut value = serde_json::to_value(Project::sample()).unwrap();
    for bus in value["buses"].as_array_mut().unwrap() {
        bus.as_object_mut().unwrap().remove("bracing_test_xr");
    }
    for device in value["devices"].as_array_mut().unwrap() {
        device.as_object_mut().unwrap().remove("interrupting");
    }
    let text = serde_json::to_string(&value).unwrap();
    assert!(!text.contains("bracing_test_xr"));
    let loaded: Project = serde_json::from_str(&text).unwrap();
    assert!(loaded.validate().is_empty(), "{:?}", loaded.validate());
    assert!(loaded.buses.iter().all(|bus| bus.bracing_test_xr.is_none()));
    assert!(loaded.devices.iter().all(|device| device.interrupting.is_none()));
}

#[test]
fn bad_ratings_fail_validation_and_exec_can_set_them() {
    let mut project = one_bus(8.0, Some(10.0), Some(0.0), rating(10.0, 8.0, 480.0));
    assert!(project.validate().iter().any(|error| error.contains("bracing test X/R")));
    project.buses[0].bracing_test_xr = Some(6.0);
    project.devices[0].interrupting.as_mut().unwrap().symmetrical_ka = 0.0;
    assert!(project.validate().iter().any(|error| error.contains("interrupting rating")));
    project.devices[0].interrupting.as_mut().unwrap().symmetrical_ka = 10.0;
    project.devices[0].interrupting.as_mut().unwrap().voltage_v = -1.0;
    assert!(project.validate().iter().any(|error| error.contains("interrupting rating")));
    project.devices[0].interrupting.as_mut().unwrap().voltage_v = 0.0;
    assert!(project.validate().is_empty(), "{:?}", project.validate());

    let response = flashmob::exec::exec_request(r#"{
        "commands": [
            {"op": "add_bus", "id": "b", "kv": 0.48, "bracing_ka": 22.0},
            {"op": "set_bus", "id": "b", "bracing_test_xr": 6.6},
            {"op": "add_source", "id": "s", "bus": "b", "mva_sc": 10.0, "xr": 8.0, "is_slack": true},
            {"op": "add_device", "id": "d", "bus": "b", "curve": {"type": "definite", "pickup_a": 100.0, "time_s": 0.1}, "breaker_interrupting_s": 0.05},
            {"op": "set_device", "id": "d", "interrupting": {"symmetrical_ka": 18.0, "test_xr": 4.9, "voltage_v": 480.0}}
        ]
    }"#);
    assert!(response.ok, "{:?} {:?}", response.error, response.validation_errors);
    assert_eq!(response.project.buses[0].bracing_test_xr, Some(6.6));
    let set = response.project.devices[0].interrupting.clone().unwrap();
    assert_eq!((set.symmetrical_ka, set.test_xr, set.voltage_v), (18.0, 4.9, 480.0));

    let cleared = flashmob::exec::exec_request(&serde_json::json!({
        "project": response.project,
        "commands": [{"op": "set_device", "id": "d", "clear": ["interrupting"]}]
    }).to_string());
    assert!(cleared.ok, "{:?} {:?}", cleared.error, cleared.validation_errors);
    assert!(cleared.project.devices[0].interrupting.is_none());
    assert_eq!(cleared.project.buses[0].bracing_test_xr, Some(6.6));

    let both = flashmob::exec::exec_request(r#"{
        "commands": [
            {"op": "sample"},
            {"op": "set_device", "id": "brk-main", "interrupting": {"symmetrical_ka": 10.0, "test_xr": 5.0, "voltage_v": 480.0}, "clear": ["interrupting"]}
        ]
    }"#);
    assert!(!both.ok);
    assert!(both.error.unwrap_or_default().contains("both sets and clears"));

    let schema = flashmob::exec::schema();
    let duty = schema["interrupting_duty"].as_str().expect("schema key");
    assert!(duty.contains("test X/R") && duty.contains("not_adjusted") && duty.contains("sqrt"));
    assert!(schema["apply_library_device"].as_str().unwrap().starts_with("Copies a library point curve"));
}
