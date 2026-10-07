mod common;

use common::*;
use flashmob::{fault, study::{self, Studies}};

#[test]
fn tiny_contact_fault_with_weak_source_survives_roundoff() {
    let mut p = radial();
    p.loads.clear();
    for b in &mut p.buses { b.kv = 13.8; }
    p.sources[0].mva_sc = 1.0;
    p.branches[0] = line("ab", "a", "b", 1e-8, 1e-8);
    assert!(p.validate().is_empty());
    let result = study::run(&p, Studies { loadflow: false, fault: true, arcflash: false, coordination: false }).unwrap();
    let result = result.fault.unwrap();
    assert!(result.valid);
    let ka = result.buses.iter().find(|b| b.id == "a").unwrap().three_phase.as_ref().unwrap().symmetrical_ka;
    assert!((ka / 0.041837 - 1.0).abs() < 1e-3, "{ka} kA");
    assert!(fault::solve(&p, None).unwrap().valid);
}
