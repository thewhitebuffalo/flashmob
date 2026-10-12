//! Interrupting duty compared with a symmetrical rating and its test X/R.
//! The half-cycle factor is `fault::half_cycle_factor`, the same one the
//! fault study uses for first-half-cycle asymmetrical rms.

use crate::fault::half_cycle_factor;

#[derive(Clone, Copy, Debug)]
pub struct DutyInput {
    pub symmetrical_ka: f64,
    /// None means infinite X/R, matching `FaultPoint.x_over_r`.
    pub system_xr: Option<f64>,
    pub rated_symmetrical_ka: f64,
    /// None means bracing with no test X/R.
    pub test_xr: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
pub struct DutyComparison {
    pub half_cycle_rms_ka: f64,
    pub rated_half_cycle_rms_ka: Option<f64>,
    pub adjusted_symmetrical_ka: Option<f64>,
    pub status: DutyStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DutyStatus {
    WithinRating,
    /// Above 90% of the compared rating and still within it.
    Marginal,
    ExceedsRating,
    NotAdjusted,
}

impl DutyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WithinRating => "within_rating",
            Self::Marginal => "marginal",
            Self::ExceedsRating => "exceeds_rating",
            Self::NotAdjusted => "not_adjusted",
        }
    }
}

/// Share of the compared rating at which duty becomes marginal.
const MARGINAL_FRACTION: f64 = 0.90;

fn band(duty: f64, rating: f64, below: DutyStatus) -> DutyStatus {
    let tol = 1.0 + 1e-9;
    if duty > rating * tol {
        DutyStatus::ExceedsRating
    } else if duty > rating * MARGINAL_FRACTION * tol {
        DutyStatus::Marginal
    } else {
        below
    }
}

/// Breaker asymmetrical-to-symmetrical capability at a contact parting time.
///
/// The stated ratios are 1.4, 1.2, 1.1, and 1.0 at 1, 2, 3, and 4 cycles.
/// Times between those points are linear. A 1.5-cycle parting time, the
/// minimum for a 2-cycle breaker, falls on 1.3. At and beyond 4 cycles the
/// ratio stays 1. Parting faster than 1 cycle is not this interrupting check.
pub fn asymmetry_capability(contact_parting_cycles: f64) -> Result<f64, String> {
    if !contact_parting_cycles.is_finite() || contact_parting_cycles < 1.0 {
        return Err("contact parting time must be at least 1 cycle".into());
    }
    const KNOTS: [(f64, f64); 4] = [(1.0, 1.4), (2.0, 1.2), (3.0, 1.1), (4.0, 1.0)];
    if contact_parting_cycles >= 4.0 {
        return Ok(1.0);
    }
    for pair in KNOTS.windows(2) {
        let [(t0, s0), (t1, s1)] = [pair[0], pair[1]];
        if contact_parting_cycles <= t1 {
            let fraction = (contact_parting_cycles - t0) / (t1 - t0);
            return Ok(s0 + fraction * (s1 - s0));
        }
    }
    Ok(1.0)
}

/// Remote E/X multiplier: DC decrement only, divided by the breaker capability.
///
/// The asymmetrical rms factor at the contact parting time is divided by
/// `asymmetry_capability`. The result is not taken below 1, so a low X/R does
/// not credit the breaker with more than its symmetrical rating. This is the
/// remote factor. It is not the local factor that also includes AC decrement,
/// and it is not reduced by a no-AC-decrement ratio.
pub fn remote_multiplying_factor(system_xr: Option<f64>, contact_parting_cycles: f64) -> Result<f64, String> {
    let asymmetrical = crate::fault::asymmetrical_rms_factor(system_xr, contact_parting_cycles)?;
    let capability = asymmetry_capability(contact_parting_cycles)?;
    let factor = asymmetrical / capability;
    if !factor.is_finite() {
        return Err("non-finite remote multiplying factor".into());
    }
    Ok(factor.max(1.0))
}

