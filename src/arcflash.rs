//! IEEE 1584-2018 arc-flash model.
//!
//! Incident energy and arc-flash boundary follow the 2018 equations, including
//! enclosure-size correction, arcing-current variation, and the low-voltage
//! final-current correction. Coefficients are the model coefficients from
//! IEEE 1584-2018. Annex D.1 and D.2 of that guide are the regression tests.

use crate::model::Electrode;

const J_PER_CAL: f64 = 4.184;
const AFB_ENERGY_J: f64 = 1.2 * J_PER_CAL;
const MM_TO_IN_PRINTED: f64 = 0.03937;

#[derive(Clone, Copy, Debug)]
struct K10 {
    k: [f64; 10],
}

#[derive(Clone, Copy, Debug)]
struct K13 {
    k: [f64; 13],
}

#[derive(Clone, Debug)]
pub struct IeeeInput {
    pub voc_kv: f64,
    pub ibf_ka: f64,
    pub gap_mm: f64,
    pub distance_mm: f64,
    pub height_mm: f64,
    pub width_mm: f64,
    pub depth_mm: f64,
    pub electrode: Electrode,
    pub time_s: f64,
    pub time_min_s: f64,
}

#[derive(Clone, Debug)]
pub struct IeeeOutput {
    pub standard: &'static str,
    pub ibf_ka: f64,
    pub i_arc_ka: f64,
    pub i_arc_min_ka: f64,
    pub var_cf: f64,
    pub cf: f64,
    pub enclosure: &'static str,
    pub time_s: f64,
    pub time_min_s: f64,
    pub energy_j_cm2: f64,
    pub energy_min_j_cm2: f64,
    pub energy_cal_cm2: f64,
    pub energy_min_cal_cm2: f64,
    pub governing: &'static str,
    pub governing_j_cm2: f64,
    pub governing_cal_cm2: f64,
    pub afb_mm: f64,
    pub afb_in: f64,
    pub warnings: Vec<String>,
}

pub fn calculate(input: &IeeeInput) -> Result<IeeeOutput, String> {
    let mut warnings = Vec::new();
    if !(0.208..=15.0).contains(&input.voc_kv) {
        return Err(format!(
            "IEEE 1584-2018 covers 0.208 kV to 15 kV; this case is {:.3} kV",
            input.voc_kv
        ));
    }
    if input.ibf_ka <= 0.0 || input.gap_mm <= 0.0 || input.distance_mm <= 0.0 || input.time_s <= 0.0 {
        return Err("bolted current, gap, distance, and arc duration must be positive".into());
    }
    if input.voc_kv <= 0.6 {
        if !(0.5..=106.0).contains(&input.ibf_ka) {
            warnings.push(format!(
                "bolted current {:.3} kA is outside the 0.5–106 kA low-voltage model range",
                input.ibf_ka
            ));
        }
        if !(6.35..=76.2).contains(&input.gap_mm) {
            warnings.push(format!("gap {:.1} mm is outside the 6.35–76.2 mm low-voltage model range", input.gap_mm));
        }
    } else {
        if !(0.2..=65.0).contains(&input.ibf_ka) {
            warnings.push(format!(
                "bolted current {:.3} kA is outside the 0.2–65 kA medium-voltage model range",
                input.ibf_ka
            ));
        }
        if !(19.05..=254.0).contains(&input.gap_mm) {
            warnings.push(format!("gap {:.1} mm is outside the 19.05–254 mm medium-voltage model range", input.gap_mm));
        }
    }
    if input.distance_mm < 305.0 {
        warnings.push("working distance is below 305 mm".into());
    }
    if input.electrode.is_box() && input.width_mm < 4.0 * input.gap_mm {
        warnings.push("enclosure width is less than four times the gap".into());
    }

    let enc = enclosure(input)?;
    let var_cf = variation_factor(input.electrode, input.voc_kv);
    let full = evaluate(input, input.time_s, false, enc.cf, var_cf)?;
    let reduced = evaluate(input, input.time_min_s.max(1e-6), true, enc.cf, var_cf)?;
    let (governing, gov_j, gov_mm) = if reduced.energy_j >= full.energy_j {
        ("reduced_arcing", reduced.energy_j, reduced.afb_mm)
    } else {
        ("arcing", full.energy_j, full.afb_mm)
    };

    Ok(IeeeOutput {
        standard: "IEEE 1584-2018",
        ibf_ka: input.ibf_ka,
        i_arc_ka: full.i_arc,
        i_arc_min_ka: reduced.i_arc,
        var_cf,
        cf: enc.cf,
        enclosure: enc.kind,
        time_s: input.time_s,
        time_min_s: input.time_min_s,
        energy_j_cm2: full.energy_j,
        energy_min_j_cm2: reduced.energy_j,
        energy_cal_cm2: full.energy_j / J_PER_CAL,
        energy_min_cal_cm2: reduced.energy_j / J_PER_CAL,
        governing,
        governing_j_cm2: gov_j,
        governing_cal_cm2: gov_j / J_PER_CAL,
        afb_mm: gov_mm,
        afb_in: gov_mm / 25.4,
        warnings,
    })
}

