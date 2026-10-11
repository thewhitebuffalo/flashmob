//! Copy one library characteristic into a project device.
//!
//! The curve on the device is a snapshot. Studies use that curve and do not
//! read the library again. A point curve's band identity is inferred, so the
//! result is marked assumed. The slower band is not treated as total clearing
//! unless the caller sets `fuse_total_clearing`. A dial family or a trip
//! setting is chosen only by the request, not by the device category.

use crate::curves::{self, PointTime};
use crate::library::{Function, Library, LibraryDevice, PointCurve, Rating, TripUnit};
use crate::model::{BandInterpretation, CurveSpec, Device, InterruptingRating, LibraryLink, PointTail};

/// 0.05% of the requested rating.
const RATING_MATCH: f64 = 0.0005;

/// What to copy out of a library, and where it goes in the project.
#[derive(Clone, Debug)]
pub struct ApplyDevice {
    pub device_id: u32,
    /// Project device id.
    pub id: String,
    /// Empty builds a name from the manufacturer, style, catalog, and rating.
    pub name: String,
    pub bus: String,
    pub protected_branch: Option<String>,
    /// Selects the trip unit when the library device has more than one.
    pub rating_a: Option<f64>,
    /// Ampere scale for a curve stored as multiples. Overrides the trip-unit rating.
    pub current_base_a: Option<f64>,
    /// Recorded on the link when set. Does not select a trip unit.
    /// When omitted, the library header voltage is used, else the first rating voltage.
    pub voltage_v: Option<f64>,
    /// Selects the point curve when the trip unit has more than one.
    pub function_name: Option<String>,
    /// Label stored on `LibraryLink.library`.
    pub library_name: String,
    /// The caller has confirmed that the governing band already includes total
    /// clearing. Left false by default: a slower library band is often only the
    /// high side of a current tolerance, not a total-clearing curve.
    pub fuse_total_clearing: bool,
    /// Dial on a `time_dial_curve_family`.
    pub dial: Option<f64>,
    /// Scales a relay dial curve from secondary amperes to primary amperes.
    pub ct_ratio: Option<f64>,
    /// Relay tap, secondary amperes.
    pub tap_a: Option<f64>,
    /// Frame sensor or plug, amperes. Required on the trip-setting path.
    pub sensor_a: Option<f64>,
    pub lt_pickup: Option<SettingRef>,
    pub lt_delay: Option<SettingRef>,
    /// Current multiple at which the stored long-time delay applies.
    pub lt_delay_multiple: Option<f64>,
    pub st_pickup: Option<SettingRef>,
    pub st_delay: Option<SettingRef>,
    pub inst: Option<SettingRef>,
    /// Instantaneous time. Absent leaves operating time above that pickup unavailable.
    pub inst_s: Option<f64>,
    /// "sensor" or "long_time". Used only when a pickup function does not state its own base.
    pub pickup_base: Option<String>,
}

/// One named library setting: function name plus setting label.
#[derive(Clone, Debug)]
pub struct SettingRef {
    pub function: String,
    pub label: String,
}

/// Pair a function name and a setting label. Both absent is `None`.
pub fn setting_ref(function: &Option<String>, label: &Option<String>, name: &str) -> Result<Option<SettingRef>, String> {
    match (function, label) {
        (None, None) => Ok(None),
        (Some(function), Some(label)) => Ok(Some(SettingRef { function: function.clone(), label: label.clone() })),
        _ => Err(format!("{name} needs both a function and a label")),
    }
}

/// Copy one characteristic from `library` into a new project device.
pub fn apply_device(library: &Library, request: &ApplyDevice) -> Result<Device, String> {
    let device = library.devices.iter().find(|d| d.id == request.device_id)
        .ok_or_else(|| format!("unknown library device id {}", request.device_id))?;
    // Trip settings, then a dial family, then a point curve. Category does not choose the path.
    if trip_settings_requested(request) {
        return apply_trip_settings(device, request);
    }
    let (unit_index, unit) = select_trip_unit(device, request.rating_a)?;
    let (function_index, function) = select_function(device.id, unit, &request.function_name)?;
    if function.kind == "time_dial_curve_family" {
        return apply_dial(device, request, unit_index, unit, function_index, function);
    }
    apply_point_curve(device, request, unit_index, unit, function_index, function)
}

fn trip_settings_requested(request: &ApplyDevice) -> bool {
    request.lt_pickup.is_some() || request.lt_delay.is_some() || request.st_pickup.is_some()
        || request.st_delay.is_some() || request.inst.is_some()
}

