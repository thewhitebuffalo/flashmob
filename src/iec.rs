//! IEC 60909-0 impedance correction factors for the maximum short-circuit
//! current (clauses 6.3.3, 6.6.1, 6.7.1 and 6.7.2). Each factor is evaluated
//! from its closed-form relationship with entered data, and the corrected
//! impedance is the entered impedance times the factor. The voltage factor
//! cmax is the Table 1 value already used for the maximum current.

use serde::{Deserialize, Serialize};

use crate::fault::iec_voltage_factor;
use crate::model::{BranchKind, GeneratorKind, Project};

/// One applied correction factor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IecCorrection {
    /// Branch id for a transformer, source id for a generator.
    pub element_id: String,
    /// "K_T", "K_G", "K_S", or "K_SO".
    pub factor_name: String,
    pub factor: f64,
}

/// Network transformer factor from its relative reactance and the cmax of
/// the network on its low-voltage side.
pub fn k_t(x_t: f64, c_max: f64) -> f64 {
    0.95 * c_max / (1.0 + 0.6 * x_t)
}

/// Generator factor from network nominal voltage, generator rated voltage,
/// cmax, relative subtransient reactance and rated sin(phi).
pub fn k_g(u_n: f64, u_rg: f64, c_max: f64, x_d: f64, sin_phi: f64) -> f64 {
    (u_n / u_rg) * c_max / (1.0 + x_d * sin_phi)
}

/// Power station unit with an on-load tap changer.
pub fn k_s(u_nq: f64, u_rg: f64, u_rt_lv: f64, u_rt_hv: f64, c_max: f64, x_d: f64, x_t: f64, sin_phi: f64) -> f64 {
    (u_nq * u_nq / (u_rg * u_rg)) * (u_rt_lv * u_rt_lv / (u_rt_hv * u_rt_hv)) * c_max / (1.0 + (x_d - x_t).abs() * sin_phi)
}

/// Power station unit without an on-load tap changer. `p_g` is the
/// generator voltage regulation range and `p_t` the signed per-unit tap
/// offset in use, so the tap term is 1 + p_t.
pub fn k_so(u_nq: f64, u_rg: f64, p_g: f64, u_rt_lv: f64, u_rt_hv: f64, p_t: f64, c_max: f64, x_d: f64, sin_phi: f64) -> f64 {
    (u_nq / (u_rg * (1.0 + p_g))) * (u_rt_lv / u_rt_hv) * (1.0 + p_t) * c_max / (1.0 + x_d * sin_phi)
}

fn reactive_part(xr: f64) -> f64 {
    xr / (1.0 + xr * xr).sqrt()
}