struct Case {
    i_arc: f64,
    energy_j: f64,
    afb_mm: f64,
}

struct Enclosure {
    cf: f64,
    kind: &'static str,
}

fn evaluate(input: &IeeeInput, time_s: f64, reduced: bool, cf: f64, var_cf: f64) -> Result<Case, String> {
    let t_ms = time_s * 1000.0;
    if input.voc_kv <= 0.6 {
        let i600 = i_arc_intermediate(input.electrode, 0.6, input.ibf_ka, input.gap_mm)?;
        let i_full = i_arc_final_lv(input.voc_kv, i600, input.ibf_ka)?;
        let i_arc = if reduced { i_full * (1.0 - 0.5 * var_cf) } else { i_full };
        let energy = intermediate_energy(input.electrode, input.voc_kv, i_arc, input.ibf_ka, input.gap_mm, input.distance_mm, cf, t_ms, Some(i600))?;
        let afb = afb_from_energy(input.electrode, input.voc_kv, energy, input.distance_mm)?;
        Ok(Case { i_arc, energy_j: energy, afb_mm: afb })
    } else {
        let levels = [0.6, 2.7, 14.3];
        let mut i_levels = [0.0; 3];
        for (n, v) in levels.iter().enumerate() {
            let i = i_arc_intermediate(input.electrode, *v, input.ibf_ka, input.gap_mm)?;
            i_levels[n] = if reduced { i * (1.0 - 0.5 * var_cf) } else { i };
        }
        let i_arc = interpolate(input.voc_kv, i_levels[0], i_levels[1], i_levels[2]);
        let mut e_levels = [0.0; 3];
        let mut b_levels = [0.0; 3];
        for n in 0..3 {
            e_levels[n] = intermediate_energy(
                input.electrode,
                levels[n],
                i_levels[n],
                input.ibf_ka,
                input.gap_mm,
                input.distance_mm,
                cf,
                t_ms,
                None,
            )?;
            b_levels[n] = afb_from_energy(input.electrode, levels[n], e_levels[n], input.distance_mm)?;
        }
        Ok(Case {
            i_arc,
            energy_j: interpolate(input.voc_kv, e_levels[0], e_levels[1], e_levels[2]),
            afb_mm: interpolate(input.voc_kv, b_levels[0], b_levels[1], b_levels[2]),
        })
    }
}

