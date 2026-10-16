use flashmob::model::{FaultMethod, MachineStepDown, MotorKind, Project};
use flashmob::study::{self, Studies};

fn fault_ka(project: &Project) -> f64 {
    let output = study::run(project, Studies { loadflow: false, fault: true, arcflash: false, coordination: false })
        .expect("valid study");
    output.fault.as_ref().unwrap().buses.iter()
        .find(|b| b.id == "mcc").unwrap()
        .three_phase.as_ref().unwrap().symmetrical_ka
}

#[test]
fn default_fault_method_is_comprehensive() {
    assert_eq!(Project::sample().fault_method, FaultMethod::Comprehensive);
    let text = serde_json::to_string(&Project::sample()).unwrap();
    assert!(!text.contains("fault_method"));
}

#[test]
fn ansi_skips_step_down_cycle_order_validation() {
    let mut p = Project::sample();
    p.fault_method = FaultMethod::Ansi;
    p.machine_step_down = Some(MachineStepDown {
        percent_of_rated: 30.0,
        after_cycles: 1.0, // below the 5-cycle motor window
        apply_to_generators: true,
        apply_to_synchronous_motors: true,
        synchronous_motor_percent_of_rated: None,
    });
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    p.fault_method = FaultMethod::Comprehensive;
    assert!(p.validate().iter().any(|e| e.contains("after_cycles")), "{:?}", p.validate());
}

#[test]
fn ansi_uses_first_cycle_network_and_drops_step_down() {
    let mut base = Project::sample();
    base.motors[0].machine_kind = Some(MotorKind::Induction);
    base.motors[0].hp = 200.0; // medium induction: 1.2 first-cycle factor
    base.motors[0].synchronous_rpm = Some(1800.0);
    let comprehensive = fault_ka(&base);

    let mut ansi = base.clone();
    ansi.fault_method = FaultMethod::Ansi;
    ansi.machine_step_down = Some(MachineStepDown {
        percent_of_rated: 50.0,
        after_cycles: 10.0,
        apply_to_generators: true,
        apply_to_synchronous_motors: true,
        synchronous_motor_percent_of_rated: None,
    });
    let output = study::run(&ansi, Studies { loadflow: false, fault: true, arcflash: true, coordination: false }).unwrap();
    assert!(output.warnings.iter().any(|w| w.contains("first-cycle") && w.contains("step-down")), "{:?}", output.warnings);
    let ansi_ka = output.fault.as_ref().unwrap().buses.iter()
        .find(|b| b.id == "mcc").unwrap().three_phase.as_ref().unwrap().symmetrical_ka;
    // Scaling induction X by 1.2 lowers motor contribution, so bolted current falls.
    assert!(ansi_ka < comprehensive - 1e-6, "ansi {ansi_ka} vs comprehensive {comprehensive}");
    let row = output.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == "af-mcc").unwrap();
    assert!(!row.label_reasons.iter().any(|r| r == "machine_step_down_assumed"), "{:?}", row.label_reasons);
    assert!(!row.duration_note.contains("step down"), "{}", row.duration_note);
}
