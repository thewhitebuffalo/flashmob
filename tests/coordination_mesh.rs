mod common;

use common::*;
use flashmob::{model::*, study::{self, Studies}};

#[test]
fn meshed_coordination_reports_the_larger_terminal_device_current() {
    let mut p = radial();
    p.loads.clear();
    p.buses.push(bus("c", 0.48));
    p.branches.extend([line("ac", "a", "c", 0.02, 0.04), line("cb", "c", "b", 0.02, 0.04)]);
    p.devices.push(Device { id: "d".into(), name: "d".into(), bus: "a".into(),
        protected_branch: Some("ab".into()), curve: CurveSpec::Definite { pickup_a: 100.0, time_s: 0.1 },
        breaker_interrupting_s: Some(0.04), fuse_total_clearing: false, curve_is_total_clearing: false, basis: "collected".into(), library: None, interrupting: None });
    assert!(p.validate().is_empty());
    let studies = Studies { loadflow: false, fault: true, arcflash: false, coordination: true };
    for reversed in [false, true] {
        let mut p = p.clone();
        if reversed {
            let b = &mut p.branches[0];
            std::mem::swap(&mut b.from, &mut b.to);
        }
        let out = study::run(&p, studies).unwrap();
        let row = &out.coordination.unwrap()[0];
        assert_eq!(row.fault_bus_id, "b");
        assert!((row.bolted_ka.unwrap() / 5.2446 - 1.0).abs() < 1e-4);
        assert_eq!(row.trip_at_bolted_s, Some(0.1));
        assert!((row.total_clearing_s.unwrap() - 0.14).abs() < 1e-12);
        assert!(row.note.contains("protected side undetermined in a meshed/parallel network; reported the terminal fault giving the larger device current (other terminal a:"), "{}", row.note);
        let faults = out.fault.unwrap();
        let terminal = |bus: &str| {
            let c = faults.buses.iter().find(|b| b.id == bus).unwrap().terminal_currents.iter().find(|c| c.branch_id == "ab").unwrap();
            if reversed { c.to_a / 1000.0 } else { c.from_a / 1000.0 }
        };
        assert!((row.bolted_ka.unwrap() - terminal("b")).abs() < 1e-9);
        assert!(terminal("b") > terminal("a"));
    }
}