fn i_arc_intermediate(ec: Electrode, voc_kv: f64, ibf: f64, gap: f64) -> Result<f64, String> {
    let k = iarc_coeff(ec, voc_kv);
    let x1 = k.k[0] + k.k[1] * ibf.log10() + k.k[2] * gap.log10();
    let x2 = k.k[3] * ibf.powi(6)
        + k.k[4] * ibf.powi(5)
        + k.k[5] * ibf.powi(4)
        + k.k[6] * ibf.powi(3)
        + k.k[7] * ibf.powi(2)
        + k.k[8] * ibf
        + k.k[9];
    if x2 <= 0.0 {
        return Err("IEEE 1584 arcing-current term is not positive at this bolted current".into());
    }
    Ok(10_f64.powf(x1) * x2)
}

fn i_arc_final_lv(voc: f64, i600: f64, ibf: f64) -> Result<f64, String> {
    let x1 = (0.6 / voc).powi(2);
    let x2 = 1.0 / (i600 * i600);
    let x3 = (0.6 * 0.6 - voc * voc) / (0.6 * 0.6 * ibf * ibf);
    let inside = x1 * (x2 - x3);
    if inside <= 0.0 {
        return Err("IEEE 1584 low-voltage arcing current is not defined for this combination".into());
    }
    Ok(1.0 / inside.sqrt())
}

fn intermediate_energy(
    ec: Electrode,
    voc_kv: f64,
    i_arc: f64,
    ibf: f64,
    gap: f64,
    distance: f64,
    cf: f64,
    t_ms: f64,
    i_arc_600: Option<f64>,
) -> Result<f64, String> {
    if cf <= 0.0 || i_arc <= 0.0 || ibf <= 0.0 {
        return Err("enclosure correction or current is not positive".into());
    }
    let k = energy_coeff(ec, voc_kv);
    let x1 = 12.552 / 50.0 * t_ms;
    let x2 = k.k[0] + k.k[1] * gap.log10();
    let num_i = i_arc_600.unwrap_or(i_arc);
    let den = k.k[3] * ibf.powi(7)
        + k.k[4] * ibf.powi(6)
        + k.k[5] * ibf.powi(5)
        + k.k[6] * ibf.powi(4)
        + k.k[7] * ibf.powi(3)
        + k.k[8] * ibf.powi(2)
        + k.k[9] * ibf;
    if den.abs() < 1e-18 {
        return Err("IEEE 1584 incident-energy denominator is zero".into());
    }
    let x3 = k.k[2] * num_i / den;
    let x4 = k.k[10] * ibf.log10() + k.k[12] * i_arc.log10() + (1.0 / cf).log10();
    let x5 = k.k[11] * distance.log10();
    Ok(x1 * 10_f64.powf(x2 + x3 + x4 + x5))
}

fn afb_from_energy(ec: Electrode, voc_kv: f64, energy_j: f64, distance_mm: f64) -> Result<f64, String> {
    let k12 = energy_coeff(ec, voc_kv).k[11];
    if k12 == 0.0 || energy_j <= 0.0 {
        return Err("arc-flash boundary is not defined".into());
    }
    let f = energy_j / distance_mm.powf(k12);
    Ok((AFB_ENERGY_J / f).powf(1.0 / k12))
}

fn interpolate(voc: f64, x600: f64, x2700: f64, x14300: f64) -> f64 {
    let x1 = ((x2700 - x600) / 2.1) * (voc - 2.7) + x2700;
    let x2 = ((x14300 - x2700) / 11.6) * (voc - 14.3) + x14300;
    let x3 = (x1 * (2.7 - voc) / 2.1) + (x2 * (voc - 0.6) / 2.1);
    if voc > 2.7 { x2 } else { x3 }
}

