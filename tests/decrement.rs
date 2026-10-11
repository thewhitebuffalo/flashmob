use flashmob::model::{
    Branch, BranchKind, Bus, CurveSpec, DecrementPoint, Device, EquipmentState, IecKind, OperatingCase, Project, SourceState,
    Switch, SwitchGroup, SwitchKind, SwitchState,
};
use flashmob::arcflash::{calculate, IeeeInput};
use flashmob::model::Electrode;
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
fn decrement_uses_device_trip_plus_breaker_interruption_without_manual_duration() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    project.sources[0].decrement_curve = curve(0.3);
    let output = run_arc(&project);
    let row = arc(&output, "af-mcc");
    close(row.time_s, 0.09, 1e-10);
    close(row.time_min_s, 0.09, 1e-10);
    assert!(row.decrement_applied);
    assert!(!row.assumed);
    assert!(row.duration_note.contains("relay trip") && row.duration_note.contains("breaker interruption"));
    assert!(!output.arc_flash_failures.iter().any(|failure| failure.equipment_id == "af-mcc"));
}

#[test]
fn declining_current_delays_inverse_relay_and_reduced_arcing_path() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.devices[0].curve = CurveSpec::Iec {
        kind: IecKind::Si, pickup_a: 500.0, tms: 0.1,
        inst_a: None, inst_s: None,
    };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    let static_row = arc(&run_arc(&project), "af-mcc").clone();
    project.sources[0].decrement_curve = curve(1.0);
    let constant = arc(&run_arc(&project), "af-mcc").clone();
    close(constant.time_s, static_row.time_s, 1e-8);
    close(constant.time_min_s, static_row.time_min_s, 1e-8);
    close(constant.governing_cal_cm2, static_row.governing_cal_cm2, 1e-3);
    if let CurveSpec::Iec { inst_a, .. } = &mut project.devices[0].curve {
        *inst_a = Some(1_000_000.0);
    }
    let dormant_instantaneous = arc(&run_arc(&project), "af-mcc").clone();
    close(dormant_instantaneous.time_s, static_row.time_s, 1e-8);
    if let CurveSpec::Iec { inst_a, .. } = &mut project.devices[0].curve {
        *inst_a = None;
    }
    project.sources[0].decrement_curve = curve(0.3);
    let output = run_arc(&project);
    let dynamic = arc(&output, "af-mcc");
    assert!(dynamic.time_s > static_row.time_s,
        "dynamic {}, static {}", dynamic.time_s, static_row.time_s);
    assert!(dynamic.time_min_s > static_row.time_min_s,
        "dynamic reduced {}, static reduced {}", dynamic.time_min_s, static_row.time_min_s);
    assert!(dynamic.time_min_s > dynamic.time_s,
        "reduced {}, full {}", dynamic.time_min_s, dynamic.time_s);
}

#[test]
fn decrement_without_relay_operation_has_explicit_failure() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.sources[0].decrement_curve = curve(0.3);
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1_000_000.0, time_s: 0.05 };
    let output = run_arc(&project);
    assert!(!output.arc_flash.as_ref().unwrap().iter().any(|row| row.equipment_id == "af-mcc"));
    let failure = output.arc_flash_failures.iter()
        .find(|failure| failure.equipment_id == "af-mcc").expect("missing explicit arc-flash failure");
    assert!(failure.error.contains("within the 2.000 s study cap"), "{}", failure.error);
    assert!(failure.error.contains("below every entered pickup"), "{}", failure.error);
}

fn assert_same_capped_result(static_row: &ArcRow, profiled: &ArcRow, cap: f64) {
    assert!(static_row.duration_capped && profiled.duration_capped);
    close(static_row.time_s, cap, 1e-12);
    close(profiled.time_s, static_row.time_s, 1e-12);
    close(profiled.time_min_s, static_row.time_min_s, 1e-12);
    close(profiled.energy_cal_cm2, static_row.energy_cal_cm2, 1e-3);
    close(profiled.energy_min_cal_cm2, static_row.energy_min_cal_cm2, 1e-3);
    close(profiled.governing_cal_cm2, static_row.governing_cal_cm2, 1e-3);
    close(profiled.afb_mm, static_row.afb_mm, 1e-3);
    assert_eq!(profiled.assumed, static_row.assumed);
    assert!(profiled.duration_note.contains("cap"), "{}", profiled.duration_note);
    assert!(profiled.decrement_applied && !static_row.decrement_applied);
}

// Formerly a failure ("beyond the 0.080 s study cap"). The constant-current
// path caps such a duration at the study limit, so decrement now does too.
#[test]
fn breaker_interruption_beyond_study_cap_is_capped_like_constant_current() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.arc_duration_cap_s = 0.08;
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    let static_row = arc(&run_arc(&project), "af-mcc").clone();
    project.sources[0].decrement_curve = curve(1.0);
    let output = run_arc(&project);
    assert!(!output.arc_flash_failures.iter().any(|failure| failure.equipment_id == "af-mcc"));
    assert_same_capped_result(&static_row, arc(&output, "af-mcc"), 0.08);
}