/// Local E/X multiplier from an entered symmetrical-current ratio.
///
/// `ac_ratio` is the symmetrical current remaining divided by the initial
/// symmetrical current. The rms ratio is
/// `sqrt(ac_ratio^2 + 2*exp(-4*pi*cycles/(X/R)))`. Infinite X/R uses
/// `sqrt(ac_ratio^2 + 2)`. A purely resistive circuit has no DC term, so the
/// rms ratio is `ac_ratio`. The result is that rms ratio divided by
/// `asymmetry_capability`, and not taken below 1. An `ac_ratio` of 1 matches
/// `remote_multiplying_factor`.
pub fn local_multiplying_factor(system_xr: Option<f64>, contact_parting_cycles: f64, ac_ratio: f64) -> Result<f64, String> {
    if !ac_ratio.is_finite() || ac_ratio <= 0.0 {
        return Err("local AC ratio must be finite and positive".into());
    }
    let capability = asymmetry_capability(contact_parting_cycles)?;
    let dc = match system_xr {
        None => 2.0,
        Some(xr) if xr == 0.0 => 0.0,
        Some(xr) => {
            if !xr.is_finite() || xr < 0.0 {
                return Err("X/R must be finite and nonnegative".into());
            }
            2.0 * (-4.0 * std::f64::consts::PI * contact_parting_cycles / xr).exp()
        }
    };
    let rms = (ac_ratio * ac_ratio + dc).sqrt();
    let factor = rms / capability;
    if !rms.is_finite() || !factor.is_finite() {
        return Err("non-finite local multiplying factor".into());
    }
    Ok(factor.max(1.0))
}

/// `nacd * remote + (1 - nacd) * local`, with both factors kept at or above 1.
pub fn nacd_interpolated_factor(remote: f64, local: f64, nacd: f64) -> Result<f64, String> {
    if !remote.is_finite() || !local.is_finite() || !nacd.is_finite() || !(0.0..=1.0).contains(&nacd) {
        return Err("NACD interpolation needs finite factors and a NACD from 0 to 1".into());
    }
    let factor = nacd * remote.max(1.0) + (1.0 - nacd) * local.max(1.0);
    if !factor.is_finite() {
        return Err("non-finite interpolated multiplying factor".into());
    }
    Ok(factor)
}

/// Symmetrical duty against a symmetrical rating. The bottom band is within rating.
pub fn rated_symmetrical_status(duty_ka: f64, rated_ka: f64) -> Result<DutyStatus, String> {
    if !duty_ka.is_finite() || duty_ka < 0.0 || !rated_ka.is_finite() || rated_ka < 0.0 {
        return Err("remote duty and rating must be finite and nonnegative".into());
    }
    Ok(band(duty_ka, rated_ka, DutyStatus::WithinRating))
}

