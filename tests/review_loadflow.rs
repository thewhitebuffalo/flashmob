mod common;

use common::*;
use flashmob::{loadflow, model::*, study::{self, Studies}};

#[test]
fn disconnected_230kv_island_does_not_change_energized_loadflow() {
    let mut p = radial();
    p.prefault = Prefault::Loadflow;
    p.buses[1].shunt_kvar = 400.0;
    let mut eq = Project::sample().equipment.remove(1);
    eq.bus = "b".into(); eq.upstream_device = None; eq.clearing_s = Some(0.1);
    p.equipment.push(eq);
    let before = study::run(&p, Studies::all()).unwrap();
    p.buses.extend([bus("island1", 230.0), bus("island2", 230.0)]);
    p.branches.push(line("contact", "island1", "island2", 1e-8, 1e-8));
    assert!(p.validate().is_empty());
    let after = study::run(&p, Studies::all()).unwrap();
    let (a, b) = (before.loadflow.unwrap(), after.loadflow.unwrap());
    assert!(a.converged && b.converged);
    assert_eq!(a.iterations, b.iterations);
    for (a, b) in a.buses.iter().zip(&b.buses) {
        for (a, b) in [(a.v_pu, b.v_pu), (a.angle_deg, b.angle_deg), (a.p_mw, b.p_mw), (a.q_mvar, b.q_mvar)] {
            assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        }
    }
    for (a, b) in a.branches.iter().zip(&b.branches) {
        for (a, b) in [(a.p_from_mw, b.p_from_mw), (a.q_from_mvar, b.q_from_mvar),
            (a.p_to_mw, b.p_to_mw), (a.q_to_mvar, b.q_to_mvar), (a.i_from_a, b.i_from_a), (a.p_loss_mw, b.p_loss_mw)] {
            assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        }
    }
    let energy = |rows: Vec<study::ArcRow>| rows.into_iter().find(|r| r.bus_id == "b").unwrap().governing_cal_cm2;
    assert!((energy(before.arc_flash.unwrap()) - energy(after.arc_flash.unwrap())).abs() < 1e-9);
}

#[test]
fn excessive_roundoff_does_not_accept_a_flat_start() {
    let mut p = radial();
    for b in &mut p.buses { b.kv = 230.0; }
    p.branches[0] = line("ab", "a", "b", 1e-8, 1e-8);
    let result = loadflow::solve(&p).unwrap();
    assert!(result.iterations > 1);
    if result.converged {
        assert!(result.max_mismatch_pu < 1e-5);
    } else {
        assert!(result.message.unwrap().contains("1e-5 pu roundoff limit"));
    }
}
