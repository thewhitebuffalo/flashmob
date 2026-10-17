//! Buses above 15 kV fall back to the Lee (1982) theoretical model.

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
    assert!((row.governing_cal_cm2 - lee.energy_cal_cm2).abs() < 1e-9);
    assert!((row.afb_mm - lee.boundary_mm).abs() < 1e-6);
    assert!(row.label_reasons.iter().any(|r| r == "lee_method"), "{:?}", row.label_reasons);
    assert!(!row.label_ready);
    assert!(row.warnings.iter().any(|w| w.contains("motor window")), "{:?}", row.warnings);
    assert!(LABEL_REASONS.iter().any(|(code, _)| *code == "lee_method"));
}
