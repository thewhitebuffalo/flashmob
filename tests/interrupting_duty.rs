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
    InterruptingRating { symmetrical_ka, test_xr, voltage_v, contact_parting_cycles: None, voltage_range_factor: None }
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
    project.sources.push(Source { machine_kind: None, transient_mva_sc: None,
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
    assert!(!early_row.local_ac_applied);
}

#[test]
fn entered_generator_decrement_replaces_the_remote_multiplier_when_nacd_is_zero() {
    let mut high = rating(0.50, 80.0, 13800.0);
    high.contact_parting_cycles = Some(3.0);
    let mut project = one_bus(80.0, None, None, high);
    project.buses[0].kv = 13.8;
    project.sources[0].decrement_curve = vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.05, current_ratio: 0.70 },
    ];
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    assert!(row.local_ac_applied);
    let nacd = row.nacd.expect("nacd");
    assert!(nacd < 1e-6, "{nacd}");
    let ac = 0.70;
    let dc = 2.0 * (-4.0 * std::f64::consts::PI * 3.0 / 80.0).exp();
    let local = (ac * ac + dc).sqrt() / 1.1;
    let remote = (1.0 + dc).sqrt() / 1.1;
    close(row.remote_multiplying_factor.expect("factor"), local);
    assert!(local < remote - 0.1, "local {local} remote {remote}");
    assert!(row.note.contains("60 Hz"), "{}", row.note);
    assert!(row.note.contains("entered generator"), "{}", row.note);
    assert!(row.note.contains("not a lookup"), "{}", row.note);
    let warning = out.warnings.iter().find(|line| line.contains("interrupting multiplier")).expect("blended warning");
    assert!(warning.contains("Entered generator AC decrement is included"), "{warning}");
    let report = study::text_report(&project, &out);
    assert!(report.contains("Interrupting multiplier"));
    assert!(report.contains("entered generator decrement at 60 Hz"));
}

#[test]
fn nacd_blends_a_remote_source_with_an_entered_generator_decrement() {
    let mut high = rating(0.50, 80.0, 13800.0);
    high.contact_parting_cycles = Some(3.0);
    let mut project = one_bus(80.0, None, None, high);
    project.buses[0].kv = 13.8;
    project.sources.push(Source { machine_kind: None, transient_mva_sc: None,
        id: "gen".into(),
        name: "Generator".into(),
        bus: "bus".into(),
        in_service: true,
        v_pu: 1.0,
        angle_deg: 0.0,
        mva_sc: 10.0,
        decrement_curve: vec![
            DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
            DecrementPoint { time_s: 0.05, current_ratio: 0.70 },
        ],
        xr: 80.0,
        x0_over_x1: 1.0,
        r0_over_r1: 1.0,
        is_slack: false,
        p_mw: 0.0,
        qmin_mvar: None,
        qmax_mvar: None,
        defaulted: Vec::new(),
    });
    assert!(project.validate().is_empty(), "{:?}", project.validate());
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    assert!(row.local_ac_applied);
    let nacd = row.nacd.expect("nacd");
    assert!((nacd - 0.5).abs() < 1e-6, "{nacd}");
    let xr = row.system_xr.expect("system X/R");
    assert!((xr - 80.0).abs() < 1e-6, "{xr}");
    let dc = 2.0 * (-4.0 * std::f64::consts::PI * 3.0 / xr).exp();
    let remote = (1.0 + dc).sqrt() / 1.1;
    let local = (0.70 * 0.70 + dc).sqrt() / 1.1;
    close(row.remote_multiplying_factor.expect("factor"), 0.5 * remote + 0.5 * local);
}

