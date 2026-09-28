use flashmob::model::{
    Branch, BranchKind, Bus, CurveSpec, DecrementPoint, EquipmentState, OperatingCase, Project, SourceState,
    Switch, SwitchGroup, SwitchKind, SwitchState,
};
use flashmob::study::{self, ArcRow, Studies, StudyOutput};

fn curve(final_ratio: f64) -> Vec<DecrementPoint> {
    vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.1, current_ratio: final_ratio },
        DecrementPoint { time_s: 0.5, current_ratio: final_ratio },
    ]
}

fn arc_studies() -> Studies {
    Studies { loadflow: false, fault: false, arcflash: true, coordination: false }
}

fn arc<'a>(output: &'a StudyOutput, equipment_id: &str) -> &'a ArcRow {
    output.arc_flash.as_ref().unwrap().iter()
        .find(|row| row.equipment_id == equipment_id).unwrap()
}

fn run_arc(project: &Project) -> StudyOutput {
    study::run(project, arc_studies()).unwrap()
}

fn manual_source_project() -> Project {
    let mut project = Project::sample();
    project.sources[0].bus = "mcc".into();
    project.sources[0].mva_sc = 20.0;
    project.motors.clear();
    project.loads.clear();
    project.devices.clear();
    project.equipment.truncate(1);
    project.equipment[0].upstream_device = None;
    project.equipment[0].clearing_s = Some(0.4);
    project
}

fn close(got: f64, expected: f64, relative: f64) {
    let tolerance = relative * expected.abs().max(1.0);
    assert!((got - expected).abs() <= tolerance, "got {got}, expected {expected}");
}

#[test]
fn constant_decrement_curve_matches_static_arc_energy_and_boundary() {
    let static_project = manual_source_project();
    let static_row = arc(&run_arc(&static_project), "af-mcc").clone();

    let mut profiled = static_project;
    profiled.sources[0].decrement_curve = curve(1.0);
    let profiled_row = arc(&run_arc(&profiled), "af-mcc").clone();
    close(profiled_row.energy_cal_cm2, static_row.energy_cal_cm2, 1e-3);
    close(profiled_row.energy_min_cal_cm2, static_row.energy_min_cal_cm2, 1e-3);
    close(profiled_row.governing_cal_cm2, static_row.governing_cal_cm2, 1e-3);
    close(profiled_row.afb_mm, static_row.afb_mm, 1e-3);
}

#[test]
fn declining_generator_current_reduces_manual_duration_energy() {
    let static_project = manual_source_project();
    let static_row = arc(&run_arc(&static_project), "af-mcc").clone();

    let mut declining = static_project;
    declining.sources[0].decrement_curve = curve(0.3);
    let declined = arc(&run_arc(&declining), "af-mcc").clone();
    assert!(declined.governing_cal_cm2 < static_row.governing_cal_cm2,
        "declining {}, static {}", declined.governing_cal_cm2, static_row.governing_cal_cm2);
    assert!(declined.afb_mm < static_row.afb_mm,
        "declining {}, static {}", declined.afb_mm, static_row.afb_mm);
    close(declined.time_s, 0.4, 1e-10);
}

#[test]
fn mixed_utility_and_generator_retains_utility_contribution() {
    let mut mixed = manual_source_project();
    mixed.sources[0].id = "utility".into();
    let mut generator = mixed.sources[0].clone();
    generator.id = "generator".into();
    generator.name = "Generator".into();
    generator.is_slack = false;
    generator.decrement_curve = curve(0.25);
    mixed.sources.push(generator);

    let mixed_energy = arc(&run_arc(&mixed), "af-mcc").governing_cal_cm2;
    let mut all_static = mixed.clone();
    all_static.sources[1].decrement_curve.clear();
    let initial_energy = arc(&run_arc(&all_static), "af-mcc").governing_cal_cm2;
    let mut utility_only = mixed.clone();
    utility_only.sources.pop();
    let utility_energy = arc(&run_arc(&utility_only), "af-mcc").governing_cal_cm2;
    let mut naively_scaled = all_static;
    for source in &mut naively_scaled.sources { source.mva_sc *= 0.25; }
    let naive_energy = arc(&run_arc(&naively_scaled), "af-mcc").governing_cal_cm2;

    assert!(utility_energy < mixed_energy && mixed_energy < initial_energy,
        "utility {utility_energy}, mixed {mixed_energy}, initial {initial_energy}");
    assert!(mixed_energy > naive_energy,
        "mixed {mixed_energy}, naive total scaling {naive_energy}");
}

#[test]
fn decrement_with_device_only_clearing_fails_visibly() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.sources[0].decrement_curve = curve(0.3);
    let output = run_arc(&project);
    assert!(!output.arc_flash.as_ref().unwrap().iter().any(|row| row.equipment_id == "af-mcc"));
    let failure = output.arc_flash_failures.iter()
        .find(|failure| failure.equipment_id == "af-mcc").expect("missing explicit arc-flash failure");
    let message = failure.error.to_lowercase();
    assert!(message.contains("decrement") || message.contains("time-varying"), "{}", failure.error);
}

