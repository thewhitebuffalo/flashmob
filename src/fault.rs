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
    /// IEC 60909 impedance correction factors applied to the maximum current.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub iec_corrections: Vec<crate::iec::IecCorrection>,
    /// Data gaps that left a correction factor unapplied or used a fallback.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub iec_warnings: Vec<String>,
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
    /// Calculation failure confined to this bus (for example a capacitive or
    /// non-finite Thevenin impedance). Other buses are still reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub terminal_currents: Vec<TerminalCurrent>,
    /// Three-phase no-AC-decrement ratio. Symmetrical current from sources
    /// that have no decrement curve, divided by the total three-phase fault
    /// current. Motors and sources with a decrement curve are the local part.
    /// Absent when that split does not account for the fault current.
    /// Line-to-ground ratio is not calculated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nacd: Option<f64>,
    /// Machines that make up `nacd`. Empty when the ratio is omitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub infeeds: Vec<ThreePhaseInfeed>,
}

/// One machine's symmetrical contribution to the three-phase fault.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreePhaseInfeed {
    /// Source id. Absent for a motor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Magnitude of this machine's current, per unit on the study base.
    pub current_pu: f64,
    /// True when the machine has no decrement curve.
    pub remote: bool,
}

/// Positive-sequence branch-terminal current for a bolted three-phase fault,
/// flowing into the branch at each terminal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerminalCurrent {
    pub branch_id: String,
    /// Magnitude of the total current, `|prefault - increment|`.
    pub from_a: f64,
    pub to_a: f64,
    /// Rectangular amperes, [real, imaginary]. Flat prefault neglects initial load flow.
    pub from_prefault_a: [f64; 2],
    pub to_prefault_a: [f64; 2],
    /// Rectangular amperes, [real, imaginary], of the branch current driven by
    /// the network's voltage *drop* (prefault minus faulted voltage). The sign
    /// convention is subtractive: total = prefault - increment, and
    /// prefault - k * increment for a fault current scaled by k.
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
    /// IEC 60909 peak: kappa (peak_basis) times sqrt(2) times symmetrical_max_ka.
    pub iec_peak_ka: f64,
    pub half_cycle_rms_ka: f64,
    /// IEC 60909-0 Table 1 voltage factor c at this bus nominal voltage.
    /// Minimum and maximum symmetrical currents are this fault current times
    /// c / |prefault|. The maximum uses the network with the IEC 60909
    /// impedance correction factors when `iec_max_corrected` is true; the
    /// minimum is uncorrected.
    #[serde(default)]
    pub c_min: f64,
    #[serde(default)]
    pub c_max: f64,
    #[serde(default)]
    pub symmetrical_min_ka: f64,
    #[serde(default)]
    pub symmetrical_max_ka: f64,
    #[serde(default)]
    pub iec_max_corrected: bool,
    /// IEC 60909 peak basis: "radial" (no 1.15), "meshed" (1.15 applied), or
    /// "meshed_low_rx" (meshed, every branch R/X below 0.3, no 1.15).
    #[serde(default)]
    pub peak_basis: String,
}

pub fn solve(project: &Project, loadflow: Option<&LoadflowResult>) -> Result<FaultResult, String> {
    let model = network::build(project)?;
    solve_with_model(project, loadflow, &model)
}

/// Fault study without the IEC 60909 impedance-corrected maximum current.
/// Used where only the symmetrical, peak, and X/R values are read.
pub fn solve_uncorrected(project: &Project, loadflow: Option<&LoadflowResult>) -> Result<FaultResult, String> {
    let model = network::build(project)?;
    solve_core(project, loadflow, &model)
}

pub(crate) fn solve_with_model(project: &Project, loadflow: Option<&LoadflowResult>, model: &network::SystemModel) -> Result<FaultResult, String> {
    let mut result = solve_core(project, loadflow, model)?;
    if !result.valid { return Ok(result); }
    let (corrections, warnings) = crate::iec::corrections(project);
    result.iec_warnings = warnings;
    if corrections.is_empty() { return Ok(result); }
    let corrected = crate::iec::corrected_project(project, &corrections);
    let corrected_model = network::build_validated(&corrected)?;
    let corrected_result = solve_core(&corrected, loadflow, &corrected_model)?;
    for (bus, fixed) in result.buses.iter_mut().zip(&corrected_result.buses) {
        let prefault = bus.prefault_pu;
        let pairs = [
            (&mut bus.three_phase, &fixed.three_phase),
            (&mut bus.line_to_ground, &fixed.line_to_ground),
            (&mut bus.line_to_line, &fixed.line_to_line),
            (&mut bus.line_to_line_ground, &fixed.line_to_line_ground),
        ];
        for (point, fixed) in pairs {
            let (Some(point), Some(fixed)) = (point.as_mut(), fixed.as_ref()) else { continue };
            if prefault <= 1e-12 { continue; }
            let max = fixed.symmetrical_ka * point.c_max / prefault;
            if !max.is_finite() { return Err("non-finite corrected maximum current".into()); }
            // Same kappa; the peak follows the corrected maximum.
            if point.symmetrical_max_ka > 0.0 { point.iec_peak_ka *= max / point.symmetrical_max_ka; }
            point.symmetrical_max_ka = max;
            point.iec_max_corrected = true;
        }
    }
    result.iec_corrections = corrections;
    Ok(result)
}

