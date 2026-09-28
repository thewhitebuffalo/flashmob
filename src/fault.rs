use serde::{Deserialize, Serialize};

use crate::cplx::Cplx;
use crate::loadflow::LoadflowResult;
use crate::model::{i_base_ka, Prefault, Project};
use crate::network::{self, Net};
use crate::solve::AdmittanceSolver;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaultResult {
    pub prefault: String,
    pub prefault_requested: String,
    pub valid: bool,
    pub error: Option<String>,
    pub buses: Vec<BusFault>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusFault {
    pub id: String,
    pub name: String,
    pub kv: f64,
    pub prefault_pu: f64,
    pub three_phase: Option<FaultPoint>,
    pub line_to_ground: Option<FaultPoint>,
    pub line_to_line: Option<FaultPoint>,
    pub line_to_line_ground: Option<FaultPoint>,
    pub note: Option<String>,
    pub terminal_currents: Vec<TerminalCurrent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerminalCurrent {
    pub branch_id: String,
    pub from_a: f64,
    pub to_a: f64,
    /// Rectangular amperes, [real, imaginary]. Flat prefault neglects initial load flow.
    pub from_prefault_a: [f64; 2],
    pub to_prefault_a: [f64; 2],
    pub from_increment_a: [f64; 2],
    pub to_increment_a: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaultPoint {
    pub symmetrical_ka: f64,
    pub ia_ka: f64,
    pub ib_ka: f64,
    pub ic_ka: f64,
    pub ig_ka: f64,
    /// None denotes the purely inductive limit (infinite X/R).
    pub x_over_r: Option<f64>,
    pub iec_peak_ka: f64,
    pub half_cycle_rms_ka: f64,
}

pub fn solve(project: &Project, loadflow: Option<&LoadflowResult>) -> Result<FaultResult, String> {
    let model = network::build(project)?;
    solve_with_model(project, loadflow, &model)
}

pub(crate) fn solve_with_model(project: &Project, loadflow: Option<&LoadflowResult>, model: &network::SystemModel) -> Result<FaultResult, String> {
    let n = model.index.n;
    let (z1, map1) = reduced(&model.pos, &model.energized)?;
    let (z2, map2) = reduced(&model.neg, &model.energized)?;
    let (z0, map0) = reduced(&model.zero, &model.energized)?;

    let mut pos = AdmittanceSolver::new(&z1);
    let mut neg = AdmittanceSolver::new(&z2);
    let mut zero = AdmittanceSolver::new(&z0);

    let requested = if project.prefault == Prefault::Loadflow { "loadflow" } else { "flat_1.0_pu" };
    let lf_valid = loadflow.map_or(false, |lf| lf.converged && model.index.id_of.iter().enumerate().all(|(i, id)|
        lf.buses.iter().any(|b| b.id == *id && b.angle_deg.is_finite() && if model.energized[i] {
            b.v_pu.is_finite() && b.v_pu > 0.0
        } else {
            b.kind == "unenergized" && b.v_pu == 0.0
        })));
    let valid = project.prefault != Prefault::Loadflow || lf_valid;
    let error = if valid { None } else { Some("requested loadflow prefault unavailable or unconverged; method used: none; fault and arc-flash results invalid".to_string()) };
    let mut prefault_v = vec![Cplx::real(1.0); n];
    let mut label = "flat_1.0_pu".to_string();
    if project.prefault == Prefault::Loadflow {
        if let Some(lf) = loadflow.filter(|_| lf_valid) {
            for bus in &lf.buses {
                if let Ok(i) = model.index.of(&bus.id) {
                    prefault_v[i] = Cplx::from_polar(bus.v_pu, bus.angle_deg.to_radians());
                }
            }
            label = "loadflow".into();
        } else {
            label = "none".into();
        }
    }

    let mut buses = Vec::with_capacity(n);
    for i in 0..n {
        if !valid || !model.energized[i] {
            buses.push(BusFault {
                id: model.index.id_of[i].clone(), name: model.index.name_of[i].clone(), kv: model.index.kv[i],
                prefault_pu: 0.0, three_phase: None, line_to_ground: None, line_to_line: None, line_to_line_ground: None,
                terminal_currents: Vec::new(),
                note: Some(error.clone().unwrap_or_else(|| "unenergized island: no utility/generator source".into())),
            });
            continue;
        }
        let v = prefault_v[i];
        let ibase = i_base_ka(model.index.kv[i], project.s_base_mva);
        let column1 = map1[i].map(|k| pos.column(k)).transpose()?;
        let zz1 = map1[i].zip(column1.as_ref()).map(|(k, col)| col[k]);
        let zz2 = neg.zth(map2[i])?;
        let zz0 = zero.zth(map0[i])?;
        let mut note = None;
        let three = match zz1 {
            Some(z) => { checked_impedance(z)?; Some(point(v / z, Cplx::ZERO, Cplx::ZERO, z, ibase)?) },
            None => None,
        };
        let line_line = match (zz1, zz2) {
            (Some(a), Some(b)) => {
                checked_impedance(a + b)?;
                let i1 = v / (a + b);
                Some(point(i1, -i1, Cplx::ZERO, a + b, ibase)?)
            }
            _ => None,
        };
        let (line_ground, double) = match (zz1, zz2, zz0) {
            (Some(a), Some(b), Some(c)) => {
                checked_impedance(a + b + c)?;
                checked_impedance(b + c)?;
                let i1 = v / (a + b + c);
                let lg = point(i1, i1, i1, a + b + c, ibase)?;
                let zpar = (b * c) / (b + c);
                checked_impedance(a + zpar)?;
                let i1g = v / (a + zpar);
                let i2 = -i1g * c / (b + c);
                let i0 = -i1g * b / (b + c);
                (Some(lg), Some(point(i1g, i2, i0, a + zpar, ibase)?))
            }
            (Some(a), Some(b), None) => {
                note = Some("no zero-sequence path; ground fault current is zero".into());
                checked_impedance(a + b)?;
                let i1 = v / (a + b);
                (Some(zero_point()), Some(point(i1, -i1, Cplx::ZERO, a + b, ibase)?))
            }
            _ => (None, None),
        };
        if three.is_none() {
            note = Some("bus is not connected to a source".into());
        }
        // Solve the voltage decrement caused by the fault current injection.
        // LF prefault includes initial branch flow; flat prefault neglects initial load flow.
        let mut terminal_currents = Vec::new();
        if let (Some(mut drop), Some(z)) = (column1, zz1) {
            // Y^-1 (e_k * I_fault) = (Y^-1 e_k) * I_fault.
            let current = v / z;
            for value in &mut drop { *value = *value * current; }
            if drop.iter().any(|v| !v.re.is_finite() || !v.im.is_finite()) {
                return Err("non-finite linear solution".into());
            }
            terminal_currents.reserve(project.branches.len());
            for (branch, stamp) in project.branches.iter().zip(&model.branches) {
                if !stamp.active { continue; }
                let (Some(f), Some(t)) = (map1[stamp.from], map1[stamp.to]) else { continue };
                let q = stamp.positive;
                let bf = i_base_ka(model.index.kv[stamp.from], project.s_base_mva) * 1000.0;
                let bt = i_base_ka(model.index.kv[stamp.to], project.s_base_mva) * 1000.0;
                let pre_f = if project.prefault == Prefault::Loadflow { (q.yff * prefault_v[stamp.from] + q.yft * prefault_v[stamp.to]) * bf } else { Cplx::ZERO };
                let pre_t = if project.prefault == Prefault::Loadflow { (q.ytf * prefault_v[stamp.from] + q.ytt * prefault_v[stamp.to]) * bt } else { Cplx::ZERO };
                let inc_f = (q.yff * drop[f] + q.yft * drop[t]) * bf;
                let inc_t = (q.ytf * drop[f] + q.ytt * drop[t]) * bt;
                terminal_currents.push(TerminalCurrent {
                    branch_id: branch.id.clone(),
                    from_a: (pre_f - inc_f).abs(), to_a: (pre_t - inc_t).abs(),
                    from_prefault_a: [pre_f.re, pre_f.im], to_prefault_a: [pre_t.re, pre_t.im],
                    from_increment_a: [inc_f.re, inc_f.im], to_increment_a: [inc_t.re, inc_t.im],
                });
            }
        }
        buses.push(BusFault {
            id: model.index.id_of[i].clone(),
            name: model.index.name_of[i].clone(),
            kv: model.index.kv[i],
            prefault_pu: v.abs(),
            three_phase: three,
            line_to_ground: line_ground,
            line_to_line: line_line,
            line_to_line_ground: double,
            note,
            terminal_currents,
        });
    }
    Ok(FaultResult { prefault: label, prefault_requested: requested.into(), valid, error, buses })
}

fn reduced(net: &Net, energized: &[bool]) -> Result<(crate::network::SparseY, Vec<Option<usize>>), String> {
    let mask: Vec<bool> = network::reaches_shunt(net).iter().zip(energized).map(|(a, b)| *a && *b).collect();
    Ok(network::reduce(&net.y, &mask))
}

fn point(i1: Cplx, i2: Cplx, i0: Cplx, zth: Cplx, ibase: f64) -> Result<FaultPoint, String> {
    let (ia, ib, ic) = phases(i0, i1, i2);
    let ig = i0 * 3.0;
    let sym = ia.abs().max(ib.abs()).max(ic.abs()) * ibase;
    let (xr, peak, asym) = asymmetry(sym, zth)?;
    Ok(FaultPoint {
        symmetrical_ka: sym,
        ia_ka: ia.abs() * ibase,
        ib_ka: ib.abs() * ibase,
        ic_ka: ic.abs() * ibase,
        ig_ka: ig.abs() * ibase,
        x_over_r: xr,
        iec_peak_ka: peak,
        half_cycle_rms_ka: asym,
    })
}

fn zero_point() -> FaultPoint {
    FaultPoint {
        symmetrical_ka: 0.0,
        ia_ka: 0.0,
        ib_ka: 0.0,
        ic_ka: 0.0,
        ig_ka: 0.0,
        x_over_r: Some(0.0),
        iec_peak_ka: 0.0,
        half_cycle_rms_ka: 0.0,
    }
}

fn phases(i0: Cplx, i1: Cplx, i2: Cplx) -> (Cplx, Cplx, Cplx) {
    let a = Cplx::from_polar(1.0, 2.0 * std::f64::consts::PI / 3.0);
    let a2 = a.conj();
    (i0 + i1 + i2, i0 + a2 * i1 + a * i2, i0 + a * i1 + a2 * i2)
}

fn checked_impedance(z: Cplx) -> Result<(), String> {
    if z.inv().is_none() { Err("zero or non-finite fault impedance denominator".into()) } else { Ok(()) }
}

fn asymmetry(i_sym: f64, z: Cplx) -> Result<(Option<f64>, f64, f64), String> {
    // Remove relative roundoff from phase-shift stamps, not physical reactance.
    let tol = z.abs() * 1e-12;
    let z = Cplx::new(if z.re.abs() <= tol { 0.0 } else { z.re }, if z.im.abs() <= tol { 0.0 } else { z.im });
    if !i_sym.is_finite() || i_sym < 0.0 || !z.re.is_finite() || !z.im.is_finite()
        || z.re < 0.0 || z.im < 0.0 || z.abs() == 0.0 {
        return Err("asymmetry requires finite nonnegative R and X and nonzero impedance".into());
    }
    if z.im == 0.0 {
        return Ok((Some(0.0), std::f64::consts::SQRT_2 * i_sym, i_sym));
    }
    let r_over_x = z.re / z.im;
    let xr = if z.re == 0.0 { None } else { Some(z.im / z.re) };
    let kappa = 1.02 + 0.98 * (-3.0 * r_over_x).exp();
    let peak = kappa * std::f64::consts::SQRT_2 * i_sym;
    let asym = i_sym * (1.0 + 2.0 * (-2.0 * std::f64::consts::PI * r_over_x).exp()).sqrt();
    if !peak.is_finite() || !asym.is_finite() { return Err("non-finite asymmetrical current".into()); }
    Ok((xr, peak, asym))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn utility_fault_matches_short_circuit_mva() {
        let project = Project {
            buses: vec![Bus { id: "a".into(), name: "A".into(), kv: 13.8, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None }],
            sources: vec![Source {
                id: "s".into(),
                name: "grid".into(),
                bus: "a".into(),
                in_service: true,
                decrement_curve: Vec::new(),
                v_pu: 1.0,
                angle_deg: 0.0,
                mva_sc: 500.0,
                xr: 15.0,
                x0_over_x1: 1.0,
                r0_over_r1: 1.0,
                is_slack: true,
                p_mw: 0.0,
                qmin_mvar: None,
                qmax_mvar: None,
            }],
            ..Project::default()
        };
        let fault = solve(&project, None).unwrap();
        let expect = 500.0 / (3.0_f64.sqrt() * 13.8);
        let got = fault.buses[0].three_phase.as_ref().unwrap().symmetrical_ka;
        assert!((got - expect).abs() / expect < 1e-6, "got {got}, expected {expect}");
    }

    #[test]
    fn dyn_transformer_blocks_utility_zero_sequence() {
        let project = Project {
            s_base_mva: 100.0,
            buses: vec![
                Bus { id: "hv".into(), name: "HV".into(), kv: 12.47, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None },
                Bus { id: "lv".into(), name: "LV".into(), kv: 0.48, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None },
            ],
            branches: vec![Branch {
                id: "t".into(),
                name: "T".into(),
                from: "hv".into(),
                to: "lv".into(),
                kind: BranchKind::Transformer {
                    kva: 1000.0,
                    z_percent: 5.0,
                    xr: 20.0,
                    hv_kv: 12.47,
                    lv_kv: 0.48,
                    connection: XfmrConn::Dyn,
                    tap_percent: 0.0,
                    x0_over_x1: 1.0,
                    r0_over_r1: 1.0,
                },
            }],
            sources: vec![Source {
                id: "s".into(),
                name: "grid".into(),
                bus: "hv".into(),
                in_service: true,
                decrement_curve: Vec::new(),
                v_pu: 1.0,
                angle_deg: 0.0,
                mva_sc: 1000.0,
                xr: 20.0,
                x0_over_x1: 1.0,
                r0_over_r1: 1.0,
                is_slack: true,
                p_mw: 0.0,
                qmin_mvar: None,
                qmax_mvar: None,
            }],
            ..Project::default()
        };
        let fault = solve(&project, None).unwrap();
        let lv = fault.buses.iter().find(|b| b.id == "lv").unwrap();
        let zs = 100.0 / 1000.0;
        let zt = 0.05 * (100.0 / 1.0);
        let z1 = zs + zt;
        let ibase = i_base_ka(0.48, 100.0);
        let i3 = ibase / z1;
        let ilg = 3.0 * ibase / (z1 + z1 + zt);
        let got3 = lv.three_phase.as_ref().unwrap().symmetrical_ka;
        let gotg = lv.line_to_ground.as_ref().unwrap().symmetrical_ka;
        assert!((got3 - i3).abs() / i3 < 1e-4, "3P got {got3}, expected {i3}");
        assert!((gotg - ilg).abs() / ilg < 1e-4, "LG got {gotg}, expected {ilg}");
    }

    #[test]
    fn open_contact_removes_fault_path_and_terminal_current() {
        let mut project = Project::sample();
        project.switches.push(Switch {
            id: "panel-switch".into(), name: "Panel switch".into(), branch_id: "feeder".into(),
            kind: SwitchKind::Breaker, closed: false,
        });
        let model = network::build(&project).unwrap();
        assert_eq!(model.branches.len(), project.branches.len());
        assert!(!model.branches[1].active);
        let open = solve_with_model(&project, None, &model).unwrap();
        assert!(open.buses.iter().find(|b| b.id == "mcc").unwrap().three_phase.is_some());
        assert!(open.buses.iter().find(|b| b.id == "pnl").unwrap().three_phase.is_none());
        assert!(open.buses.iter().find(|b| b.id == "mcc").unwrap().terminal_currents.iter().all(|i| i.branch_id != "feeder"));

        project.switches[0].closed = true;
        let closed = solve(&project, None).unwrap();
        assert!(closed.buses.iter().find(|b| b.id == "pnl").unwrap().three_phase.is_some());
        project.sources[0].in_service = false;
        let unavailable = solve(&project, None).unwrap();
        assert!(unavailable.buses.iter().all(|b| b.three_phase.is_none()));
    }
}

#[cfg(test)]
mod asymmetry_regression {
    use super::*;
    #[test]
    fn inductive_resistive_zero_and_invalid_limits() {
        let (xr, peak, rms) = asymmetry(10.0, Cplx::new(0.0, 1.0)).unwrap();
        assert_eq!(xr, None);
        assert!((peak-20.0*2.0_f64.sqrt()).abs() < 1e-12);
        assert!((rms-10.0*3.0_f64.sqrt()).abs() < 1e-12);
        for x in [0.0, 1e-15] {
            let (xr, peak, rms) = asymmetry(10.0, Cplx::new(1.0, x)).unwrap();
            assert_eq!(xr, Some(0.0));
            assert!((peak-10.0*2.0_f64.sqrt()).abs() < 1e-12);
            assert_eq!(rms, 10.0);
        }
        for z in [Cplx::ZERO, Cplx::new(-1.0,1.0), Cplx::new(1.0,-1.0), Cplx::new(f64::NAN,1.0), Cplx::new(f64::INFINITY,1.0)] {
            assert!(asymmetry(10.0,z).is_err());
        }
    }
}
