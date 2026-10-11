#![allow(dead_code)]

use flashmob::model::*;

pub fn bus(id: &str, kv: f64) -> Bus {
    Bus { id: id.into(), name: id.into(), kv, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None }
}

pub fn line(id: &str, from: &str, to: &str, r: f64, x: f64) -> Branch {
    Branch { id: id.into(), name: id.into(), from: from.into(), to: to.into(),
        kind: BranchKind::Line { r_ohm: r, x_ohm: x, r0_ohm: 0.0, x0_ohm: 0.0, b_siemens: 0.0, ampacity_a: None } }
}

pub fn radial() -> Project {
    let mut p = Project::sample();
    p.motors.clear(); p.loads.clear(); p.devices.clear(); p.equipment.clear();
    p.buses = vec![bus("a", 0.48), bus("b", 0.48)];
    p.sources[0].bus = "a".into(); p.sources[0].mva_sc = 40.0;
    p.branches = vec![line("ab", "a", "b", 0.02, 0.04)];
    p.loads.push(Load { id: "load".into(), name: "load".into(), bus: "b".into(),
        kw: 100.0, kvar: 20.0, basis: "collected".into() });
    p
}
