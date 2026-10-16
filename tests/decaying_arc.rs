use flashmob::model::{CurveSpec, DecrementPoint, IeeeKind, Project};
use flashmob::study::{self, ArcRow, Studies};

fn mcc(project: &Project) -> ArcRow {
    let output = study::run(project, Studies { loadflow: false, fault: true, arcflash: true, coordination: false })
        .expect("valid study");
    output.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-mcc")
        .unwrap_or_else(|| panic!("af-mcc row missing: {:?}", output.arc_flash_failures.iter().map(|f| format!("{f:?}")).collect::<Vec<_>>()))
        .clone()
}

fn inverse_main(project: &mut Project) {
    let main = project.devices.iter_mut().find(|device| device.id == "brk-main").unwrap();
    main.curve = CurveSpec::Ieee { kind: IeeeKind::VeryInverse, pickup_a: 4000.0, td: 1.0, inst_a: None, inst_s: None };
}

fn decaying_grid(project: &mut Project) {
    project.sources[0].decrement_curve = vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.2, current_ratio: 0.5 },
    ];
}

#[test]
fn inverse_relay_is_slower_on_a_decaying_current_than_at_the_initial_current() {
    let mut steady = Project::sample();
    inverse_main(&mut steady);
    let steady_row = mcc(&steady);
    assert_eq!(steady_row.clearing_basis, "staged_multi_device", "{}", steady_row.duration_note);

    let mut decaying = steady.clone();
    decaying_grid(&mut decaying);
    // Retiming is off by default: the device keeps its initial-current time.
    let untimed = mcc(&decaying);
    assert!((untimed.time_s - steady_row.time_s).abs() < 1e-9, "{} vs {}", untimed.time_s, steady_row.time_s);
    assert_eq!(untimed.clearing_basis, "staged_multi_device");
    decaying.retime_on_reduced_current = true;
    let row = mcc(&decaying);
    assert_eq!(row.clearing_basis, "staged_decaying_current", "{}", row.duration_note);
    assert!(row.label_reasons.iter().any(|r| r == "staged_decaying_current"));
    assert!(!row.label_ready);
    assert!(row.time_s > steady_row.time_s + 0.005, "decaying {} vs initial {}: {}", row.time_s, steady_row.time_s, row.duration_note);
    assert!(row.duration_note.contains("re-timed"), "{}", row.duration_note);
}

#[test]
fn motor_decay_lowers_energy_without_changing_the_window() {
    let base = mcc(&Project::sample());
    let mut project = Project::sample();
    project.motors[0].decay_time_constant_s = Some(0.03);
    let row = mcc(&project);
    assert_eq!(row.clearing_basis, "staged_multi_device");
    assert!(row.label_reasons.iter().any(|r| r == "motor_decay_assumed"), "{:?}", row.label_reasons);
    assert!((row.time_s - base.time_s).abs() < 1e-9, "{} vs {}", row.time_s, base.time_s);
    assert!(row.governing_cal_cm2 < base.governing_cal_cm2, "{} vs {}", row.governing_cal_cm2, base.governing_cal_cm2);
    assert!(row.duration_note.contains("decay_time_constant_s"), "{}", row.duration_note);
}

#[test]
fn total_clearing_curve_keeps_initial_current_timing_and_is_reported() {
    let mut steady = Project::sample();
    inverse_main(&mut steady);
    let main = steady.devices.iter_mut().find(|device| device.id == "brk-main").unwrap();
    main.curve_is_total_clearing = true;
    main.breaker_interrupting_s = None;
    let steady_row = mcc(&steady);
    let mut decaying = steady.clone();
    decaying_grid(&mut decaying);
    decaying.retime_on_reduced_current = true;
    let row = mcc(&decaying);
    assert!(row.label_reasons.iter().any(|r| r == "static_device_timing"), "{:?}", row.label_reasons);
    assert!((row.time_s - steady_row.time_s).abs() < 1e-9, "{} vs {}", row.time_s, steady_row.time_s);
    assert!(row.duration_note.contains("does not by itself establish clearing"), "{}", row.duration_note);
}

#[test]
fn motor_decay_time_constant_round_trips_and_is_validated() {
    let mut project = Project::sample();
    project.motors[0].decay_time_constant_s = Some(0.05);
    let text = serde_json::to_string(&project).unwrap();
    let back: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(back.motors[0].decay_time_constant_s, Some(0.05));
    assert!(!serde_json::to_string(&Project::sample()).unwrap().contains("decay_time_constant_s"));
    project.motors[0].decay_time_constant_s = Some(-1.0);
    assert!(project.validate().iter().any(|e| e.contains("decay_time_constant_s")));
}
