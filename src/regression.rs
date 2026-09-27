//! Independent circuit checks for the engineering defect fixes.
use crate::{cplx::Cplx, curves, exec, fault, linalg, loadflow, model::*, network, study::{self, Studies}, tcc};

fn near(got: f64, expected: f64, tol: f64) {
    assert!((got - expected).abs() <= tol, "got {got:.12}, expected {expected:.12} (tol {tol})");
}
fn transformer() -> Project {
    let mut p = Project::sample();
    p.buses.truncate(2);
    p.branches.truncate(1);
    p.loads.clear(); p.motors.clear(); p.devices.clear(); p.equipment.clear();
    p.buses[0].kv = 11.0;
    p.buses[1].kv = 0.5;
    p.branches[0].kind = BranchKind::Transformer {
        kva: 1000.0, z_percent: 5.0, xr: 0.0, hv_kv: 12.47, lv_kv: 0.48,
        connection: XfmrConn::Dyn, tap_percent: 0.0, x0_over_x1: 1.0, r0_over_r1: 1.0,
    };
    p.sources[0].xr = 0.0;
    p
}
#[test]
fn transformer_ratio_loaded_loss_reversal_taps_and_mva_base() {
    // Physical LV leakage resistance = .05 * .48^2 / 1 MVA ohms.
    // With unity-PF load, E = V + R*P/V in line-line kV and MW.
    let r = 0.05 * 0.48_f64.powi(2);
    for tap in [-5.0, 0.0, 5.0] {
        let e = 11.0 * 0.48 / (12.47 * (1.0 + tap / 100.0));
        for reverse in [false, true] {
            for base in [1.0, 100.0, 250.0] {
                let mut p = transformer(); p.s_base_mva = base;
                if let BranchKind::Transformer { tap_percent, connection, .. } = &mut p.branches[0].kind {
                    *tap_percent = tap;
                    if reverse { *connection = XfmrConn::Ynd; }
                }
                if reverse { let b = &mut p.branches[0]; std::mem::swap(&mut b.from, &mut b.to); }
                let lf = loadflow::solve(&p).unwrap(); assert!(lf.converged, "{:?}", lf.message);
                near(lf.buses[1].v_pu * 0.5, e, 1e-7);
                near(lf.buses[1].angle_deg, -30.0, 1e-5);
                p.loads.push(Load { id: "load".into(), name: "load".into(), bus: "mcc".into(), kw: 100.0, kvar: 0.0, basis: String::new() });
                let lf = loadflow::solve(&p).unwrap(); assert!(lf.converged, "{:?}", lf.message);
                let v = (e + (e*e - 4.0*r*0.1).sqrt()) / 2.0;
                let loss = r * (0.1 / v).powi(2);
                near(lf.buses[1].v_pu * 0.5, v, 1e-7);
                near(lf.branches[0].p_loss_mw, loss, 1e-8);
                let i_lv = 100.0 / (3.0_f64.sqrt() * v);
                let expected_i = if reverse { i_lv } else { i_lv * 0.48 / (12.47 * (1.0 + tap/100.0)) };
                near(lf.branches[0].i_from_a, expected_i, 1e-4);
                // Flat-prefault physical fault current is independent of the numerical MVA base.
                let ratio = 0.48 / (12.47 * (1.0 + tap/100.0));
                let z = r + 11.0_f64.powi(2) / p.sources[0].mva_sc * ratio.powi(2);
                let expected_ka = 0.5 / (3.0_f64.sqrt() * z);
                let f = fault::solve(&p, None).unwrap();
                near(f.buses[1].three_phase.as_ref().unwrap().symmetrical_ka, expected_ka, 1e-7);
                let expected_lg = 3.0 * 0.5 / (3.0_f64.sqrt() * (2.0*z + r));
                near(f.buses[1].line_to_ground.as_ref().unwrap().symmetrical_ka, expected_lg, 1e-7);
            }
        }
    }
    let mut p = transformer(); p.buses[1].kv = 0.48;
    near(loadflow::solve(&p).unwrap().buses[1].v_pu, 11.0/12.47, 1e-7);
}
fn pv_project() -> Project {
    let mut p = transformer();
    p.buses[0].kv = 13.8; p.buses[1].kv = 13.8;
    p.branches[0].kind = BranchKind::Line { r_ohm: 0.0, x_ohm: 0.1*13.8*13.8/100.0, b_siemens: 0.0, r0_ohm: 0.0, x0_ohm: 0.0, ampacity_a: None };
    let mut g = p.sources[0].clone(); g.id = "gen".into(); g.bus = "mcc".into(); g.is_slack = false;
    g.qmin_mvar = Some(-5.0); g.qmax_mvar = Some(5.0); p.sources.push(g);
    p.loads.push(Load { id: "load".into(), name: "load".into(), bus: "mcc".into(), kw: 0.0, kvar: 20_000.0, basis: String::new() }); p
}
#[test]
fn pv_limit_is_generator_q_minus_local_demand_and_sums_generators() {
    for split in [false, true] {
        let mut p = pv_project();
        if split {
            let mut g = p.sources[1].clone(); g.id = "gen2".into();
            g.qmax_mvar = Some(3.0); p.sources[1].qmax_mvar = Some(2.0); p.sources.push(g);
        }
        let lf = loadflow::solve(&p).unwrap(); assert!(lf.converged);
        near(lf.buses[1].q_mvar, -15.0, 1e-6);
        near(lf.buses[1].v_pu, (1.0 + (1.0-4.0*0.015_f64).sqrt())/2.0, 1e-8);
        assert_eq!(lf.buses[1].kind, "pq");
    }
}
#[test]
fn intermediate_newton_q_does_not_lock_pv_at_a_limit() {
    let mut p = pv_project(); p.loads.clear();
    p.sources[1].v_pu = 1.05; p.sources[1].p_mw = 50.0;
    p.sources[1].qmin_mvar = Some(53.0); p.sources[1].qmax_mvar = Some(100.0);
    // Initial Q is 52.5 Mvar, but the converged PV Q is above 53 Mvar.
    let expected_q = (1.05_f64.powi(2) - 1.05*(1.0-(0.05_f64/1.05).powi(2)).sqrt())/0.1*100.0;
    let lf = loadflow::solve(&p).unwrap(); assert!(lf.converged);
    assert_eq!(lf.buses[1].kind, "pv"); near(lf.buses[1].v_pu, 1.05, 1e-10);
    near(lf.buses[1].q_mvar, expected_q, 1e-6);
}
#[test]
fn resistive_source_has_no_dc_offset() {
    let mut p = transformer(); p.buses.truncate(1); p.branches.clear();
    let f = fault::solve(&p, None).unwrap(); let f = f.buses[0].three_phase.as_ref().unwrap();
    let i = 250.0 / (3.0_f64.sqrt()*11.0);
    near(f.symmetrical_ka, i, 1e-8); near(f.iec_peak_ka, 2.0_f64.sqrt()*i, 1e-8);
    near(f.half_cycle_rms_ka, i, 1e-8); assert_eq!(f.x_over_r, Some(0.0));
}
#[test]
fn capacitor_and_motor_cannot_energize_an_island() {
    let mut p = Project::sample(); p.branches.clear(); p.devices.clear(); p.equipment.clear();
    p.buses[1].shunt_kvar = 100.0;
    let f = fault::solve(&p, None).unwrap();
    assert!(f.buses[0].three_phase.is_some());
    for b in &f.buses[1..] { assert_eq!(b.prefault_pu, 0.0); assert!(b.three_phase.is_none()); assert!(b.line_to_ground.is_none()); }
    p.sources.clear();
    assert!(fault::solve(&p, None).unwrap().buses.iter().all(|b| b.three_phase.is_none()));
}
#[test]
fn motor_impedance_uses_nameplate_voltage_and_both_bases() {
    let mut p = Project::sample(); p.buses.truncate(1); p.branches.clear(); p.loads.clear(); p.devices.clear(); p.equipment.clear();
    p.buses[0].kv = 0.48; p.motors[0].bus = "util".into(); p.motors[0].kv = 0.46;
    for base in [1.0, 100.0] {
        p.s_base_mva = base;
        let net = network::build(&p).unwrap();
        let z = net.pos.y.rows()[0][0].1.inv().unwrap();
        let motor = &p.motors[0];
        let kva = motor.hp*0.746/motor.efficiency/motor.pf;
        let x_ohm = motor.x_subtransient_pu*0.46*0.46/(kva/1000.0);
        let zm = Cplx::new(x_ohm/motor.xr, x_ohm);
        let zs = Cplx::from_mag_xr(0.48*0.48/p.sources[0].mva_sc, p.sources[0].xr);
        let expected = (zm.inv().unwrap()+zs.inv().unwrap()).inv().unwrap();
        near(z.re*0.48*0.48/base, expected.re, 1e-10); near(z.im*0.48*0.48/base, expected.im, 1e-10);
    }
    p.motors[0].kv = 4.16; assert!(!p.validate().is_empty());
    p.motors[0].kv = 0.46; p.motors[0].xr = 0.0; assert!(network::build(&p).is_err());
}
#[test]
fn zero_sequence_scales_resistance_and_reactance_separately() {
    let mut p = transformer();
    if let BranchKind::Transformer { xr, r0_over_r1, x0_over_x1, .. } = &mut p.branches[0].kind { *xr = 4.0; *r0_over_r1 = 2.0; *x0_over_x1 = 3.0; }
    let net = network::build(&p).unwrap();
    let z0 = net.zero.y.rows()[1][0].1.inv().unwrap();
    let mag = 0.05*100.0*(0.48_f64/0.5).powi(2);
    let r = mag/17.0_f64.sqrt(); near(z0.re, 2.0*r, 1e-10); near(z0.im, 12.0*r, 1e-10);
    p = pv_project();
    if let BranchKind::Line { b_siemens, .. } = &mut p.branches[0].kind { *b_siemens = 0.001; }
    let net = network::build(&p).unwrap();
    assert!(net.warnings.iter().any(|w| w.contains("R0=3R1")));
    assert!(net.warnings.iter().any(|w| w.contains("charging omitted")));
    if let network::ZeroStamp::Series(q) = net.branches[0].zero { near((q.yff + q.yft).abs(), 0.0, 1e-12); } else { panic!(); }
}
#[test]
fn missing_prefault_invalidates_fault_and_preserves_failed_arc_cases() {
    let mut p = Project::sample(); p.prefault = Prefault::Loadflow;
    let f = fault::solve(&p, None).unwrap(); assert!(!f.valid); assert_eq!(f.prefault, "none"); assert_eq!(f.prefault_requested, "loadflow");
    assert!(f.buses.iter().all(|b| b.three_phase.is_none()));
    p.sources[0].is_slack = false;
    let out = study::run(&p, Studies::all()).unwrap();
    assert!(!out.fault.as_ref().unwrap().valid); assert!(out.arc_flash.as_ref().unwrap().is_empty());
    assert_eq!(out.arc_flash_failures.len(), 3);
    assert!(study::text_report(&p, &out).contains("method used: none"));
}
#[test]
fn unknown_curves_return_status_and_no_unbounded_decade_loop() {
    let mut p = Project::sample();
    for d in &mut p.devices { d.curve = CurveSpec::SettingsNotCollected { note: String::new() }; }
    assert!(tcc::plot(&p, None, None, &[]).unwrap_err().contains("no plottable"));
    assert!(tcc::plot(&Project::sample(), None, Some(f64::INFINITY), &[]).is_err());
}
#[test]
fn total_clearing_needs_explicit_breaker_time_and_instantaneous_has_no_default() {
    let mut d = Project::sample().devices.remove(0);
    d.curve = CurveSpec::Iec { kind: IecKind::Si, pickup_a: 100.0, tms: 1.0, inst_a: Some(500.0), inst_s: None };
    assert!(curves::trip_time(&d.curve, 1000.0).is_none());
    let expected = 0.14/(2.0_f64.powf(0.02)-1.0);
    near(curves::trip_time(&d.curve, 200.0).unwrap(), expected, 1e-10);
    d.curve = CurveSpec::Definite { pickup_a: 100.0, time_s: 0.2 };
    d.breaker_interrupting_s = None; assert!(curves::clearing_time(&d, 1000.0).is_none());
    d.breaker_interrupting_s = Some(0.08); near(curves::clearing_time(&d, 1000.0).unwrap(), 0.28, 1e-12);
    d.breaker_interrupting_s = None; d.fuse_total_clearing = true; near(curves::clearing_time(&d, 1000.0).unwrap(), 0.2, 1e-12);
}
#[test]
fn invalid_inputs_are_rejected_not_silently_dropped() {
    assert!(linalg::solve_sparse(1, &[(0,0,1.0),(0,0,f64::NAN)], &[1.0]).is_err());
    assert!(linalg::solve_sparse(1, &[(0,0,1.0)], &[f64::INFINITY]).is_err());
    assert!(Cplx::ZERO.inv().is_none()); assert!(Cplx::new(f64::NAN, 1.0).inv().is_none());
    assert!(std::panic::catch_unwind(|| Cplx::real(1.0)/Cplx::ZERO).is_err());
    let mut p = transformer(); if let BranchKind::Transformer { tap_percent, .. } = &mut p.branches[0].kind { *tap_percent = -100.0; }
    assert!(study::run(&p, Studies::all()).is_err());
    let mut p = Project::sample(); p.motors[0].x_subtransient_pu = -0.1; assert!(!p.validate().is_empty());
    p = Project::sample(); p.devices[0].curve = CurveSpec::Definite { pickup_a: -1.0, time_s: 0.1 }; assert!(!p.validate().is_empty());
    p = Project::sample(); p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: -0.1 }; assert!(!p.validate().is_empty());
    p = Project::sample(); p.equipment[0].width_mm = 0.0; assert!(!p.validate().is_empty());
    p = Project::sample(); p.s_base_mva = f64::NAN; assert!(network::build(&p).is_err());
    p = Project::sample(); if let BranchKind::Line { r_ohm, .. } = &mut p.branches[1].kind { *r_ohm = f64::INFINITY; } assert!(network::build(&p).is_err());
}
#[test]
fn model_mutation_clears_every_artifact_and_partial_runs_are_completed() {
    let r = exec::exec_request(r#"{"commands":[{"op":"sample"},{"op":"run"},{"op":"sld"},{"op":"tcc"},{"op":"export_skm"},{"op":"set_source","id":"grid","mva_sc":25}] }"#);
    assert!(r.ok, "{:?}", r.error); assert!(r.results.is_none() && r.sld_svg.is_none() && r.tcc_svg.is_none() && r.tcc_csv.is_none() && r.skm.is_none());
    let r = exec::exec_request(r#"{"commands":[{"op":"sample"},{"op":"run"},{"op":"set_source","id":"grid","mva_sc":25},{"op":"sld"}]}"#);
    assert!(r.ok, "{:?}", r.error);
    // Utility-bus fault includes the sample motor behind the transformer.
    let zs = Cplx::from_mag_xr(12.47*12.47/25.0, 12.0);
    let zt = Cplx::from_mag_xr(0.0575*12.47*12.47/2.5, 7.5);
    let xm = 0.17*12.47*12.47 / (200.0*0.746/0.94/0.88/1000.0);
    let zm = Cplx::new(xm/6.0, xm);
    let expected = 12.47/3.0_f64.sqrt() * (zs.inv().unwrap() + (zt+zm).inv().unwrap()).abs();
    near(r.results.as_ref().unwrap().fault.as_ref().unwrap().buses[0].three_phase.as_ref().unwrap().symmetrical_ka, expected, 1e-8);
    let r = exec::exec_request(r#"{"commands":[{"op":"sample"},{"op":"run","studies":["loadflow"]},{"op":"sld"}]}"#);
    assert!(r.ok); assert!(r.results.unwrap().fault.is_some());
    let r = exec::exec_request(r#"{"commands":[{"op":"sample"},{"op":"sld"},{"op":"run","studies":["bad"]}]}"#);
    assert!(!r.ok); assert!(r.results.is_none() && r.sld_svg.is_none());
}
#[test]
fn parallel_infeeds_use_terminal_contributions_not_total_bus_current() {
    let mut p = Project::sample(); p.motors.clear(); p.loads.clear(); p.equipment.clear();
    for b in &mut p.buses { b.kv = 0.48; }
    for (j,b) in p.branches.iter_mut().enumerate() {
        b.from = if j == 0 { "util".into() } else { "pnl".into() }; b.to = "mcc".into();
        b.kind = BranchKind::Line { r_ohm: 0.01, x_ohm: 0.0, b_siemens: 0.0, r0_ohm: 0.03, x0_ohm: 0.0, ampacity_a: None };
    }
    p.sources[0].xr = 0.0; p.sources[0].mva_sc = 0.48*0.48/0.01;
    let mut s = p.sources[0].clone(); s.id = "second".into(); s.bus = "pnl".into(); s.is_slack = false; p.sources.push(s);
    p.devices.truncate(1); p.devices[0].curve = CurveSpec::Definite { pickup_a: 100.0, time_s: 0.2 };
    p.devices[0].breaker_interrupting_s = Some(0.08);
    let out = study::run(&p, Studies::all()).unwrap();
    let expected_branch_a = 480.0/(3.0_f64.sqrt()*0.02);
    let f = &out.fault.as_ref().unwrap().buses[1];
    near(f.three_phase.as_ref().unwrap().symmetrical_ka*1000.0, 2.0*expected_branch_a, 1e-7);
    for terminal in &f.terminal_currents { near(terminal.to_a, expected_branch_a, 1e-7); }
    let c = &out.coordination.as_ref().unwrap()[0]; near(c.bolted_ka.unwrap()*1000.0, expected_branch_a, 1e-7);
    near(c.trip_at_bolted_s.unwrap(), 0.2, 1e-12); near(c.total_clearing_s.unwrap(), 0.28, 1e-12);
    // A lone device on the bus is not inferred as upstream equipment protection.
    assert!(out.arc_flash_failures.iter().any(|f| f.bus_id == "mcc" && f.error.contains("no upstream")));
}

#[test]
fn reactive_transformer_load_matches_physical_quadratic_in_both_directions() {
    for tap in [-5.0, 5.0] {
        for reverse in [false, true] {
            for base in [1.0, 250.0] {
                let mut p = transformer(); p.s_base_mva = base;
                if let BranchKind::Transformer { xr, tap_percent, connection, .. } = &mut p.branches[0].kind {
                    *xr = 7.5; *tap_percent = tap;
                    if reverse { *connection = XfmrConn::Ynd; }
                }
                if reverse { let b = &mut p.branches[0]; std::mem::swap(&mut b.from, &mut b.to); }
                p.loads.push(Load { id:"load".into(), name:"load".into(), bus:"mcc".into(), kw:100.0, kvar:30.0, basis:String::new() });
                let r = (0.05*0.48*0.48)/ (1.0+7.5_f64.powi(2)).sqrt(); let x = 7.5*r;
                let e = 11.0*0.48/(12.47*(1.0+tap/100.0));
                let c = e*e - 2.0*(r*0.1+x*0.03);
                let u = (c+(c*c-4.0*(r*r+x*x)*(0.1*0.1+0.03*0.03)).sqrt())/2.0;
                let loss = r*(0.1*0.1+0.03*0.03)/u;
                let lf = loadflow::solve(&p).unwrap(); assert!(lf.converged);
                near(lf.buses[1].v_pu*0.5, u.sqrt(), 1e-9);
                near(lf.branches[0].p_loss_mw, loss, 1e-9);
                let v_phase = lf.buses[1].angle_deg.to_radians();
                // E leads secondary voltage by the angle of V + Z*S*/V.
                let angle = -30.0 - ((x*0.1-r*0.03)/(u+r*0.1+x*0.03)).atan().to_degrees();
                near(v_phase.to_degrees(), angle, 1e-7);
            }
        }
    }
}

#[test]
fn arc_failures_remain_visible_and_manual_time_can_complete_a_case() {
    let mut p = Project::sample(); p.devices[0].protected_branch = None;
    let out = study::run(&p, Studies::all()).unwrap();
    assert!(out.arc_flash_failures.iter().any(|f| f.equipment_id == "af-mcc" && f.error.contains("terminal")));
    let svg = crate::sld::to_svg(&crate::sld::diagram(&p, &out)); assert!(svg.contains("ARC FLASH FAILED"));
    p.equipment[0].clearing_s = Some(0.2);
    let out = study::run(&p, Studies::all()).unwrap();
    let row = out.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == "af-mcc").unwrap(); near(row.time_s, 0.2, 1e-12);
    p.buses[0].kv = 20.0;
    let out = study::run(&p, Studies::all()).unwrap();
    assert!(out.arc_flash_failures.iter().any(|f| f.bus_id == "util" && f.error.contains("outside")));
}

#[test]
fn tcc_decades_and_skm_package_have_explicit_status_and_identity() {
    assert!(tcc::decades(f64::MAX, f64::INFINITY).is_empty());
    assert!(tcc::decades(0.0, 100.0).is_empty());
    assert_eq!(tcc::decades(1.0, 1000.0), vec![1.0,10.0,100.0,1000.0]);
    let package = crate::skm::export(&Project::sample());
    let notes = &package.files.iter().find(|f| f.name == "IMPORT.txt").unwrap().content;
    assert!(notes.contains("engineering data package for mapping"));
    assert!(notes.contains("not a verified PTW import"));
    assert!(!package.xml().contains("SKMDataExchange"));
    assert!(package.files.iter().any(|f| f.name == "project.json"));
}

#[test]
fn pv_minimum_limit_accounts_for_local_reactive_supply() {
    let mut p = pv_project(); p.loads[0].kvar = -20_000.0;
    let lf = loadflow::solve(&p).unwrap(); assert!(lf.converged);
    near(lf.buses[1].q_mvar, 15.0, 1e-7);
    near(lf.buses[1].v_pu, (1.0+(1.0+4.0*0.015_f64).sqrt())/2.0, 1e-9);
}

#[test]
fn nonconverged_prefault_does_not_fall_back_to_flat() {
    let mut p = pv_project(); p.sources.truncate(1); p.prefault = Prefault::Loadflow;
    p.loads[0].kw = 2_000_000.0; p.loads[0].kvar = 0.0;
    // Pmax through X=.1 pu is 500 MW, well below this 2000 MW load.
    let lf = loadflow::solve(&p).unwrap(); assert!(!lf.converged);
    let f = fault::solve(&p, Some(&lf)).unwrap(); assert!(!f.valid);
    assert_eq!(f.prefault, "none"); assert!(f.buses.iter().all(|b| b.three_phase.is_none()));
}
