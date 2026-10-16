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
    let model = network::build(project)?;
    solve_with_model(project, &model)
}

pub(crate) fn solve_with_model(project: &Project, model: &network::SystemModel) -> Result<LoadflowResult, String> {
    if project.buses.is_empty() { return Err("the project has no buses".into()); }
    let n = model.index.n;
    let sbase = project.s_base_mva;
    // A source-free island has no voltage reference and cannot enter the
    // Newton equations. Energized islands each require a slack source.
    let mut adjacent = vec![Vec::new(); n];
    for &(from, to) in &model.pos.edges {
        adjacent[from].push(to);
        adjacent[to].push(from);
    }
    let mut has_slack_path = vec![false; n];
    let mut stack = Vec::new();
    for source in project.sources.iter().filter(|source| source.in_service && source.is_slack) {
        let i = model.index.of(&source.bus)?;
        if !has_slack_path[i] {
            has_slack_path[i] = true;
            stack.push(i);
        }
    }
    while let Some(i) = stack.pop() {
        for &j in &adjacent[i] {
            if !has_slack_path[j] {
                has_slack_path[j] = true;
                stack.push(j);
            }
        }
    }
    if let Some(i) = (0..n).find(|&i| model.energized[i] && !has_slack_path[i]) {
        return Err(format!("energized island at bus '{}' has no slack source", model.index.id_of[i]));
    }
    let mut spec = vec![
        Spec { kind: Kind::Pq, v: 1.0, ang: 0.0, p: 0.0, q: 0.0, qmin: 0.0, qmax: 0.0 };
        n
    ];

    for source in &project.sources {
        if !source.in_service { continue; }
        let i = model.index.of(&source.bus)?;
        if source.is_slack {
            spec[i].kind = Kind::Slack;
            spec[i].v = source.v_pu;
            spec[i].ang = source.angle_deg.to_radians();
        } else if spec[i].kind != Kind::Slack {
            spec[i].kind = Kind::Pv;
            spec[i].v = source.v_pu;
            spec[i].p += source.p_mw / sbase;
            spec[i].qmin += source.qmin_mvar.unwrap_or(f64::NEG_INFINITY) / sbase;
            spec[i].qmax += source.qmax_mvar.unwrap_or(f64::INFINITY) / sbase;
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

    let demand: Vec<f64> = spec.iter().map(|s| -s.q).collect();
    let pv: Vec<bool> = spec.iter().map(|s| s.kind == Kind::Pv).collect();
    let mut limited = vec![0i8; n];
    let mut v: Vec<f64> = spec.iter().enumerate()
        .map(|(i, s)| if model.energized[i] { s.v } else { 0.0 }).collect();
    let mut ang: Vec<f64> = spec.iter().map(|s| s.ang).collect();
    let mut converged = false;
    let mut iterations = 0;
    let mut max_mismatch = f64::MAX;
    let mut message = None;
    let (tol, roundoff_limited) = mismatch_tolerance(&model.loadflow, &spec, &model.energized, sbase);

    for iter in 1..=100 {
        iterations = iter;
        let (p, q) = injections(&model.loadflow, &v, &ang);
        let (ang_idx, v_idx, nunk) = unknowns(&spec, &model.energized);
        if nunk == 0 {
            max_mismatch = 0.0;
            converged = true;
            break;
        }
        let mut mismatch = vec![0.0; nunk];
        for i in 0..n {
            if let Some(row) = ang_idx[i] {
                mismatch[row] = spec[i].p - p[i];
            }
            if let Some(row) = v_idx[i] {
                mismatch[row] = spec[i].q - q[i];
            }
        }
        // f64::max ignores NaN, so a non-finite mismatch must be caught explicitly.
        let Some(largest) = finite_max_abs(&mismatch) else {
            max_mismatch = f64::MAX;
            message = Some("load flow produced a non-finite power mismatch; not converged".into());
            break;
        };
        max_mismatch = largest;
        // A fixed physical MW/Mvar tolerance makes convergence independent of
        // MVA base, raised to the roundoff floor of the largest admittance.
        if max_mismatch < tol {
            // Change the active set only after solving the current equations.
            // A bound can be released if the resulting voltage contradicts it.
            let mut changed = false;
            for i in 0..n {
                if !pv[i] { continue; }
                if spec[i].kind == Kind::Pv {
                    let qgen = q[i] + demand[i];
                    if qgen > spec[i].qmax + tol || qgen < spec[i].qmin - tol {
                        limited[i] = if qgen > spec[i].qmax { 1 } else { -1 };
                        spec[i].q = qgen.clamp(spec[i].qmin, spec[i].qmax) - demand[i];
                        spec[i].kind = Kind::Pq;
                        changed = true;
                    }
                } else if (limited[i] == 1 && v[i] > spec[i].v + 1e-7)
                    || (limited[i] == -1 && v[i] < spec[i].v - 1e-7) {
                    spec[i].kind = Kind::Pv;
                    v[i] = spec[i].v;
                    limited[i] = 0;
                    changed = true;
                }
            }
            if changed { continue; }
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
    if !converged && roundoff_limited {
        message = Some(format!("{}; roundoff estimate exceeds the 1e-5 pu roundoff limit on the system base",
            message.unwrap_or_default()));
    }
    if converged {
        let skipped = model.energized.iter().filter(|&&on| !on).count();
        if skipped > 0 {
            message = Some(format!("{skipped} unenergized bus(es) excluded from load flow"));
        }
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
            kind: if !model.energized[i] { "unenergized" } else { match spec[i].kind {
                Kind::Slack => "slack",
                Kind::Pv => "pv",
                Kind::Pq => "pq",
            }}
            .into(),
        });
    }

    let volts: Vec<Cplx> = (0..n).map(|i| Cplx::from_polar(v[i], ang[i])).collect();
    let mut branches = Vec::new();
    for (branch, stamp) in project.branches.iter().zip(&model.branches) {
        if !stamp.active {
            branches.push(BranchResult {
                id: branch.id.clone(), name: branch.name.clone(), from: branch.from.clone(), to: branch.to.clone(),
                p_from_mw: 0.0, q_from_mvar: 0.0, p_to_mw: 0.0, q_to_mvar: 0.0,
                i_from_a: 0.0, loading_pct: None, p_loss_mw: 0.0,
            });
            continue;
        }
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

/// Physical tolerance raised to the active equations' roundoff floor, capped at 1e-5 pu.
fn mismatch_tolerance(y: &SparseY, spec: &[Spec], energized: &[bool], sbase: f64) -> (f64, bool) {
    let y_max = y.rows().iter().enumerate()
        .filter(|(i, _)| energized[*i] && spec[*i].kind != Kind::Slack)
        .flat_map(|(i, row)| row.iter().filter(move |(j, _)| *j == i).map(|(_, v)| v.abs()))
        .fold(0.0, f64::max);
    let v_max = spec.iter().enumerate().filter(|(i, _)| energized[*i])
        .map(|(_, s)| s.v.abs()).fold(1.0, f64::max);
    let roundoff = 1e3 * f64::EPSILON * y_max * v_max * v_max;
    ((1e-8 / sbase).max(roundoff).min(1e-5), roundoff > 1e-5)
}

/// Largest absolute value, or None when any entry is NaN or infinite.
fn finite_max_abs(values: &[f64]) -> Option<f64> {
    let mut largest = 0.0_f64;
    for value in values {
        if !value.is_finite() { return None; }
        largest = largest.max(value.abs());
    }
    Some(largest)
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

fn unknowns(spec: &[Spec], energized: &[bool]) -> (Vec<Option<usize>>, Vec<Option<usize>>, usize) {
    let n = spec.len();
    let mut ang_idx = vec![None; n];
    let mut v_idx = vec![None; n];
    let mut k = 0;
    for (i, s) in spec.iter().enumerate() {
        if energized[i] && s.kind != Kind::Slack {
            ang_idx[i] = Some(k);
            k += 1;
        }
    }
    for (i, s) in spec.iter().enumerate() {
        if energized[i] && s.kind == Kind::Pq {
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
                Bus { id: "a".into(), name: "A".into(), kv: 13.8, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None },
                Bus { id: "b".into(), name: "B".into(), kv: 13.8, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None },
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
            sources: vec![Source { generator: None, machine_kind: None, transient_mva_sc: None,
                id: "s".into(),
                name: "slack".into(),
                bus: "a".into(),
                in_service: true,
                decrement_curve: Vec::new(),
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
                defaulted: Vec::new(),
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

    #[test]
    fn tiny_contact_impedance_converges_and_warns_below_one_micro_pu() {
        for (contact_ohm, warns) in [(1e-5, false), (1e-6, true), (1e-7, true)] {
            let mut project = Project::sample();
            project.motors.clear(); project.devices.clear(); project.equipment.clear(); project.loads.clear();
            for bus in &mut project.buses { bus.kv = 13.8; }
            project.branches[0].kind = BranchKind::Line {
                r_ohm: contact_ohm, x_ohm: contact_ohm, b_siemens: 0.0, r0_ohm: 0.0, x0_ohm: 0.0, ampacity_a: None,
            };
            if let BranchKind::Line { r_ohm, x_ohm, .. } = &mut project.branches[1].kind { *r_ohm = 0.2; *x_ohm = 0.4; }
            project.loads.push(Load { id: "ld".into(), name: "load".into(), bus: "pnl".into(), kw: 2000.0, kvar: 800.0, basis: String::new() });
            let model = network::build(&project).unwrap();
            assert_eq!(model.warnings.iter().any(|w| w.contains("below 1e-6 pu")), warns, "{:?}", model.warnings);
            let result = solve_with_model(&project, &model).unwrap();
            assert!(result.converged, "{contact_ohm} ohm: {:?}", result.message);
            let a = &result.buses[0];
            let b = &result.buses[1];
            // The contact carries the full load with a negligible drop.
            assert!((a.v_pu - b.v_pu).abs() < 1e-6 && (a.angle_deg - b.angle_deg).abs() < 1e-4);
            assert!((result.branches[0].p_from_mw - result.branches[1].p_from_mw).abs() < 1e-6);
        }
    }

    #[test]
    fn non_finite_mismatch_is_never_converged() {
        assert_eq!(finite_max_abs(&[1.0, -3.0, 2.0]), Some(3.0));
        assert_eq!(finite_max_abs(&[]), Some(0.0));
        // f64::max(0.0, NaN) returns 0.0, which used to look converged.
        assert_eq!(0.0_f64.max(f64::NAN), 0.0);
        assert_eq!(finite_max_abs(&[0.0, f64::NAN]), None);
        assert_eq!(finite_max_abs(&[f64::INFINITY]), None);
    }

    #[test]
    fn open_branch_reports_zero_and_all_sources_off_is_unenergized() {
        let mut project = Project::sample();
        project.switches.push(Switch {
            id: "panel-switch".into(), name: "Panel switch".into(), branch_id: "feeder".into(),
            kind: SwitchKind::Breaker, closed: false,
        });
        let open = solve(&project).unwrap();
        assert!(open.converged, "{:?}", open.message);
        let panel = open.buses.iter().find(|b| b.id == "pnl").unwrap();
        assert_eq!(panel.kind, "unenergized");
        assert_eq!(panel.v_pu, 0.0);
        let feeder = open.branches.iter().find(|b| b.id == "feeder").unwrap();
        assert_eq!(feeder.i_from_a, 0.0);
        assert_eq!(feeder.p_from_mw, 0.0);
        assert_eq!(feeder.loading_pct, None);

        project.sources[0].in_service = false;
        let offline = solve(&project).unwrap();
        assert!(offline.converged);
        assert!(offline.buses.iter().all(|b| b.kind == "unenergized" && b.v_pu == 0.0));
    }
}