fn apply_point_curve(device: &LibraryDevice, request: &ApplyDevice, unit_index: usize, unit: &TripUnit, function_index: usize, function: &Function) -> Result<Device, String> {
    let (scale, scale_note) = current_scale(function, unit, request.current_base_a)?;
    let (points, max_points) = point_bands(device.id, function, scale)?;
    let mut unsupported_elements = unsupported(device, unit_index, function_index);
    if let Some(note) = scale_note {
        unsupported_elements.push(note);
    }
    // Checked 100 A curves (LPJ, LPS-RK, FRS-R, AJT, TRS): three of the five
    // slower bands are the +10% side of one curve (current ratio 1.10/0.90),
    // not total clearing. Do not infer the flag from the category.
    if max_points.is_some() && !request.fuse_total_clearing {
        unsupported_elements.push("slower band is not marked total clearing".into());
    }
    Ok(finish(request, device, unit, CurveSpec::Points {
        points,
        max_points,
        beyond_last: PointTail::Unavailable,
        band_interpretation: BandInterpretation::Assumed,
    }, unsupported_elements))
}

fn select_trip_unit(device: &LibraryDevice, rating_a: Option<f64>) -> Result<(usize, &TripUnit), String> {
    let list = if device.trip_units.is_empty() {
        "none".to_string()
    } else {
        device.trip_units.iter().map(|unit| format!("{} A", plain(unit.rating_a))).collect::<Vec<_>>().join(", ")
    };
    let matches: Vec<usize> = match rating_a {
        Some(rating) => device.trip_units.iter().enumerate()
            .filter(|(_, unit)| ratings_close(unit.rating_a, rating))
            .map(|(index, _)| index)
            .collect(),
        None if device.trip_units.len() == 1 => vec![0],
        None if device.trip_units.is_empty() => {
            return Err(format!("device {}: no trip unit (ratings: {list})", device.id));
        }
        None => {
            return Err(format!("device {}: rating_a is required to select a trip unit (ratings: {list})", device.id));
        }
    };
    match matches.as_slice() {
        [index] => Ok((*index, &device.trip_units[*index])),
        [] => Err(format!("device {}: rating {} A matches no trip unit (ratings: {list})", device.id, plain(rating_a.unwrap_or(0.0)))),
        [..] => Err(format!("device {}: rating {} A matches {} trip units (ratings: {list})", device.id, plain(rating_a.unwrap_or(0.0)), matches.len())),
    }
}

fn ratings_close(unit: f64, requested: f64) -> bool {
    if !unit.is_finite() || !requested.is_finite() {
        return false;
    }
    if requested == 0.0 {
        return unit == 0.0;
    }
    ((unit - requested) / requested).abs() <= RATING_MATCH
}

fn select_function<'a>(device_id: u32, unit: &'a TripUnit, function_name: &Option<String>) -> Result<(usize, &'a Function), String> {
    let usable: Vec<usize> = unit.functions.iter().enumerate().filter(|(_, function)| is_usable(function)).map(|(index, _)| index).collect();
    if usable.is_empty() {
        return Err(format!("device {device_id}: no usable point curve or dial curve"));
    }
    // Keep the point-curve wording when no dial family is usable.
    let points_only = usable.iter().all(|index| is_point_curve(&unit.functions[*index]));
    let noun = if points_only { "point curve" } else { "curve" };
    let available = || usable.iter().map(|index| function_label(&unit.functions[*index])).collect::<Vec<_>>().join(", ");
    let index = match function_name {
        None if usable.len() == 1 => usable[0],
        None => return Err(format!("device {device_id}: {} {noun}s; function_name is required ({})", usable.len(), available())),
        Some(wanted) => {
            let found: Vec<usize> = usable.iter().copied()
                .filter(|index| unit.functions[*index].name.to_lowercase() == wanted.to_lowercase())
                .collect();
            match found.as_slice() {
                [index] => *index,
                [] => return Err(format!("device {device_id}: no {noun} named {wanted:?} ({})", available())),
                [..] => return Err(format!("device {device_id}: function_name {wanted:?} matches {} {noun}s ({})", found.len(), available())),
            }
        }
    };
    Ok((index, &unit.functions[index]))
}

fn is_usable(function: &Function) -> bool {
    is_point_curve(function) || is_dial_family(function)
}

fn is_point_curve(function: &Function) -> bool {
    matches!(function.kind.as_str(), "point_curve" | "point_curve_bands") && !function.data.curves.is_empty()
}

fn is_dial_family(function: &Function) -> bool {
    function.kind == "time_dial_curve_family" && !function.data.dial_curves.is_empty()
}