/// IEC 60909 peak basis at a bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeakBasis {
    /// The faulted bus's network has no closed loop of branches: kappa from
    /// the faulted-bus R/X (clauses 8.1.1 and 8.1.2).
    Radial,
    /// A closed loop exists: method B, kappa times 1.15 (clause 8.1.3).
    Meshed,
    /// A closed loop exists, but every branch in it has R/X below 0.3, so the
    /// 1.15 factor is not needed (clause 8.1.3).
    MeshedLowRx,
}

impl PeakBasis {
    pub fn as_str(self) -> &'static str {
        match self { PeakBasis::Radial => "radial", PeakBasis::Meshed => "meshed", PeakBasis::MeshedLowRx => "meshed_low_rx" }
    }
}

/// Peak basis for every bus from the closed, energized branch graph. A
/// connected group of buses is meshed when it has at least as many branches
/// as buses (parallel branches count as a loop). Sources and motors are not
/// joined through ground, so separate radial infeeds stay radial.
pub fn peak_basis(project: &Project, model: &network::SystemModel) -> Vec<PeakBasis> {
    let n = model.index.n;
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i { parent[i] = parent[parent[i]]; i = parent[i]; }
        i
    }
    let active: Vec<(usize, usize, f64)> = project.branches.iter().zip(&model.branches)
        .filter(|(_, stamp)| stamp.active && model.energized[stamp.from] && model.energized[stamp.to])
        .map(|(branch, stamp)| {
            let r_over_x = match &branch.kind {
                crate::model::BranchKind::Line { r_ohm, x_ohm, .. } => if *x_ohm > 0.0 { r_ohm / x_ohm } else { f64::INFINITY },
                crate::model::BranchKind::Transformer { xr, .. } => 1.0 / xr,
            };
            (stamp.from, stamp.to, r_over_x)
        }).collect();
    for &(a, b, _) in &active {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb { parent[ra] = rb; }
    }
    let mut nodes = vec![0usize; n];
    let mut edges = vec![0usize; n];
    let mut high_rx = vec![false; n];
    for i in 0..n { let r = find(&mut parent, i); nodes[r] += 1; }
    for &(a, _, r_over_x) in &active {
        let r = find(&mut parent, a);
        edges[r] += 1;
        if !(r_over_x < 0.3) { high_rx[r] = true; }
    }
    (0..n).map(|i| {
        let r = find(&mut parent, i);
        if edges[r] < nodes[r] { PeakBasis::Radial } else if high_rx[r] { PeakBasis::Meshed } else { PeakBasis::MeshedLowRx }
    }).collect()
}

fn set_peak(point: &mut FaultPoint, kv: f64, basis: PeakBasis) {
    point.peak_basis = basis.as_str().into();
    // IEC peak is kappa times sqrt(2) times the maximum initial symmetrical
    // current (voltage factor cmax, and the impedance correction factors once
    // they are applied). Purely resistive or zero current has kappa 1.
    let kappa = match point.x_over_r {
        None => iec_kappa(0.0, kv, basis == PeakBasis::Meshed),
        Some(xr) if xr > 0.0 => iec_kappa(1.0 / xr, kv, basis == PeakBasis::Meshed),
        Some(_) => 1.0,
    };
    point.iec_peak_ka = kappa * std::f64::consts::SQRT_2 * point.symmetrical_max_ka;
}