#[test]
fn slow_device_with_flat_decrement_is_capped_like_constant_current() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 3.0 };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    let static_row = arc(&run_arc(&project), "af-mcc").clone();
    project.sources[0].decrement_curve = curve(1.0);
    let output = run_arc(&project);
    assert!(!output.arc_flash_failures.iter().any(|failure| failure.equipment_id == "af-mcc"));
    let profiled = arc(&output, "af-mcc");
    assert_same_capped_result(&static_row, profiled, 2.0);
    assert!(profiled.duration_note.contains("still timing"), "{}", profiled.duration_note);
}

#[test]
fn fuse_total_clearing_curve_does_not_masquerade_as_dynamic_relay() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.sources[0].decrement_curve = curve(0.3);
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    project.devices[0].fuse_total_clearing = true;
    project.devices[0].breaker_interrupting_s = None;
    let output = run_arc(&project);
    let failure = output.arc_flash_failures.iter()
        .find(|failure| failure.equipment_id == "af-mcc").unwrap();
    assert!(failure.error.contains("fuse total clearing"), "{}", failure.error);
}

#[test]
fn short_initial_pickup_does_not_claim_a_trip_after_current_decays() {
    let mut project = Project::sample();
    project.motors.clear();
    project.equipment.truncate(1);
    project.sources[0].mva_sc = 20.0;
    project.arc_duration_cap_s = 0.5;
    project.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    project.devices[0].breaker_interrupting_s = Some(0.04);
    let initial = arc(&run_arc(&project), "af-mcc").arcing_ka * 1000.0;
    project.devices[0].curve = CurveSpec::Definite { pickup_a: initial * 0.75, time_s: 0.3 };
    project.sources[0].decrement_curve = curve(0.1);
    let output = run_arc(&project);
    assert!(!output.arc_flash.as_ref().unwrap().iter().any(|row| row.equipment_id == "af-mcc"));
    let failure = output.arc_flash_failures.iter()
        .find(|failure| failure.equipment_id == "af-mcc").unwrap();
    assert!(failure.error.contains("within the 0.500 s study cap"), "{}", failure.error);
    assert!(failure.error.contains("below every entered pickup"), "{}", failure.error);
}

// Formerly a hard failure. A constant out-of-range bolted current is computed
// with a model-range warning, so the profile path now does the same and marks
// the row assumed rather than validated.
#[test]
fn out_of_range_late_current_is_computed_and_marked_assumed() {
    let mut project = manual_source_project();
    project.sources[0].decrement_curve = curve(0.01);
    project.operating_cases.push(OperatingCase {
        id: "repeat".into(), name: "Repeat energized state".into(),
        switch_states: Vec::new(), source_states: Vec::new(),
        equipment_states: vec![EquipmentState {
            equipment_id: "af-mcc".into(), upstream_device: None,
            clearing_s: Some(0.4), fallback_duration_s: None,
        }],
    });
    let output = run_arc(&project);
    assert!(!output.arc_flash_failures.iter().any(|failure| failure.equipment_id == "af-mcc"));
    let row = arc(&output, "af-mcc");
    assert!(row.assumed);
    assert!(row.warnings.iter().any(|w| w.contains("outside the 0.5–106 kA low-voltage model range")
        && w.contains("assumed")), "{:?}", row.warnings);
    assert!(output.warnings.iter().any(|w| w.contains("model range")));
    let in_range = arc(&run_arc(&manual_source_project()), "af-mcc").clone();
    assert!(!in_range.assumed);
    assert!(row.governing_cal_cm2 < in_range.governing_cal_cm2);
    let envelope = output.worst_case.as_ref().unwrap().arc_equipment.iter()
        .find(|item| item.equipment_id == "af-mcc").unwrap();
    assert!(envelope.complete);
}

#[test]
fn out_of_range_profile_matches_constant_current_at_the_same_out_of_range_level() {
    // A constant 0.5 % decrement stays below 0.5 kA throughout: the integrated
    // result must equal the static calculation at that bolted current.
    let mut static_project = manual_source_project();
    static_project.sources[0].mva_sc = 20.0 * 0.005;
    let static_row = arc(&run_arc(&static_project), "af-mcc").clone();
    assert!(static_row.bolted_ka < 0.5 && static_row.assumed, "{}", static_row.bolted_ka);
    let mut profiled = manual_source_project();
    profiled.sources[0].mva_sc = 20.0 * 0.005;
    profiled.sources[0].decrement_curve = curve(1.0);
    let row = arc(&run_arc(&profiled), "af-mcc").clone();
    assert!(row.assumed);
    close(row.governing_cal_cm2, static_row.governing_cal_cm2, 1e-3);
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
        bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None }
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

