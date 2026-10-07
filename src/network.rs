use std::collections::HashMap;

use crate::cplx::Cplx;
use crate::model::{nearest_winding_kv, zbase_ohm, Branch, BranchKind, GroundingMethod, Motor, Project, Source, Winding, XfmrConn};

#[derive(Clone, Debug)]
pub struct Index {
    pub n: usize,
    pub kv: Vec<f64>,
    pub id_of: Vec<String>,
    pub name_of: Vec<String>,
    pub map: HashMap<String, usize>,
}

impl Index {
    pub fn build(project: &Project) -> Result<Self, String> {
        let mut map = HashMap::new();
        let mut kv = Vec::new();
        let mut id_of = Vec::new();
        let mut name_of = Vec::new();
        for bus in &project.buses {
            if map.insert(bus.id.clone(), id_of.len()).is_some() {
                return Err(format!("duplicate bus id '{}'", bus.id));
            }
            id_of.push(bus.id.clone());
            name_of.push(if bus.name.is_empty() { bus.id.clone() } else { bus.name.clone() });
            kv.push(bus.kv);
        }
        Ok(Self { n: id_of.len(), kv, id_of, name_of, map })
    }

    pub fn of(&self, id: &str) -> Result<usize, String> {
        self.map.get(id).copied().ok_or_else(|| format!("unknown bus '{id}'"))
    }
}

#[derive(Clone, Debug)]
pub struct SparseY {
    pub n: usize,
    rows: Vec<Vec<(usize, Cplx)>>,
}

impl SparseY {
    pub fn new(n: usize) -> Self {
        Self { n, rows: vec![Vec::new(); n] }
    }

    pub fn add(&mut self, i: usize, j: usize, y: Cplx) {
        if y.abs() < 1e-18 {
            return;
        }
        if let Some(slot) = self.rows[i].iter_mut().find(|(col, _)| *col == j) {
            slot.1 += y;
        } else {
            self.rows[i].push((j, y));
        }
    }

    pub fn rows(&self) -> &[Vec<(usize, Cplx)>] {
        &self.rows
    }