fn solve_core(project: &Project, loadflow: Option<&LoadflowResult>, model: &network::SystemModel) -> Result<FaultResult, String> {
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
                error: None,
                nacd: None,
                infeeds: Vec::new(),
            });
            continue;
        }
        let v = prefault_v[i];
        // Network factorization failures affect every bus and still abort the study.
        let column1 = map1[i].map(|k| pos.column(k)).transpose()?;
        let zz2 = neg.zth(map2[i])?;
        let zz0 = zero.zth(map0[i])?;
        let bus = match bus_fault(project, model, &map1, &prefault_v, i, v, column1, zz2, zz0) {
            Ok(bus) => bus,
            // A bad Thevenin impedance at one bus is recorded there; other
            // buses are still calculated.
            Err(err) => BusFault {
                id: model.index.id_of[i].clone(), name: model.index.name_of[i].clone(), kv: model.index.kv[i],
                prefault_pu: v.abs(), three_phase: None, line_to_ground: None, line_to_line: None, line_to_line_ground: None,
                terminal_currents: Vec::new(),
                note: Some(format!("fault calculation failed at this bus: {err}")),
                error: Some(err),
                nacd: None,
                infeeds: Vec::new(),
            },
        };
        buses.push(bus);
    }
    let basis = peak_basis(project, model);
    for (i, bus) in buses.iter_mut().enumerate() {
        for point in [&mut bus.three_phase, &mut bus.line_to_ground, &mut bus.line_to_line, &mut bus.line_to_line_ground].into_iter().flatten() {
            set_peak(point, model.index.kv[i], basis[i]);
        }
    }
    Ok(FaultResult { prefault: label, prefault_requested: requested.into(), valid, error, buses, iec_corrections: Vec::new(), iec_warnings: Vec::new() })
}