#[test]
fn ats_transfer_calculates_each_active_breaker_without_manual_duration() {
    let mut project = ats_project(true);
    project.equipment[0].clearing_s = None;
    project.operating_cases[0].equipment_states[0].clearing_s = None;
    for (id, branch) in [("utility_breaker", "utility_throw"), ("generator_breaker", "generator_throw")] {
        project.devices.push(Device {
            id: id.into(), name: id.into(), bus: "load_bus".into(),
            curve: CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 },
            protected_branch: Some(branch.into()), breaker_interrupting_s: Some(0.04),
            fuse_total_clearing: false, curve_is_total_clearing: false, basis: "collected".into(), library: None, interrupting: None, current_limiting: None,
        });
    }
    let output = run_arc(&project);
    let normal = arc(&output, "load_gear");
    close(normal.time_s, 0.09, 1e-10);
    assert_eq!(normal.upstream_device.as_deref(), Some("utility_breaker"));
    assert!(!normal.decrement_applied);

    let transferred = output.operating_cases[0].result.as_deref().unwrap();
    let alternate = arc(transferred, "load_gear");
    close(alternate.time_s, 0.09, 1e-10);
    close(alternate.time_min_s, 0.09, 1e-10);
    assert_eq!(alternate.upstream_device.as_deref(), Some("generator_breaker"));
    assert_eq!(alternate.decrement_source_ids, vec!["generator"]);
    assert!(alternate.decrement_applied);
    assert!(!alternate.assumed);
    assert!(output.worst_case.as_ref().unwrap().arc_equipment.iter()
        .find(|item| item.equipment_id == "load_gear").unwrap().complete);
}

// Hand-checked two-step profiles. Each slice's bolted current is the curved
// source's scaled strength (the only contribution here), and each slice's
// incident energy is calculated independently with the IEEE 1584 equations.
fn two_step_curve() -> Vec<DecrementPoint> {
    vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.2, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.21, current_ratio: 0.5 },
    ]
}

/// (current ratio, seconds) for the two-step curve over `duration_s`: the
/// 10 ms ramp slice is sampled at its midpoint ratio of 0.75.
fn two_step_slices(duration_s: f64) -> [(f64, f64); 3] {
    [(1.0, 0.2), (0.75, 0.01), (0.5, duration_s - 0.21)]
}

fn hand_integrated(project: &Project, duration_s: f64) -> (f64, f64) {
    let equipment = &project.equipment[0];
    let kv = project.bus(&equipment.bus).unwrap().kv;
    let initial_ka = project.sources[0].mva_sc / (3f64.sqrt() * kv);
    let mut full = 0.0;
    let mut reduced = 0.0;
    for (ratio, seconds) in two_step_slices(duration_s) {
        let slice = calculate(&IeeeInput {
            voc_kv: kv, ibf_ka: initial_ka * ratio, gap_mm: equipment.gap_mm,
            distance_mm: equipment.distance_mm, height_mm: equipment.height_mm,
            width_mm: equipment.width_mm, depth_mm: equipment.depth_mm,
            electrode: equipment.electrode, time_s: seconds, time_min_s: seconds,
            arc_ka_override: None, arc_min_ka_override: None,
        }).unwrap();
        full += slice.energy_cal_cm2;
        reduced += slice.energy_min_cal_cm2;
    }
    (full, reduced)
}

fn check_two_step(mut project: Project, duration_s: f64) {
    let kv = project.bus(&project.equipment[0].bus).unwrap().kv;
    let static_row = arc(&run_arc(&project), &project.equipment[0].id).clone();
    let initial_ka = project.sources[0].mva_sc / (3f64.sqrt() * kv);
    close(static_row.bolted_ka, initial_ka, 1e-6);
    project.sources[0].decrement_curve = two_step_curve();
    let row = arc(&run_arc(&project), &project.equipment[0].id).clone();
    let (full, reduced) = hand_integrated(&project, duration_s);
    close(row.time_s, duration_s, 1e-12);
    close(row.energy_cal_cm2, full, 1e-9);
    close(row.energy_min_cal_cm2, reduced, 1e-9);
    close(row.governing_cal_cm2, full.max(reduced), 1e-9);
    assert!(row.governing_cal_cm2 < static_row.governing_cal_cm2);
    assert!(row.afb_mm < static_row.afb_mm);
    close(row.final_bolted_ka.unwrap(), 0.5 * initial_ka, 1e-6);
}

#[test]
fn low_voltage_two_step_decrement_matches_hand_summed_slice_energies() {
    let project = manual_source_project();
    check_two_step(project, 0.4);
}

#[test]
fn medium_voltage_two_step_decrement_matches_hand_summed_slice_energies() {
    let mut project = manual_source_project();
    project.buses = vec![Bus { id: "swgr".into(), name: "4.16 kV switchgear".into(), kv: 4.16,
        shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None }];
    project.branches.clear();
    project.sources[0].bus = "swgr".into();
    project.sources[0].mva_sc = 150.0;
    let equipment = &mut project.equipment[0];
    equipment.bus = "swgr".into();
    equipment.electrode = Electrode::VCB;
    equipment.gap_mm = 104.0;
    equipment.distance_mm = 914.4;
    equipment.height_mm = 1143.0;
    equipment.width_mm = 762.0;
    equipment.depth_mm = 508.0;
    equipment.clearing_s = Some(0.3);
    check_two_step(project, 0.3);
}