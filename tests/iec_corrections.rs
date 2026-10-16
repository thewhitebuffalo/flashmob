//! IEC 60909-0 impedance correction factors and the meshed/radial peak basis.
//! Built from code, not a client project.

use flashmob::fault::{self, iec_kappa, iec_voltage_factor};
use flashmob::iec::{self, k_g, k_s, k_so, k_t};
use flashmob::model::*;
use flashmob::study::{self, Studies};

fn close(got: f64, want: f64) {
    let scale = got.abs().max(want.abs()).max(1.0);
    assert!((got - want).abs() <= 1e-9 * scale, "{got} vs {want}");
}

fn fault_only() -> Studies {
    Studies { loadflow: false, fault: true, arcflash: false, coordination: false }
}

fn bus<'a>(f: &'a fault::FaultResult, id: &str) -> &'a fault::BusFault {
    f.buses.iter().find(|b| b.id == id).unwrap()
}

fn relative_reactance(z_percent: f64, xr: f64) -> f64 {
    z_percent / 100.0 * xr / (1.0 + xr * xr).sqrt()
}

#[test]
fn network_transformer_factor_corrects_only_the_maximum() {
    let project = Project::sample();
    let result = fault::solve(&project, None).unwrap();
    assert_eq!(result.iec_corrections.len(), 1);
    let c = &result.iec_corrections[0];
    assert_eq!((c.element_id.as_str(), c.factor_name.as_str()), ("t1", "K_T"));
    let cmax_lv = iec_voltage_factor(0.48, false).unwrap().1;
    close(c.factor, k_t(relative_reactance(5.75, 7.5), cmax_lv));
    // The maximum equals the fault current of the hand-corrected network times cmax.
    let mut corrected = project.clone();
    if let BranchKind::Transformer { z_percent, .. } = &mut corrected.branches[0].kind { *z_percent *= c.factor; }
    let by_hand = fault::solve_uncorrected(&corrected, None).unwrap();
    for id in ["mcc", "pnl"] {
        let p = bus(&result, id).three_phase.as_ref().unwrap();
        assert!(p.iec_max_corrected);
        close(p.symmetrical_max_ka, bus(&by_hand, id).three_phase.as_ref().unwrap().symmetrical_ka * p.c_max);
        close(p.symmetrical_min_ka, p.symmetrical_ka * p.c_min);
        assert!(p.symmetrical_max_ka > p.symmetrical_ka * p.c_max);
    }
    // The uncorrected path keeps the old maximum.
    let plain = fault::solve_uncorrected(&project, None).unwrap();
    let p = bus(&plain, "mcc").three_phase.as_ref().unwrap();
    assert!(!p.iec_max_corrected);
    close(p.symmetrical_max_ka, p.symmetrical_ka * p.c_max);
}

fn expected_peak(p: &fault::FaultPoint, kv: f64, meshed: bool) -> f64 {
    let rx = p.x_over_r.map_or(0.0, |xr| 1.0 / xr);
    iec_kappa(rx, kv, meshed) * std::f64::consts::SQRT_2 * p.symmetrical_max_ka
}

#[test]
fn radial_networks_drop_the_meshed_factor() {
    let result = fault::solve(&Project::sample(), None).unwrap();
    for b in &result.buses {
        for p in [&b.three_phase, &b.line_to_ground].into_iter().flatten() {
            assert_eq!(p.peak_basis, "radial");
            close(p.iec_peak_ka, expected_peak(p, b.kv, false));
        }
    }
}

#[test]
fn a_loop_selects_method_b_unless_every_branch_has_low_r_over_x() {
    let mut meshed = Project::sample();
    let mut twin = meshed.branches[1].clone();
    twin.id = "feeder_b".into();
    meshed.branches.push(twin);
    let result = fault::solve(&meshed, None).unwrap();
    let p = bus(&result, "pnl").three_phase.as_ref().unwrap();
    assert_eq!(p.peak_basis, "meshed");
    close(p.iec_peak_ka, expected_peak(p, 0.48, true));
    // The utility bus is joined to the loop through t1, so it is meshed too.
    assert_eq!(bus(&result, "util").three_phase.as_ref().unwrap().peak_basis, "meshed");

    let mut low = Project::sample();
    if let BranchKind::Line { x_ohm, .. } = &mut low.branches[1].kind { *x_ohm = 0.02; }
    let mut t1b = low.branches[0].clone();
    t1b.id = "t1b".into();
    low.branches.push(t1b);
    let result = fault::solve(&low, None).unwrap();
    let p = bus(&result, "mcc").three_phase.as_ref().unwrap();
    assert_eq!(p.peak_basis, "meshed_low_rx");
    close(p.iec_peak_ka, expected_peak(p, 0.48, false));
    // An open switch on the parallel branch makes the network radial again.
    low.switches.push(Switch { id: "sw".into(), name: "SW".into(), branch_id: "t1b".into(), kind: SwitchKind::Breaker, closed: false });
    assert!(low.validate().is_empty(), "{:?}", low.validate());
    let result = fault::solve(&low, None).unwrap();
    assert_eq!(bus(&result, "mcc").three_phase.as_ref().unwrap().peak_basis, "radial");
}