/// Correction factors for `project` (validated) and data-gap warnings.
pub fn corrections(project: &Project) -> (Vec<IecCorrection>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let lv_tol = project.low_voltage_tolerance_plus_10;
    let cmax = |kv: f64| iec_voltage_factor(kv, lv_tol).map(|(_, c)| c).unwrap_or(1.0);
    let mut unit_transformers = Vec::new();
    for source in project.sources.iter().filter(|s| s.in_service) {
        let is_machine = matches!(source.machine_kind, Some(GeneratorKind::Turbogenerator | GeneratorKind::HydroWithAmortisseur | GeneratorKind::HydroWithoutAmortisseur | GeneratorKind::SynchronousCondenser));
        let Some(rating) = &source.generator else {
            if is_machine {
                warnings.push(format!("source {}: generator rating is absent, so the IEC 60909 generator correction factor was not applied to the maximum current", source.id));
            }
            continue;
        };
        let Some(bus) = project.bus(&source.bus) else { continue };
        let u_rg = match rating.rated_kv {
            Some(kv) => kv,
            None => {
                warnings.push(format!("source {}: generator rated_kv is absent; the bus nominal {} kV was used as the rated voltage", source.id, bus.kv));
                bus.kv
            }
        };
        let x_g_ohm = bus.kv * bus.kv / source.mva_sc * reactive_part(source.xr);
        let x_d = x_g_ohm * rating.rated_mva / (u_rg * u_rg);
        let sin_phi = (1.0 - rating.rated_pf * rating.rated_pf).max(0.0).sqrt();
        let unit = rating.unit.as_ref().and_then(|unit| {
            let branch = project.branches.iter().find(|b| b.id == unit.transformer)?;
            let BranchKind::Transformer { z_percent, xr, hv_kv, lv_kv, .. } = &branch.kind else { return None };
            let hv_bus = if branch.from == source.bus { &branch.to } else { &branch.from };
            let u_nq = project.bus(hv_bus)?.kv;
            Some((unit, branch, *z_percent / 100.0 * reactive_part(*xr), *hv_kv, *lv_kv, u_nq))
        });
        match unit {
            Some((unit, branch, x_t, hv, lv, u_nq)) => {
                let c = cmax(u_nq);
                let (name, factor) = if unit.on_load_tap_changer {
                    ("K_S", k_s(u_nq, u_rg, lv, hv, c, x_d, x_t, sin_phi))
                } else {
                    let p_g = unit.p_g.unwrap_or_else(|| {
                        warnings.push(format!("source {}: unit p_g is absent; 0 was used in K_SO", source.id));
                        0.0
                    });
                    let p_t = unit.p_t.unwrap_or_else(|| {
                        warnings.push(format!("source {}: unit p_t is absent; 0 was used in K_SO", source.id));
                        0.0
                    });
                    ("K_SO", k_so(u_nq, u_rg, p_g, lv, hv, p_t, c, x_d, sin_phi))
                };
                unit_transformers.push(branch.id.clone());
                out.push(IecCorrection { element_id: source.id.clone(), factor_name: name.into(), factor });
                out.push(IecCorrection { element_id: branch.id.clone(), factor_name: name.into(), factor });
            }
            None => {
                let factor = k_g(bus.kv, u_rg, cmax(bus.kv), x_d, sin_phi);
                out.push(IecCorrection { element_id: source.id.clone(), factor_name: "K_G".into(), factor });
            }
        }
    }
    for branch in &project.branches {
        if !project.branch_closed(branch) || unit_transformers.contains(&branch.id) { continue; }
        let BranchKind::Transformer { z_percent, xr, .. } = &branch.kind else { continue };
        let (Some(a), Some(b)) = (project.bus(&branch.from), project.bus(&branch.to)) else { continue };
        let lv_kv = a.kv.min(b.kv);
        let factor = k_t(z_percent / 100.0 * reactive_part(*xr), cmax(lv_kv));
        out.push(IecCorrection { element_id: branch.id.clone(), factor_name: "K_T".into(), factor });
    }
    (out, warnings)
}

/// `project` with every correction factor applied to its impedance.
pub fn corrected_project(project: &Project, corrections: &[IecCorrection]) -> Project {
    let mut copy = project.clone();
    for correction in corrections {
        if let Some(source) = copy.sources.iter_mut().find(|s| s.id == correction.element_id) {
            source.mva_sc /= correction.factor;
        } else if let Some(branch) = copy.branches.iter_mut().find(|b| b.id == correction.element_id) {
            if let BranchKind::Transformer { z_percent, .. } = &mut branch.kind { *z_percent *= correction.factor; }
        }
    }
    copy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) { assert!((a - b).abs() < 1e-12, "{a} vs {b}"); }

    #[test]
    fn closed_forms() {
        close(k_t(0.0, 1.05), 0.9975);
        close(k_t(0.1, 1.1), 0.95 * 1.1 / 1.06);
        close(k_g(10.0, 10.5, 1.1, 0.2, 0.6), 10.0 / 10.5 * 1.1 / 1.12);
        close(k_s(110.0, 10.5, 10.5, 115.0, 1.1, 0.2, 0.12, 0.6), (110.0f64 / 10.5).powi(2) * (10.5f64 / 115.0).powi(2) * 1.1 / (1.0 + 0.08 * 0.6));
        // x_d below x_T uses the magnitude of the difference.
        close(k_s(110.0, 10.5, 10.5, 115.0, 1.1, 0.12, 0.2, 0.6), k_s(110.0, 10.5, 10.5, 115.0, 1.1, 0.2, 0.12, 0.6));
        close(k_so(110.0, 10.5, 0.05, 10.5, 115.0, -0.025, 1.1, 0.2, 0.6), 110.0 / (10.5 * 1.05) * (10.5 / 115.0) * 0.975 * 1.1 / 1.12);
    }
}