#[test]
fn close_and_latch_applies_above_one_kilovolt_and_uses_k() {
    let mut high = rating(0.20, 15.0, 13800.0);
    high.voltage_range_factor = Some(2.27);
    let mut project = one_bus(15.0, None, None, high);
    project.buses[0].kv = 13.8;
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    let duty_ka = 1.6 * row.symmetrical_ka;
    let capability = 1.6 * 2.27 * 0.20;
    close(row.close_latch_ka.expect("close-latch duty"), duty_ka);
    close(row.close_latch_capability_ka.expect("close-latch capability"), capability);
    assert_eq!(row.voltage_range_factor, Some(2.27));
    assert_eq!(row.status, "exceeds_rating");
    assert_eq!(row.close_latch_status.as_deref(), Some("marginal"));
    assert!(row.note.contains("does not replace the half-cycle comparison"), "{}", row.note);
    let warning = out.warnings.iter().find(|line| line.contains("close-and-latch")).expect("close-latch warning");
    assert!(warning.contains("above 90%"), "{warning}");
    let report = study::text_report(&project, &out);
    assert!(report.contains("Closing and latching"));
    assert!(report.contains("first-cycle machine reactances"));

    let mut low = rating(0.20, 15.0, 480.0);
    low.voltage_range_factor = Some(2.27);
    let low_out = study::run(&one_bus(15.0, None, None, low), fault_only()).unwrap();
    let low_row = device_row(&low_out);
    assert!(low_row.close_latch_status.is_none());
    assert!(low_row.note.contains("not applied at or below 1 kV"), "{}", low_row.note);
    assert!(!low_out.warnings.iter().any(|line| line.contains("close-and-latch")));

    let mut plain = one_bus(15.0, None, None, rating(0.20, 15.0, 13800.0));
    plain.buses[0].kv = 13.8;
    let plain_out = study::run(&plain, fault_only()).unwrap();
    let plain_row = device_row(&plain_out);
    assert_eq!(plain_row.voltage_range_factor, Some(1.0));
    close(plain_row.close_latch_capability_ka.expect("capability"), 1.6 * 0.20);

    let mut bad = one_bus(15.0, None, None, rating(10.0, 15.0, 13800.0));
    bad.devices[0].interrupting.as_mut().unwrap().voltage_range_factor = Some(0.5);
    assert!(bad.validate().iter().any(|error| error.contains("voltage range factor")), "{:?}", bad.validate());
}

