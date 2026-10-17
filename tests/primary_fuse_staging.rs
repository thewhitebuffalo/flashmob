//! A primary-side fuse feeding a transformer clears a fault on the secondary.

mod common;

use common::*;
use flashmob::model::{CurveSpec, Project};
use flashmob::study::{self, Studies};

/// Pole bus, fuse, line to the transformer primary, transformer, secondary
/// bus with a motor, and a feeder to a panel with a second motor and no device.
fn primary_fused() -> Project {
    let mut p = Project::sample();
    p.loads.clear();
    p.equipment.truncate(1);
    p.buses.push(bus("pole", 12.47));
    p.branches.push(line("drop", "pole", "util", 0.5, 1.0));
    p.sources[0].bus = "pole".into();
    let mut fuse = p.devices[0].clone();
    fuse.id = "pri-fuse".into();
    fuse.name = "Primary fuse".into();
    fuse.bus = "pole".into();
    fuse.protected_branch = Some("drop".into());
    fuse.curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.2 };
    fuse.breaker_interrupting_s = None;
    fuse.curve_is_total_clearing = true;
    p.devices = vec![fuse];
    let mut m2 = p.motors[0].clone();
    m2.id = "m2".into();
    m2.bus = "pnl".into();
    m2.hp = 50.0;
    p.motors.push(m2);
    p.equipment[0].upstream_device = Some("pri-fuse".into());
    p
}

#[test]
fn primary_fuse_is_the_clearing_device_for_the_secondary_bus() {
    let p = primary_fused();
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    let out = study::run(&p, Studies { loadflow: false, fault: true, arcflash: true, coordination: false }).expect("valid study");
    let row = out.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == "af-mcc")
        .unwrap_or_else(|| panic!("af-mcc failed: {:?}", out.arc_flash_failures));
    assert_eq!(row.governing_device.as_deref(), Some("pri-fuse"), "{}", row.duration_note);
    assert!((row.time_s - 0.2).abs() < 1e-6, "{}: {}", row.time_s, row.duration_note);
    assert!(row.governing_cal_cm2 > 0.0);
}
