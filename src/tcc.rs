use crate::curves::trip_time;
use crate::model::{CurveSpec, Device, Project};
use crate::study::StudyOutput;

#[derive(Clone, Debug)]
pub struct TccPlot {
    pub title: String,
    pub ref_kv: f64,
    pub i_min: f64,
    pub i_max: f64,
    pub t_min: f64,
    pub t_max: f64,
    pub curves: Vec<TccCurve>,
    pub markers: Vec<TccMarker>,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct TccCurve {
    pub name: String,
    pub color: &'static str,
    pub points: Vec<(f64, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    /// Bolted three-phase fault current at a bus.
    Fault,
    /// Arcing current from a completed arc-flash row.
    Arc,
    /// Arcing current from a row that used assumed inputs.
    ArcAssumed,
}

#[derive(Clone, Debug)]
pub struct TccMarker {
    pub label: String,
    pub amps: f64,
    /// Color for the light-background SVG export. The GUI picks its own by `kind`.
    pub color: &'static str,
    pub kind: MarkerKind,
}

/// Curve colors for the light SVG background (#fffdf8). The dark GUI uses its own palette.
pub const SVG_COLORS: [&str; 6] = ["#1d4ed8", "#b91c1c", "#047857", "#7c3aed", "#b45309", "#0f766e"];
const FAULT_MARKER: &str = "#44403c";
const ARC_MARKER: &str = "#c2410c";

/// Lowest time axis bound. Faster elements extend the axis down to this.
const T_FLOOR: f64 = 1e-4;

pub fn plot(project: &Project, study: Option<&StudyOutput>, ref_kv: Option<f64>, only: &[String]) -> Result<TccPlot, String> {
    if project.devices.is_empty() {
        return Err("the project has no protective devices to plot".into());
    }
    let devices: Vec<&Device> = project
        .devices
        .iter()
        .filter(|d| only.is_empty() || only.iter().any(|id| id == &d.id || id == &d.name))
        .collect();
    if devices.is_empty() {
        return Err(format!("no protective device matches the requested id or name: {}", only.join(", ")));
    }
    let ref_kv = match ref_kv {
        Some(kv) => kv,
        None => devices
            .iter()
            .filter_map(|d| project.bus(&d.bus).map(|b| b.kv))
            .filter(|kv| kv.is_finite() && *kv > 0.0)
            .reduce(f64::min)
            .ok_or("cannot choose a reference voltage: no selected device is on a bus with a positive kV; give ref_kv")?,
    };
    if !ref_kv.is_finite() || ref_kv <= 0.0 {
        return Err("reference voltage must be positive".into());
    }
    let mut curves = Vec::new();
    let mut i_min = f64::MAX;
    let mut i_max: f64 = 100.0;
    let mut fastest = f64::MAX;
    for (n, device) in devices.iter().enumerate() {
        let kv = project.bus(&device.bus).map(|b| b.kv).unwrap_or(ref_kv);
        let points = sample_curve(&device.curve, kv, ref_kv);
        if points.is_empty() { continue; }
        for (amp, time) in &points {
            i_min = i_min.min(*amp);
            i_max = i_max.max(*amp);
            fastest = fastest.min(*time);
        }
        curves.push(TccCurve {
            name: device.name.clone(),
            color: SVG_COLORS[n % SVG_COLORS.len()],
            points,
        });
    }
    if curves.is_empty() { return Err("no plottable curve: selected devices have no usable settings".into()); }
    i_min = (i_min * 0.7).max(1.0);
    i_max = (i_max * 1.4).max(i_min * 10.0);

    let mut markers = Vec::new();
    if let Some(study) = study {
        if let Some(fault) = &study.fault {
            for bus in &fault.buses {
                if let Some(point) = &bus.three_phase {
                    let amps = point.symmetrical_ka * 1000.0 * bus.kv / ref_kv;
                    if amps.is_finite() && amps > 0.0 {
                        markers.push(TccMarker {
                            label: format!("{} 3P", bus.name),
                            amps,
                            color: FAULT_MARKER,
                            kind: MarkerKind::Fault,
                        });
                        i_max = i_max.max(amps * 1.2);
                    }
                }
            }
        }
        if let Some(rows) = &study.arc_flash {
            for row in rows {
                let kv = project.bus(&row.bus_id).map(|b| b.kv).unwrap_or(ref_kv);
                let amps = row.arcing_ka * 1000.0 * kv / ref_kv;
                if !amps.is_finite() || amps <= 0.0 {
                    continue;
                }
                let (label, kind) = if row.assumed {
                    (format!("{} arc (assumed)", row.bus_name), MarkerKind::ArcAssumed)
                } else {
                    (format!("{} arc", row.bus_name), MarkerKind::Arc)
                };
                markers.push(TccMarker { label, amps, color: ARC_MARKER, kind });
                i_max = i_max.max(amps * 1.2);
                i_min = i_min.min((amps * 0.8).max(1.0));
            }
        }
    }

    if !i_min.is_finite() || !i_max.is_finite() || i_min <= 0.0 || i_max <= i_min {
        return Err("invalid TCC axis bounds: finite positive ordered bounds required".into());
    }
    // The usual 0.01 s floor, extended by decades so faster elements stay visible.
    let mut exponent = -2;
    while fastest < 10f64.powi(exponent) && 10f64.powi(exponent) > T_FLOOR {
        exponent -= 1;
    }
    let t_min = 10f64.powi(exponent);
    Ok(TccPlot {
        title: project.name.clone(),
        ref_kv,
        i_min,
        i_max,
        t_min,
        t_max: 1000.0,
        curves,
        markers,
        note: format!(
            "Current is referred to {ref_kv:.3} kV. Time-current curves use IEC 60255, IEEE C37.112, definite time, or the simplified breaker model stored on each device."
        ),
    })
}

fn sample_curve(curve: &CurveSpec, device_kv: f64, ref_kv: f64) -> Vec<(f64, f64)> {
    let Some(pickup) = pickup_amps(curve) else {
        return Vec::new();
    };
    let inst = inst_amps(curve);
    let mut probes = Vec::new();
    let lo = (pickup * 1.05).max(0.1);
    let hi = (pickup * 80.0).max(inst.unwrap_or(pickup) * 3.0).max(lo * 10.0);
    for k in 0..48 {
        let t = k as f64 / 47.0;
        probes.push(lo * (hi / lo).powf(t));
    }
    // Probe just either side of each step so it draws as a vertical edge.
    const EPS: f64 = 1e-4;
    let mut steps = Vec::new();
    if let Some(inst) = inst {
        steps.push(inst);
        probes.push(inst * 5.0);
    }
    if let CurveSpec::ThermalMagnetic { st_pickup_a: Some(st), .. } = curve {
        steps.push(*st);
    }
    if let CurveSpec::Definite { pickup_a, .. } = curve {
        // A definite-time element starts at its pickup.
        probes.push(pickup_a * (1.0 + EPS));
    }
    for step in steps {
        if step.is_finite() && step > 0.0 {
            probes.push(step * (1.0 - EPS));
            probes.push(step * (1.0 + EPS));
        }
    }
    probes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    probes.dedup_by(|a, b| (*a - *b).abs() < 1e-9 * b.abs().max(1.0));
    let scale = device_kv / ref_kv;
    probes
        .into_iter()
        .filter_map(|amps| trip_time(curve, amps).map(|time| (amps * scale, time)))
        .filter(|(amp, time)| amp.is_finite() && *amp > 0.0 && time.is_finite() && *time > 0.0)
        .collect()
}

fn pickup_amps(curve: &CurveSpec) -> Option<f64> {
    match curve {
        CurveSpec::Iec { pickup_a, .. } | CurveSpec::Ieee { pickup_a, .. } | CurveSpec::Definite { pickup_a, .. } => Some(*pickup_a),
        CurveSpec::ThermalMagnetic { lt_pickup_a, .. } => Some(*lt_pickup_a),
        CurveSpec::SettingsNotCollected { .. } => None,
    }
}

fn inst_amps(curve: &CurveSpec) -> Option<f64> {
    match curve {
        CurveSpec::Iec { inst_a, .. } | CurveSpec::Ieee { inst_a, .. } => *inst_a,
        CurveSpec::ThermalMagnetic { inst_a, .. } => *inst_a,
        CurveSpec::Definite { .. } | CurveSpec::SettingsNotCollected { .. } => None,
    }
}

pub fn to_svg(plot: &TccPlot) -> String {
    let left = 70.0;
    let top = 56.0;
    let width = 760.0;
    let height = 520.0;
    let right = left + width;
    let bottom = top + height;
    let mut s = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="980" height="680" viewBox="0 0 980 680">
<rect width="100%" height="100%" fill="#f6f4ef"/>
<text x="24" y="30" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="20" font-weight="700" fill="#1c1917">Time-current curve</text>
<text x="230" y="30" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="20" fill="#44403c">{title}</text>
<text x="24" y="50" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="11" fill="#57534e">{note}</text>
<rect x="{left}" y="{top}" width="{width}" height="{height}" fill="#fffdf8" stroke="#d6d3d1"/>
"##,
        title = esc(&plot.title),
        note = esc(&plot.note),
    );
    for amp in decades(plot.i_min, plot.i_max) {
        let x = x_of(plot, amp, left, width);
        s.push_str(&format!(
            r##"<line x1="{x:.1}" y1="{top}" x2="{x:.1}" y2="{bottom}" stroke="#e7e5e4"/>
<text x="{x:.1}" y="{y:.1}" text-anchor="middle" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="10" fill="#78716c">{lab}</text>"##,
            y = bottom + 16.0,
            lab = amp_label(amp),
        ));
    }
    for time in decades(plot.t_min, plot.t_max) {
        let y = y_of(plot, time, top, height);
        s.push_str(&format!(
            r##"<line x1="{left}" y1="{y:.1}" x2="{right}" y2="{y:.1}" stroke="#e7e5e4"/>
<text x="{x:.1}" y="{y:.1}" text-anchor="end" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="10" fill="#78716c">{lab}</text>"##,
            x = left - 8.0,
            lab = time_label(time),
        ));
    }
    s.push_str(&format!(
        r##"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="12" fill="#44403c">Current at {kv:.3} kV (A)</text>"##,
        x = left + width / 2.0,
        y = bottom + 36.0,
        kv = plot.ref_kv,
    ));
    s.push_str(&format!(
        r##"<text x="18" y="{y:.1}" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="12" fill="#44403c" transform="rotate(-90 18 {y:.1})">Time (s)</text>"##,
        y = top + height / 2.0,
    ));
    for marker in &plot.markers {
        if marker.amps < plot.i_min || marker.amps > plot.i_max {
            continue;
        }
        let x = x_of(plot, marker.amps, left, width);
        let dash = if marker.kind == MarkerKind::ArcAssumed { "1 3" } else { "4 3" };
        s.push_str(&format!(
            r##"<line x1="{x:.1}" y1="{top}" x2="{x:.1}" y2="{bottom}" stroke="{c}" stroke-dasharray="{dash}" stroke-width="1.2"/>
<text x="{x:.1}" y="{y:.1}" text-anchor="end" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="10" fill="{c}" transform="rotate(-90 {x:.1} {y:.1})">{lab}</text>"##,
            c = marker.color,
            y = top + 14.0,
            lab = esc(&marker.label),
        ));
    }
    for curve in &plot.curves {
        let d = svg_path(plot, &curve.points, left, top, width, height);
        if !d.is_empty() {
            s.push_str(&format!(
                r##"<path d="{d}" fill="none" stroke="{c}" stroke-width="2.2" stroke-linejoin="round"/>"##,
                c = curve.color
            ));
        }
    }
    let mut ly = 78.0;
    for curve in &plot.curves {
        s.push_str(&format!(
            r##"<line x1="850" y1="{ly:.1}" x2="874" y2="{ly:.1}" stroke="{c}" stroke-width="2.4"/>
<text x="882" y="{y:.1}" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="12" fill="#1c1917">{name}</text>"##,
            c = curve.color,
            y = ly + 4.0,
            name = esc(&curve.name),
        ));
        ly += 22.0;
    }
    s.push_str("</svg>\n");
    s
}

/// Path data for in-range points. A point outside the axes breaks the path,
/// so the next visible point starts a new subpath instead of joining across the gap.
fn svg_path(plot: &TccPlot, points: &[(f64, f64)], left: f64, top: f64, width: f64, height: f64) -> String {
    let mut d = String::new();
    let mut pen_down = false;
    for (amp, time) in points {
        if *amp < plot.i_min || *amp > plot.i_max || *time < plot.t_min || *time > plot.t_max {
            pen_down = false;
            continue;
        }
        let x = x_of(plot, *amp, left, width);
        let y = y_of(plot, *time, top, height);
        if !d.is_empty() {
            d.push(' ');
        }
        d.push_str(&format!("{} {x:.2} {y:.2}", if pen_down { "L" } else { "M" }));
        pen_down = true;
    }
    d
}

pub fn to_csv(plot: &TccPlot) -> String {
    let mut s = format!(
        "device,amps_at_{:.3}_kV,time_s\n",
        plot.ref_kv
    );
    for curve in &plot.curves {
        for (amp, time) in &curve.points {
            s.push_str(&format!("{},{amp:.6},{time:.6}\n", csv_cell(&curve.name)));
        }
    }
    s
}

fn x_of(plot: &TccPlot, amp: f64, left: f64, width: f64) -> f64 {
    let t = (amp.log10() - plot.i_min.log10()) / (plot.i_max.log10() - plot.i_min.log10());
    left + t * width
}

fn y_of(plot: &TccPlot, time: f64, top: f64, height: f64) -> f64 {
    let t = (time.log10() - plot.t_min.log10()) / (plot.t_max.log10() - plot.t_min.log10());
    top + (1.0 - t) * height
}

fn amp_label(amp: f64) -> String {
    if amp >= 1000.0 { format!("{:.0}k", amp / 1000.0) } else { format!("{amp:.0}") }
}

fn time_label(time: f64) -> String {
    if time >= 1.0 { format!("{time:.0}") } else { format!("{time}") }
}

fn esc(value: &str) -> String {
    crate::package::strip_controls(value)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A text CSV cell: control characters removed, formula prefixes neutralized, quoted when needed.
fn csv_cell(value: &str) -> String {
    let value = crate::package::formula_guard(&crate::package::strip_controls(value));
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::study::{self, Studies};

    #[test]
    fn sample_tcc_contains_both_devices() {
        let project = crate::model::Project::sample();
        let study = study::run(&project, Studies::all()).unwrap();
        let plot = plot(&project, Some(&study), Some(0.48), &[]).unwrap();
        let svg = to_svg(&plot);
        let csv = to_csv(&plot);
        assert!(svg.contains("MCC main"), "{svg}");
        assert!(svg.contains("Panel feeder"));
        assert!(svg.contains("<path"));
        assert!(csv.contains("MCC main"));
        assert!(plot.curves.iter().all(|c| c.points.len() > 5));
    }

    #[test]
    fn svg_uses_a_light_palette_and_csv_neutralizes_formulas() {
        let mut project = crate::model::Project::sample();
        project.devices[0].name = "=HYPERLINK(\"evil\")".into();
        project.devices[1].name = "Feeder\u{7}<b>".into();
        let drawn = plot(&project, None, Some(0.48), &[]).unwrap();
        let svg = to_svg(&drawn);
        for dark in ["#E2E8F0", "#F8FAFC", "#00FF66"] {
            assert!(!svg.contains(dark), "{dark} is unreadable on the light SVG background");
        }
        assert!(svg.contains("Feeder&lt;b&gt;"));
        assert!(!svg.chars().any(|c| c < ' ' && c != '\n'));
        let csv = to_csv(&drawn);
        assert!(csv.contains("\"'=HYPERLINK(\"\"evil\"\")\""), "{csv}");
        assert!(!csv.contains('\u{7}'));
    }

    #[test]
    fn device_filter_and_reference_voltage_errors_are_specific() {
        let project = crate::model::Project::sample();
        let err = plot(&project, None, None, &["nope".into()]).unwrap_err();
        assert!(err.contains("no protective device matches") && err.contains("nope"), "{err}");
        let mut lost = project.clone();
        for d in &mut lost.devices { d.bus = "missing".into(); }
        let err = plot(&lost, None, None, &[]).unwrap_err();
        assert!(err.contains("reference voltage"), "{err}");
        let mut empty = project.clone();
        empty.devices.clear();
        assert!(plot(&empty, None, None, &[]).unwrap_err().contains("no protective devices"));
    }

    #[test]
    fn steps_are_vertical_fast_elements_stay_on_axis_and_paths_break_at_gaps() {
        let mut project = crate::model::Project::sample();
        if let CurveSpec::Iec { inst_s, .. } = &mut project.devices[1].curve { *inst_s = Some(0.004); }
        let drawn = plot(&project, None, Some(0.48), &[]).unwrap();
        assert!(drawn.t_min <= 0.004 && drawn.t_min >= T_FLOOR, "t_min {}", drawn.t_min);
        let feeder = &drawn.curves[1];
        assert!(feeder.points.iter().any(|(_, t)| (*t - 0.004).abs() < 1e-12));
        // Each step has points within 0.02% of its pickup on both sides.
        let main = &drawn.curves[0];
        for pickup in [16000.0, 36000.0] {
            let near: Vec<_> = main.points.iter().filter(|(a, _)| (*a / pickup - 1.0).abs() < 2e-4).collect();
            assert!(near.len() >= 2, "step at {pickup}: {near:?}");
            assert!(near.iter().any(|(a, _)| *a < pickup) && near.iter().any(|(a, _)| *a > pickup));
        }
        let gap = TccPlot { title: String::new(), ref_kv: 0.48, i_min: 10.0, i_max: 1000.0, t_min: 0.01, t_max: 100.0,
            curves: Vec::new(), markers: Vec::new(), note: String::new() };
        let d = svg_path(&gap, &[(20.0, 1.0), (30.0, 0.5), (40.0, 0.001), (50.0, 0.2), (60.0, 0.1)], 0.0, 0.0, 100.0, 100.0);
        assert_eq!(d.matches('M').count(), 2, "{d}");
        assert_eq!(d.matches('L').count(), 2, "{d}");
    }

    #[test]
    fn assumed_arc_markers_are_drawn_and_labeled() {
        let mut project = crate::model::Project::sample();
        project.motors.clear();
        let mut study = study::run(&project, Studies::all()).unwrap();
        let rows = study.arc_flash.as_mut().unwrap();
        assert!(!rows.is_empty());
        rows[0].assumed = true;
        let drawn = plot(&project, Some(&study), Some(0.48), &[]).unwrap();
        let assumed: Vec<_> = drawn.markers.iter().filter(|m| m.kind == MarkerKind::ArcAssumed).collect();
        assert_eq!(assumed.len(), 1);
        assert!(assumed[0].label.contains("assumed"));
        assert!(drawn.markers.iter().all(|m| m.amps.is_finite() && m.amps <= drawn.i_max));
        assert!(to_svg(&drawn).contains("arc (assumed)"));
    }
}

/// A bounded enumeration, including when external callers construct an invalid plot.
pub fn decades(lo: f64, hi: f64) -> Vec<f64> {
    if !lo.is_finite() || !hi.is_finite() || lo <= 0.0 || hi <= lo { return Vec::new(); }
    let first = lo.log10().floor() as i32;
    let last = hi.log10().ceil() as i32;
    (first.max(-323)..=last.min(308)).map(|e| 10_f64.powi(e))
        .filter(|d| d.is_finite() && *d >= lo && *d <= hi).collect()
}
