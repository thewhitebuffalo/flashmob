mod common;

use common::*;
use flashmob::model::{BranchKind, CurveSpec, MachineStepDown, MotorKind, Prefault, Project};
use flashmob::study::{self, ArcRow, Studies, StudyOutput};

fn run(project: &Project) -> StudyOutput {
    study::run(project, Studies { loadflow: false, fault: true, arcflash: true, coordination: false }).expect("valid study")
}

fn row(output: &StudyOutput, id: &str) -> ArcRow {
    output.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == id)
        .unwrap_or_else(|| panic!("{id} missing: {:?}", output.arc_flash_failures)).clone()
}

/// MCC fed by two lines: a strong utility through a fast breaker and a weak
/// source through a slow breaker.
fn two_feeds() -> Project {
    let mut p = Project::sample();
    p.motors.clear(); p.loads.clear(); p.equipment.truncate(1);
    p.equipment[0].upstream_device = None;
    for b in &mut p.buses { b.kv = 0.48; }
    for (j, b) in p.branches.iter_mut().enumerate() {
        b.from = if j == 0 { "util".into() } else { "pnl".into() };
        b.to = "mcc".into();
        b.kind = BranchKind::Line { r_ohm: 0.001, x_ohm: 0.004, b_siemens: 0.0, r0_ohm: 0.003, x0_ohm: 0.012, ampacity_a: None };
    }
    p.sources[0].mva_sc = 40.0;
    let mut weak = p.sources[0].clone();
    weak.id = "weak".into(); weak.bus = "pnl".into(); weak.is_slack = false; weak.mva_sc = 4.0;
    p.sources.push(weak);
    let template = p.devices[0].clone();
    p.devices.clear();
    for (id, branch, time_s) in [("fast", "t1", 0.05), ("slow", "feeder", 0.5)] {
        let mut d = template.clone();
        d.id = id.into(); d.name = id.into(); d.bus = "mcc".into(); d.protected_branch = Some(branch.into());
        d.curve = CurveSpec::Definite { pickup_a: 1.0, time_s }; d.breaker_interrupting_s = Some(0.03);
        p.devices.push(d);
    }
    p
}

#[test]
fn threshold_extinguishes_the_arc_and_names_the_crossing_device() {
    let p = two_feeds();
    let r = row(&run(&p), "af-mcc");
    assert!((r.time_s - 0.08).abs() < 1e-9, "{}: {}", r.time_s, r.duration_note);
    assert_eq!(r.governing_device.as_deref(), Some("fast"));
    assert!(r.label_reasons.iter().any(|c| c == "remaining_current_excluded"), "{:?}", r.label_reasons);
    assert!(r.duration_note.contains("excluded"), "{}", r.duration_note);

    let mut all = p.clone();
    all.cleared_fault_fraction = 1.0;
    let r_all = row(&run(&all), "af-mcc");
    assert!((r_all.time_s - 0.53).abs() < 1e-9, "{}", r_all.time_s);
    assert_eq!(r_all.governing_device.as_deref(), Some("slow"));
    assert!(!r_all.label_reasons.iter().any(|c| c == "remaining_current_excluded"));
    assert!(r_all.governing_cal_cm2 > r.governing_cal_cm2);
}

#[test]
fn default_sample_keeps_its_clearing_and_reports_the_main_breaker() {
    let p = Project::sample();
    assert_eq!(p.cleared_fault_fraction, 0.80);
    assert!(!p.retime_on_reduced_current && p.machine_step_down.is_none());
    let r = row(&run(&p), "af-mcc");
    assert_eq!(r.governing_device.as_deref(), Some("brk-main"));
    assert!(!r.label_reasons.iter().any(|c| c == "remaining_current_excluded"), "{:?}", r.label_reasons);
}

#[test]
fn synchronous_motor_step_down_lowers_energy_and_is_validated() {
    let mut p = Project::sample();
    p.motors[0].machine_kind = Some(MotorKind::Synchronous);
    let held = row(&run(&p), "af-mcc");
    // The synchronous motor has no feeder device; the main breaker clears the
    // utility share and its remaining current is excluded.
    assert!(held.label_reasons.iter().any(|c| c == "remaining_current_excluded"), "{:?}", held.label_reasons);
    p.machine_step_down = Some(MachineStepDown { percent_of_rated: 0.0, after_cycles: 6.0 });
    let stepped = row(&run(&p), "af-mcc");
    assert!(stepped.label_reasons.iter().any(|c| c == "machine_step_down_assumed"));
    assert!(stepped.governing_cal_cm2 < held.governing_cal_cm2, "{} vs {}", stepped.governing_cal_cm2, held.governing_cal_cm2);
    p.machine_step_down = Some(MachineStepDown { percent_of_rated: 50.0, after_cycles: 4.0 });
    assert!(p.validate().iter().any(|e| e.contains("after_cycles")));
}

#[test]
fn prefault_choices_scale_the_one_solution() {
    let flat = run(&Project::sample());
    let ka = |out: &StudyOutput, bus: &str| out.fault.as_ref().unwrap().buses.iter().find(|b| b.id == bus).unwrap().three_phase.as_ref().unwrap().symmetrical_ka;
    let mut single = Project::sample();
    single.prefault_pu = 1.05;
    let high = run(&single);
    assert!((ka(&high, "mcc") / ka(&flat, "mcc") - 1.05).abs() < 1e-9);

    let mut tapped = Project::sample();
    tapped.prefault = Prefault::NoLoadTap;
    if let BranchKind::Transformer { tap_percent, .. } = &mut tapped.branches[0].kind { *tap_percent = 5.0; }
    let out = run(&tapped);
    let mcc = out.fault.as_ref().unwrap().buses.iter().find(|b| b.id == "mcc").unwrap();
    assert_eq!(out.fault.as_ref().unwrap().prefault, "no_load_tap");
    assert!((mcc.prefault_pu - 1.0 / 1.05).abs() < 1e-9, "{}", mcc.prefault_pu);

    let mut per_bus = Project::sample();
    per_bus.prefault = Prefault::PerBus;
    assert!(per_bus.validate().iter().any(|e| e.contains("prefault_pu")));
    for b in &mut per_bus.buses { b.prefault_pu = Some(0.98); }
    let out = run(&per_bus);
    assert!((ka(&out, "mcc") / ka(&flat, "mcc") - 0.98).abs() < 1e-9);
}

#[test]
fn bus_with_zero_fault_current_is_reported_out_of_service() {
    let mut p = Project::sample();
    p.buses.push(bus("dead", 0.48));
    let mut eq = p.equipment[0].clone();
    eq.id = "af-dead".into(); eq.bus = "dead".into(); eq.upstream_device = None;
    p.equipment.push(eq);
    let out = run(&p);
    let failure = out.arc_flash_failures.iter().find(|f| f.equipment_id == "af-dead").expect("dead bus fails");
    assert!(failure.error.contains("out of service"), "{}", failure.error);
}