/// Compare a symmetrical duty current with a rating.
///
/// With a test X/R, capability is the rated symmetrical current times the
/// half-cycle factor at that test X/R. The adjusted symmetrical rating is
/// `rated * factor(test) / factor(system)`. Without a test X/R the comparison
/// is symmetrical only and is never reported as within rating.
pub fn compare(input: &DutyInput) -> Result<DutyComparison, String> {
    if !input.symmetrical_ka.is_finite() || input.symmetrical_ka < 0.0 {
        return Err("symmetrical current must be finite and nonnegative".into());
    }
    if !input.rated_symmetrical_ka.is_finite() || input.rated_symmetrical_ka < 0.0 {
        return Err("rated symmetrical current must be finite and nonnegative".into());
    }
    let system_factor = half_cycle_factor(input.system_xr)?;
    if system_factor <= 0.0 {
        return Err("half-cycle factor must be positive".into());
    }
    let half_cycle_rms_ka = input.symmetrical_ka * system_factor;
    if !half_cycle_rms_ka.is_finite() {
        return Err("non-finite half-cycle current".into());
    }
    if let Some(test_xr) = input.test_xr {
        let test_factor = half_cycle_factor(Some(test_xr))?;
        let capability = input.rated_symmetrical_ka * test_factor;
        let adjusted = capability / system_factor;
        if !capability.is_finite() || !adjusted.is_finite() {
            return Err("non-finite adjusted rating".into());
        }
        let status = band(half_cycle_rms_ka, capability, DutyStatus::WithinRating);
        Ok(DutyComparison {
            half_cycle_rms_ka,
            rated_half_cycle_rms_ka: Some(capability),
            adjusted_symmetrical_ka: Some(adjusted),
            status,
        })
    } else {
        let status = band(input.symmetrical_ka, input.rated_symmetrical_ka, DutyStatus::NotAdjusted);
        Ok(DutyComparison {
            half_cycle_rms_ka,
            rated_half_cycle_rms_ka: None,
            adjusted_symmetrical_ka: None,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(symmetrical_ka: f64, test_xr: Option<f64>) -> DutyInput {
        DutyInput { symmetrical_ka, system_xr: Some(5.0), rated_symmetrical_ka: 10.0, test_xr }
    }

    #[test]
    fn ninety_percent_band_is_marginal_when_the_test_x_over_r_matches() {
        // Equal X/R makes the half-cycle ratio equal the symmetrical ratio.
        assert_eq!(compare(&input(8.9, Some(5.0))).unwrap().status, DutyStatus::WithinRating);
        assert_eq!(compare(&input(9.0, Some(5.0))).unwrap().status, DutyStatus::WithinRating);
        assert_eq!(compare(&input(9.5, Some(5.0))).unwrap().status, DutyStatus::Marginal);
        assert_eq!(compare(&input(10.0, Some(5.0))).unwrap().status, DutyStatus::Marginal);
        assert_eq!(compare(&input(10.1, Some(5.0))).unwrap().status, DutyStatus::ExceedsRating);
    }

    #[test]
    fn symmetrical_only_comparison_stays_not_adjusted_below_ninety_percent() {
        assert_eq!(compare(&input(8.9, None)).unwrap().status, DutyStatus::NotAdjusted);
        assert_eq!(compare(&input(9.5, None)).unwrap().status, DutyStatus::Marginal);
        assert_eq!(compare(&input(10.1, None)).unwrap().status, DutyStatus::ExceedsRating);
    }

    #[test]
    fn remote_factor_at_eighty_and_three_cycles_is_the_dc_only_ratio() {
        // At X/R 80 and 3 cycles the DC-only ratio is 1.5. Dividing by the
        // 1.1 capability gives the remote multiplier. The local-curve example
        // of 1.21 already includes AC decrement and is a different number.
        let cycles = 3.0;
        let asymmetrical = (1.0 + 2.0 * (-4.0 * std::f64::consts::PI * cycles / 80.0).exp()).sqrt();
        assert!((asymmetrical - 1.5).abs() < 1e-3, "{asymmetrical}");
        let remote = remote_multiplying_factor(Some(80.0), cycles).unwrap();
        assert!((remote - asymmetrical / 1.1).abs() < 1e-12);
        assert!((remote - 1.21).abs() > 0.1, "local-curve example must stay distinct, got {remote}");
        assert!((asymmetry_capability(1.5).unwrap() - 1.3).abs() < 1e-12);
        assert!((asymmetry_capability(4.0).unwrap() - 1.0).abs() < 1e-12);
        assert!((asymmetry_capability(5.0).unwrap() - 1.0).abs() < 1e-12);
        assert!(asymmetry_capability(0.5).is_err());
    }

    #[test]
    fn local_factor_uses_the_entered_ac_ratio_and_blends_by_nacd() {
        let cycles = 3.0;
        let ac = 0.70;
        let dc = 2.0 * (-4.0 * std::f64::consts::PI * cycles / 80.0).exp();
        let rms = (ac * ac + dc).sqrt();
        let local = local_multiplying_factor(Some(80.0), cycles, ac).unwrap();
        assert!((local - rms / 1.1).abs() < 1e-12, "{local}");
        // The closed form is about 1.20. A rounded reading of the same point is 1.21.
        assert!((local - 1.20).abs() < 0.01, "{local}");
        assert!((local - 1.21).abs() > 0.005, "{local}");
        let remote = remote_multiplying_factor(Some(80.0), cycles).unwrap();
        assert!((local_multiplying_factor(Some(80.0), cycles, 1.0).unwrap() - remote).abs() < 1e-12);
        assert!((nacd_interpolated_factor(remote, local, 1.0).unwrap() - remote).abs() < 1e-12);
        assert!((nacd_interpolated_factor(remote, local, 0.0).unwrap() - local).abs() < 1e-12);
        let mid = nacd_interpolated_factor(remote, local, 0.5).unwrap();
        assert!((mid - 0.5 * (remote + local)).abs() < 1e-12, "{mid}");
        let resistive = local_multiplying_factor(Some(0.0), cycles, ac).unwrap();
        assert!((resistive - 1.0).abs() < 1e-12, "{resistive}");
        let infinite = local_multiplying_factor(None, cycles, ac).unwrap();
        assert!((infinite - (ac * ac + 2.0).sqrt() / 1.1).abs() < 1e-12, "{infinite}");
        assert!(local_multiplying_factor(Some(80.0), cycles, 0.0).is_err());
        assert!(nacd_interpolated_factor(remote, local, 1.1).is_err());
    }

    #[test]
    fn remote_factor_stays_at_one_when_the_x_over_r_is_fifteen_at_three_cycles() {
        let factor = remote_multiplying_factor(Some(15.0), 3.0).unwrap();
        assert!((factor - 1.0).abs() < 1e-12, "{factor}");
        assert_eq!(rated_symmetrical_status(9.0, 10.0).unwrap(), DutyStatus::WithinRating);
        assert_eq!(rated_symmetrical_status(9.5, 10.0).unwrap(), DutyStatus::Marginal);
        assert_eq!(rated_symmetrical_status(10.1, 10.0).unwrap(), DutyStatus::ExceedsRating);
    }
}
