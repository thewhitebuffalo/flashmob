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
    ExceedsRating,
    NotAdjusted,
}

impl DutyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WithinRating => "within_rating",
            Self::ExceedsRating => "exceeds_rating",
            Self::NotAdjusted => "not_adjusted",
        }
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
        let status = if half_cycle_rms_ka <= capability * (1.0 + 1e-9) {
            DutyStatus::WithinRating
        } else {
            DutyStatus::ExceedsRating
        };
        Ok(DutyComparison {
            half_cycle_rms_ka,
            rated_half_cycle_rms_ka: Some(capability),
            adjusted_symmetrical_ka: Some(adjusted),
            status,
        })
    } else {
        let status = if input.symmetrical_ka > input.rated_symmetrical_ka * (1.0 + 1e-9) {
            DutyStatus::ExceedsRating
        } else {
            DutyStatus::NotAdjusted
        };
        Ok(DutyComparison {
            half_cycle_rms_ka,
            rated_half_cycle_rms_ka: None,
            adjusted_symmetrical_ka: None,
            status,
        })
    }
}
