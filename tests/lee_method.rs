//! Cases outside the IEEE 1584-2018 model range fall back to the Lee (1982) theoretical model.

mod common;

use common::*;
use flashmob::arcflash;
use flashmob::model::{BranchKind, CurveSpec, Electrode, Project};
use flashmob::study::{self, Studies, LABEL_REASONS};

#[test]
fn lee_closed_form_matches_hand_values() {
    // 25 kV, 10 kA, 0.1 s, 914.4 mm: 2.142e6 * 25 * 10 * 0.1 = 5.355e7.
    let out = arcflash::lee(25.0, 10.0, 0.1, 914.4);
    let k = 5.355e7;
    assert!((out.energy_j_cm2 - k / (914.4 * 914.4)).abs() < 1e-9);
    assert!((out.energy_cal_cm2 - out.energy_j_cm2 / 4.184).abs() < 1e-12);
    assert!((out.boundary_mm - (k / (1.2 * 4.184)).sqrt()).abs() < 1e-6);
    // At the boundary the energy is 1.2 cal/cm².
    let at = arcflash::lee(25.0, 10.0, 0.1, out.boundary_mm);
    assert!((at.energy_cal_cm2 - 1.2).abs() < 1e-9);
}

/// 24.94 kV primary fed from a pole bus through a relayed line; a transformer
/// to a 480 V bus with an induction motor that backfeeds the primary.
fn above_range() -> Project {
    let mut p = Project::sample();
    p.loads.clear();
    p.equipment.truncate(1);
    p.buses[0].kv = 24.94;
    if let BranchKind::Transformer { hv_kv, .. } = &mut p.branches[0].kind { *hv_kv = 24.94; }
    p.buses.push(bus("pole", 24.94));
    p.branches.push(line("drop", "pole", "util", 1.0, 2.0));
    p.sources[0].bus = "pole".into();
    let mut relay = p.devices[0].clone();
    relay.id = "relay".into();
    relay.name = "Relay".into();
    relay.bus = "pole".into();
    relay.protected_branch = Some("drop".into());
    relay.curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.2 };
    relay.breaker_interrupting_s = Some(0.05);
    p.devices = vec![relay];
    let mut eq = p.equipment[0].clone();
    eq.id = "af-util".into();
    eq.name = "Primary".into();
    eq.bus = "util".into();
    eq.electrode = Electrode::VCB;
    eq.gap_mm = 152.0;
    eq.distance_mm = 914.4;
    eq.height_mm = 1143.0;
    eq.width_mm = 762.0;
    eq.depth_mm = 762.0;
    eq.upstream_device = Some("relay".into());
    p.equipment = vec![eq];
    p
}

#[test]
fn bus_above_fifteen_kv_uses_lee_with_bolted_current_and_a_label_reason() {
    let p = above_range();
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    let out = study::run(&p, Studies { loadflow: false, fault: true, arcflash: true, coordination: false }).expect("valid study");
    let row = out.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == "af-util")
        .unwrap_or_else(|| panic!("af-util failed: {:?}", out.arc_flash_failures));
    assert_eq!(row.governing, "lee");
    assert_eq!(row.standard, arcflash::LEE_STANDARD);
    assert_eq!(row.arcing_ka, row.bolted_ka);
    assert!((row.time_s - 0.25).abs() < 1e-9, "{}", row.time_s);
    let lee = arcflash::lee(24.94, row.bolted_ka, 0.25, 914.4);
    // Item-7 staging on the Lee path: the motor feeds for its 5-cycle window,
    // then the source-only current runs until the relay clears.
    assert_eq!(row.clearing_basis, "staged_multi_device");
    let mut no_motor = p.clone();
    no_motor.motors.clear();
    let without = study::run(&no_motor, Studies { loadflow: false, fault: true, arcflash: true, coordination: false }).unwrap();
    let source_ka = without.arc_flash.as_ref().unwrap()[0].bolted_ka;
    assert!(source_ka < row.bolted_ka);
    let window = 5.0 / 60.0;
    let expected = arcflash::lee(24.94, row.bolted_ka, window, 914.4).energy_cal_cm2
        + arcflash::lee(24.94, source_ka, 0.25 - window, 914.4).energy_cal_cm2;
    assert!((row.governing_cal_cm2 - expected).abs() < 1e-6, "{} vs {}", row.governing_cal_cm2, expected);
    assert!(row.governing_cal_cm2 < lee.energy_cal_cm2);
    assert!(row.label_reasons.iter().any(|r| r == "lee_voltage"), "{:?}", row.label_reasons);
    assert!(!row.label_ready);
    assert!(LABEL_REASONS.iter().any(|(code, _)| *code == "lee_voltage"));
}