fn current_scale(function: &Function, unit: &TripUnit, current_base_a: Option<f64>) -> Result<(f64, Option<String>), String> {
    match function.data.current_unit.as_deref() {
        Some("A") => {
            if current_base_a.is_some() {
                return Err("current_unit is A; current_base_a would scale an ampere curve twice".into());
            }
            Ok((1.0, None))
        }
        Some("multiple") => {
            let scale = current_base_a.unwrap_or(unit.rating_a);
            if !scale.is_finite() || scale <= 0.0 {
                return Err("current scale for a multiple curve must be positive and finite".into());
            }
            Ok((scale, Some(format!("current scale {} A", plain(scale)))))
        }
        Some(other) => Err(format!("current_unit {other:?} is not A or multiple")),
        None => Err("point curve is missing current_unit".into()),
    }
}

fn point_bands(device_id: u32, function: &Function, scale: f64) -> Result<(Vec<[f64; 2]>, Option<Vec<[f64; 2]>>), String> {
    let curves = &function.data.curves;
    match curves.len() {
        0 => return Err(format!("device {device_id}: point curve has no bands")),
        1 | 2 => {}
        n => return Err(format!("device {device_id}: point curve has {n} bands")),
    }
    let mut bands = Vec::with_capacity(curves.len());
    for curve in curves {
        let points = band_points(curve, scale)?;
        // Do not sort. A band whose currents are not strictly increasing, or
        // whose times increase, is rejected rather than repaired.
        curves::check_point_curve(&points, None)?;
        bands.push(points);
    }
    if bands.len() == 1 {
        return Ok((bands.remove(0), None));
    }
    let slow_first = first_is_slower(&bands[0], &bands[1])?;
    let (fast, slow) = if slow_first {
        let slow = bands.remove(0);
        (bands.remove(0), slow)
    } else {
        let fast = bands.remove(0);
        (fast, bands.remove(0))
    };
    curves::check_point_curve(&fast, Some(&slow))?;
    Ok((fast, Some(slow)))
}

fn band_points(curve: &PointCurve, scale: f64) -> Result<Vec<[f64; 2]>, String> {
    if curve.current.len() != curve.time_s.len() {
        return Err(format!("curve has {} currents and {} times", curve.current.len(), curve.time_s.len()));
    }
    let mut points = Vec::with_capacity(curve.current.len());
    for (&current, &time_s) in curve.current.iter().zip(curve.time_s.iter()) {
        if !current.is_finite() || current <= 0.0 || !time_s.is_finite() || time_s <= 0.0 {
            return Err("curve current and time must be positive and finite".into());
        }
        let amps = current * scale;
        if !amps.is_finite() || amps <= 0.0 {
            return Err("scaled current must be positive and finite".into());
        }
        points.push([amps, time_s]);
    }
    Ok(points)
}

/// True when band `a` is slower than `b` over their overlap.
fn first_is_slower(a: &[[f64; 2]], b: &[[f64; 2]]) -> Result<bool, String> {
    let (a_lo, a_hi) = band_ends(a)?;
    let (b_lo, b_hi) = band_ends(b)?;
    let lo = a_lo.max(b_lo);
    let hi = a_hi.min(b_hi);
    if !(lo.is_finite() && hi.is_finite() && lo <= hi) {
        return Err("point-curve bands do not overlap in current".into());
    }
    let mid = overlap_mid(lo, hi)?;
    let ta = within_time(a, mid)?;
    let tb = within_time(b, mid)?;
    let a_slower = if strictly_slower(ta, tb) {
        true
    } else if strictly_slower(tb, ta) {
        false
    } else {
        return Err("point-curve bands are not consistently ordered".into());
    };
    let (slow, fast) = if a_slower { (a, b) } else { (b, a) };
    // Both bands are piecewise linear in ln(I) and ln(t). Between stored
    // currents the order cannot reverse, so the stored currents catch a cross.
    for points in [a, b] {
        for &[amps, _] in points {
            if amps < lo || amps > hi {
                continue;
            }
            let slow_t = within_time(slow, amps)?;
            let fast_t = within_time(fast, amps)?;
            if !not_faster(slow_t, fast_t) {
                return Err("point-curve bands cross".into());
            }
        }
    }
    Ok(a_slower)
}

fn band_ends(points: &[[f64; 2]]) -> Result<(f64, f64), String> {
    match (points.first(), points.last()) {
        (Some(first), Some(last)) if points.len() >= 2 => Ok((first[0], last[0])),
        _ => Err("point curve band is empty".into()),
    }
}

fn overlap_mid(lo: f64, hi: f64) -> Result<f64, String> {
    if lo == hi {
        return Ok(lo);
    }
    let mean = ((lo.ln() + hi.ln()) * 0.5).exp();
    if !mean.is_finite() {
        return Err("point-curve overlap has a non-finite geometric-mean current".into());
    }
    // A rounding step can land just outside the overlap; the comparison
    // current has to stay where both bands are defined.
    Ok(mean.clamp(lo, hi))
}