#[allow(clippy::too_many_arguments)]
fn bus_fault(
    project: &Project, model: &network::SystemModel, map1: &[Option<usize>], prefault_v: &[Cplx],
    i: usize, v: Cplx, column1: Option<Vec<Cplx>>, zz2: Option<Cplx>, zz0: Option<Cplx>,
) -> Result<BusFault, String> {
    let kv = model.index.kv[i];
    let ibase = i_base_ka(kv, project.s_base_mva);
    let zz1 = map1[i].zip(column1.as_ref()).map(|(k, col)| col[k]);
    let mut note = None;
    let three = match zz1 {
        Some(z) => { checked_impedance(z)?; Some(point(v / z, Cplx::ZERO, Cplx::ZERO, z, ibase, kv, v.abs(), project.low_voltage_tolerance_plus_10)?) },
        None => None,
    };
    let line_line = match (zz1, zz2) {
        (Some(a), Some(b)) => {
            checked_impedance(a + b)?;
            let i1 = v / (a + b);
            Some(point(i1, -i1, Cplx::ZERO, a + b, ibase, kv, v.abs(), project.low_voltage_tolerance_plus_10)?)
        }
        _ => None,
    };
    let (line_ground, double) = match (zz1, zz2, zz0) {
        (Some(a), Some(b), Some(c)) => {
            checked_impedance(a + b + c)?;
            checked_impedance(b + c)?;
            let i1 = v / (a + b + c);
            let lg = point(i1, i1, i1, a + b + c, ibase, kv, v.abs(), project.low_voltage_tolerance_plus_10)?;
            let zpar = (b * c) / (b + c);
            checked_impedance(a + zpar)?;
            let i1g = v / (a + zpar);
            let i2 = -i1g * c / (b + c);
            let i0 = -i1g * b / (b + c);
            (Some(lg), Some(point(i1g, i2, i0, a + zpar, ibase, kv, v.abs(), project.low_voltage_tolerance_plus_10)?))
        }
        (Some(a), Some(b), None) => {
            note = Some("no zero-sequence path (ungrounded or delta-isolated); ground fault current is zero".into());
            checked_impedance(a + b)?;
            let i1 = v / (a + b);
            (Some(zero_point(kv, project.low_voltage_tolerance_plus_10)?), Some(point(i1, -i1, Cplx::ZERO, a + b, ibase, kv, v.abs(), project.low_voltage_tolerance_plus_10)?))
        }
        _ => (None, None),
    };
    if three.is_none() {
        note = Some("bus is not connected to a source".into());
    }
    // Solve the voltage decrement caused by the fault current injection.
    // LF prefault includes initial branch flow; flat prefault neglects initial load flow.
    let mut terminal_currents = Vec::new();
    let mut nacd = None;
    let mut infeeds = Vec::new();
    if let (Some(mut drop), Some(z)) = (column1, zz1) {
        // Y^-1 (e_k * I_fault) = (Y^-1 e_k) * I_fault is the voltage drop.
        let current = v / z;
        for value in &mut drop { *value = *value * current; }
        (nacd, infeeds) = three_phase_split(model, map1, &drop, current);
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
            // Current driven by the voltage drop; see TerminalCurrent for the
            // subtractive sign convention (total = prefault - increment).
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
    Ok(BusFault {
        id: model.index.id_of[i].clone(),
        name: model.index.name_of[i].clone(),
        kv,
        prefault_pu: v.abs(),
        three_phase: three,
        line_to_ground: line_ground,
        line_to_line: line_line,
        line_to_line_ground: double,
        note,
        error: None,
        terminal_currents,
        nacd,
        infeeds,
    })
}

/// Remote symmetrical current over the total three-phase fault current, and
/// the machine currents behind that ratio.
///
/// Each infeed current is its admittance times the fault voltage drop. A
/// source with no decrement curve is remote. A motor, or a source that has a
/// decrement curve, is local. Both results are omitted when those infeeds do
/// not add up to the fault current, which happens when another shunt is present.
fn three_phase_split(model: &network::SystemModel, map1: &[Option<usize>], drop: &[Cplx], i_fault: Cplx) -> (Option<f64>, Vec<ThreePhaseInfeed>) {
    let total = i_fault.abs();
    if model.infeeds.is_empty() || total < 1e-12 {
        return (None, Vec::new());
    }
    let mut remote = Cplx::ZERO;
    let mut accounted = Cplx::ZERO;
    let mut rows = Vec::with_capacity(model.infeeds.len());
    for infeed in &model.infeeds {
        let Some(index) = map1.get(infeed.bus).copied().flatten() else { continue };
        let current = infeed.y * drop[index];
        let magnitude = current.abs();
        if !magnitude.is_finite() {
            return (None, Vec::new());
        }
        accounted += current;
        if infeed.remote {
            remote += current;
        }
        rows.push(ThreePhaseInfeed {
            source_id: infeed.source_id.clone(),
            current_pu: magnitude,
            remote: infeed.remote,
        });
    }
    let mismatch = (accounted - i_fault).abs() / total;
    if !mismatch.is_finite() || mismatch > 0.02 {
        return (None, Vec::new());
    }
    let ratio = remote.abs() / total;
    if !ratio.is_finite() {
        return (None, Vec::new());
    }
    (Some(ratio), rows)
}

/// Separate-network X/R at one bus: reactance from the reactance-only
/// network over resistance from the resistance-only network.
#[derive(Clone, Debug, PartialEq)]
pub struct SeparateXr {
    pub bus_id: String,
    /// Three-phase fault X/R. None means no resistance (infinite X/R).
    pub three_phase: Option<Option<f64>>,
    /// Line-to-ground fault X/R from X1+X2+X0 over R1+R2+R0.
    pub line_to_ground: Option<Option<f64>>,
}

/// Ratio below which the floored part of a separate network is treated as
/// absent: R under this fraction of X is no resistance, and the reverse.
const SEPARATE_XR_NEGLIGIBLE: f64 = 100.0 * network::PART_FLOOR;

/// X over R from separate Thevenin values. None is infinite X/R.
pub fn separate_ratio(x: f64, r: f64) -> Result<Option<f64>, String> {
    if !x.is_finite() || !r.is_finite() || x < 0.0 || r < 0.0 || (x == 0.0 && r == 0.0) {
        return Err(format!("separate-network X/R needs finite nonnegative R and X, got R {r} and X {x}"));
    }
    if r <= SEPARATE_XR_NEGLIGIBLE * x { return Ok(None); }
    if x <= SEPARATE_XR_NEGLIGIBLE * r { return Ok(Some(0.0)); }
    Ok(Some(x / r))
}

/// Faulted-bus X/R for every energized bus from a resistance-only and a
/// reactance-only reduction of `project` (validated). Shunt capacitance is
/// omitted from both. Unenergized buses and buses without a positive-sequence
/// path give None.
pub fn separate_xr(project: &Project) -> Result<Vec<SeparateXr>, String> {
    let r_model = network::build_part(project, network::Part::Resistance)?;
    let x_model = network::build_part(project, network::Part::Reactance)?;
    let thevenin = |model: &network::SystemModel| -> Result<Vec<(Option<Cplx>, Option<Cplx>, Option<Cplx>)>, String> {
        let (y1, m1) = reduced(&model.pos, &model.energized)?;
        let (y2, m2) = reduced(&model.neg, &model.energized)?;
        let (y0, m0) = reduced(&model.zero, &model.energized)?;
        let (mut s1, mut s2, mut s0) = (AdmittanceSolver::new(&y1), AdmittanceSolver::new(&y2), AdmittanceSolver::new(&y0));
        (0..model.index.n).map(|i| {
            if !model.energized[i] { return Ok((None, None, None)); }
            Ok((s1.zth(m1[i])?, s2.zth(m2[i])?, s0.zth(m0[i])?))
        }).collect()
    };
    let r = thevenin(&r_model)?;
    let x = thevenin(&x_model)?;
    let mut rows = Vec::with_capacity(r.len());
    for (i, (rz, xz)) in r.iter().zip(&x).enumerate() {
        let three_phase = match (rz.0, xz.0) {
            (Some(r1), Some(x1)) => Some(separate_ratio(x1.im, r1.re)?),
            _ => None,
        };
        let line_to_ground = match (rz, xz) {
            ((Some(r1), Some(r2), Some(r0)), (Some(x1), Some(x2), Some(x0))) =>
                Some(separate_ratio(x1.im + x2.im + x0.im, r1.re + r2.re + r0.re)?),
            _ => None,
        };
        rows.push(SeparateXr { bus_id: r_model.index.id_of[i].clone(), three_phase, line_to_ground });
    }
    Ok(rows)
}

fn reduced(net: &Net, energized: &[bool]) -> Result<(crate::network::SparseY, Vec<Option<usize>>), String> {
    let mask: Vec<bool> = network::reaches_shunt(net).iter().zip(energized).map(|(a, b)| *a && *b).collect();
    Ok(network::reduce(&net.y, &mask))
}

fn point(i1: Cplx, i2: Cplx, i0: Cplx, zth: Cplx, ibase: f64, kv: f64, prefault_pu: f64, low_voltage_tolerance_plus_10: bool) -> Result<FaultPoint, String> {
    let (ia, ib, ic) = phases(i0, i1, i2);
    let ig = i0 * 3.0;
    let sym = ia.abs().max(ib.abs()).max(ic.abs()) * ibase;
    let (xr, peak, asym) = asymmetry(sym, zth, kv)?;
    let (c_min, c_max, symmetrical_min_ka, symmetrical_max_ka) = scaled_symmetrical(sym, kv, prefault_pu, low_voltage_tolerance_plus_10)?;
    Ok(FaultPoint {
        symmetrical_ka: sym,
        ia_ka: ia.abs() * ibase,
        ib_ka: ib.abs() * ibase,
        ic_ka: ic.abs() * ibase,
        ig_ka: ig.abs() * ibase,
        x_over_r: xr,
        iec_peak_ka: peak,
        half_cycle_rms_ka: asym,
        c_min,
        c_max,
        symmetrical_min_ka,
        symmetrical_max_ka,
        iec_max_corrected: false,
        peak_basis: String::new(),
    })
}

fn zero_point(kv: f64, low_voltage_tolerance_plus_10: bool) -> Result<FaultPoint, String> {
    let (c_min, c_max, _, _) = scaled_symmetrical(0.0, kv, 0.0, low_voltage_tolerance_plus_10)?;
    Ok(FaultPoint {
        symmetrical_ka: 0.0,
        ia_ka: 0.0,
        ib_ka: 0.0,
        ic_ka: 0.0,
        ig_ka: 0.0,
        x_over_r: Some(0.0),
        iec_peak_ka: 0.0,
        half_cycle_rms_ka: 0.0,
        c_min,
        c_max,
        symmetrical_min_ka: 0.0,
        symmetrical_max_ka: 0.0,
        iec_max_corrected: false,
        peak_basis: String::new(),
    })
}

fn scaled_symmetrical(sym: f64, kv: f64, prefault_pu: f64, low_voltage_tolerance_plus_10: bool) -> Result<(f64, f64, f64, f64), String> {
    let (c_min, c_max) = iec_voltage_factor(kv, low_voltage_tolerance_plus_10)?;
    let (symmetrical_min_ka, symmetrical_max_ka) = if prefault_pu > 1e-12 {
        (sym * c_min / prefault_pu, sym * c_max / prefault_pu)
    } else {
        (0.0, 0.0)
    };
    if !symmetrical_min_ka.is_finite() || !symmetrical_max_ka.is_finite() {
        return Err("non-finite voltage-factor current".into());
    }
    Ok((c_min, c_max, symmetrical_min_ka, symmetrical_max_ka))
}

/// IEC 60909-0 Table 1 voltage factor c. Returns `(cmin, cmax)`.
///
/// At or below 1 kV, cmin is 0.95. cmax is 1.05 when the low-voltage tolerance
/// flag is false (+6%) and 1.10 when it is true (+10%). Above 1 kV, cmin is
/// 1.00 and cmax is 1.10 through medium and high voltage. The 35 kV boundary
/// in the table does not change c.
pub fn iec_voltage_factor(kv: f64, low_voltage_tolerance_plus_10: bool) -> Result<(f64, f64), String> {
    if !kv.is_finite() || kv <= 0.0 {
        return Err("voltage factor requires a positive finite nominal voltage".into());
    }
    if kv <= 1.0 {
        Ok((0.95, if low_voltage_tolerance_plus_10 { 1.10 } else { 1.05 }))
    } else {
        Ok((1.00, 1.10))
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

/// IEC 60909-0 peak factor kappa from the R/X at the fault location
/// (clause 8.1). With `meshed` set this is method B of clause 8.1.3: kappa
/// times 1.15, limited to 1.8 at or below 1 kV and 2.0 above, and never below
/// kappa itself. Without it (radial networks, or meshed networks whose
/// branches all have R/X below 0.3) kappa is used directly. A purely
/// resistive impedance has no DC component (kappa = 1).
pub fn iec_kappa(r_over_x: f64, kv: f64, meshed: bool) -> f64 {
    if r_over_x.is_infinite() { return 1.0; }
    let kappa_b = 1.02 + 0.98 * (-3.0 * r_over_x).exp();
    if !meshed { return kappa_b; }
    let cap = if kv <= 1.0 { 1.8 } else { 2.0 };
    kappa_b.max((1.15 * kappa_b).min(cap))
}

/// Asymmetrical rms / symmetrical rms at a contact parting time, with no AC decrement.
///
/// `sqrt(1 + 2 * exp(-4 * pi * cycles / (X/R)))`. Cycles are at the system
/// frequency, so the frequency cancels. None means infinite X/R: the factor
/// is sqrt(3) at every parting time. Some(0.0) is purely resistive: factor 1.
pub fn asymmetrical_rms_factor(x_over_r: Option<f64>, contact_parting_cycles: f64) -> Result<f64, String> {
    if !contact_parting_cycles.is_finite() || contact_parting_cycles <= 0.0 {
        return Err("contact parting time must be a positive finite number of cycles".into());
    }
    let Some(xr) = x_over_r else {
        return Ok(3.0_f64.sqrt());
    };
    if !xr.is_finite() || xr < 0.0 {
        return Err("X/R must be finite and nonnegative".into());
    }
    if xr == 0.0 {
        return Ok(1.0);
    }
    let factor = (1.0 + 2.0 * (-4.0 * std::f64::consts::PI * contact_parting_cycles / xr).exp()).sqrt();
    if !factor.is_finite() {
        return Err("non-finite asymmetrical factor".into());
    }
    Ok(factor)
}

/// First-half-cycle asymmetrical rms / symmetrical rms.
/// None means infinite X/R (purely inductive): factor is sqrt(3).
/// Some(0.0) is purely resistive: factor is 1.
pub fn half_cycle_factor(x_over_r: Option<f64>) -> Result<f64, String> {
    asymmetrical_rms_factor(x_over_r, 0.5)
}

/// Peak and half-cycle asymmetrical currents from the Thevenin impedance.
/// The half-cycle RMS uses the Thevenin X/R directly (no meshed-network
/// correction); it is not an ANSI C37.010 separate-R/X calculation.
fn asymmetry(i_sym: f64, z: Cplx, kv: f64) -> Result<(Option<f64>, f64, f64), String> {
    // Remove relative roundoff from phase-shift stamps, not physical reactance.
    let tol = z.abs() * 1e-12;
    let z = Cplx::new(if z.re.abs() <= tol { 0.0 } else { z.re }, if z.im.abs() <= tol { 0.0 } else { z.im });
    if !i_sym.is_finite() || i_sym < 0.0 || !z.re.is_finite() || !z.im.is_finite()
        || z.re < 0.0 || z.im < 0.0 || z.abs() == 0.0 {
        return Err("asymmetry requires finite nonnegative R and X and nonzero impedance".into());
    }
    if z.im == 0.0 {
        return Ok((Some(0.0), std::f64::consts::SQRT_2 * i_sym, i_sym * half_cycle_factor(Some(0.0))?));
    }
    let r_over_x = z.re / z.im;
    let xr = if z.re == 0.0 { None } else { Some(z.im / z.re) };
    let kappa = iec_kappa(r_over_x, kv, true);
    let peak = kappa * std::f64::consts::SQRT_2 * i_sym;
    let asym = i_sym * half_cycle_factor(xr)?;
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
            buses: vec![Bus { id: "a".into(), name: "A".into(), kv: 13.8, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None }],
            sources: vec![Source { generator: None, machine_kind: None, transient_mva_sc: None,
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
                defaulted: Vec::new(),
            }],
            ..Project::default()
        };
        let fault = solve(&project, None).unwrap();
        let expect = 500.0 / (3.0_f64.sqrt() * 13.8);
        let point = fault.buses[0].three_phase.as_ref().unwrap();
        let got = point.symmetrical_ka;
        assert!((got - expect).abs() / expect < 1e-6, "got {got}, expected {expect}");
        // 13.8 kV is above 1 kV: IEC 60909-0 cmin = 1.00, cmax = 1.10.
        // Flat prefault is 1.0 pu, so the factor multiplies the symmetrical current.
        assert!((point.c_min - 1.00).abs() < 1e-12);
        assert!((point.c_max - 1.10).abs() < 1e-12);
        assert!((point.symmetrical_min_ka - expect).abs() / expect < 1e-6);
        assert!((point.symmetrical_max_ka - 1.10 * expect).abs() / expect < 1e-6);
        let nacd = fault.buses[0].nacd.expect("utility-only fault reports NACD");
        assert!((nacd - 1.0).abs() < 1e-6, "utility-only NACD {nacd}");
        assert_eq!(fault.buses[0].infeeds.len(), 1);
        assert!(fault.buses[0].infeeds[0].remote);
        assert_eq!(fault.buses[0].infeeds[0].source_id.as_deref(), Some("s"));
    }

    #[test]
    fn nacd_drops_when_a_motor_or_a_decrement_curve_is_local() {
        let mut project = Project {
            buses: vec![Bus { id: "a".into(), name: "A".into(), kv: 13.8, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None }],
            sources: vec![Source { generator: None, machine_kind: None, transient_mva_sc: None,
                id: "s".into(), name: "grid".into(), bus: "a".into(), in_service: true,
                decrement_curve: Vec::new(), v_pu: 1.0, angle_deg: 0.0, mva_sc: 500.0, xr: 15.0,
                x0_over_x1: 1.0, r0_over_r1: 1.0, is_slack: true, p_mw: 0.0,
                qmin_mvar: None, qmax_mvar: None, defaulted: Vec::new(),
            }],
            motors: vec![Motor { machine_kind: None, synchronous_rpm: None, phases: None,
                id: "m".into(), name: "M".into(), bus: "a".into(), hp: 1000.0, kv: 13.8,
                pf: 0.85, efficiency: 0.94, x_subtransient_pu: 0.17, xr: 6.0, defaulted: Vec::new(),
            }],
            ..Project::default()
        };
        let with_motor = solve(&project, None).unwrap();
        let motor_bus = &with_motor.buses[0];
        let motor_nacd = motor_bus.nacd.expect("motor fault reports NACD");
        assert!(motor_nacd < 0.99 && motor_nacd > 0.5, "motor NACD {motor_nacd}");
        assert!(motor_bus.infeeds.iter().any(|row| row.source_id.as_deref() == Some("s") && row.remote));
        assert!(motor_bus.infeeds.iter().any(|row| row.source_id.is_none() && !row.remote));

        project.motors.clear();
        project.sources[0].decrement_curve = vec![
            DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
            DecrementPoint { time_s: 0.1, current_ratio: 0.7 },
        ];
        let local_fault = solve(&project, None).unwrap();
        let local_bus = &local_fault.buses[0];
        let local = local_bus.nacd.expect("local source reports NACD");
        assert!(local < 1e-6, "decrement-curve source NACD {local}");
        assert!(local_bus.infeeds.iter().any(|row| row.source_id.as_deref() == Some("s") && !row.remote));
    }

    #[test]
    fn voltage_factor_follows_the_one_kilovolt_boundary() {
        assert_eq!(iec_voltage_factor(0.48, false).unwrap(), (0.95, 1.05));
        assert_eq!(iec_voltage_factor(1.0, false).unwrap(), (0.95, 1.05));
        assert_eq!(iec_voltage_factor(0.48, true).unwrap(), (0.95, 1.10));
        assert_eq!(iec_voltage_factor(1.001, false).unwrap(), (1.00, 1.10));
        assert_eq!(iec_voltage_factor(35.0, true).unwrap(), (1.00, 1.10));
        assert_eq!(iec_voltage_factor(230.0, false).unwrap(), (1.00, 1.10));
        assert!(iec_voltage_factor(0.0, false).is_err());
    }

    #[test]
    fn dyn_transformer_blocks_utility_zero_sequence() {
        let project = Project {
            s_base_mva: 100.0,
            buses: vec![
                Bus { id: "hv".into(), name: "HV".into(), kv: 12.47, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None },
                Bus { id: "lv".into(), name: "LV".into(), kv: 0.48, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None },
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
            sources: vec![Source { generator: None, machine_kind: None, transient_mva_sc: None,
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
                defaulted: Vec::new(),
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
        for kv in [0.48, 13.8] {
            // Purely inductive: kappa_b = 2.0 already exceeds either cap.
            let (xr, peak, rms) = asymmetry(10.0, Cplx::new(0.0, 1.0), kv).unwrap();
            assert_eq!(xr, None);
            assert!((peak-20.0*2.0_f64.sqrt()).abs() < 1e-12);
            assert!((rms-10.0*3.0_f64.sqrt()).abs() < 1e-12);
            for x in [0.0, 1e-15] {
                let (xr, peak, rms) = asymmetry(10.0, Cplx::new(1.0, x), kv).unwrap();
                assert_eq!(xr, Some(0.0));
                assert!((peak-10.0*2.0_f64.sqrt()).abs() < 1e-12);
                assert_eq!(rms, 10.0);
            }
            for z in [Cplx::ZERO, Cplx::new(-1.0,1.0), Cplx::new(1.0,-1.0), Cplx::new(f64::NAN,1.0), Cplx::new(f64::INFINITY,1.0)] {
                assert!(asymmetry(10.0,z,kv).is_err());
            }
        }
    }

    #[test]
    fn iec_method_b_applies_1_15_with_voltage_dependent_cap() {
        let kappa_b = |rx: f64| 1.02 + 0.98 * (-3.0 * rx).exp();
        // R/X = 0.5: 1.15 * kappa_b = 1.4245 is below both caps.
        for kv in [0.48, 13.8] {
            assert!((iec_kappa(0.5, kv, true) - 1.15 * kappa_b(0.5)).abs() < 1e-12);
        }
        // R/X = 0.1: 1.15 * 1.7460 = 2.008, limited to 1.8 at LV and 2.0 above 1 kV.
        assert!((iec_kappa(0.1, 0.48, true) - 1.8).abs() < 1e-12);
        assert!((iec_kappa(0.1, 1.0, true) - 1.8).abs() < 1e-12);
        assert!((iec_kappa(0.1, 4.16, true) - 2.0).abs() < 1e-12);
        // R/X = 0.02: kappa_b = 1.9229 exceeds the LV cap and is kept.
        assert!((iec_kappa(0.02, 0.48, true) - kappa_b(0.02)).abs() < 1e-12);
        assert_eq!(iec_kappa(f64::INFINITY, 0.48, true), 1.0);
        // Peak uses kappa; X/R 10 at 0.48 kV gives sqrt(2) * 1.8 * I.
        let (_, peak, _) = asymmetry(10.0, Cplx::new(0.1, 1.0), 0.48).unwrap();
        assert!((peak - 1.8 * 2.0_f64.sqrt() * 10.0).abs() < 1e-12);
    }

    #[test]
    fn one_bad_bus_is_reported_without_aborting_other_buses() {
        use crate::model::*;
        let mut project = Project::sample();
        project.motors.clear(); project.devices.clear(); project.equipment.clear(); project.loads.clear();
        project.buses.truncate(2);
        project.branches.truncate(1);
        for bus in &mut project.buses { bus.kv = 13.8; }
        project.sources[0].mva_sc = 1000.0; project.sources[0].xr = 10.0;
        project.branches[0].kind = BranchKind::Line {
            r_ohm: 0.01 * 1.9044, x_ohm: 0.1 * 1.9044, b_siemens: 0.0, r0_ohm: 0.0, x0_ohm: 0.0, ampacity_a: None,
        };
        // 20 pu of capacitance makes the far-bus Thevenin impedance capacitive (X < 0).
        project.buses[1].shunt_kvar = 2_000_000.0;
        let result = solve(&project, None).unwrap();
        let near = &result.buses[0];
        assert!(near.error.is_none() && near.three_phase.is_some() && near.line_to_ground.is_some());
        let far = &result.buses[1];
        assert!(far.three_phase.is_none());
        assert!(far.error.as_deref().is_some_and(|e| e.contains("nonnegative R and X")), "{:?}", far.error);
        assert!(far.note.as_deref().is_some_and(|n| n.contains("fault calculation failed")));
        let json = serde_json::to_value(&result).unwrap();
        assert!(json["buses"][0].get("error").is_none());
        assert!(json["buses"][1]["error"].is_string());
    }
}