/// A primary device that clears in 17 ms, inside the motor window, leaves the
/// backfeeding motor connected. The bus is staged instead of rejected.
#[test]
fn lee_bus_with_a_fast_primary_device_and_a_backfeeding_motor_is_staged() {
    let mut p = above_range();
    p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.001 };
    p.devices[0].breaker_interrupting_s = Some(0.016);
    let out = study::run(&p, Studies { loadflow: false, fault: true, arcflash: true, coordination: false }).expect("valid study");
    let row = out.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == "af-util")
        .unwrap_or_else(|| panic!("af-util failed: {:?}", out.arc_flash_failures));
    assert_eq!(row.governing, "lee");
    assert_eq!(row.clearing_basis, "staged_multi_device");
    // The motor carries about 1% of the bolted current, so the 80% cleared-fault
    // threshold ends the arc when the device opens and the motor tail is excluded.
    assert!((row.time_s - 0.017).abs() < 1e-9, "{}", row.time_s);
    let lee = arcflash::lee(24.94, row.bolted_ka, 0.017, 914.4);
    assert!((row.governing_cal_cm2 - lee.energy_cal_cm2).abs() < 1e-9);
    assert!(row.warnings.iter().any(|w| w.contains("threshold") && w.contains("excluded")), "{:?}", row.warnings);
}

#[test]
fn lee_triggers_follow_the_fixed_order_and_bands() {
    let mut case = Project::sample().equipment[0].clone();
    case.electrode = Electrode::VCB;
    case.gap_mm = 32.0;
    case.width_mm = 508.0;
    let causes = |c: &flashmob::model::ArcEquipment, kv: f64, ka: f64| study::lee_causes(c, kv, ka);
    assert!(causes(&case, 0.48, 30.0).is_empty());
    // Low-voltage current band: 0.5 to 106 kA, each bound with a 1e-4 tolerance.
    assert!(causes(&case, 0.48, 106.0).is_empty());
    assert_eq!(causes(&case, 0.48, 0.4), ["lee_bolted_current"]);
    assert_eq!(causes(&case, 0.48, 110.0), ["lee_bolted_current"]);
    // Above 0.6 kV the band is 0.2 to 65 kA and the gap band is 19.05 to 254 mm.
    assert!(causes(&case, 4.16, 65.0).is_empty());
    assert_eq!(causes(&case, 4.16, 70.0), ["lee_bolted_current"]);
    // Voltage out of range reports voltage only, never current as well.
    assert_eq!(causes(&case, 0.19, 200.0), ["lee_voltage"]);
    assert_eq!(causes(&case, 24.94, 200.0), ["lee_voltage"]);
    // Gap and box are independent and follow in that order.
    let mut wide_gap = case.clone();
    wide_gap.gap_mm = 300.0;
    assert_eq!(causes(&wide_gap, 24.94, 10.0), ["lee_voltage", "lee_gap", "lee_box"]);
    wide_gap.gap_mm = 130.0;
    assert_eq!(causes(&wide_gap, 4.16, 10.0), ["lee_box"]);
    assert_eq!(causes(&wide_gap, 0.48, 10.0), ["lee_gap", "lee_box"]);
    let mut narrow = case.clone();
    narrow.width_mm = 128.0;
    assert_eq!(causes(&narrow, 0.48, 30.0), ["lee_box"]);
    // Open air has no box.
    narrow.electrode = Electrode::VOA;
    assert!(causes(&narrow, 0.48, 30.0).is_empty());
    for code in ["lee_voltage", "lee_bolted_current", "lee_gap", "lee_box"] {
        assert!(LABEL_REASONS.iter().any(|(c, _)| *c == code), "{code}");
    }
}

#[test]
fn low_voltage_bus_with_a_too_narrow_box_is_calculated_with_lee() {
    let mut p = Project::sample();
    p.motors.clear();
    p.equipment.truncate(1);
    p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
    p.devices[0].breaker_interrupting_s = Some(0.04);
    p.equipment[0].electrode = Electrode::VCB;
    p.equipment[0].width_mm = 4.0 * p.equipment[0].gap_mm;
    let out = study::run(&p, Studies { loadflow: false, fault: true, arcflash: true, coordination: false }).unwrap();
    let id = p.equipment[0].id.clone();
    let row = out.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == id)
        .unwrap_or_else(|| panic!("{:?}", out.arc_flash_failures));
    assert_eq!(row.governing, "lee");
    assert_eq!(row.label_reasons, vec!["lee_box".to_string()]);
    let lee = arcflash::lee(p.buses.iter().find(|b| b.id == p.equipment[0].bus).unwrap().kv, row.bolted_ka, row.time_s, p.equipment[0].distance_mm);
    assert!((row.governing_cal_cm2 - lee.energy_cal_cm2).abs() < 1e-9);
}