fn within_time(points: &[[f64; 2]], amps: f64) -> Result<f64, String> {
    match curves::point_time(points, amps) {
        PointTime::Within(time_s) => Ok(time_s),
        PointTime::Below | PointTime::Beyond { .. } => Err(format!("no band time at {amps} A")),
    }
}

fn strictly_slower(slow: f64, fast: f64) -> bool {
    slow > fast * (1.0 + 1e-9)
}

fn not_faster(slow: f64, fast: f64) -> bool {
    slow >= fast * (1.0 - 1e-9)
}

fn unsupported(device: &LibraryDevice, unit_index: usize, function_index: usize) -> Vec<String> {
    unsupported_except(device, unit_index, &[function_index])
}

fn unsupported_except(device: &LibraryDevice, unit_index: usize, applied: &[usize]) -> Vec<String> {
    let mut out = Vec::new();
    for (index, unit) in device.trip_units.iter().enumerate() {
        if index == unit_index {
            for (function_i, function) in unit.functions.iter().enumerate() {
                if !applied.contains(&function_i) {
                    out.push(function_label(function));
                }
            }
        } else {
            out.push(trip_unit_label(unit));
        }
    }
    out
}

fn function_label(function: &Function) -> String {
    let name = if function.name.is_empty() { "unnamed" } else { function.name.as_str() };
    format!("function {name} ({})", function.kind)
}

fn trip_unit_label(unit: &TripUnit) -> String {
    let rating = format!("trip unit {} A", plain(unit.rating_a));
    if unit.functions.is_empty() {
        rating
    } else {
        let functions = unit.functions.iter().map(function_label).collect::<Vec<_>>().join(", ");
        format!("{rating}: {functions}")
    }
}

fn apply_dial(device: &LibraryDevice, request: &ApplyDevice, unit_index: usize, unit: &TripUnit, function_index: usize, function: &Function) -> Result<Device, String> {
    let ct_ratio = match request.ct_ratio {
        Some(ratio) if ratio.is_finite() && ratio > 0.0 => ratio,
        Some(_) => return Err("ct_ratio must be positive and finite".into()),
        None => return Err("ct_ratio is required to scale a relay dial curve to primary amperes".into()),
    };
    let requested = request.dial.ok_or_else(|| format!("dial is required to select a relay dial curve (dials: {})", dial_list(function)))?;
    if !requested.is_finite() {
        return Err(format!("dial is not finite (dials: {})", dial_list(function)));
    }
    let matches: Vec<usize> = function.data.dial_curves.iter().enumerate()
        .filter(|(_, curve)| dials_close(curve.dial, requested))
        .map(|(index, _)| index)
        .collect();
    let dial_index = match matches.as_slice() {
        [index] => *index,
        [] => return Err(format!("device {}: dial {} matches no dial curve (dials: {})", device.id, plain(requested), dial_list(function))),
        _ => return Err(format!("device {}: dial {} matches {} dial curves (dials: {})", device.id, plain(requested), matches.len(), dial_list(function))),
    };
    let dial = function.data.dial_curves[dial_index].dial;
    let (tap_a, tap_note) = resolve_tap(request.tap_a, function, dial)?;
    let points = dial_points(&function.data.dial_current_multiples, &function.data.dial_curves[dial_index].time_s, tap_a, ct_ratio)?;
    curves::check_point_curve(&points, None)?;
    let mut unsupported_elements = unsupported(device, unit_index, function_index);
    if let Some(note) = tap_note {
        unsupported_elements.push(note);
    }
    unsupported_elements.push(format!("dial {} scaled by tap {} A and CT ratio {}", plain(dial), plain(tap_a), plain(ct_ratio)));
    Ok(finish(request, device, unit, CurveSpec::Points {
        points,
        max_points: None,
        beyond_last: PointTail::Unavailable,
        band_interpretation: BandInterpretation::Assumed,
    }, unsupported_elements))
}

fn dials_close(stored: f64, requested: f64) -> bool {
    stored.is_finite() && requested.is_finite() && ((stored - requested).abs() <= 1e-9 || ratings_close(stored, requested))
}

fn dial_list(function: &Function) -> String {
    if function.data.dial_curves.is_empty() {
        "none".into()
    } else {
        function.data.dial_curves.iter().map(|curve| plain(curve.dial)).collect::<Vec<_>>().join(", ")
    }
}

