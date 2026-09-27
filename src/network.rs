use std::collections::HashMap;

use crate::cplx::Cplx;
use crate::model::{zbase_ohm, Branch, BranchKind, Project, XfmrConn};

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
}

pub fn build(project: &Project) -> Result<SystemModel, String> {
    let index = Index::build(project)?;
    let n = index.n;
    let mut loadflow = SparseY::new(n);
    let mut pos = Net::new(n);
    let mut neg = Net::new(n);
    let mut zero = Net::new(n);
    let mut warnings = Vec::new();
    let mut branches = Vec::new();

    for branch in &project.branches {
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
        let i = index.of(&source.bus)?;
        let z1 = Cplx::from_mag_xr(project.s_base_mva / source.mva_sc, source.xr);
        let y1 = z1.inv().ok_or_else(|| format!("source '{}' has zero impedance", source.id))?;
        pos.shunt(i, y1);
        neg.shunt(i, y1);
        let z0 = Cplx::new(z1.re * source.r0_over_r1, z1.im * source.x0_over_x1);
        if let Some(y0) = z0.inv() {
            zero.shunt(i, y0);
        }
    }

    for motor in &project.motors {
        let i = index.of(&motor.bus)?;
        let kw = motor.hp * 0.746 / motor.efficiency;
        let kva = kw / motor.pf;
        let x = motor.x_subtransient_pu * (project.s_base_mva / (kva / 1000.0));
        let r = x / motor.xr.max(0.1);
        let y = Cplx::new(r, x).inv().ok_or_else(|| format!("motor '{}' has zero impedance", motor.id))?;
        pos.shunt(i, y);
        neg.shunt(i, y);
    }

    Ok(SystemModel { index, loadflow, pos, neg, zero, branches, warnings })
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
            let zb = zbase_ohm(index.kv[i], project.s_base_mva);
            let z1 = Cplx::new(r_ohm / zb, x_ohm / zb);
            let y = z1.inv().ok_or_else(|| format!("line '{}' has zero impedance", branch.id))?;
            let b = Cplx::new(0.0, b_siemens * zb / 2.0);
            let q = Quad { yff: y + b, yft: -y, ytf: -y, ytt: y + b };
            let (r0, x0) = if r0_ohm.abs() < 1e-12 && x0_ohm.abs() < 1e-12 {
                (3.0 * r_ohm, 3.0 * x_ohm)
            } else {
                (*r0_ohm, *x0_ohm)
            };
            let zero = match Cplx::new(r0 / zb, x0 / zb).inv() {
                Some(y0) => ZeroStamp::Series(Quad { yff: y0 + b, yft: -y0, ytf: -y0, ytt: y0 + b }),
                None => ZeroStamp::Open,
            };
            Ok(BranchStamp {
                from: i,
                to: j,
                positive: q,
                negative: q,
                zero,
                ampacity_a: *ampacity_a,
                rating_kva: None,
            })
        }
        BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, connection, tap_percent, x0_over_x1 } => {
            let from_rated = nearest(index.kv[i], *hv_kv, *lv_kv);
            let to_rated = nearest(index.kv[j], *hv_kv, *lv_kv);
            if (from_rated - to_rated).abs() < 1e-9 {
                return Err(format!(
                    "transformer '{}' buses both match the {:.3} kV winding",
                    branch.name, from_rated
                ));
            }
            let z_mag = (z_percent / 100.0) * (project.s_base_mva / (kva / 1000.0)) * (from_rated / index.kv[i]).powi(2);
            let z1 = Cplx::from_mag_xr(z_mag, *xr);
            let y1 = z1.inv().ok_or_else(|| format!("transformer '{}' has zero impedance", branch.id))?;
            let a_mag = (1.0 + tap_percent / 100.0) * (index.kv[i] / from_rated) * (to_rated / index.kv[j]);
            let phi = match connection {
                XfmrConn::Dyn => 30.0_f64.to_radians(),
                XfmrConn::Ynd => -30.0_f64.to_radians(),
                XfmrConn::Ynyn | XfmrConn::Dd => 0.0,
            };
            let positive = quad(y1, Cplx::from_polar(a_mag, phi));
            let negative = quad(y1, Cplx::from_polar(a_mag, -phi));
            let z0 = z1 * *x0_over_x1;
            let zero = match (connection, z0.inv()) {
                (XfmrConn::Dyn, Some(y0)) => ZeroStamp::Shunt { bus_is_from: false, y: y0 },
                (XfmrConn::Ynd, Some(y0)) => ZeroStamp::Shunt { bus_is_from: true, y: y0 },
                (XfmrConn::Ynyn, Some(y0)) => ZeroStamp::Series(quad(y0, Cplx::real(a_mag))),
                _ => ZeroStamp::Open,
            };
            Ok(BranchStamp {
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

fn quad(y: Cplx, a: Cplx) -> Quad {
    let a2 = a.re * a.re + a.im * a.im;
    Quad {
        yff: y * (1.0 / a2),
        yft: -(y / a.conj()),
        ytf: -(y / a),
        ytt: y,
    }
}

fn nearest(bus_kv: f64, hv: f64, lv: f64) -> f64 {
    if (bus_kv / hv).ln().abs() <= (bus_kv / lv).ln().abs() { hv } else { lv }
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