fn variation_factor(ec: Electrode, voc_kv: f64) -> f64 {
    let k = match ec {
        Electrode::VCB => [0.0, -0.0000014269, 0.000083137, -0.0019382, 0.022366, -0.12645, 0.30226],
        Electrode::VCBB => [1.138e-6, -6.0287e-5, 0.0012758, -0.013778, 0.080217, -0.24066, 0.33524],
        Electrode::HCB => [0.0, -3.097e-6, 0.00016405, -0.0033609, 0.033308, -0.16182, 0.34627],
        Electrode::VOA => [9.5606e-7, -5.1543e-5, 0.0011161, -0.01242, 0.075125, -0.23584, 0.33696],
        Electrode::HOA => [0.0, -3.1555e-6, 0.0001682, -0.0034607, 0.034124, -0.1599, 0.34629],
    };
    k[0] * voc_kv.powi(6) + k[1] * voc_kv.powi(5) + k[2] * voc_kv.powi(4) + k[3] * voc_kv.powi(3) + k[4] * voc_kv.powi(2)
        + k[5] * voc_kv
        + k[6]
}

fn enclosure(input: &IeeeInput) -> Result<Enclosure, String> {
    if !input.electrode.is_box() {
        return Ok(Enclosure { cf: 1.0, kind: "open_air" });
    }
    let shallow = input.voc_kv < 0.6 && input.height_mm < 508.0 && input.width_mm < 508.0 && input.depth_mm <= 203.2;
    let kind = if shallow { "shallow" } else { "typical" };
    let (a, b) = match input.electrode {
        Electrode::VCB => (4.0, 20.0),
        Electrode::VCBB => (10.0, 24.0),
        Electrode::HCB => (10.0, 22.0),
        _ => unreachable!(),
    };
    let width_1 = equivalent_dim(input.width_mm, input.voc_kv, a, b, shallow, false, input.electrode);
    let height_1 = equivalent_dim(input.height_mm, input.voc_kv, a, b, shallow, true, input.electrode);
    let ees = (width_1 + height_1) / 2.0;
    let (b1, b2, b3) = cf_coeff(shallow, input.electrode);
    let poly = b1 * ees * ees + b2 * ees + b3;
    if poly <= 0.0 {
        return Err("enclosure correction factor is not positive".into());
    }
    let cf = if shallow { 1.0 / poly } else { poly };
    Ok(Enclosure { cf, kind })
}

fn equivalent_dim(dim: f64, voc: f64, a: f64, b: f64, shallow: bool, height: bool, ec: Electrode) -> f64 {
    if dim < 508.0 {
        return if shallow { MM_TO_IN_PRINTED * dim } else { 20.0 };
    }
    if dim <= 660.4 {
        return MM_TO_IN_PRINTED * dim;
    }
    if dim <= 1244.6 {
        if height && ec == Electrode::VCB {
            return MM_TO_IN_PRINTED * dim;
        }
        return eq_11_12(dim, voc, a, b);
    }
    if height && ec == Electrode::VCB {
        49.0
    } else {
        eq_11_12(1244.6, voc, a, b)
    }
}

fn eq_11_12(dim_mm: f64, voc: f64, a: f64, b: f64) -> f64 {
    let y1 = dim_mm - 660.4;
    let y2 = (voc + a) / b;
    (660.4 + y1 * y2) / 25.4
}

fn cf_coeff(shallow: bool, ec: Electrode) -> (f64, f64, f64) {
    match (shallow, ec) {
        (false, Electrode::VCB) => (-0.000302, 0.03441, 0.4325),
        (false, Electrode::VCBB) => (-0.0002976, 0.032, 0.479),
        (false, Electrode::HCB) => (-0.0001923, 0.01935, 0.6899),
        (true, Electrode::VCB) => (0.002222, -0.02556, 0.6222),
        (true, Electrode::VCBB) => (-0.002778, 0.1194, -0.2778),
        (true, Electrode::HCB) => (-0.0005556, 0.03722, 0.4778),
        _ => (0.0, 0.0, 1.0),
    }
}