fn resolve_tap(tap_a: Option<f64>, function: &Function, dial: f64) -> Result<(f64, Option<String>), String> {
    if let Some(tap) = tap_a {
        if !tap.is_finite() || tap <= 0.0 {
            return Err("tap_a (secondary amperes) must be positive and finite".into());
        }
        return Ok((tap, None));
    }
    // A trailing A is the only sign that the dial number is already secondary amperes.
    let matched: Vec<_> = function.data.settings.iter().filter(|setting| dials_close(setting.value, dial)).collect();
    if let [setting] = matched.as_slice() {
        if setting.label.ends_with('A') || setting.label.ends_with('a') {
            return Ok((dial, Some(format!("tap {} A taken from the dial label, assumed secondary amperes", plain(dial)))));
        }
    }
    Err("tap_a (secondary amperes) is required; ct_ratio alone does not set the pickup".into())
}

fn dial_points(multiples: &[f64], time_s: &[f64], tap_a: f64, ct_ratio: f64) -> Result<Vec<[f64; 2]>, String> {
    if multiples.len() != time_s.len() {
        return Err(format!("dial curve has {} currents and {} times", multiples.len(), time_s.len()));
    }
    let mut points = Vec::with_capacity(multiples.len());
    for (&multiple, &time_s) in multiples.iter().zip(time_s.iter()) {
        if !multiple.is_finite() || multiple <= 0.0 || !time_s.is_finite() || time_s <= 0.0 {
            return Err("curve current and time must be positive and finite".into());
        }
        let amps = multiple * tap_a * ct_ratio;
        if !amps.is_finite() || amps <= 0.0 {
            return Err("scaled current must be positive and finite".into());
        }
        points.push([amps, time_s]);
    }
    Ok(points)
}

fn apply_trip_settings(device: &LibraryDevice, request: &ApplyDevice) -> Result<Device, String> {
    let sensor_a = match request.sensor_a {
        Some(sensor) if sensor.is_finite() && sensor > 0.0 => sensor,
        Some(_) => return Err("sensor_a must be positive and finite".into()),
        None => return Err("sensor_a is required".into()),
    };
    if !device.frames.iter().any(|frame| frame.sensors_a.iter().any(|sensor| ratings_close(*sensor, sensor_a))) {
        return Err(format!("sensor {} A matches no frame sensor (sensors: {})", plain(sensor_a), sensor_list(device)));
    }
    let (unit_index, unit) = select_trip_unit(device, request.rating_a)?;
    let lt_pickup_ref = request.lt_pickup.as_ref().ok_or("long-time pickup setting is required")?;
    let lt_delay_ref = request.lt_delay.as_ref().ok_or("long-time delay setting is required")?;
    let (lt_pu_index, lt_pu_fn, _, lt_pu_setting) = find_setting(device.id, unit, lt_pickup_ref)?;
    let (lt_d_index, lt_d_fn, lt_d_at, lt_d_setting) = find_setting(device.id, unit, lt_delay_ref)?;
    let multiple = match request.lt_delay_multiple {
        Some(value) if value.is_finite() && value > 0.0 => value,
        Some(_) => return Err("lt_delay_multiple must be positive and finite".into()),
        None => return Err("the library does not state the current multiple at which the long-time delay applies; pass lt_delay_multiple (6 when that time is at 6 times long-time pickup)".into()),
    };
    let (t_used, slower) = slower_time(lt_d_fn, lt_d_at, lt_d_setting)?;
    // The thermal-magnetic model stores long-time delay as seconds at 6× pickup.
    let lt_delay_s = t_used * (multiple / 6.0).powi(2);
    if !lt_delay_s.is_finite() || lt_delay_s <= 0.0 {
        return Err("long-time delay must be positive and finite".into());
    }
    let lt_pickup_a = scale_pickup(lt_pu_fn, lt_pu_setting, sensor_a, None, request.pickup_base.as_deref())?;
    let short = short_time(device.id, unit, request, sensor_a, lt_pickup_a)?;
    let instant = instantaneous(device.id, unit, request, sensor_a, lt_pickup_a)?;
    let mut applied = vec![lt_pu_index];
    if lt_d_index != lt_pu_index {
        applied.push(lt_d_index);
    }
    applied.extend(short.applied);
    applied.extend(instant.applied);
    let mut unsupported_elements = unsupported_except(device, unit_index, &applied);
    unsupported_elements.push(format!("sensor {} A", plain(sensor_a)));
    if slower {
        unsupported_elements.push(format!("long-time delay uses the slower stored time {} s (setting {} s)", plain(t_used), plain(lt_d_setting)));
    }
    unsupported_elements.push(format!("long-time delay {} s at {}× converted to {} s at 6×", plain(t_used), plain(multiple), plain(lt_delay_s)));
    unsupported_elements.extend(short.notes);
    unsupported_elements.extend(instant.notes);
    Ok(finish(request, device, unit, CurveSpec::ThermalMagnetic {
        lt_pickup_a,
        lt_delay_s,
        st_pickup_a: short.pickup_a,
        st_delay_s: short.delay_s,
        inst_a: instant.amps,
        inst_s: instant.time_s,
    }, unsupported_elements))
}

