use crate::model::{CurveSpec, Device, IecKind, IeeeKind};

/// Trip time in seconds at `amps`. None means the device does not trip.
pub fn trip_time(curve: &CurveSpec, amps: f64) -> Option<f64> {
    if !amps.is_finite() || amps <= 0.0 {
        return None;
    }
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

fn instant(amps: f64, pickup: Option<f64>, time: Option<f64>) -> Option<f64> {
    let pickup = pickup?;
    if amps >= pickup {
        Some(time.unwrap_or(0.05))
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
}
