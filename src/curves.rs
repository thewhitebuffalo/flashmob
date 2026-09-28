use crate::model::{CurveSpec, Device, IecKind, IeeeKind};

/// Relay/element operating time, not breaker total clearing. None means unavailable or no operation.
pub fn trip_time(curve: &CurveSpec, amps: f64) -> Option<f64> {
    if !amps.is_finite() || amps <= 0.0 {
        return None;
    }
    let incomplete_active_stage = match curve {
        CurveSpec::Iec { inst_a, inst_s, .. } | CurveSpec::Ieee { inst_a, inst_s, .. } =>
            inst_a.is_some_and(|p| amps >= p) && inst_s.is_none(),
        CurveSpec::ThermalMagnetic { inst_a, inst_s, st_pickup_a, st_delay_s, .. } =>
            (inst_a.is_some_and(|p| amps >= p) && inst_s.is_none()) ||
            (st_pickup_a.is_some_and(|p| amps >= p) && st_delay_s.is_none()),
        _ => false,
    };
    if incomplete_active_stage { return None; }
    match curve {
        CurveSpec::Iec { kind, pickup_a, tms, inst_a, inst_s } => {
            let thermal = if amps > *pickup_a && *pickup_a > 0.0 {
                let (k, alpha) = iec_constants(*kind);
                let m = amps / pickup_a;
                Some(tms * k / (m.powf(alpha) - 1.0))
            } else {
                None
            };
            fastest(thermal, instant(amps, *inst_a, *inst_s))
        }
        CurveSpec::Ieee { kind, pickup_a, td, inst_a, inst_s } => {
            let thermal = if amps > *pickup_a && *pickup_a > 0.0 {
                let (a, b, p) = ieee_constants(*kind);
                let m = amps / pickup_a;
                Some(td * (a / (m.powf(p) - 1.0) + b))
            } else {
                None
            };
            fastest(thermal, instant(amps, *inst_a, *inst_s))
        }
        CurveSpec::Definite { pickup_a, time_s } => {
            if amps >= *pickup_a {
                Some(*time_s)
            } else {
                None
            }
        }
        CurveSpec::ThermalMagnetic {
            lt_pickup_a,
            lt_delay_s,
            st_pickup_a,
            st_delay_s,
            inst_a,
            inst_s,
        } => {
            let mut times = Vec::new();
            if amps > *lt_pickup_a && *lt_pickup_a > 0.0 {
                let m = amps / lt_pickup_a;
                times.push(lt_delay_s * (6.0 / m).powi(2));
            }
            if let (Some(pickup), Some(delay)) = (st_pickup_a, st_delay_s) {
                if amps >= *pickup {
                    times.push(*delay);
                }
            }
            if let Some(t) = instant(amps, *inst_a, *inst_s) {
                times.push(t);
            }
            times.into_iter().filter(|t| t.is_finite() && *t > 0.0).reduce(f64::min)
        }
        CurveSpec::SettingsNotCollected { .. } => None,
    }
}

pub fn trip_time_device(device: &Device, amps: f64) -> Option<f64> {
    trip_time(&device.curve, amps)
}

/// Total clearing requires either an explicit fuse total-clearing curve or breaker time.
pub fn clearing_time(device: &Device, amps: f64) -> Option<f64> {
    let operating = trip_time_device(device, amps).filter(|t| t.is_finite() && *t > 0.0)?;
    if device.fuse_total_clearing && device.breaker_interrupting_s.is_some() { return None; }
    if device.fuse_total_clearing { Some(operating) }
    else { device.breaker_interrupting_s.filter(|t| t.is_finite() && *t > 0.0).map(|t| operating + t).filter(|t| t.is_finite()) }
}

/// Tracks the separately timed trip elements as fault current changes.
///
/// Each element accumulates `dt / trip_time(current)` while above its pickup.
/// An element resets immediately below pickup. Real relay reset behavior can
/// differ; this conservative prototype assumption must be replaced with the
/// entered relay's reset characteristic when that information is available.
#[derive(Clone, Debug)]
pub struct DynamicRelay {
    curve: CurveSpec,
    progress: [f64; 3],
    operated: bool,
    reset_occurred: bool,
}

