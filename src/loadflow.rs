use serde::{Deserialize, Serialize};

use crate::cplx::Cplx;
use crate::linalg::solve_sparse;
use crate::model::Project;
use crate::network::{self, SparseY};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadflowResult {
    pub converged: bool,
    pub iterations: usize,
    pub max_mismatch_pu: f64,
    pub buses: Vec<BusResult>,
    pub branches: Vec<BranchResult>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusResult {
    pub id: String,
    pub name: String,
    pub kv_nominal: f64,
    pub v_pu: f64,
    pub angle_deg: f64,
    pub p_mw: f64,
    pub q_mvar: f64,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BranchResult {
    pub id: String,
    pub name: String,
    pub from: String,
    pub to: String,
    pub p_from_mw: f64,
    pub q_from_mvar: f64,
    pub p_to_mw: f64,
    pub q_to_mvar: f64,
    pub i_from_a: f64,
    pub loading_pct: Option<f64>,
    pub p_loss_mw: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Slack,
    Pv,
    Pq,
}

#[derive(Clone)]
struct Spec {
    kind: Kind,
    v: f64,
    ang: f64,
    p: f64,
    q: f64,
    qmin: f64,
    qmax: f64,
}

pub fn solve(project: &Project) -> Result<LoadflowResult, String> {
    if project.buses.is_empty() {
        return Err("the project has no buses".into());
    }
    if !project.sources.iter().any(|s| s.is_slack) {
        return Err("mark one source as the slack bus (is_slack: true)".into());
    }
    let model = network::build(project)?;
    let n = model.index.n;
    let sbase = project.s_base_mva;
    let mut spec = vec![
        Spec { kind: Kind::Pq, v: 1.0, ang: 0.0, p: 0.0, q: 0.0, qmin: -1e9, qmax: 1e9 };
        n
    ];

    for source in &project.sources {
        let i = model.index.of(&source.bus)?;
        if source.is_slack {
            spec[i].kind = Kind::Slack;
            spec[i].v = source.v_pu;
            spec[i].ang = source.angle_deg.to_radians();
        } else if spec[i].kind != Kind::Slack {
            spec[i].kind = Kind::Pv;
            spec[i].v = source.v_pu;
            spec[i].p += source.p_mw / sbase;
            if let Some(q) = source.qmin_mvar {
                spec[i].qmin = q / sbase;
            }
            if let Some(q) = source.qmax_mvar {
                spec[i].qmax = q / sbase;
            }
        }
    }
    for load in &project.loads {
        let i = model.index.of(&load.bus)?;
        spec[i].p -= load.kw / (sbase * 1000.0);
        spec[i].q -= load.kvar / (sbase * 1000.0);
    }
    for motor in &project.motors {
        let i = model.index.of(&motor.bus)?;
        let kw = motor.hp * 0.746 / motor.efficiency;
        let kvar = kw * ((1.0 / (motor.pf * motor.pf) - 1.0).sqrt());
        spec[i].p -= kw / (sbase * 1000.0);
        spec[i].q -= kvar / (sbase * 1000.0);
    }

    let mut v: Vec<f64> = spec.iter().map(|s| s.v).collect();
    let mut ang: Vec<f64> = spec.iter().map(|s| s.ang).collect();
    let mut converged = false;
    let mut iterations = 0;
    let mut max_mismatch = f64::MAX;
    let mut message = None;

    for iter in 1..=30 {
        iterations = iter;
        let (p, q) = injections(&model.loadflow, &v, &ang);
        for (s, q_calc) in spec.iter_mut().zip(&q) {
            if s.kind == Kind::Pv && (*q_calc > s.qmax || *q_calc < s.qmin) {
                s.q = q_calc.clamp(s.qmin, s.qmax);
                s.kind = Kind::Pq;
            }
        }
        let (ang_idx, v_idx, nunk) = unknowns(&spec);
        max_mismatch = 0.0;
        if nunk == 0 {
            converged = true;
            break;
        }
        let mut mismatch = vec![0.0; nunk];
        for i in 0..n {
            if let Some(row) = ang_idx[i] {
                mismatch[row] = spec[i].p - p[i];
                max_mismatch = max_mismatch.max(mismatch[row].abs());
            }
            if let Some(row) = v_idx[i] {
                mismatch[row] = spec[i].q - q[i];
                max_mismatch = max_mismatch.max(mismatch[row].abs());
            }
        }
        if max_mismatch < 1e-8 {
            converged = true;
            break;
        }
        let jac = jacobian(&model.loadflow, &v, &ang, &p, &q, &ang_idx, &v_idx, nunk);
        let dx = match solve_sparse(nunk, &jac, &mismatch) {
            Ok(dx) => dx,
            Err(err) => {
                message = Some(err);
                break;
            }
        };
        let mut scale: f64 = 1.0;
        for i in 0..n {
            if let Some(row) = ang_idx[i] {
                let step = dx[row].abs();
                if step > 0.5 {
                    scale = scale.min(0.5 / step);
                }
            }
            if let Some(row) = v_idx[i] {
                let step = dx[row].abs();
                if step > 0.15 {
                    scale = scale.min(0.15 / step);
                }
            }
        }
        for i in 0..n {
            if let Some(row) = ang_idx[i] {
                ang[i] += scale * dx[row];
            }
            if let Some(row) = v_idx[i] {
                v[i] = (v[i] + scale * dx[row]).max(0.05);
            }
        }
    }
    if !converged && message.is_none() {
        message = Some(format!("load flow did not converge (mismatch {max_mismatch:.3e} pu)"));
    }

    let (p, q) = injections(&model.loadflow, &v, &ang);
    let mut buses = Vec::new();
    for i in 0..n {
        buses.push(BusResult {
            id: model.index.id_of[i].clone(),
            name: model.index.name_of[i].clone(),
            kv_nominal: model.index.kv[i],
            v_pu: v[i],
            angle_deg: ang[i].to_degrees(),
            p_mw: p[i] * sbase,
            q_mvar: q[i] * sbase,
            kind: match spec[i].kind {
                Kind::Slack => "slack",
                Kind::Pv => "pv",
                Kind::Pq => "pq",
            }
            .into(),
        });
    }

    let volts: Vec<Cplx> = (0..n).map(|i| Cplx::from_polar(v[i], ang[i])).collect();
    let mut branches = Vec::new();
    for (branch, stamp) in project.branches.iter().zip(&model.branches) {
        let vi = volts[stamp.from];
        let vj = volts[stamp.to];
        let q = stamp.positive;
        let i_from = q.yff * vi + q.yft * vj;
        let i_to = q.ytf * vi + q.ytt * vj;
        let s_from = vi * i_from.conj();
        let s_to = vj * i_to.conj();
        let i_a = i_from.abs() * crate::model::i_base_ka(model.index.kv[stamp.from], sbase) * 1000.0;
        let loading = if let Some(kva) = stamp.rating_kva {
            Some(s_from.abs() * sbase / (kva / 1000.0) * 100.0)
        } else {
            stamp.ampacity_a.map(|amp| i_a / amp * 100.0)
        };
        branches.push(BranchResult {
            id: branch.id.clone(),
            name: branch.name.clone(),
            from: branch.from.clone(),
            to: branch.to.clone(),
            p_from_mw: s_from.re * sbase,
            q_from_mvar: s_from.im * sbase,
            p_to_mw: s_to.re * sbase,
            q_to_mvar: s_to.im * sbase,
            i_from_a: i_a,
            loading_pct: loading,
            p_loss_mw: (s_from.re + s_to.re) * sbase,
        });
    }

    Ok(LoadflowResult { converged, iterations, max_mismatch_pu: max_mismatch, buses, branches, message })
}

fn injections(y: &SparseY, v: &[f64], ang: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let volts: Vec<Cplx> = (0..y.n).map(|i| Cplx::from_polar(v[i], ang[i])).collect();
    let current = y.mul(&volts);
    let mut p = vec![0.0; y.n];
    let mut q = vec![0.0; y.n];
    for i in 0..y.n {
        let s = volts[i] * current[i].conj();
        p[i] = s.re;
        q[i] = s.im;
    }
    (p, q)
}

fn unknowns(spec: &[Spec]) -> (Vec<Option<usize>>, Vec<Option<usize>>, usize) {
    let n = spec.len();
    let mut ang_idx = vec![None; n];
    let mut v_idx = vec![None; n];
    let mut k = 0;
    for (i, s) in spec.iter().enumerate() {
        if s.kind != Kind::Slack {
            ang_idx[i] = Some(k);
            k += 1;
        }
    }
    for (i, s) in spec.iter().enumerate() {
        if s.kind == Kind::Pq {
            v_idx[i] = Some(k);
            k += 1;
        }
    }
    (ang_idx, v_idx, k)
}

fn jacobian(
    y: &SparseY,
    v: &[f64],
    ang: &[f64],
    p: &[f64],
    q: &[f64],
    ang_idx: &[Option<usize>],
    v_idx: &[Option<usize>],
    _nunk: usize,
) -> Vec<(usize, usize, f64)> {
    let mut trips = Vec::new();
    for (i, row) in y.rows().iter().enumerate() {
        let mut gii = 0.0;
        let mut bii = 0.0;
        for &(j, yij) in row {
            if i == j {
                gii = yij.re;
                bii = yij.im;
                continue;
            }
            let th = ang[i] - ang[j];
            let (s, c) = th.sin_cos();
            let g = yij.re;
            let b = yij.im;
            if let (Some(r), Some(col)) = (ang_idx[i], ang_idx[j]) {
                trips.push((r, col, v[i] * v[j] * (g * s - b * c)));
            }
            if let (Some(r), Some(col)) = (ang_idx[i], v_idx[j]) {
                trips.push((r, col, v[i] * (g * c + b * s)));
            }
            if let (Some(r), Some(col)) = (v_idx[i], ang_idx[j]) {
                trips.push((r, col, v[i] * v[j] * (-g * c - b * s)));
            }
            if let (Some(r), Some(col)) = (v_idx[i], v_idx[j]) {
                trips.push((r, col, v[i] * (g * s - b * c)));
            }
        }
        if let Some(r) = ang_idx[i] {
            if let Some(col) = ang_idx[i] {
                trips.push((r, col, -q[i] - bii * v[i] * v[i]));
            }
            if let Some(col) = v_idx[i] {
                trips.push((r, col, p[i] / v[i] + gii * v[i]));
            }
        }
        if let Some(r) = v_idx[i] {
            if let Some(col) = ang_idx[i] {
                trips.push((r, col, p[i] - gii * v[i] * v[i]));
            }
            if let Some(col) = v_idx[i] {
                trips.push((r, col, q[i] / v[i] - bii * v[i]));
            }
        }
    }
    trips
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn two_bus_lossless_line_matches_the_closed_form() {
        let x_ohm = 0.1 * zbase_ohm(13.8, 100.0);
        let project = Project {
            name: "two bus".into(),
            s_base_mva: 100.0,
            buses: vec![
                Bus { id: "a".into(), name: "A".into(), kv: 13.8, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None },
                Bus { id: "b".into(), name: "B".into(), kv: 13.8, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None },
            ],
            branches: vec![Branch {
                id: "l".into(),
                name: "L".into(),
                from: "a".into(),
                to: "b".into(),
                kind: BranchKind::Line {
                    r_ohm: 0.0,
                    x_ohm,
                    b_siemens: 0.0,
                    r0_ohm: 0.0,
                    x0_ohm: 0.0,
                    ampacity_a: None,
                },
            }],
            loads: vec![Load { id: "ld".into(), name: "load".into(), bus: "b".into(), kw: 50_000.0, kvar: 0.0, basis: String::new() }],
            sources: vec![Source {
                id: "s".into(),
                name: "slack".into(),
                bus: "a".into(),
                v_pu: 1.0,
                angle_deg: 0.0,
                mva_sc: 10_000.0,
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
        let result = solve(&project).unwrap();
        assert!(result.converged, "{:?}", result.message);
        let u = (1.0 + 0.99_f64.sqrt()) / 2.0;
        let v_expected = u.sqrt();
        let ang_expected = -v_expected.acos().to_degrees();
        let bus = result.buses.iter().find(|b| b.id == "b").unwrap();
        assert!((bus.v_pu - v_expected).abs() < 1e-5, "v = {}, expected {v_expected}", bus.v_pu);
        assert!((bus.angle_deg - ang_expected).abs() < 1e-3, "angle = {}, expected {ang_expected}", bus.angle_deg);
        assert!(result.max_mismatch_pu < 1e-8);
    }
}
