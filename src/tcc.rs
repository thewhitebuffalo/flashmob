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

#[derive(Clone, Debug)]
pub struct TccMarker {
    pub label: String,
    pub amps: f64,
    pub color: &'static str,
}

const COLORS: [&str; 6] = ["#E2E8F0", "#00FF66", "#FFB300", "#94A3B8", "#64748B", "#F8FAFC"];

pub fn plot(project: &Project, study: Option<&StudyOutput>, ref_kv: Option<f64>, only: &[String]) -> Result<TccPlot, String> {
    let devices: Vec<&Device> = project
        .devices
        .iter()
        .filter(|d| only.is_empty() || only.iter().any(|id| id == &d.id || id == &d.name))
        .collect();
    if devices.is_empty() {
        return Err("the project has no protective devices to plot".into());
    }
    let ref_kv = ref_kv.unwrap_or_else(|| {
        devices.iter().filter_map(|d| project.bus(&d.bus).map(|b| b.kv)).fold(f64::MAX, f64::min)
    });
    if ref_kv <= 0.0 {
        return Err("reference voltage must be positive".into());
    }
    let mut curves = Vec::new();
    let mut i_min = f64::MAX;
    let mut i_max: f64 = 100.0;
    for (n, device) in devices.iter().enumerate() {
        let kv = project.bus(&device.bus).map(|b| b.kv).unwrap_or(ref_kv);
        let points = sample_curve(&device.curve, kv, ref_kv);
        if let Some((lo, _)) = points.first() {
            i_min = i_min.min(*lo);
        }
        if let Some((hi, _)) = points.last() {
            i_max = i_max.max(*hi);
        }
        curves.push(TccCurve {
            name: device.name.clone(),
            color: COLORS[n % COLORS.len()],
            points,
        });
    }
    if !i_min.is_finite() {
        i_min = 10.0;
    }
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
                            color: "#44403c",
                        });
                        i_max = i_max.max(amps * 1.2);
                    }
                }
            }
        }
        if let Some(rows) = &study.arc_flash {
            for row in rows {
                if row.assumed {
                    continue;
                }
                let kv = project.bus(&row.bus_id).map(|b| b.kv).unwrap_or(ref_kv);
                let amps = row.arcing_ka * 1000.0 * kv / ref_kv;
                markers.push(TccMarker {
                    label: format!("{} arc", row.bus_name),
                    amps,
                    color: "#c2410c",
                });
            }
        }
    }

    Ok(TccPlot {
        title: project.name.clone(),
        ref_kv,
        i_min,
        i_max,
        t_min: 0.01,
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
    if let Some(inst) = inst {
        probes.push(inst * 0.98);
        probes.push(inst * 1.02);
        probes.push(inst * 5.0);
    }
    probes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    probes.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let scale = device_kv / ref_kv;
    probes
        .into_iter()
        .filter_map(|amps| trip_time(curve, amps).map(|time| (amps * scale, time)))
        .filter(|(_, time)| time.is_finite() && *time > 0.0)
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
    let decades = |lo: f64, hi: f64| {
        let mut v = Vec::new();
        let mut d = 10_f64.powf(lo.log10().floor());
        while d <= hi * 1.001 {
            if d >= lo * 0.999 {
                v.push(d);
            }
            d *= 10.0;
        }
        v
    };
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
        s.push_str(&format!(
            r##"<line x1="{x:.1}" y1="{top}" x2="{x:.1}" y2="{bottom}" stroke="{c}" stroke-dasharray="4 3" stroke-width="1.2"/>
<text x="{x:.1}" y="{y:.1}" text-anchor="end" font-family="ui-sans-serif, Helvetica, sans-serif" font-size="10" fill="{c}" transform="rotate(-90 {x:.1} {y:.1})">{lab}</text>"##,
            c = marker.color,
            y = top + 14.0,
            lab = esc(&marker.label),
        ));
    }
    for curve in &plot.curves {
        let mut d = String::new();
        for (amp, time) in &curve.points {
            if *amp < plot.i_min || *amp > plot.i_max || *time < plot.t_min || *time > plot.t_max {
                continue;
            }
            let x = x_of(plot, *amp, left, width);
            let y = y_of(plot, *time, top, height);
            if d.is_empty() {
                d.push_str(&format!("M {x:.2} {y:.2}"));
            } else {
                d.push_str(&format!(" L {x:.2} {y:.2}"));
            }
        }
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
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
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
}