#[test]
fn motor_contribution_does_not_reduce_the_remote_multiplier() {
    let mut high = rating(0.50, 80.0, 13800.0);
    high.contact_parting_cycles = Some(3.0);
    let mut project = one_bus(80.0, None, None, high);
    project.buses[0].kv = 13.8;
    project.motors.push(Motor { machine_kind: None, synchronous_rpm: None, phases: None,
        id: "m".into(),
        name: "M".into(),
        bus: "bus".into(),
        hp: 1000.0,
        kv: 13.8,
        pf: 0.85,
        efficiency: 0.94,
        x_subtransient_pu: 0.17,
        xr: 6.0,
        defaulted: Vec::new(),
    });
    assert!(project.validate().is_empty(), "{:?}", project.validate());
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    assert!(!row.local_ac_applied);
    let nacd = row.nacd.expect("nacd");
    assert!(nacd < 0.99 && nacd > 0.5, "{nacd}");
    let factor = duty::remote_multiplying_factor(row.system_xr, 3.0).unwrap();
    close(row.remote_multiplying_factor.expect("factor"), factor);
    assert!(row.note.contains("Generator AC decrement was not entered"), "{}", row.note);
    assert!(row.note.contains("Local AC-decrement curves are not applied"), "{}", row.note);
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

fn medium_voltage_motor_bus(kind: Option<MotorKind>, hp: f64, rpm: Option<f64>, phases: Option<u8>) -> Project {
    let mut high = rating(0.50, 15.0, 13800.0);
    high.contact_parting_cycles = Some(3.0);
    let mut project = one_bus(15.0, Some(1.0), Some(15.0), high);
    project.buses[0].kv = 13.8;
    project.motors.push(Motor {
        id: "m".into(),
        name: "M".into(),
        bus: "bus".into(),
        hp,
        kv: 13.8,
        pf: 0.85,
        efficiency: 0.94,
        x_subtransient_pu: 0.17,
        xr: 15.0,
        machine_kind: kind,
        synchronous_rpm: rpm,
        phases,
        defaulted: Vec::new(),
    });
    assert!(project.validate().is_empty(), "{:?}", project.validate());
    project
}

/// Interrupting-network symmetrical current behind the reported multiplier.
fn interrupting_ka(row: &study::InterruptingDuty) -> f64 {
    row.multiplied_symmetrical_ka.expect("multiplied") / row.remote_multiplying_factor.expect("factor")
}

#[test]
fn classified_motor_duties_use_separate_first_cycle_and_interrupting_reactance() {
    let base = study::run(&medium_voltage_motor_bus(None, 400.0, None, None), fault_only()).unwrap();
    let base_row = device_row(&base);
    // Unclassified: same current for both duties, reported as unscaled.
    close(interrupting_ka(base_row), base_row.symmetrical_ka);
    assert!(base.warnings.iter().any(|w| w.contains("machine_kind is absent")), "{:?}", base.warnings);
    let source_only = {
        let mut p = medium_voltage_motor_bus(None, 400.0, None, None);
        p.motors.clear();
        device_row(&study::run(&p, fault_only()).unwrap()).symmetrical_ka
    };
    let motor_ka = base_row.symmetrical_ka - source_only;
    assert!(motor_ka > 0.0);

    for (kind, hp, rpm, first, interrupting) in [
        (MotorKind::Synchronous, 400.0, None, 1.0, 1.5),
        (MotorKind::Induction, 400.0, Some(1800.0), 1.2, 3.0),
        (MotorKind::Induction, 400.0, Some(3600.0), 1.0, 1.5),
    ] {
        let out = study::run(&medium_voltage_motor_bus(Some(kind), hp, rpm, Some(3)), fault_only()).unwrap();
        let row = device_row(&out);
        let first_ka = row.symmetrical_ka;
        let int_ka = interrupting_ka(row);
        assert!(int_ka < first_ka - 1e-6, "{kind:?} {rpm:?}: {int_ka} vs {first_ka}");
        // A purely reactive-ratio-preserving scale on a same-X/R machine adds
        // its contribution divided by the multiplier (same X/R as the source).
        close(first_ka - source_only, motor_ka / first);
        close(int_ka - source_only, motor_ka / interrupting);
        close(row.close_latch_ka.unwrap(), 1.6 * first_ka);
        let bracing = bracing_row(&out);
        close(bracing.symmetrical_ka, first_ka);
        assert!(row.note.contains("interrupting machine reactances"), "{}", row.note);
        assert!(!out.warnings.iter().any(|w| w.contains("machine_kind is absent")));
    }
}

#[test]
fn small_and_single_phase_motors_drop_out_of_both_duty_networks() {
    let mut p = medium_voltage_motor_bus(None, 400.0, None, None);
    p.motors.clear();
    let source_only = device_row(&study::run(&p, fault_only()).unwrap()).symmetrical_ka;
    for project in [
        medium_voltage_motor_bus(Some(MotorKind::Induction), 40.0, Some(1800.0), Some(3)),
        medium_voltage_motor_bus(Some(MotorKind::Induction), 400.0, Some(1800.0), Some(1)),
    ] {
        let out = study::run(&project, fault_only()).unwrap();
        let row = device_row(&out);
        close(row.symmetrical_ka, source_only);
        close(interrupting_ka(row), source_only);
        // The main fault study still carries the motor.
        let bus = &out.fault.as_ref().unwrap().buses[0];
        assert!(bus.three_phase.as_ref().unwrap().symmetrical_ka > source_only + 1e-6);
    }
}

#[test]
fn induction_motor_without_speed_takes_the_larger_class_and_warns() {
    let out = study::run(&medium_voltage_motor_bus(Some(MotorKind::Induction), 600.0, None, None), fault_only()).unwrap();
    let with_speed = study::run(&medium_voltage_motor_bus(Some(MotorKind::Induction), 600.0, Some(3600.0), None), fault_only()).unwrap();
    close(device_row(&out).symmetrical_ka, device_row(&with_speed).symmetrical_ka);
    assert!(out.warnings.iter().any(|w| w.contains("synchronous_rpm is absent")), "{:?}", out.warnings);
    assert!(!with_speed.warnings.iter().any(|w| w.contains("synchronous_rpm is absent")));
}

#[test]
fn machine_fields_load_from_json_and_exec() {
    let response = flashmob::exec::exec_request(r#"{
        "commands": [
            {"op": "add_bus", "id": "b", "kv": 13.8},
            {"op": "add_source", "id": "s", "bus": "b", "mva_sc": 100.0, "xr": 15.0, "is_slack": true, "machine_kind": "utility"},
            {"op": "add_source", "id": "g", "bus": "b", "mva_sc": 60.0, "xr": 30.0, "machine_kind": "hydro_without_amortisseur"},
            {"op": "set_source", "id": "g", "transient_mva_sc": 40.0},
            {"op": "add_motor", "id": "m", "bus": "b", "hp": 2000, "kv": 13.2, "machine_kind": "induction", "synchronous_rpm": 1200, "phases": 3}
        ]
    }"#);
    assert!(response.ok, "{:?}", response.error);
    let loaded = response.project;
    assert_eq!(loaded.motors[0].machine_kind, Some(MotorKind::Induction));
    assert_eq!(loaded.motors[0].synchronous_rpm, Some(1200.0));
    assert_eq!(loaded.sources[1].machine_kind, Some(GeneratorKind::HydroWithoutAmortisseur));
    assert_eq!(loaded.sources[1].transient_mva_sc, Some(40.0));
    let text = serde_json::to_string(&loaded).unwrap();
    let loaded: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(loaded.motors[0].phases, Some(3));
    let mut bad = loaded.clone();
    bad.motors[0].phases = Some(2);
    assert!(bad.validate().iter().any(|e| e.contains("phases")));
    // Omitted fields are not written back out.
    let plain = serde_json::to_string(&Project::sample()).unwrap();
    assert!(!plain.contains("machine_kind") && !plain.contains("synchronous_rpm") && !plain.contains("transient_mva_sc"));
}

/// Two buses, each with a source of very different X/R, joined by a
/// resistive line, so the complex Thevenin X/R and the separate-network X/R
/// at bus "bus" differ.
fn mixed_xr_project() -> (Project, f64) {
    let mut project = one_bus(20.0, Some(1.0), Some(15.0), rating(0.5, 15.0, 480.0));
    project.buses.push(Bus { id: "far".into(), name: "Far".into(), kv: 0.48, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None });
    let mut far = project.sources[0].clone();
    far.id = "far_src".into();
    far.bus = "far".into();
    far.xr = 1.0;
    far.is_slack = false;
    project.sources.push(far);
    project.branches.push(Branch {
        id: "tie".into(), name: "Tie".into(), from: "bus".into(), to: "far".into(),
        kind: BranchKind::Line { r_ohm: 0.0002, x_ohm: 0.0001, b_siemens: 0.0, r0_ohm: 0.0006, x0_ohm: 0.0003, ampacity_a: None },
    });
    assert!(project.validate().is_empty(), "{:?}", project.validate());
    // Hand reduction in per unit on the 100 MVA, 0.48 kV base.
    let zb = 0.48 * 0.48 / project.s_base_mva;
    let part = |mag: f64, xr: f64| { let r = mag / (1.0 + xr * xr).sqrt(); (r, r * xr) };
    let (ra, xa) = part(project.s_base_mva / 10.0, 20.0);
    let (rb, xb) = part(project.s_base_mva / 10.0, 1.0);
    let (rl, xl) = (0.0002 / zb, 0.0001 / zb);
    let parallel = |a: f64, b: f64| a * b / (a + b);
    let want = parallel(xa, xb + xl) / parallel(ra, rb + rl);
    (project, want)
}

#[test]
fn separate_network_xr_differs_from_the_complex_thevenin_xr() {
    let (project, want) = mixed_xr_project();
    let separate = flashmob::fault::separate_xr(&project).unwrap();
    let at_bus = separate.iter().find(|row| row.bus_id == "bus").unwrap();
    close(at_bus.three_phase.unwrap().unwrap(), want);
    let out = study::run(&project, fault_only()).unwrap();
    let complex = out.fault.as_ref().unwrap().buses.iter().find(|b| b.id == "bus").unwrap().three_phase.as_ref().unwrap().x_over_r.unwrap();
    assert!((complex / want - 1.0).abs() > 0.01, "complex {complex} separate {want}");
    let row = device_row(&out);
    assert_eq!(row.fault, "three_phase");
    close(row.system_xr.unwrap(), want);
    close(row.half_cycle_rms_ka, row.symmetrical_ka * half_cycle_factor(Some(want)).unwrap());
    // The duty symmetrical current is still the complex-network current.
    let bus_sym = out.fault.as_ref().unwrap().buses.iter().find(|b| b.id == "bus").unwrap().three_phase.as_ref().unwrap().symmetrical_ka;
    close(row.symmetrical_ka, bus_sym);
    let bracing = bracing_row(&out);
    if bracing.fault == "three_phase" { close(bracing.system_xr.unwrap(), want); }
    assert!(row.note.contains("separate resistance and reactance networks"), "{}", row.note);
    // A single source gives the same X/R either way.
    let single = one_bus(12.0, None, None, rating(0.5, 15.0, 480.0));
    close(device_row(&study::run(&single, fault_only()).unwrap()).system_xr.unwrap(), 12.0);
}

#[test]
fn line_to_ground_separate_xr_sums_the_sequence_parts() {
    let (project, _) = mixed_xr_project();
    let separate = flashmob::fault::separate_xr(&project).unwrap();
    let lg = separate.iter().find(|row| row.bus_id == "bus").unwrap().line_to_ground.unwrap().unwrap();
    let three = separate.iter().find(|row| row.bus_id == "bus").unwrap().three_phase.unwrap().unwrap();
    assert!(lg.is_finite() && lg > 0.0);
    assert!((lg - three).abs() > 1e-6, "zero-sequence line R0/X0 should move the ground-fault X/R");
}

#[test]
fn separate_ratio_edge_cases() {
    use flashmob::fault::separate_ratio;
    assert_eq!(separate_ratio(5.0, 0.0).unwrap(), None);
    assert_eq!(separate_ratio(5.0, 5.0e-5).unwrap(), None);
    assert_eq!(separate_ratio(0.0, 3.0).unwrap(), Some(0.0));
    assert_eq!(separate_ratio(3.0e-5, 3.0).unwrap(), Some(0.0));
    close(separate_ratio(8.0, 2.0).unwrap().unwrap(), 4.0);
    assert!(separate_ratio(0.0, 0.0).is_err());
    assert!(separate_ratio(-1.0, 1.0).is_err());
    assert!(separate_ratio(1.0, f64::NAN).is_err());
}

#[test]
fn nearly_pure_reactance_and_resistance_sources_reach_the_limits() {
    let reactive = one_bus(1.0e7, None, None, rating(0.5, 15.0, 480.0));
    let row = flashmob::fault::separate_xr(&reactive).unwrap().remove(0);
    assert_eq!(row.three_phase, Some(None), "{row:?}");
    let resistive = one_bus(1.0e-7, None, None, rating(0.5, 15.0, 480.0));
    let row = flashmob::fault::separate_xr(&resistive).unwrap().remove(0);
    assert_eq!(row.three_phase, Some(Some(0.0)), "{row:?}");
    // Duty rows use those limits: no resistance gives the sqrt(3) half-cycle factor.
    let out = study::run(&reactive, fault_only()).unwrap();
    let duty = device_row(&out);
    assert_eq!(duty.system_xr, None);
    close(duty.half_cycle_rms_ka, duty.symmetrical_ka * 3f64.sqrt());
}

#[test]
fn first_cycle_and_interrupting_separate_xr_follow_the_machine_multipliers() {
    let mut project = medium_voltage_motor_bus(Some(MotorKind::Induction), 400.0, Some(1800.0), Some(3));
    project.motors[0].xr = 4.0;
    let out = study::run(&project, fault_only()).unwrap();
    let row = device_row(&out);
    let expect = |duty| flashmob::fault::separate_xr(&flashmob::machine::duty_project(&project, duty).unwrap()).unwrap()[0].three_phase.unwrap().unwrap();
    let first = expect(flashmob::machine::Duty::FirstCycle);
    let interrupting = expect(flashmob::machine::Duty::Interrupting);
    close(row.system_xr.unwrap(), first);
    // A weaker low-X/R motor in the interrupting network raises the bus X/R.
    assert!(interrupting > first + 1e-6, "{interrupting} vs {first}");
    let factor = duty::remote_multiplying_factor(Some(interrupting), 3.0).unwrap();
    close(row.remote_multiplying_factor.unwrap(), factor);
}