impl DynamicRelay {
    pub fn new(curve: &CurveSpec) -> Result<Self, String> {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        let optional_stage = |pickup: Option<f64>, delay: Option<f64>, name: &str| {
            if delay.is_some() && pickup.is_none() {
                return Err(format!("{name} delay requires a pickup"));
            }
            if pickup.is_some_and(|value| !positive(value))
                || delay.is_some_and(|value| !positive(value))
            {
                return Err(format!("{name} pickup and delay must be positive and finite"));
            }
            Ok(())
        };
        match curve {
            CurveSpec::Iec { pickup_a, tms, inst_a, inst_s, .. } => {
                if !positive(*pickup_a) || !positive(*tms) {
                    return Err("IEC pickup and time multiplier must be positive and finite".into());
                }
                optional_stage(*inst_a, *inst_s, "instantaneous")?;
            }
            CurveSpec::Ieee { pickup_a, td, inst_a, inst_s, .. } => {
                if !positive(*pickup_a) || !positive(*td) {
                    return Err("IEEE pickup and time dial must be positive and finite".into());
                }
                optional_stage(*inst_a, *inst_s, "instantaneous")?;
            }
            CurveSpec::Definite { pickup_a, time_s } => {
                if !positive(*pickup_a) || !positive(*time_s) {
                    return Err("definite pickup and delay must be positive and finite".into());
                }
            }
            CurveSpec::ThermalMagnetic {
                lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s,
            } => {
                if !positive(*lt_pickup_a) || !positive(*lt_delay_s) {
                    return Err("long-time pickup and delay must be positive and finite".into());
                }
                optional_stage(*st_pickup_a, *st_delay_s, "short-time")?;
                optional_stage(*inst_a, *inst_s, "instantaneous")?;
            }
            CurveSpec::SettingsNotCollected { .. } => {
                return Err("trip settings not collected".into());
            }
        }
        Ok(Self { curve: curve.clone(), progress: [0.0; 3], operated: false, reset_occurred: false })
    }

    /// Whether a timed element with accumulated operation reset below pickup.
    pub fn reset_occurred(&self) -> bool { self.reset_occurred }

    /// Entered current pickups, including optional short-time and instantaneous elements.
    /// Callers can split sampled-current intervals at these thresholds before advancing.
    pub fn pickup_thresholds(&self) -> Vec<f64> {
        let mut pickups = match &self.curve {
            CurveSpec::Iec { pickup_a, inst_a, .. }
            | CurveSpec::Ieee { pickup_a, inst_a, .. } => {
                let mut pickups = vec![*pickup_a];
                if let Some(pickup) = inst_a { pickups.push(*pickup); }
                pickups
            }
            CurveSpec::Definite { pickup_a, .. } => vec![*pickup_a],
            CurveSpec::ThermalMagnetic { lt_pickup_a, st_pickup_a, inst_a, .. } => {
                let mut pickups = vec![*lt_pickup_a];
                if let Some(pickup) = st_pickup_a { pickups.push(*pickup); }
                if let Some(pickup) = inst_a { pickups.push(*pickup); }
                pickups
            }
            CurveSpec::SettingsNotCollected { .. } => unreachable!("rejected by new"),
        };
        pickups.retain(|pickup| pickup.is_finite());
        pickups.sort_by(f64::total_cmp);
        pickups.dedup();
        pickups
    }

    /// Advance one constant-current slice. Returns seconds from the start of
    /// this slice to the earliest element operation, if one operates.
    pub fn advance(&mut self, amps: f64, dt_s: f64) -> Result<Option<f64>, String> {
        if self.operated { return Ok(Some(0.0)); }
        if !amps.is_finite() || amps < 0.0 || !dt_s.is_finite() || dt_s <= 0.0 {
            return Err("sampled current must be finite and nonnegative, and slice duration must be positive and finite".into());
        }
        let times = match &self.curve {
            CurveSpec::Iec { kind, pickup_a, tms, inst_a, inst_s } => {
                let inverse = if amps > *pickup_a {
                    let (k, alpha) = iec_constants(*kind);
                    let multiple = amps / pickup_a;
                    Some(tms * k / (multiple.powf(alpha) - 1.0))
                } else { None };
                [inverse, None, active_stage_delay(amps, *inst_a, *inst_s, "instantaneous")?]
            }
            CurveSpec::Ieee { kind, pickup_a, td, inst_a, inst_s } => {
                let inverse = if amps > *pickup_a {
                    let (a, b, p) = ieee_constants(*kind);
                    let multiple = amps / pickup_a;
                    Some(td * (a / (multiple.powf(p) - 1.0) + b))
                } else { None };
                [inverse, None, active_stage_delay(amps, *inst_a, *inst_s, "instantaneous")?]
            }
            CurveSpec::Definite { pickup_a, time_s } => {
                [(*pickup_a <= amps).then_some(*time_s), None, None]
            }
            CurveSpec::ThermalMagnetic {
                lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s,
            } => {
                let long = if amps > *lt_pickup_a {
                    let multiple = amps / lt_pickup_a;
                    Some(lt_delay_s * (6.0 / multiple).powi(2))
                } else { None };
                let short = active_stage_delay(amps, *st_pickup_a, *st_delay_s, "short-time")?;
                let immediate = active_stage_delay(amps, *inst_a, *inst_s, "instantaneous")?;
                [long, short, immediate]
            }
            CurveSpec::SettingsNotCollected { .. } => unreachable!("rejected by new"),
        };
        let mut earliest: Option<f64> = None;
        for (progress, time) in self.progress.iter_mut().zip(times) {
            match time {
                None => {
                    self.reset_occurred |= *progress > 0.0;
                    *progress = 0.0;
                }
                Some(time) if time.is_infinite() => {},
                Some(time) if time > 0.0 => {
                    let remaining = (1.0 - *progress).max(0.0) * time;
                    if remaining <= dt_s {
                        earliest = Some(earliest.map_or(remaining, |prior| prior.min(remaining)));
                    } else {
                        *progress += dt_s / time;
                    }
                }
                Some(_) => return Err("calculated relay operating time is invalid at the sampled current".into()),
            }
        }
        self.operated = earliest.is_some();
        Ok(earliest)
    }
}