fn iarc_coeff(ec: Electrode, voc: f64) -> K10 {
    let row: [f64; 10] = match (ec, voc_band(voc)) {
        (Electrode::VCB, 0) => [-0.04287, 1.035, -0.083, 0.0, 0.0, -4.783e-9, 1.962e-6, -0.000229, 0.003141, 1.092],
        (Electrode::VCB, 1) => [0.0065, 1.001, -0.024, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729],
        (Electrode::VCB, _) => [0.005795, 1.015, -0.011, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729],
        (Electrode::VCBB, 0) => [-0.017432, 0.98, -0.05, 0.0, 0.0, -5.767e-9, 2.524e-6, -0.00034, 0.01187, 1.013],
        (Electrode::VCBB, 1) => [0.002823, 0.995, -0.0125, 0.0, -9.204e-11, 2.901e-8, -3.262e-6, 0.0001569, -0.004003, 0.9825],
        (Electrode::VCBB, _) => [0.014827, 1.01, -0.01, 0.0, -9.204e-11, 2.901e-8, -3.262e-6, 0.0001569, -0.004003, 0.9825],
        (Electrode::HCB, 0) => [0.054922, 0.988, -0.11, 0.0, 0.0, -5.382e-9, 2.316e-6, -0.000302, 0.0091, 0.9725],
        (Electrode::HCB, 1) => [0.001011, 1.003, -0.0249, 0.0, 0.0, 4.859e-10, -1.814e-7, -9.128e-6, -0.0007, 0.9881],
        (Electrode::HCB, _) => [0.008693, 0.999, -0.02, 0.0, -5.043e-11, 2.233e-8, -3.046e-6, 0.000116, -0.001145, 0.9839],
        (Electrode::VOA, 0) => [0.043785, 1.04, -0.18, 0.0, 0.0, -4.783e-9, 1.962e-6, -0.000229, 0.003141, 1.092],
        (Electrode::VOA, 1) => [-0.02395, 1.006, -0.0188, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729],
        (Electrode::VOA, _) => [0.005371, 1.0102, -0.029, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729],
        (Electrode::HOA, 0) => [0.111147, 1.008, -0.24, 0.0, 0.0, -3.895e-9, 1.641e-6, -0.000197, 0.002615, 1.1],
        (Electrode::HOA, 1) => [0.000435, 1.006, -0.038, 0.0, 0.0, 7.859e-10, -1.914e-7, -9.128e-6, -0.0007, 0.9981],
        (Electrode::HOA, _) => [0.000904, 0.999, -0.02, 0.0, 0.0, 7.859e-10, -1.914e-7, -9.128e-6, -0.0007, 0.9981],
    };
    K10 { k: row }
}

