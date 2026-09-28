use flashmob::model::{Branch, BranchKind, Bus, CurveSpec, DecrementPoint, Device, Project, Switch, SwitchKind};
use flashmob::study::{self, Studies, StudyOutput};

fn run_arc(project: &Project) -> StudyOutput {
    study::run(project, Studies {
        loadflow: false,
        fault: true,
        arcflash: true,
        coordination: false,
    }).expect("valid motor-backfeed study")
}

fn arc_failure<'a>(output: &'a StudyOutput, equipment_id: &str) -> &'a str {
    assert!(output.arc_flash.as_ref().unwrap().iter().all(|row| row.equipment_id != equipment_id),
        "{equipment_id} must not have a completed arc-flash row");
    &output.arc_flash_failures.iter()
        .find(|failure| failure.equipment_id == equipment_id)
        .expect("missing explicit arc-flash failure")
        .error
}

fn mcc_bolted_ka(output: &StudyOutput) -> f64 {
    output.fault.as_ref().unwrap().buses.iter()
        .find(|bus| bus.id == "mcc").unwrap()
        .three_phase.as_ref().unwrap().symmetrical_ka
}

#[test]
fn sample_main_cannot_clear_motor_backfeed_but_panel_feeder_can() {
    let output = run_arc(&Project::sample());
    let error = arc_failure(&output, "af-mcc");
    assert!(error.to_lowercase().contains("motor") && error.contains("m1"), "{error}");

    assert!(output.arc_flash.as_ref().unwrap().iter().any(|row| row.equipment_id == "af-pnl"),
        "opening the panel feeder isolates its fault from the utility and MCC motor");
    assert!(output.arc_flash_failures.iter().all(|failure| failure.equipment_id != "af-pnl"));
}

#[test]
fn source_decrement_does_not_erase_connected_motor_backfeed() {
    let mut project = Project::sample();
    project.sources[0].decrement_curve = vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.2, current_ratio: 0.5 },
    ];
    let output = run_arc(&project);
    let error = arc_failure(&output, "af-mcc");
    assert!(error.contains("motor(s) m1 connected"), "{error}");
}

#[test]
fn motor_feeder_breaker_only_removes_backfeed_when_its_contact_is_open() {
    let mut project = Project::sample();
    project.equipment.truncate(1);
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    project.buses.push(Bus {
        id: "motor-bus".into(), name: "Motor feeder load end".into(), kv: 0.48,
        shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None,
    });
    project.branches.push(Branch {
        id: "motor-feeder".into(), name: "Motor feeder".into(),
        from: "mcc".into(), to: "motor-bus".into(),
        kind: BranchKind::Line {
            r_ohm: 0.003, x_ohm: 0.005, b_siemens: 0.0,
            r0_ohm: 0.009, x0_ohm: 0.015, ampacity_a: Some(400.0),
        },
    });
    project.motors[0].bus = "motor-bus".into();
    project.switches.push(Switch {
        id: "motor-breaker-contact".into(), name: "Motor feeder breaker".into(),
        branch_id: "motor-feeder".into(), kind: SwitchKind::Breaker, closed: true,
    });
    project.devices.push(Device {
        id: "brk-motor".into(), name: "Motor feeder trip unit".into(),
        bus: "mcc".into(), protected_branch: Some("motor-feeder".into()),
        curve: CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 },
        breaker_interrupting_s: Some(0.04), fuse_total_clearing: false,
        basis: "collected".into(),
    });

    let closed = run_arc(&project);
    let error = arc_failure(&closed, "af-mcc");
    assert!(error.to_lowercase().contains("motor") && error.contains("m1"), "{error}");

    project.switches[0].closed = false;
    let open = run_arc(&project);
    assert!(open.arc_flash.as_ref().unwrap().iter().any(|row| row.equipment_id == "af-mcc"),
        "the main breaker can clear the MCC fault when the motor feeder is already open");
    assert!(open.arc_flash_failures.iter().all(|failure| failure.equipment_id != "af-mcc"));
    assert!(mcc_bolted_ka(&closed) > mcc_bolted_ka(&open),
        "the closed motor feeder must actually contribute fault current");
}
