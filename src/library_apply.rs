//! Copy one library point curve into a project device.
//!
//! The curve on the device is a snapshot. Studies use that curve and do not
//! read the library again. Which band is minimum and which is maximum is
//! inferred, so the result is marked assumed. The slower band is not treated
//! as total clearing unless the caller sets `fuse_total_clearing`.

use crate::curves::{self, PointTime};
use crate::library::{Function, Library, LibraryDevice, PointCurve, TripUnit};
use crate::model::{BandInterpretation, CurveSpec, Device, LibraryLink, PointTail};

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
}

/// Copy one point curve from `library` into a new project device.
pub fn apply_device(library: &Library, request: &ApplyDevice) -> Result<Device, String> {
    let device = library.devices.iter().find(|d| d.id == request.device_id)
        .ok_or_else(|| format!("unknown library device id {}", request.device_id))?;
    let (unit_index, unit) = select_trip_unit(device, request.rating_a)?;
    let (function_index, function) = select_function(device.id, unit, &request.function_name)?;
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
    Ok(Device {
        id: request.id.clone(),
        name: applied_name(request, device, unit.rating_a),
        bus: request.bus.clone(),
        curve: CurveSpec::Points {
            points,
            max_points,
            beyond_last: PointTail::Unavailable,
            band_interpretation: BandInterpretation::Assumed,
        },
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
    })
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
    let usable: Vec<usize> = unit.functions.iter().enumerate().filter(|(_, function)| is_point_curve(function)).map(|(index, _)| index).collect();
    if usable.is_empty() {
        return Err(format!("device {device_id}: no usable point curve"));
    }
    let available = || usable.iter().map(|index| function_label(&unit.functions[*index])).collect::<Vec<_>>().join(", ");
    let index = match function_name {
        None if usable.len() == 1 => usable[0],
        None => return Err(format!("device {device_id}: {} point curves; function_name is required ({})", usable.len(), available())),
        Some(wanted) => {
            let found: Vec<usize> = usable.iter().copied()
                .filter(|index| unit.functions[*index].name.to_lowercase() == wanted.to_lowercase())
                .collect();
            match found.as_slice() {
                [index] => *index,
                [] => return Err(format!("device {device_id}: no point curve named {wanted:?} ({})", available())),
                [..] => return Err(format!("device {device_id}: function_name {wanted:?} matches {} point curves ({})", found.len(), available())),
            }
        }
    };
    Ok((index, &unit.functions[index]))
}

fn is_point_curve(function: &Function) -> bool {
    matches!(function.kind.as_str(), "point_curve" | "point_curve_bands") && !function.data.curves.is_empty()
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
    let mut out = Vec::new();
    for (index, unit) in device.trip_units.iter().enumerate() {
        if index == unit_index {
            for (function_i, function) in unit.functions.iter().enumerate() {
                if function_i != function_index {
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
