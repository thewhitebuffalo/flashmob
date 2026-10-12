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
}