fn sensor_list(device: &LibraryDevice) -> String {
    let sensors: Vec<String> = device.frames.iter().flat_map(|frame| frame.sensors_a.iter().copied().map(plain)).collect();
    if sensors.is_empty() { "none".into() } else { sensors.join(", ") }
}

fn find_setting<'a>(device_id: u32, unit: &'a TripUnit, wanted: &SettingRef) -> Result<(usize, &'a Function, usize, f64), String> {
    let functions: Vec<usize> = unit.functions.iter().enumerate()
        .filter(|(_, function)| function.name.to_lowercase() == wanted.function.to_lowercase())
        .map(|(index, _)| index)
        .collect();
    let names = || unit.functions.iter().map(function_label).collect::<Vec<_>>().join(", ");
    let function_index = match functions.as_slice() {
        [index] => *index,
        [] => return Err(format!("device {device_id}: no function named {:?} ({})", wanted.function, names())),
        _ => return Err(format!("device {device_id}: {:?} matches {} functions ({})", wanted.function, functions.len(), names())),
    };
    let function = &unit.functions[function_index];
    let settings: Vec<usize> = function.data.settings.iter().enumerate()
        .filter(|(_, setting)| setting.label.to_lowercase() == wanted.label.to_lowercase())
        .map(|(index, _)| index)
        .collect();
    let labels = || {
        if function.data.settings.is_empty() { "none".to_string() }
        else { function.data.settings.iter().map(|setting| setting.label.as_str()).collect::<Vec<_>>().join(", ") }
    };
    let setting_index = match settings.as_slice() {
        [index] => *index,
        [] => return Err(format!("device {device_id}: function {:?} has no setting {:?} ({})", wanted.function, wanted.label, labels())),
        _ => return Err(format!("device {device_id}: function {:?} setting {:?} matches {} settings ({})", wanted.function, wanted.label, settings.len(), labels())),
    };
    Ok((function_index, function, setting_index, function.data.settings[setting_index].value))
}

/// Same-length `data.arrays` entries are the only extra times considered.
/// A longer or shorter array is not treated as a tolerance band.
fn slower_time(function: &Function, index: usize, setting: f64) -> Result<(f64, bool), String> {
    if !setting.is_finite() {
        return Err(format!("delay setting on {} is not finite", shown_name(function)));
    }
    let mut used = setting;
    let count = function.data.settings.len();
    for array in &function.data.arrays {
        if array.len() != count {
            continue;
        }
        let value = array[index];
        if !value.is_finite() {
            return Err(format!("delay array on {} has a non-finite time", shown_name(function)));
        }
        if value > used {
            used = value;
        }
    }
    if !(used.is_finite() && used > 0.0) {
        return Err(format!("delay time on {} must be positive and finite", shown_name(function)));
    }
    Ok((used, used > setting))
}

fn scale_pickup(function: &Function, value: f64, sensor_a: f64, long_time_a: Option<f64>, pickup_base: Option<&str>) -> Result<f64, String> {
    let name = shown_name(function);
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("pickup on {name} must be positive and finite"));
    }
    let amps = if function.data.header_codes[1] == 10 {
        value
    } else if name_has_long_time_base(&function.name) {
        let Some(base) = long_time_a else {
            return Err(format!("long-time pickup cannot be a multiple of long-time pickup ({name})"));
        };
        value * base
    } else if name_has_sensor_base(&function.name) {
        value * sensor_a
    } else {
        match pickup_base {
            Some("sensor") => value * sensor_a,
            Some("long_time") => {
                let Some(base) = long_time_a else {
                    return Err(format!("long-time pickup cannot use long_time as its base ({name})"));
                };
                value * base
            }
            _ => return Err(format!("pickup base is not stated for {name}; pass pickup_base sensor or long_time")),
        }
    };
    if !amps.is_finite() || amps <= 0.0 {
        return Err(format!("scaled pickup for {name} must be positive and finite"));
    }
    Ok(amps)
}

fn shown_name(function: &Function) -> &str {
    if function.name.is_empty() { "unnamed" } else { function.name.as_str() }
}

fn name_has_long_time_base(name: &str) -> bool {
    contains_base_phrase(name, "x ltpu") || contains_base_phrase(name, "x ir") || contains_base_phrase(name, "× ltpu")
}