fn energy_coeff(ec: Electrode, voc: f64) -> K13 {
    let row: [f64; 13] = match (ec, if voc <= 0.6 { 0 } else { voc_band(voc) }) {
        (Electrode::VCB, 0) => [0.753364, 0.566, 1.752636, 0.0, 0.0, -4.783e-9, 0.000001962, -0.000229, 0.003141, 1.092, 0.0, -1.598, 0.957],
        (Electrode::VCBB, 0) => [3.068459, 0.26, -0.098107, 0.0, 0.0, -5.767e-9, 0.000002524, -0.00034, 0.01187, 1.013, -0.06, -1.809, 1.19],
        (Electrode::HCB, 0) => [4.073745, 0.344, -0.370259, 0.0, 0.0, -5.382e-9, 0.000002316, -0.000302, 0.0091, 0.9725, 0.0, -2.03, 1.036],
        (Electrode::VOA, 0) => [0.679294, 0.746, 1.222636, 0.0, 0.0, -4.783e-9, 0.000001962, -0.000229, 0.003141, 1.092, 0.0, -1.598, 0.997],
        (Electrode::HOA, 0) => [3.470417, 0.465, -0.261863, 0.0, 0.0, -3.895e-9, 0.000001641, -0.000197, 0.002615, 1.1, 0.0, -1.99, 1.04],
        (Electrode::VCB, 1) => [2.40021, 0.165, 0.354202, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729, 0.0, -1.569, 0.9778],
        (Electrode::VCBB, 1) => [3.870592, 0.185, -0.736618, 0.0, -9.204e-11, 2.901e-8, -3.262e-6, 0.0001569, -0.004003, 0.9825, 0.0, -1.742, 1.09],
        (Electrode::HCB, 1) => [3.486391, 0.177, -0.193101, 0.0, 0.0, 4.859e-10, -1.814e-7, -9.128e-6, -0.0007, 0.9881, 0.027, -1.723, 1.055],
        (Electrode::VOA, 1) => [3.880724, 0.105, -1.906033, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729, 0.0, -1.515, 1.115],
        (Electrode::HOA, 1) => [3.616266, 0.149, -0.761561, 0.0, 0.0, 7.859e-10, -1.914e-7, -9.128e-6, -0.0007, 0.9981, 0.0, -1.639, 1.078],
        (Electrode::VCB, _) => [3.825917, 0.11, -0.999749, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729, 0.0, -1.568, 0.99],
        (Electrode::VCBB, _) => [3.644309, 0.215, -0.585522, 0.0, -9.204e-11, 2.901e-8, -3.262e-6, 0.0001569, -0.004003, 0.9825, 0.0, -1.677, 1.06],
        (Electrode::HCB, _) => [3.044516, 0.125, 0.245106, 0.0, -5.043e-11, 2.233e-8, -3.046e-6, 0.000116, -0.001145, 0.9839, 0.0, -1.655, 1.084],
        (Electrode::VOA, _) => [3.405454, 0.12, -0.93245, -1.557e-12, 4.556e-10, -4.186e-8, 8.346e-7, 5.482e-5, -0.003191, 0.9729, 0.0, -1.534, 0.979],
        (Electrode::HOA, _) => [2.04049, 0.177, 1.005092, 0.0, 0.0, 7.859e-10, -1.914e-7, -9.128e-6, -0.0007, 0.9981, -0.05, -1.633, 1.151],
    };
    K13 { k: row }
}

fn voc_band(voc: f64) -> usize {
    if (voc - 0.6).abs() < 1e-9 {
        0
    } else if (voc - 2.7).abs() < 1e-9 {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: f64, expect: f64, tol: f64) {
        assert!((got - expect).abs() <= tol, "got {got}, expected {expect}");
    }

    #[test]
    fn annex_d1_medium_voltage() {
        let full = calculate(&IeeeInput {
            voc_kv: 4.16,
            ibf_ka: 15.0,
            gap_mm: 104.0,
            distance_mm: 914.4,
            height_mm: 1143.0,
            width_mm: 762.0,
            depth_mm: 508.0,
            electrode: Electrode::VCB,
            time_s: 0.197,
            time_min_s: 0.223,
        })
        .unwrap();
        close(full.i_arc_ka, 12.979, 0.0015);
        close(full.cf, 1.284, 0.0015);
        close(full.var_cf, 0.047, 0.0015);
        close(full.energy_j_cm2, 12.152, 0.02);
        close(full.i_arc_min_ka, 12.675, 0.0015);
        close(full.energy_min_j_cm2, 13.343, 0.02);
        close(full.afb_mm, 1704.0, 2.0);
        assert_eq!(full.governing, "reduced_arcing");
    }

    #[test]
    fn annex_d2_low_voltage() {
        let out = calculate(&IeeeInput {
            voc_kv: 0.48,
            ibf_ka: 45.0,
            gap_mm: 32.0,
            distance_mm: 609.6,
            height_mm: 610.0,
            width_mm: 610.0,
            depth_mm: 254.0,
            electrode: Electrode::VCB,
            time_s: 0.0613,
            time_min_s: 0.319,
        })
        .unwrap();
        close(out.i_arc_ka, 28.793, 0.002);
        close(out.cf, 1.085, 0.0015);
        close(out.var_cf, 0.247, 0.0015);
        close(out.energy_j_cm2, 11.585, 0.02);
        close(out.i_arc_min_ka, 25.244, 0.002);
        close(out.energy_min_j_cm2, 53.156, 0.05);
        close(out.afb_mm, 2669.0, 2.0);
    }
}