fn generator_project(rating: Option<GeneratorRating>) -> Project {
    let mut p = Project::sample();
    let mut gen = p.sources[0].clone();
    gen.id = "gen".into();
    gen.bus = "mcc".into();
    gen.is_slack = false;
    gen.mva_sc = 8.0;
    gen.xr = 20.0;
    gen.machine_kind = Some(GeneratorKind::Turbogenerator);
    gen.generator = rating;
    p.sources.push(gen);
    p
}

#[test]
fn generator_factor_uses_the_entered_rating() {
    let rating = GeneratorRating { rated_mva: 1.0, rated_pf: 0.8, rated_kv: Some(0.48), unit: None };
    let p = generator_project(Some(rating));
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    let result = fault::solve(&p, None).unwrap();
    let g = result.iec_corrections.iter().find(|c| c.element_id == "gen").unwrap();
    assert_eq!(g.factor_name, "K_G");
    // x_d'' on the generator base from the entered short-circuit MVA and X/R.
    let x_ohm = 0.48 * 0.48 / 8.0 * 20.0 / 401f64.sqrt();
    let x_d = x_ohm * 1.0 / (0.48 * 0.48);
    close(g.factor, k_g(0.48, 0.48, 1.05, x_d, 0.6));
    assert!(result.iec_corrections.iter().any(|c| c.element_id == "t1" && c.factor_name == "K_T"));
    assert!(result.iec_warnings.is_empty(), "{:?}", result.iec_warnings);
}

#[test]
fn missing_generator_data_is_reported_not_guessed() {
    let p = generator_project(None);
    let out = study::run(&p, fault_only()).unwrap();
    assert!(out.warnings.iter().any(|w| w.contains("source gen: generator rating is absent")), "{:?}", out.warnings);
    assert!(!out.fault.as_ref().unwrap().iec_corrections.iter().any(|c| c.element_id == "gen"));
    let p = generator_project(Some(GeneratorRating { rated_mva: 1.0, rated_pf: 0.8, rated_kv: None, unit: None }));
    let result = fault::solve(&p, None).unwrap();
    assert!(result.iec_warnings.iter().any(|w| w.contains("rated_kv is absent")), "{:?}", result.iec_warnings);
}

fn unit_project(on_load_tap_changer: bool, p_g: Option<f64>, p_t: Option<f64>) -> Project {
    // The generator sits on the transformer's LV bus, so t1 is its unit transformer.
    let mut p = generator_project(Some(GeneratorRating {
        rated_mva: 2.5, rated_pf: 0.85, rated_kv: Some(0.48),
        unit: Some(PowerStationUnit { transformer: "t1".into(), on_load_tap_changer, p_g, p_t }),
    }));
    p.sources[1].mva_sc = 20.0;
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    p
}

#[test]
fn power_station_unit_replaces_the_generator_and_transformer_factors() {
    let x_ohm = 0.48 * 0.48 / 20.0 * 20.0 / 401f64.sqrt();
    let x_d = x_ohm * 2.5 / (0.48 * 0.48);
    let x_t = relative_reactance(5.75, 7.5);
    let sin_phi = (1.0 - 0.85f64 * 0.85).sqrt();
    let result = fault::solve(&unit_project(true, None, None), None).unwrap();
    let names: Vec<_> = result.iec_corrections.iter().map(|c| (c.element_id.as_str(), c.factor_name.as_str())).collect();
    assert_eq!(names, vec![("gen", "K_S"), ("t1", "K_S")]);
    close(result.iec_corrections[0].factor, k_s(12.47, 0.48, 0.48, 12.47, 1.10, x_d, x_t, sin_phi));

    let result = fault::solve(&unit_project(false, Some(0.05), Some(-0.025)), None).unwrap();
    assert_eq!(result.iec_corrections[0].factor_name, "K_SO");
    close(result.iec_corrections[0].factor, k_so(12.47, 0.48, 0.05, 0.48, 12.47, -0.025, 1.10, x_d, sin_phi));
    assert!(result.iec_warnings.is_empty());

    let result = fault::solve(&unit_project(false, None, None), None).unwrap();
    assert_eq!(result.iec_warnings.len(), 2, "{:?}", result.iec_warnings);
}

#[test]
fn generator_rating_is_validated_and_round_trips() {
    let mut p = unit_project(true, None, None);
    p.sources[1].generator.as_mut().unwrap().unit.as_mut().unwrap().transformer = "feeder".into();
    assert!(p.validate().iter().any(|e| e.contains("generator rating")), "{:?}", p.validate());
    let mut p = generator_project(Some(GeneratorRating { rated_mva: 1.0, rated_pf: 1.2, rated_kv: None, unit: None }));
    assert!(p.validate().iter().any(|e| e.contains("generator rating")));
    p.sources[1].generator.as_mut().unwrap().rated_pf = 0.9;
    p.sources[1].machine_kind = Some(GeneratorKind::Utility);
    assert!(p.validate().iter().any(|e| e.contains("generator rating")));

    let p = unit_project(false, Some(0.05), None);
    let text = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(back.sources[1].generator, p.sources[1].generator);
    assert!(!serde_json::to_string(&Project::sample()).unwrap().contains("generator"));
    let _ = iec::corrections(&back);
}
