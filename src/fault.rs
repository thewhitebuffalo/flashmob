use serde::{Deserialize, Serialize};

use crate::cplx::Cplx;
use crate::loadflow::LoadflowResult;
use crate::model::{i_base_ka, Prefault, Project};
use crate::network::{self, Net};
use crate::solve::zth;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaultResult {
    pub prefault: String,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaultPoint {
    pub symmetrical_ka: f64,
    pub ia_ka: f64,
    pub ib_ka: f64,
    pub ic_ka: f64,
    pub ig_ka: f64,
    pub x_over_r: f64,
    pub iec_peak_ka: f64,
    pub half_cycle_rms_ka: f64,
}

pub fn solve(project: &Project, loadflow: Option<&LoadflowResult>) -> Result<FaultResult, String> {
    let model = network::build(project)?;
    let n = model.index.n;
    let (z1, map1) = reduced(&model.pos)?;
    let (z2, map2) = reduced(&model.neg)?;
    let (z0, map0) = reduced(&model.zero)?;

    let mut prefault_v = vec![Cplx::real(1.0); n];
    let mut label = "flat_1.0_pu".to_string();
    if project.prefault == Prefault::Loadflow {
        if let Some(lf) = loadflow.filter(|lf| lf.converged) {
            for bus in &lf.buses {
                if let Ok(i) = model.index.of(&bus.id) {
                    prefault_v[i] = Cplx::from_polar(bus.v_pu, bus.angle_deg.to_radians());
                }
            }
            label = "loadflow".into();
        } else {
            label = "flat_1.0_pu_loadflow_unavailable".into();
        }
    }

    let mut buses = Vec::new();
    for i in 0..n {
        let v = prefault_v[i];
        let ibase = i_base_ka(model.index.kv[i], project.s_base_mva);
        let zz1 = zth(&z1, &map1, i)?;
        let zz2 = zth(&z2, &map2, i)?;
        let zz0 = zth(&z0, &map0, i)?;
        let mut note = None;
        let three = zz1.map(|z| point(v / z, Cplx::ZERO, Cplx::ZERO, z, ibase));
        let line_line = match (zz1, zz2) {
            (Some(a), Some(b)) => {
                let i1 = v / (a + b);
                Some(point(i1, -i1, Cplx::ZERO, a + b, ibase))
            }
            _ => None,
        };
        let (line_ground, double) = match (zz1, zz2, zz0) {
            (Some(a), Some(b), Some(c)) => {
                let i1 = v / (a + b + c);
                let lg = point(i1, i1, i1, a + b + c, ibase);
                let zpar = (b * c) / (b + c);
                let i1g = v / (a + zpar);
                let i2 = -i1g * c / (b + c);
                let i0 = -i1g * b / (b + c);
                (Some(lg), Some(point(i1g, i2, i0, a + zpar, ibase)))
            }
            (Some(a), Some(b), None) => {
                note = Some("no zero-sequence path; ground fault current is zero".into());
                let i1 = v / (a + b);
                (Some(zero_point()), Some(point(i1, -i1, Cplx::ZERO, a + b, ibase)))
            }
            _ => (None, None),
        };
        if three.is_none() {
            note = Some("bus is not connected to a source".into());
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
        });
    }
    Ok(FaultResult { prefault: label, buses })
}

fn reduced(net: &Net) -> Result<(crate::network::SparseY, Vec<Option<usize>>), String> {
    let mask = network::reaches_shunt(net);
    Ok(network::reduce(&net.y, &mask))
}

fn point(i1: Cplx, i2: Cplx, i0: Cplx, zth: Cplx, ibase: f64) -> FaultPoint {
    let (ia, ib, ic) = phases(i0, i1, i2);
    let ig = i0 * 3.0;
    let sym = ia.abs().max(ib.abs()).max(ic.abs()) * ibase;
    let (xr, peak, asym) = asymmetry(sym, zth);
    FaultPoint {
        symmetrical_ka: sym,
        ia_ka: ia.abs() * ibase,
        ib_ka: ib.abs() * ibase,
        ic_ka: ic.abs() * ibase,
        ig_ka: ig.abs() * ibase,
        x_over_r: xr,
        iec_peak_ka: peak,
        half_cycle_rms_ka: asym,
    }
}

fn zero_point() -> FaultPoint {
    FaultPoint {
        symmetrical_ka: 0.0,
        ia_ka: 0.0,
        ib_ka: 0.0,
        ic_ka: 0.0,
        ig_ka: 0.0,
        x_over_r: 0.0,
        iec_peak_ka: 0.0,
        half_cycle_rms_ka: 0.0,
    }
}

fn phases(i0: Cplx, i1: Cplx, i2: Cplx) -> (Cplx, Cplx, Cplx) {
    let a = Cplx::from_polar(1.0, 2.0 * std::f64::consts::PI / 3.0);
    let a2 = a.conj();
    (i0 + i1 + i2, i0 + a2 * i1 + a * i2, i0 + a * i1 + a2 * i2)
}

fn asymmetry(i_sym: f64, z: Cplx) -> (f64, f64, f64) {
    let r_over_x = if z.im.abs() < 1e-12 { 0.0 } else { (z.re / z.im).max(0.0) };
    let xr = if z.re.abs() < 1e-12 { 1.0e6 } else { z.im / z.re };
    let kappa = 1.02 + 0.98 * (-3.0 * r_over_x).exp();
    let peak = kappa * std::f64::consts::SQRT_2 * i_sym;
    let asym = i_sym * (1.0 + 2.0 * (-2.0 * std::f64::consts::PI * r_over_x).exp()).sqrt();
    (xr, peak, asym)
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
                },
            }],
            sources: vec![Source {
                id: "s".into(),
                name: "grid".into(),
                bus: "hv".into(),
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
}