    pub fn mul(&self, v: &[Cplx]) -> Vec<Cplx> {
        let mut out = vec![Cplx::ZERO; self.n];
        for (i, row) in self.rows.iter().enumerate() {
            for &(j, y) in row {
                out[i] += y * v[j];
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct Net {
    pub y: SparseY,
    pub shunt: Vec<bool>,
    pub edges: Vec<(usize, usize)>,
}

impl Net {
    fn new(n: usize) -> Self {
        Self { y: SparseY::new(n), shunt: vec![false; n], edges: Vec::new() }
    }

    fn series(&mut self, i: usize, j: usize, q: Quad) {
        self.y.add(i, i, q.yff);
        self.y.add(i, j, q.yft);
        self.y.add(j, i, q.ytf);
        self.y.add(j, j, q.ytt);
        self.edges.push((i, j));
    }

    fn shunt(&mut self, i: usize, y: Cplx) {
        self.y.add(i, i, y);
        if y.abs() > 1e-12 {
            self.shunt[i] = true;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub yff: Cplx,
    pub yft: Cplx,
    pub ytf: Cplx,
    pub ytt: Cplx,
}

pub enum ZeroStamp {
    Series(Quad),
    Shunt { bus_is_from: bool, y: Cplx },
    Open,
}

pub struct BranchStamp {
    pub active: bool,
    pub from: usize,
    pub to: usize,
    pub positive: Quad,
    pub negative: Quad,
    pub zero: ZeroStamp,
    pub ampacity_a: Option<f64>,
    pub rating_kva: Option<f64>,
}

pub struct SystemModel {
    pub index: Index,
    pub loadflow: SparseY,
    pub pos: Net,
    pub neg: Net,
    pub zero: Net,
    pub branches: Vec<BranchStamp>,
    pub warnings: Vec<String>,
    pub energized: Vec<bool>,
}

pub fn build(project: &Project) -> Result<SystemModel, String> {
    let errors = project.validate();
    if !errors.is_empty() { return Err(errors.join("; ")); }
    build_validated(project)
}

/// Build only after validating this unchanged project snapshot.
pub(crate) fn build_validated(project: &Project) -> Result<SystemModel, String> {
    let index = Index::build(project)?;
    let n = index.n;
    let mut loadflow = SparseY::new(n);
    let mut pos = Net::new(n);
    let mut neg = Net::new(n);
    let mut zero = Net::new(n);
    let mut warnings = input_warnings(project);
    let mut branches = Vec::new();

    for branch in &project.branches {
        if !project.branch_closed(branch) {
            let from = index.of(&branch.from)?;
            let to = index.of(&branch.to)?;
            let zero_quad = Quad { yff: Cplx::ZERO, yft: Cplx::ZERO, ytf: Cplx::ZERO, ytt: Cplx::ZERO };
            branches.push(BranchStamp {
                active: false,
                from,
                to,
                positive: zero_quad,
                negative: zero_quad,
                zero: ZeroStamp::Open,
                ampacity_a: None,
                rating_kva: None,
            });
            continue;
        }
        let stamp = stamp_branch(project, &index, branch, &mut warnings)?;
        let q = stamp.positive;
        loadflow.add(stamp.from, stamp.from, q.yff);
        loadflow.add(stamp.from, stamp.to, q.yft);
        loadflow.add(stamp.to, stamp.from, q.ytf);
        loadflow.add(stamp.to, stamp.to, q.ytt);
        pos.series(stamp.from, stamp.to, stamp.positive);
        neg.series(stamp.from, stamp.to, stamp.negative);
        match stamp.zero {
            ZeroStamp::Series(q0) => zero.series(stamp.from, stamp.to, q0),
            ZeroStamp::Shunt { bus_is_from, y } => {
                zero.shunt(if bus_is_from { stamp.from } else { stamp.to }, y);
            }
            ZeroStamp::Open => {}
        }
        branches.push(stamp);
    }

    for bus in &project.buses {
        let i = index.of(&bus.id)?;
        if bus.shunt_kvar.abs() > 0.0 {
            // Positive shunt_kvar is a capacitor. B > 0 is the standard Ybus sign.
            let b = bus.shunt_kvar / (project.s_base_mva * 1000.0);
            let y = Cplx::new(0.0, b);
            loadflow.add(i, i, y);
            pos.shunt(i, y);
            neg.shunt(i, y);
        }
    }

    for source in &project.sources {
        if !source.in_service { continue; }
        let i = index.of(&source.bus)?;
        let z1 = Cplx::from_mag_xr(project.s_base_mva / source.mva_sc, source.xr);
        let y1 = z1.inv().ok_or_else(|| format!("source '{}' has zero impedance", source.id))?;
        pos.shunt(i, y1);
        neg.shunt(i, y1);
        if let Neutral::Grounded(zn) = neutral(project, &source.id, None, index.kv[i]) {
            let z0 = Cplx::new(z1.re * source.r0_over_r1, z1.im * source.x0_over_x1) + zn * 3.0;
            let y0 = z0.inv().ok_or_else(|| format!("source {}: invalid zero-sequence impedance", source.id))?;
            zero.shunt(i, y0);
        }
    }

    // Only utility/generator sources energize islands; passive shunts and motors do not.
    let mut supply = Net::new(n);
    supply.edges = pos.edges.clone();
    for source in project.sources.iter().filter(|source| source.in_service) {
        supply.shunt[index.of(&source.bus)?] = true;
    }
    let energized = reaches_shunt(&supply);

    for motor in &project.motors {
        let i = index.of(&motor.bus)?;
        if !energized[i] { continue; }
        let kw = motor.hp * 0.746 / motor.efficiency;
        let kva = kw / motor.pf;
        let x = motor.x_subtransient_pu * (project.s_base_mva / (kva / 1000.0)) * (motor.kv / index.kv[i]).powi(2);
        let r = x / motor.xr;
        let y = Cplx::new(r, x).inv().ok_or_else(|| format!("motor '{}' has zero impedance", motor.id))?;
        pos.shunt(i, y);
        neg.shunt(i, y);
    }

    for y in [&loadflow, &pos.y, &neg.y, &zero.y] {
        if y.rows().iter().flatten().any(|(_, v)| !v.re.is_finite() || !v.im.is_finite()) {
            return Err("non-finite network admittance (check impedances and voltage bases)".into());
        }
    }
    Ok(SystemModel { index, loadflow, pos, neg, zero, branches, warnings, energized })
}

/// Model-level assumptions and unusual entries reported with the study
/// warnings: omitted source/motor ratings filled with typical defaults.
fn input_warnings(project: &Project) -> Vec<String> {
    let mut warnings = Vec::new();
    for source in &project.sources {
        for field in &source.defaulted {
            if let Some(value) = Source::default_value(field) {
                warnings.push(format!("source {}: {field} not entered; {value} assumed", source.id));
            }
        }
    }
    for motor in &project.motors {
        for field in &motor.defaulted {
            if let Some(value) = Motor::default_value(field) {
                warnings.push(format!("motor {}: {field} not entered; {value} assumed", motor.id));
            }
        }
    }
    // Accepted but unusual entries that are easy to make by mistake.
    for source in &project.sources {
        if !(0.8..=1.2).contains(&source.v_pu) {
            warnings.push(format!("source {}: voltage setpoint {} pu is outside 0.8-1.2 pu", source.id, source.v_pu));
        }
    }
    for load in &project.loads {
        if load.kw < 0.0 {
            warnings.push(format!("load {}: negative kW ({}) is modeled as generation", load.id, load.kw));
        }
    }
    for branch in &project.branches {
        if let BranchKind::Line { b_siemens, .. } = branch.kind {
            if b_siemens < 0.0 {
                warnings.push(format!("line {}: negative b_siemens ({b_siemens}) is an inductive (reactor) shunt, not cable charging", branch.id));
            }
        }
    }
    warnings
}

fn stamp_branch(project: &Project, index: &Index, branch: &Branch, warnings: &mut Vec<String>) -> Result<BranchStamp, String> {
    let i = index.of(&branch.from)?;
    let j = index.of(&branch.to)?;
    match &branch.kind {
        BranchKind::Line { r_ohm, x_ohm, b_siemens, r0_ohm, x0_ohm, ampacity_a } => {
            if (index.kv[i] - index.kv[j]).abs() / index.kv[i] > 0.02 {
                warnings.push(format!(
                    "line '{}' connects {:.3} kV to {:.3} kV",
                    branch.name, index.kv[i], index.kv[j]
                ));
            }
            let zb_from = zbase_ohm(index.kv[i], project.s_base_mva);
            let zb_to = zbase_ohm(index.kv[j], project.s_base_mva);
            let base_ratio = Cplx::real(index.kv[j] / index.kv[i]);
            // A physical series impedance has different per-unit admittances
            // at differently based buses. The off-diagonal term uses both
            // voltage bases; treating them as equal creates zero-current lines
            // between unequal physical voltages.
            let z1 = Cplx::new(r_ohm / zb_to, x_ohm / zb_to);
            tiny_impedance_warning(branch, z1, warnings);
            let y = z1.inv().ok_or_else(|| format!("line '{}' has zero impedance", branch.id))?;
            let mut q = quad(y, base_ratio)?;
            q.yff += Cplx::new(0.0, b_siemens * zb_from / 2.0);
            q.ytt += Cplx::new(0.0, b_siemens * zb_to / 2.0);
            let (r0_missing, x0_missing) = (r0_ohm.abs() < 1e-12, x0_ohm.abs() < 1e-12);
            let (r0, x0) = if r0_missing && x0_missing {
                { warnings.push(format!("line {}: R0=3R1 and X0=3X1 assumed", branch.id)); (3.0 * r_ohm, 3.0 * x_ohm) }
            } else {
                if r0_missing || x0_missing {
                    // Kept as entered; a cable with only one zero-sequence term is unusual.
                    let (entered, zero) = if r0_missing { ("X0", "R0") } else { ("R0", "X0") };
                    warnings.push(format!("line {}: only {entered} entered; {zero}=0 used for the zero sequence", branch.id));
                }
                (*r0_ohm, *x0_ohm)
            };
            if *b_siemens != 0.0 { warnings.push(format!("line {}: zero-sequence charging omitted; B0 was not supplied", branch.id)); }
            let y0 = Cplx::new(r0 / zb_to, x0 / zb_to).inv()
                .ok_or_else(|| format!("line {}: zero or non-finite zero-sequence impedance", branch.id))?;
            let zero = ZeroStamp::Series(quad(y0, base_ratio)?);
            Ok(BranchStamp {
                active: true,
                from: i,
                to: j,
                positive: q,
                negative: q,
                zero,
                ampacity_a: *ampacity_a,
                rating_kva: None,
            })
        }
        BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, connection, tap_percent, x0_over_x1, r0_over_r1 } => {
            let from_rated = nearest_winding_kv(index.kv[i], *hv_kv, *lv_kv);
            let to_rated = nearest_winding_kv(index.kv[j], *hv_kv, *lv_kv);
            if (from_rated - to_rated).abs() < 1e-9 {
                return Err(format!(
                    "transformer '{}' buses both match the {:.3} kV winding",
                    branch.name, from_rated
                ));
            }
            // Tap is a change to HV winding turns, independent of branch orientation.
            let tap = 1.0 + tap_percent / 100.0;
            let from_turns = from_rated * if from_rated == *hv_kv { tap } else { 1.0 };
            let to_turns = to_rated * if to_rated == *hv_kv { tap } else { 1.0 };
            let z_mag = (z_percent / 100.0) * (project.s_base_mva / (kva / 1000.0)) * (to_turns / index.kv[j]).powi(2);
            let z1 = Cplx::from_mag_xr(z_mag, *xr);
            tiny_impedance_warning(branch, z1, warnings);
            let y1 = z1.inv().ok_or_else(|| format!("transformer '{}' has zero impedance", branch.id))?;
            for (bus, rated) in [(i, from_rated), (j, to_rated)] {
                if (index.kv[bus] / rated - 1.0).abs() > 0.10 {
                    warnings.push(format!(
                        "transformer {}: bus {} nominal {:.3} kV differs from its matched {:.3} kV winding by more than 10%",
                        branch.id, index.id_of[bus], index.kv[bus], rated
                    ));
                }
            }
            let a_mag = (from_turns / index.kv[i]) * (index.kv[j] / to_turns);
            // ANSI C57.12.00: for delta-wye and wye-delta units the HV
            // positive-sequence voltage leads the LV by 30 degrees, whichever
            // winding is delta and whichever way the branch is drawn. With
            // V_to = V_from / a at no load, the from-to angle is +30 degrees
            // when the from bus is HV and -30 degrees when it is LV.
            let from_is_hv = from_rated == *hv_kv;
            let phi = match connection {
                XfmrConn::Dyn | XfmrConn::Ynd => if from_is_hv { 30.0_f64.to_radians() } else { -30.0_f64.to_radians() },
                XfmrConn::Ynyn | XfmrConn::Dd => 0.0,
            };
            let positive = quad(y1, Cplx::from_polar(a_mag, phi))?;
            let negative = quad(y1, Cplx::from_polar(a_mag, -phi))?;
            // z0 is on the to-bus base. A neutral impedance Zn adds 3Zn in
            // series with its own wye winding's zero-sequence path; a
            // from-side quantity is referred to the to side by 1/a^2.
            let z0 = Cplx::new(z1.re * r0_over_r1, z1.im * x0_over_x1);
            if z0.inv().is_none() { return Err(format!("transformer {}: zero or non-finite zero-sequence impedance", branch.id)); }
            let (from_winding, to_winding) = if from_is_hv { (Winding::Hv, Winding::Lv) } else { (Winding::Lv, Winding::Hv) };
            let from_neutral = neutral(project, &branch.id, Some(from_winding), index.kv[i]);
            let to_neutral = neutral(project, &branch.id, Some(to_winding), index.kv[j]);
            let invert = |z: Cplx| z.inv().ok_or_else(|| format!("transformer {}: zero or non-finite zero-sequence impedance", branch.id));
            let a2 = a_mag.powi(2);
            let zero = match (connection, from_neutral, to_neutral) {
                (XfmrConn::Dyn, _, Neutral::Grounded(zn)) => ZeroStamp::Shunt { bus_is_from: false, y: invert(z0 + zn * 3.0)? },
                (XfmrConn::Ynd, Neutral::Grounded(zn), _) => ZeroStamp::Shunt { bus_is_from: true, y: invert(z0 * a2 + zn * 3.0)? },
                (XfmrConn::Ynyn, Neutral::Grounded(zf), Neutral::Grounded(zt)) =>
                    ZeroStamp::Series(quad(invert(z0 + zt * 3.0 + zf * (3.0 / a2))?, Cplx::real(a_mag))?),
                // Delta windings, and wye windings with an ungrounded neutral, block zero sequence.
                _ => ZeroStamp::Open,
            };
            Ok(BranchStamp {
                active: true,
                from: i,
                to: j,
                positive,
                negative,
                zero,
                ampacity_a: None,
                rating_kva: Some(*kva),
            })
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Neutral {
    /// Neutral impedance in per unit on the system base at the given bus kV.
    Grounded(Cplx),
    Ungrounded,
}

/// Grounding of a source or transformer winding; no entry means solid.
fn neutral(project: &Project, element_id: &str, winding: Option<Winding>, bus_kv: f64) -> Neutral {
    match project.grounding_of(element_id, winding) {
        None => Neutral::Grounded(Cplx::ZERO),
        Some(g) => match g.method {
            GroundingMethod::Solid => Neutral::Grounded(Cplx::ZERO),
            GroundingMethod::Ungrounded => Neutral::Ungrounded,
            GroundingMethod::Impedance => {
                let zb = zbase_ohm(bus_kv, project.s_base_mva);
                Neutral::Grounded(Cplx::new(g.r_ohm / zb, g.x_ohm / zb))
            }
        },
    }
}

/// Per-unit series impedance below which admittances approach double-precision
/// roundoff relative to ordinary branches.
const TINY_BRANCH_PU: f64 = 1e-6;

fn tiny_impedance_warning(branch: &Branch, z: Cplx, warnings: &mut Vec<String>) {
    if z.abs() < TINY_BRANCH_PU {
        warnings.push(format!(
            "branch {}: series impedance {:.2e} pu is below 1e-6 pu; results near this branch are limited by numerical roundoff",
            branch.id, z.abs()
        ));
    }
}

fn quad(y: Cplx, a: Cplx) -> Result<Quad, String> {
    if a.inv().is_none() { return Err("zero or non-finite transformer ratio".into()); }
    let a2 = a.re * a.re + a.im * a.im;
    Ok(Quad {
        yff: y * (1.0 / a2),
        yft: -(y / a.conj()),
        ytf: -(y / a),
        ytt: y,
    })
}

pub fn reaches_shunt(net: &Net) -> Vec<bool> {
    let n = net.y.n;
    let mut adj = vec![Vec::new(); n];
    for &(i, j) in &net.edges {
        adj[i].push(j);
        adj[j].push(i);
    }
    let mut seen = vec![false; n];
    let mut stack: Vec<usize> = net.shunt.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| i).collect();
    for &s in &stack {
        seen[s] = true;
    }
    while let Some(i) = stack.pop() {
        for &j in &adj[i] {
            if !seen[j] {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    seen
}

pub fn reduce(y: &SparseY, mask: &[bool]) -> (SparseY, Vec<Option<usize>>) {
    let mut map = vec![None; y.n];
    let mut next = 0;
    for i in 0..y.n {
        if mask[i] {
            map[i] = Some(next);
            next += 1;
        }
    }
    let mut out = SparseY::new(next);
    for (i, row) in y.rows().iter().enumerate() {
        let Some(ii) = map[i] else { continue };
        for &(j, yij) in row {
            if let Some(jj) = map[j] {
                out.add(ii, jj, yij);
            }
        }
    }
    (out, map)
}