fn name_has_sensor_base(name: &str) -> bool {
    contains_base_phrase(name, "x sensor") || contains_base_phrase(name, "x plug") || contains_base_phrase(name, "x in") || contains_base_phrase(name, "x p")
}

/// `phrase` is a base such as "x sensor". The leading x/× may follow a digit.
/// A letter before it, or a letter or digit after the phrase, is a different word.
fn contains_base_phrase(name: &str, phrase: &str) -> bool {
    let hay = name.to_lowercase();
    let phrase = phrase.to_lowercase();
    if phrase.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(at) = hay[from..].find(&phrase) {
        let start = from + at;
        let end = start + phrase.len();
        let before_ok = hay[..start].chars().next_back().map(|c| !c.is_ascii_alphabetic()).unwrap_or(true);
        let after_ok = hay[end..].chars().next().map(|c| !c.is_ascii_alphanumeric()).unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        let step = hay[start..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        from = start + step;
    }
    false
}

struct ShortTime {
    pickup_a: Option<f64>,
    delay_s: Option<f64>,
    notes: Vec<String>,
    applied: Vec<usize>,
}

fn short_time(device_id: u32, unit: &TripUnit, request: &ApplyDevice, sensor_a: f64, lt_pickup_a: f64) -> Result<ShortTime, String> {
    let none = ShortTime { pickup_a: None, delay_s: None, notes: Vec::new(), applied: Vec::new() };
    match (&request.st_delay, &request.st_pickup) {
        (None, None) => Ok(none),
        (None, Some(_)) => Err("short-time delay setting is required with short-time pickup".into()),
        (Some(delay_ref), pickup_ref) => {
            let (delay_index, delay_fn, delay_at, delay_value) = find_setting(device_id, unit, delay_ref)?;
            if is_i2t(&delay_fn.name) {
                // An I2t band is not a flat delay. Leave both short-time fields unset.
                if let Some(pickup_ref) = pickup_ref {
                    find_setting(device_id, unit, pickup_ref)?;
                }
                return Ok(ShortTime {
                    notes: vec![format!("short-time function {} is I2t; the flat short-time model was not used", shown_name(delay_fn))],
                    ..none
                });
            }
            let Some(pickup_ref) = pickup_ref else {
                return Err("short-time pickup setting is required with short-time delay".into());
            };
            let (pickup_index, pickup_fn, _, pickup_value) = find_setting(device_id, unit, pickup_ref)?;
            let pickup_a = scale_pickup(pickup_fn, pickup_value, sensor_a, Some(lt_pickup_a), request.pickup_base.as_deref())?;
            let (used, slower) = slower_time(delay_fn, delay_at, delay_value)?;
            let mut notes = Vec::new();
            if slower {
                notes.push(format!("short-time delay uses the slower stored time {} s (setting {} s)", plain(used), plain(delay_value)));
            }
            notes.push(format!("flat short-time delay {} s is assumed", plain(used)));
            let mut applied = vec![delay_index];
            if pickup_index != delay_index {
                applied.push(pickup_index);
            }
            Ok(ShortTime { pickup_a: Some(pickup_a), delay_s: Some(used), notes, applied })
        }
    }
}

fn is_i2t(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("i2t") || lower.contains("i^2t") || lower.contains("i²t")
}

struct Instant {
    amps: Option<f64>,
    time_s: Option<f64>,
    notes: Vec<String>,
    applied: Vec<usize>,
}

fn instantaneous(device_id: u32, unit: &TripUnit, request: &ApplyDevice, sensor_a: f64, lt_pickup_a: f64) -> Result<Instant, String> {
    let Some(inst_ref) = &request.inst else {
        if request.inst_s.is_some() {
            return Err("inst_s requires an instantaneous pickup setting".into());
        }
        return Ok(Instant { amps: None, time_s: None, notes: Vec::new(), applied: Vec::new() });
    };
    let (index, function, _, value) = find_setting(device_id, unit, inst_ref)?;
    let amps = scale_pickup(function, value, sensor_a, Some(lt_pickup_a), request.pickup_base.as_deref())?;
    match request.inst_s {
        Some(time) if time.is_finite() && time > 0.0 => Ok(Instant { amps: Some(amps), time_s: Some(time), notes: Vec::new(), applied: vec![index] }),
        Some(_) => Err("inst_s must be positive and finite".into()),
        None => Ok(Instant {
            amps: Some(amps),
            time_s: None,
            notes: vec!["instantaneous pickup is set and instantaneous time is not; operating time above that pickup is unavailable until inst_s is entered".into()],
            applied: vec![index],
        }),
    }
}

fn finish(request: &ApplyDevice, device: &LibraryDevice, unit: &TripUnit, curve: CurveSpec, mut unsupported_elements: Vec<String>) -> Device {
    let interrupting = select_interrupting(device, request, unit, &mut unsupported_elements);
    Device {
        id: request.id.clone(),
        name: applied_name(request, device, unit.rating_a),
        bus: request.bus.clone(),
        curve,
        protected_branch: request.protected_branch.clone(),
        breaker_interrupting_s: None,
        fuse_total_clearing: request.fuse_total_clearing,
        curve_is_total_clearing: false,
        basis: "assumed".into(),
        library: Some(LibraryLink {
            device_id: device.id,
            library: request.library_name.clone(),
            manufacturer: device.manufacturer.clone(),
            device_type: device.device_type.clone(),
            style: device.style.clone(),
            category: device.category.clone(),
            size: if device.catalog.is_empty() { plain(unit.rating_a) } else { device.catalog.clone() },
            rating_a: Some(unit.rating_a),
            voltage_v: link_voltage(device, request.voltage_v),
            modified: device.modified.clone(),
            unsupported_elements,
        }),
        interrupting,
    }
}

/// Snapshot one symmetrical interrupting rating when the library rows agree.
/// A zero interrupting rating is not a rating. Several different duties are
/// left unset and named, rather than picking one.
fn select_interrupting(device: &LibraryDevice, request: &ApplyDevice, unit: &TripUnit, notes: &mut Vec<String>) -> Option<InterruptingRating> {
    let mut usable: Vec<&Rating> = device.ratings.iter().filter(|rating| {
        rating.interrupting_ka.is_finite() && rating.interrupting_ka > 0.0
            && rating.test_xr.is_finite() && rating.test_xr > 0.0
            && rating.voltage_v.is_finite() && rating.voltage_v >= 0.0
            && rating.rating_a.is_finite()
    }).collect();
    if usable.is_empty() {
        return None;
    }
    if let Some(voltage) = request.voltage_v.filter(|voltage| voltage.is_finite() && *voltage >= 0.0) {
        let matched: Vec<&Rating> = usable.iter().copied().filter(|rating| voltage_close(rating.voltage_v, voltage)).collect();
        if matched.is_empty() {
            notes.push(format!("interrupting rating not selected: no rating at {} V", plain(voltage)));
            return None;
        }
        usable = matched;
    }
    let amp = request.sensor_a.filter(|amp| amp.is_finite() && *amp > 0.0)
        .or_else(|| (unit.rating_a > 0.0).then_some(unit.rating_a));
    if let Some(amp) = amp {
        let matched: Vec<&Rating> = usable.iter().copied().filter(|rating| ratings_close(rating.rating_a, amp)).collect();
        if !matched.is_empty() {
            usable = matched;
        }
    }
    let mut distinct: Vec<&Rating> = Vec::new();
    for rating in &usable {
        let same = distinct.iter().any(|kept| {
            ratings_close(kept.interrupting_ka, rating.interrupting_ka) && ratings_close(kept.test_xr, rating.test_xr)
        });
        if !same {
            distinct.push(rating);
        }
    }
    let Some(chosen) = distinct.first().copied().filter(|_| distinct.len() == 1) else {
        let list = distinct.iter().map(|rating| {
            format!("{} kA at {} V test X/R {}", plain(rating.interrupting_ka), plain(rating.voltage_v), plain(rating.test_xr))
        }).collect::<Vec<_>>().join("; ");
        notes.push(format!("interrupting rating not selected: {list}"));
        return None;
    };
    Some(InterruptingRating {
        symmetrical_ka: chosen.interrupting_ka,
        test_xr: chosen.test_xr,
        voltage_v: chosen.voltage_v,
    })
}

fn voltage_close(stored: f64, requested: f64) -> bool {
    if requested == 0.0 {
        stored == 0.0
    } else {
        ((stored - requested) / requested).abs() <= RATING_MATCH
    }
}

fn link_voltage(device: &LibraryDevice, requested: Option<f64>) -> Option<f64> {
    if requested.is_some() {
        return requested;
    }
    if device.header_voltage_v.is_some() {
        return device.header_voltage_v;
    }
    device.ratings.first().map(|rating| rating.voltage_v)
}

fn applied_name(request: &ApplyDevice, device: &LibraryDevice, rating_a: f64) -> String {
    if !request.name.is_empty() {
        return request.name.clone();
    }
    let rating = plain(rating_a);
    [device.manufacturer.as_str(), device.style.as_str(), device.catalog.as_str(), rating.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn plain(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}
