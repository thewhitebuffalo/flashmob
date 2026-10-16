use flashmob::model::{Branch, BranchKind, Bus, CurveSpec, DecrementPoint, Device, Project, Switch, SwitchKind};
use flashmob::arcflash::{calculate, IeeeInput};
use flashmob::study::{self, Studies, StudyOutput};

fn run_arc(project: &Project) -> StudyOutput {
    study::run(project, Studies {
        loadflow: false,
        fault: true,
        arcflash: true,
        coordination: false,
    }).expect("valid motor-backfeed study")
}

fn mcc_bolted_ka(output: &StudyOutput) -> f64 {
    output.fault.as_ref().unwrap().buses.iter()
        .find(|bus| bus.id == "mcc").unwrap()
        .three_phase.as_ref().unwrap().symmetrical_ka
}

#[test]
fn sample_main_stages_motor_contribution_and_panel_feeder_stays_single_device() {
    let output = run_arc(&Project::sample());
    let mcc = output.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-mcc")
        .expect("the MCC arc continues until the utility breaker and the motor contribution both end");
    assert!(mcc.assumed, "{}", mcc.duration_note);
    assert!(!mcc.decrement_applied, "{}", mcc.duration_note);
    assert!(mcc.duration_note.contains("m1") && mcc.duration_note.contains("initial arcing current"), "{}", mcc.duration_note);
    assert!(!mcc.duration_note.contains("was not integrated"), "{}", mcc.duration_note);
    assert!(output.arc_flash_failures.iter().all(|failure| failure.equipment_id != "af-mcc"));

    let panel = output.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-pnl").unwrap();
    assert!(!panel.assumed, "{}", panel.duration_note);
    assert!(output.arc_flash_failures.iter().all(|failure| failure.equipment_id != "af-pnl"));
}

#[test]
fn source_decrement_is_integrated_across_motor_backfeed() {
    let steady = run_arc(&Project::sample());
    let mut project = Project::sample();
    project.sources[0].decrement_curve = vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.2, current_ratio: 0.5 },
    ];
    let output = run_arc(&project);
    let row = output.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-mcc")
        .expect("motor contribution is calculated while source decrement is integrated between openings");
    let steady_row = steady.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-mcc").unwrap();
    assert!(row.assumed && row.decrement_applied, "{}", row.duration_note);
    assert!(row.duration_note.contains("decaying arcing current"), "{}", row.duration_note);
    assert_eq!(row.clearing_basis, "staged_decaying_current");
    assert!(row.duration_note.contains("m1"), "{}", row.duration_note);
    assert!(row.decrement_source_ids.iter().any(|id| id == "grid"), "{:?}", row.decrement_source_ids);
    let final_bolted = row.final_bolted_ka.expect("last decrement slice");
    assert!(final_bolted < row.bolted_ka, "final {final_bolted}, initial {}", row.bolted_ka);
    assert!(row.governing_cal_cm2 < steady_row.governing_cal_cm2, "decrement {}, steady {}", row.governing_cal_cm2, steady_row.governing_cal_cm2);
}

#[test]
fn motor_feeder_breaker_only_removes_backfeed_when_its_contact_is_open() {
    let mut project = Project::sample();
    project.equipment.truncate(1);
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    project.buses.push(Bus {
        id: "motor-bus".into(), name: "Motor feeder load end".into(), kv: 0.48,
        shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None,
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
        basis: "collected".into(), curve_is_total_clearing: false, library: None, interrupting: None, current_limiting: None,
    });

    let closed = run_arc(&project);
    let closed_row = closed.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-mcc").unwrap();
    assert!(closed_row.assumed, "{}", closed_row.duration_note);
    assert!((closed_row.time_s - 0.09).abs() < 1e-12, "{}", closed_row.time_s);
    assert!((closed_row.time_min_s - 0.09).abs() < 1e-12, "{}", closed_row.time_min_s);
    assert!(closed_row.duration_note.contains("initial arcing current"), "{}", closed_row.duration_note);
    let closed_expected = calculate(&IeeeInput {
        voc_kv: 0.48, ibf_ka: closed_row.bolted_ka, gap_mm: project.equipment[0].gap_mm,
        distance_mm: project.equipment[0].distance_mm, height_mm: project.equipment[0].height_mm,
        width_mm: project.equipment[0].width_mm, depth_mm: project.equipment[0].depth_mm,
        electrode: project.equipment[0].electrode, time_s: 0.09, time_min_s: 0.09,
        arc_ka_override: None, arc_min_ka_override: None,
    }).unwrap();
    for (got, want) in [(closed_row.energy_cal_cm2, closed_expected.energy_cal_cm2),
        (closed_row.governing_cal_cm2, closed_expected.governing_cal_cm2), (closed_row.afb_mm, closed_expected.afb_mm)] {
        assert!((got - want).abs() < 1e-9 * want, "got {got}, expected {want}");
    }

    project.switches[0].closed = false;
    let open = run_arc(&project);
    assert!(open.arc_flash.as_ref().unwrap().iter().any(|row| row.equipment_id == "af-mcc"),
        "the main breaker can clear the MCC fault when the motor feeder is already open");
    assert!(open.arc_flash_failures.iter().all(|failure| failure.equipment_id != "af-mcc"));
    assert!(mcc_bolted_ka(&closed) > mcc_bolted_ka(&open),
        "the closed motor feeder must actually contribute fault current");

    // Absolute values for the open-feeder case: main relay definite 0.05 s
    // plus entered breaker interruption 0.04 s, at both arcing currents, and
    // the IEEE 1584 energy for the af-mcc geometry at the MCC bolted current.
    let row = open.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-mcc").unwrap();
    assert_eq!(row.upstream_device.as_deref(), Some("brk-main"));
    assert!((row.time_s - 0.09).abs() < 1e-12, "{}", row.time_s);
    assert!((row.time_min_s - 0.09).abs() < 1e-12, "{}", row.time_min_s);
    assert!(!row.duration_capped && !row.assumed);
    assert!((row.bolted_ka - mcc_bolted_ka(&open)).abs() < 1e-12);
    let equipment = &project.equipment[0];
    let expected = calculate(&IeeeInput {
        voc_kv: 0.48, ibf_ka: row.bolted_ka, gap_mm: equipment.gap_mm,
        distance_mm: equipment.distance_mm, height_mm: equipment.height_mm,
        width_mm: equipment.width_mm, depth_mm: equipment.depth_mm,
        electrode: equipment.electrode, time_s: 0.09, time_min_s: 0.09,
        arc_ka_override: None, arc_min_ka_override: None,
    }).unwrap();
    for (got, want) in [(row.energy_cal_cm2, expected.energy_cal_cm2),
        (row.energy_min_cal_cm2, expected.energy_min_cal_cm2),
        (row.governing_cal_cm2, expected.governing_cal_cm2), (row.afb_mm, expected.afb_mm)] {
        assert!((got - want).abs() < 1e-9 * want, "got {got}, expected {want}");
    }
    // The utility-only MCC fault through T1 (2500 kVA, 5.75 %, 250 MVA source)
    // is about 47 kA; the resulting 0.09 s energy is a few cal/cm2.
    assert!((40.0..55.0).contains(&row.bolted_ka), "{}", row.bolted_ka);
    assert!((1.0..10.0).contains(&row.governing_cal_cm2), "{}", row.governing_cal_cm2);
}