fn active_stage_delay(amps: f64, pickup: Option<f64>, delay: Option<f64>, name: &str)
    -> Result<Option<f64>, String> {
    match pickup {
        Some(pickup) if amps >= pickup => delay.map(Some)
            .ok_or_else(|| format!("{name} pickup was reached but its delay is not entered")),
        _ => Ok(None),
    }
}

fn instant(amps: f64, pickup: Option<f64>, time: Option<f64>) -> Option<f64> {
    let pickup = pickup?;
    if amps >= pickup {
        time.filter(|t| t.is_finite() && *t > 0.0)
    } else {
        None
    }
}

fn fastest(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn iec_constants(kind: IecKind) -> (f64, f64) {
    match kind {
        IecKind::Si => (0.14, 0.02),
        IecKind::Vi => (13.5, 1.0),
        IecKind::Ei => (80.0, 2.0),
        IecKind::Lti => (120.0, 1.0),
    }
}

/// IEEE C37.112 coefficients: t = TD * (A / (M^p - 1) + B).
fn ieee_constants(kind: IeeeKind) -> (f64, f64, f64) {
    match kind {
        IeeeKind::ModeratelyInverse => (0.0515, 0.1140, 0.02),
        IeeeKind::VeryInverse => (19.61, 0.491, 2.0),
        IeeeKind::ExtremelyInverse => (28.2, 0.1217, 2.0),
    }
}

pub fn curve_label(curve: &CurveSpec) -> String {
    match curve {
        CurveSpec::Iec { kind, pickup_a, tms, .. } => {
            format!("IEC {:?}  {:.0} A  TMS {tms}", kind, pickup_a)
        }
        CurveSpec::Ieee { kind, pickup_a, td, .. } => {
            format!("IEEE {:?}  {:.0} A  TD {td}", kind, pickup_a)
        }
        CurveSpec::Definite { pickup_a, time_s } => format!("Definite {:.0} A  {time_s} s", pickup_a),
        CurveSpec::ThermalMagnetic { lt_pickup_a, .. } => format!("Breaker LT {:.0} A", lt_pickup_a),
        CurveSpec::SettingsNotCollected { .. } => "Collected device, trip settings not entered".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iec_standard_inverse_at_ten_times() {
        let curve = CurveSpec::Iec {
            kind: IecKind::Si,
            pickup_a: 100.0,
            tms: 1.0,
            inst_a: None,
            inst_s: None,
        };
        let t = trip_time(&curve, 1000.0).unwrap();
        assert!((t - 2.971).abs() < 0.01, "t = {t}");
    }

    #[test]
    fn ieee_moderately_inverse_at_five_times() {
        let curve = CurveSpec::Ieee {
            kind: IeeeKind::ModeratelyInverse,
            pickup_a: 100.0,
            td: 1.0,
            inst_a: None,
            inst_s: None,
        };
        let t = trip_time(&curve, 500.0).unwrap();
        assert!((t - 1.688).abs() < 0.01, "t = {t}");
    }

    #[test]
    fn instantaneous_overrides_a_slower_curve() {
        let curve = CurveSpec::Definite { pickup_a: 100.0, time_s: 1.0 };
        assert!((trip_time(&curve, 150.0).unwrap() - 1.0).abs() < 1e-9);
        let curve = CurveSpec::Iec {
            kind: IecKind::Si,
            pickup_a: 100.0,
            tms: 1.0,
            inst_a: Some(500.0),
            inst_s: Some(0.05),
        };
        assert!((trip_time(&curve, 800.0).unwrap() - 0.05).abs() < 1e-9);
        assert!(trip_time(&curve, 50.0).is_none());
    }

    #[test]
    fn dynamic_constant_current_matches_static_trip_time() {
        let curve = CurveSpec::Iec {
            kind: IecKind::Si,
            pickup_a: 100.0,
            tms: 1.0,
            inst_a: None,
            inst_s: None,
        };
        let expected = trip_time(&curve, 1000.0).unwrap();
        let mut relay = DynamicRelay::new(&curve).unwrap();
        assert_eq!(relay.advance(1000.0, expected * 0.4).unwrap(), None);
        let within_second_slice = relay.advance(1000.0, expected * 0.7).unwrap().unwrap();
        assert!((within_second_slice - expected * 0.6).abs() < 1e-10);
        assert!(!relay.reset_occurred());
    }

    #[test]
    fn dynamic_inverse_integrates_each_current_level() {
        let curve = CurveSpec::Iec {
            kind: IecKind::Si,
            pickup_a: 100.0,
            tms: 1.0,
            inst_a: None,
            inst_s: None,
        };
        let high_time = trip_time(&curve, 1000.0).unwrap();
        let low_time = trip_time(&curve, 500.0).unwrap();
        let mut relay = DynamicRelay::new(&curve).unwrap();
        assert_eq!(relay.advance(1000.0, 1.0).unwrap(), None);
        let expected_second_slice = (1.0 - 1.0 / high_time) * low_time;
        let actual_second_slice = relay.advance(500.0, expected_second_slice + 0.1).unwrap().unwrap();
        assert!((actual_second_slice - expected_second_slice).abs() < 1e-10);
        assert!(!relay.reset_occurred());
    }

    #[test]
    fn definite_time_requires_continuous_pickup() {
        let curve = CurveSpec::Definite { pickup_a: 100.0, time_s: 1.0 };
        let mut relay = DynamicRelay::new(&curve).unwrap();
        assert_eq!(relay.advance(150.0, 0.6).unwrap(), None);
        assert_eq!(relay.advance(90.0, 0.1).unwrap(), None);
        assert!(relay.reset_occurred());
        assert_eq!(relay.advance(150.0, 0.6).unwrap(), None);
        assert!((relay.advance(150.0, 0.5).unwrap().unwrap() - 0.4).abs() < 1e-12);
    }

    #[test]
    fn independent_instantaneous_timer_resets_below_its_pickup() {
        let curve = CurveSpec::Iec {
            kind: IecKind::Si,
            pickup_a: 100.0,
            tms: 1.0,
            inst_a: Some(700.0),
            inst_s: Some(0.1),
        };
        let mut relay = DynamicRelay::new(&curve).unwrap();
        assert_eq!(relay.advance(800.0, 0.03).unwrap(), None);
        assert_eq!(relay.advance(600.0, 0.05).unwrap(), None);
        assert!(relay.reset_occurred());
        assert_eq!(relay.advance(800.0, 0.08).unwrap(), None);
        assert!((relay.advance(800.0, 0.03).unwrap().unwrap() - 0.02).abs() < 1e-12);
    }

    #[test]
    fn missing_timed_element_settings_are_explicit() {
        let curve = CurveSpec::ThermalMagnetic {
            lt_pickup_a: 100.0,
            lt_delay_s: 1.0,
            st_pickup_a: Some(200.0),
            st_delay_s: None,
            inst_a: None,
            inst_s: None,
        };
        let mut relay = DynamicRelay::new(&curve).unwrap();
        assert_eq!(relay.advance(150.0, 0.01).unwrap(), None);
        assert!(relay.advance(250.0, 0.01).unwrap_err().contains("short-time pickup was reached"));

        let curve = CurveSpec::Iec {
            kind: IecKind::Si,
            pickup_a: 100.0,
            tms: 1.0,
            inst_a: Some(500.0),
            inst_s: None,
        };
        let mut relay = DynamicRelay::new(&curve).unwrap();
        assert_eq!(relay.advance(400.0, 0.01).unwrap(), None);
        assert!(relay.advance(600.0, 0.01).unwrap_err().contains("instantaneous pickup was reached"));
    }

    #[test]
    fn pickup_thresholds_include_distinct_timed_elements() {
        let curve = CurveSpec::ThermalMagnetic {
            lt_pickup_a: 100.0,
            lt_delay_s: 1.0,
            st_pickup_a: Some(300.0),
            st_delay_s: Some(0.2),
            inst_a: Some(300.0),
            inst_s: Some(0.05),
        };
        let relay = DynamicRelay::new(&curve).unwrap();
        assert_eq!(relay.pickup_thresholds(), vec![100.0, 300.0]);
    }
}