#[test]
fn out_of_range_late_current_invalidates_arc_envelope() {
    let mut project = manual_source_project();
    project.sources[0].decrement_curve = curve(0.001);
    project.operating_cases.push(OperatingCase {
        id: "repeat".into(), name: "Repeat energized state".into(),
        switch_states: Vec::new(), source_states: Vec::new(),
        equipment_states: vec![EquipmentState {
            equipment_id: "af-mcc".into(), upstream_device: None,
            clearing_s: Some(0.4), fallback_duration_s: None,
        }],
    });
    let output = run_arc(&project);
    let failure = output.arc_flash_failures.iter()
        .find(|failure| failure.equipment_id == "af-mcc").unwrap();
    assert!(failure.error.contains("outside the IEEE 1584-2018 bolted-current model range"), "{}", failure.error);
    let envelope = output.worst_case.as_ref().unwrap().arc_equipment.iter()
        .find(|item| item.equipment_id == "af-mcc").unwrap();
    assert!(!envelope.complete);
    assert!(envelope.failed_case_ids.contains(&"normal".to_string()));
    assert!(envelope.failed_case_ids.contains(&"repeat".to_string()));
}

#[test]
fn disconnected_curved_source_does_not_change_static_island_clearing() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    let baseline = arc(&run_arc(&project), "af-mcc").clone();
    assert!(!baseline.decrement_applied);

    project.buses.push(bus("generator_island"));
    let mut generator = project.sources[0].clone();
    generator.id = "isolated_generator".into();
    generator.bus = "generator_island".into();
    generator.mva_sc = 8.0;
    generator.decrement_curve = curve(0.3);
    project.sources.push(generator);
    let output = run_arc(&project);
    let unaffected = arc(&output, "af-mcc");
    assert!(!unaffected.decrement_applied);
    assert!(unaffected.decrement_source_ids.is_empty());
    close(unaffected.governing_cal_cm2, baseline.governing_cal_cm2, 1e-10);
    close(unaffected.afb_mm, baseline.afb_mm, 1e-10);
    close(unaffected.time_s, baseline.time_s, 1e-10);
    assert!(!output.arc_flash_failures.iter().any(|failure| failure.equipment_id == "af-mcc"));
}

fn bus(id: &str) -> Bus {
    Bus { id: id.into(), name: id.into(), kv: 0.48, shunt_kvar: 0.0,
        bracing_ka: None, main_rating_a: None }
}

fn line(id: &str, from: &str, to: &str) -> Branch {
    Branch { id: id.into(), name: id.into(), from: from.into(), to: to.into(),
        kind: BranchKind::Line { r_ohm: 0.003, x_ohm: 0.004, b_siemens: 0.0,
            r0_ohm: 0.009, x0_ohm: 0.012, ampacity_a: None } }
}

fn ats_project(with_curve: bool) -> Project {
    let mut project = manual_source_project();
    project.buses = ["utility_bus", "generator_bus", "load_bus"].map(bus).to_vec();
    project.branches = vec![
        line("utility_throw", "utility_bus", "load_bus"),
        line("generator_throw", "generator_bus", "load_bus"),
    ];
    let mut utility = project.sources[0].clone();
    utility.id = "utility".into(); utility.bus = "utility_bus".into();
    utility.mva_sc = 40.0;
    let mut generator = utility.clone();
    generator.id = "generator".into(); generator.bus = "generator_bus".into();
    generator.mva_sc = 20.0; generator.in_service = false;
    if with_curve { generator.decrement_curve = curve(0.3); }
    project.sources = vec![utility, generator];
    project.switches = vec![
        Switch { id: "utility_contact".into(), name: "Utility ATS contact".into(),
            branch_id: "utility_throw".into(), kind: SwitchKind::Ats, closed: true },
        Switch { id: "generator_contact".into(), name: "Generator ATS contact".into(),
            branch_id: "generator_throw".into(), kind: SwitchKind::Ats, closed: false },
    ];
    project.switch_groups = vec![SwitchGroup { id: "ats".into(),
        switch_ids: vec!["utility_contact".into(), "generator_contact".into()], max_closed: 1 }];
    project.operating_cases = vec![OperatingCase {
        id: "generator_feed".into(), name: "Generator feed".into(),
        switch_states: vec![
            SwitchState { switch_id: "utility_contact".into(), closed: false },
            SwitchState { switch_id: "generator_contact".into(), closed: true },
        ],
        source_states: vec![
            SourceState { source_id: "utility".into(), in_service: false },
            SourceState { source_id: "generator".into(), in_service: true },
        ],
        equipment_states: vec![EquipmentState { equipment_id: "load_gear".into(),
            upstream_device: None, clearing_s: Some(0.4), fallback_duration_s: None }],
    }];
    project.equipment[0].id = "load_gear".into();
    project.equipment[0].bus = "load_bus".into();
    project
}

#[test]
fn ats_uses_generator_curve_only_when_generator_is_active() {
    let with_curve = run_arc(&ats_project(true));
    let without_curve = run_arc(&ats_project(false));
    let normal_with = arc(&with_curve, "load_gear");
    let normal_without = arc(&without_curve, "load_gear");
    close(normal_with.governing_cal_cm2, normal_without.governing_cal_cm2, 1e-10);
    close(normal_with.afb_mm, normal_without.afb_mm, 1e-10);

    let generator_with = with_curve.operating_cases[0].result.as_deref().unwrap();
    let generator_without = without_curve.operating_cases[0].result.as_deref().unwrap();
    assert_eq!(with_curve.operating_cases[0].id, "generator_feed");
    let decreased = arc(generator_with, "load_gear");
    let static_case = arc(generator_without, "load_gear");
    assert!(decreased.governing_cal_cm2 < static_case.governing_cal_cm2,
        "decrement {}, static {}", decreased.governing_cal_cm2, static_case.governing_cal_cm2);
}
