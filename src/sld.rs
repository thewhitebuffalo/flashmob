use crate::model::Project;
use crate::study::{ArcRow, StudyOutput};

#[derive(Clone, Debug)]
pub struct Diagram {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub buses: Vec<BusGlyph>,
    pub branches: Vec<BranchGlyph>,
}

#[derive(Clone, Debug)]
pub struct BusGlyph {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub kv: f64,
    pub v_pu: Option<f64>,
    pub angle_deg: Option<f64>,
    pub fault_3p_ka: Option<f64>,
    pub fault_lg_ka: Option<f64>,
    pub arcs: Vec<ArcNote>,
    pub arc_failures: Vec<String>,
    pub card_h: f64,
    pub source: bool,
}

#[derive(Clone, Debug)]
pub struct ArcNote {
    pub name: String,
    pub cal_cm2: f64,
    pub afb_in: f64,
    pub time_s: f64,
    pub assumed: bool,
}

#[derive(Clone, Debug)]
pub struct BranchGlyph {
    pub id: String,
    pub name: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub transformer: bool,
    /// SKM one-line mark on the feeder: breaker square, fused switch, or none.
    pub protector: String,
}

const PITCH_X: f64 = 420.0;

pub fn diagram(project: &Project, study: &StudyOutput) -> Diagram {
    let placed = place_by_voltage(project, study);
    let mut buses = Vec::new();
    let mut max_x: f64 = 480.0;
    let mut max_y: f64 = 180.0;
    for idx in 0..project.buses.len() {
        let bus = &project.buses[idx];
        let (x, y) = placed[idx];
        {
            let arcs = study
                .arc_flash
                .as_ref()
                .map(|rows| notes_for(rows, &bus.id))
                .unwrap_or_default();
            let arc_failures: Vec<String> = study.arc_flash_failures.iter().filter(|f| f.bus_id == bus.id).map(|f| format!("{}: {}", f.name, f.error)).collect();
            let card_h = 112.0 + arcs.len() as f64 * 30.0 + if arc_failures.is_empty() { 0.0 } else { 20.0 };
            let fault = study.fault.as_ref().and_then(|f| f.buses.iter().find(|b| b.id == bus.id));
            let flow = study.loadflow.as_ref().filter(|lf| lf.converged).and_then(|lf| lf.buses.iter().find(|b| b.id == bus.id));
            let v_pu = flow.map(|b| b.v_pu);
            let angle_deg = flow.map(|b| b.angle_deg);
            let source = project.sources.iter().any(|s| s.bus == bus.id);
            max_x = max_x.max(x + 320.0);
            max_y = max_y.max(y + card_h + 24.0);
            buses.push(BusGlyph {
                id: bus.id.clone(),
                name: if bus.name.is_empty() { bus.id.clone() } else { bus.name.clone() },
                x,
                y,
                kv: bus.kv,
                v_pu,
                angle_deg,
                fault_3p_ka: fault.and_then(|b| b.three_phase.as_ref().map(|p| p.symmetrical_ka)),
                fault_lg_ka: fault.and_then(|b| b.line_to_ground.as_ref().map(|p| p.symmetrical_ka)),
                arcs,
                arc_failures,
                card_h,
                source,
            });
        }
    }
    let mut branches = Vec::new();
    for branch in &project.branches {
        let Some(a) = buses.iter().find(|b| b.id == branch.from) else { continue };
        let Some(b) = buses.iter().find(|b| b.id == branch.to) else { continue };
        let transformer = matches!(branch.kind, crate::model::BranchKind::Transformer { .. });
        let protector = protector_on(project, &branch.to);
        branches.push(BranchGlyph {
            id: branch.id.clone(),
            name: branch.name.clone(),
            x1: a.x,
            y1: a.y,
            x2: b.x,
            y2: b.y,
            transformer,
            protector,
        });
    }
    Diagram {
        title: project.name.clone(),
        width: max_x + 40.0,
        height: max_y + 30.0,
        buses,
        branches,
    }
}

fn notes_for(rows: &[ArcRow], bus: &str) -> Vec<ArcNote> {
    rows.iter()
        .filter(|r| r.bus_id == bus)
        .map(|r| ArcNote {
            name: r.name.clone(),
            cal_cm2: r.governing_cal_cm2,
            afb_in: r.afb_in,
            time_s: if r.governing == "reduced_arcing" { r.time_min_s } else { r.time_s },
            assumed: r.assumed,
        })
        .collect()
}

/// Highest nominal voltage on the top row, lowest on the bottom.
/// Buses on one voltage share a row and sit under the bus that feeds them.
fn place_by_voltage(project: &Project, study: &StudyOutput) -> Vec<(f64, f64)> {
    let n = project.buses.len();
    let mut at = vec![(260.0, 100.0); n];
    if n == 0 {
        return at;
    }
    let mut rows: Vec<(i64, Vec<usize>)> = Vec::new();
    for (i, bus) in project.buses.iter().enumerate() {
        let key = (bus.kv * 1000.0).round() as i64;
        if let Some(row) = rows.iter_mut().find(|(k, _)| *k == key) {
            row.1.push(i);
        } else {
            rows.push((key, vec![i]));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let mut upstream = vec![None; n];
    let mut index = std::collections::HashMap::new();
    for (i, bus) in project.buses.iter().enumerate() {
        index.insert(bus.id.as_str(), i);
    }
    for branch in &project.branches {
        if let (Some(&from), Some(&to)) = (index.get(branch.from.as_str()), index.get(branch.to.as_str())) {
            if project.buses[from].kv + 1e-6 >= project.buses[to].kv {
                upstream[to] = Some(from);
            }
        }
    }
    let mut y = 100.0;
    for (_, members) in &rows {
        let mut order = members.clone();
        order.sort_by(|&a, &b| {
            let ax = upstream[a].map(|u| at[u].0).unwrap_or(0.0);
            let bx = upstream[b].map(|u| at[u].0).unwrap_or(0.0);
            ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
        });
        let mut raw = Vec::with_capacity(order.len());
        let mut cursor = 0;
        while cursor < order.len() {
            let parent = upstream[order[cursor]];
            let mut end = cursor + 1;
            while end < order.len() && upstream[order[end]] == parent {
                end += 1;
            }
            let count = (end - cursor) as f64;
            let center = parent.map(|p| at[p].0).unwrap_or(260.0 + (cursor as f64) * PITCH_X);
            let start = center - (count - 1.0) * PITCH_X / 2.0;
            for k in 0..(end - cursor) {
                raw.push(start + k as f64 * PITCH_X);
            }
            cursor = end;
        }
        for i in 1..raw.len() {
            if raw[i] < raw[i - 1] + PITCH_X {
                raw[i] = raw[i - 1] + PITCH_X;
            }
        }
        let shift = if raw.first().copied().unwrap_or(260.0) < 260.0 {
            260.0 - raw[0]
        } else {
            0.0
        };
        let mut row_card: f64 = 80.0;
        for (k, &idx) in order.iter().enumerate() {
            let arcs = study.arc_flash.as_ref().map(|rows| rows.iter().filter(|r| r.bus_id == project.buses[idx].id).count()).unwrap_or(0);
            row_card = row_card.max(112.0 + arcs as f64 * 30.0);
            at[idx] = (raw[k] + shift, y);
        }
        y += row_card + 80.0;
    }
    at
}

impl BranchGlyph {
    /// Orthogonal feeder, dropping from the upper bus toward the lower one.
    pub fn points(&self) -> Vec<(f64, f64)> {
        if (self.y1 - self.y2).abs() < 2.0 {
            let bridge = self.y1 - 36.0;
            vec![(self.x1, self.y1), (self.x1, bridge), (self.x2, bridge), (self.x2, self.y2)]
        } else {
            let mid_y = (self.y1 + self.y2) / 2.0;
            vec![(self.x1, self.y1), (self.x1, mid_y), (self.x2, mid_y), (self.x2, self.y2)]
        }
    }

    pub fn device_at(&self) -> (f64, f64) {
        if (self.y1 - self.y2).abs() < 2.0 {
            ((self.x1 + self.x2) / 2.0, self.y1 - 36.0)
        } else if self.y2 >= self.y1 {
            (self.x1, self.y1 + 26.0)
        } else {
            (self.x1, self.y1 - 26.0)
        }
    }

    pub fn winding_at(&self) -> (f64, f64) {
        if (self.y1 - self.y2).abs() < 2.0 {
            ((self.x1 + self.x2) / 2.0, self.y1 - 28.0)
        } else {
            let mid_y = (self.y1 + self.y2) / 2.0;
            (self.x1, (self.y1 + mid_y) / 2.0)
        }
    }
}

pub fn to_svg(diagram: &Diagram) -> String {
    let focal = focal_bus(diagram);
    let mut s = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.1} {h:.1}" style="background-color:#0A0C10">
<style>
.fm-grid {{ stroke:#334155; stroke-width:1; stroke-dasharray:4 6; opacity:0.7; }}
.fm-border {{ stroke:#334155; stroke-width:1.5; fill:none; }}
.fm-bus-bar {{ stroke:#E2E8F0; stroke-width:4; stroke-linecap:square; }}
.fm-branch-line {{ stroke:#E2E8F0; stroke-width:2; fill:none; stroke-linejoin:round; }}
.fm-winding {{ stroke:#E2E8F0; stroke-width:1.5; fill:none; }}
.fm-text-lbl {{ fill:#E2E8F0; font-family:'JetBrains Mono','Fira Code','SF Mono',monospace; font-size:13px; font-weight:600; letter-spacing:0.5px; }}
.fm-text-dat {{ fill:#94A3B8; font-family:'JetBrains Mono','Fira Code','SF Mono',monospace; font-size:11px; }}
.fm-focal-node {{ stroke:#00FF66; stroke-width:2.5; fill:#0A0C10; }}
.fm-focal-pulse {{ stroke:#00FF66; stroke-width:1.5; stroke-dasharray:2 2; fill:none; }}
.fm-focal-text {{ fill:#00FF66; font-family:'JetBrains Mono','Fira Code','SF Mono',monospace; font-size:12px; font-weight:700; }}
.fm-warn {{ fill:#FFB300; font-family:'JetBrains Mono','Fira Code','SF Mono',monospace; font-size:11px; font-weight:700; }}
</style>
<rect width="100%" height="100%" fill="#0A0C10"/>
{grid}
<rect x="1" y="1" width="{inner_w:.1}" height="{inner_h:.1}" class="fm-border"/>
<text x="24" y="28" class="fm-text-lbl">FLASHMOB</text>
<text x="140" y="28" class="fm-text-dat">{title}</text>
<text x="24" y="46" class="fm-text-dat">ONE-LINE  IEEE 1584-2018  SYMMETRICAL RMS</text>
"##,
        w = diagram.width,
        h = diagram.height,
        inner_w = diagram.width - 2.0,
        inner_h = diagram.height - 2.0,
        grid = grid(diagram.width, diagram.height),
        title = esc(&diagram.title.to_uppercase()),
    );
    for branch in &diagram.branches {
        let pts = branch.points();
        let coords: Vec<String> = pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        s.push_str(&format!(r##"<polyline class="fm-branch-line" points="{}"/>"##, coords.join(" ")));
        let (dx, dy) = branch.device_at();
        if branch.protector == "breaker" {
            s.push_str(&breaker_symbol(dx, dy));
        } else if branch.protector == "switch" {
            s.push_str(&switch_symbol(dx, dy));
        }
        let (wx, wy) = branch.winding_at();
        if branch.transformer {
            s.push_str(&transformer_symbol(wx, wy));
        }
        let label = short_label(&branch.name);
        s.push_str(&format!(
            r##"<text x="{x:.1}" y="{y:.1}" class="fm-text-dat">{name}</text>"##,
            x = wx + 20.0,
            y = wy,
            name = esc(&label.to_uppercase()),
        ));
    }
    for bus in &diagram.buses {
        s.push_str(&bus_svg(bus, focal.as_deref() == Some(&bus.id)));
    }
    s.push_str("</svg>\n");
    s
}

fn protector_on(project: &Project, bus: &str) -> String {
    let mut breaker = false;
    let mut switch = false;
    for device in project.devices.iter().filter(|d| d.bus == bus) {
        match &device.curve {
            crate::model::CurveSpec::ThermalMagnetic { .. } => breaker = true,
            crate::model::CurveSpec::SettingsNotCollected { .. } => switch = true,
            _ => breaker = true,
        }
    }
    if breaker {
        "breaker".into()
    } else if switch {
        "switch".into()
    } else {
        "none".into()
    }
}

fn breaker_symbol(x: f64, y: f64) -> String {
    format!(
        r##"<g data-symbol="breaker"><rect x="{x:.1}" y="{y:.1}" width="14" height="14" fill="#0A0C10" stroke="#E2E8F0" stroke-width="1.4"/><line x1="{x1:.1}" y1="{y2:.1}" x2="{x2:.1}" y2="{y1:.1}" stroke="#E2E8F0" stroke-width="1.2"/></g>"##,
        x = x - 7.0,
        y = y - 7.0,
        x1 = x - 5.0,
        y1 = y - 5.0,
        x2 = x + 5.0,
        y2 = y + 5.0,
    )
}

fn switch_symbol(x: f64, y: f64) -> String {
    format!(
        r##"<g data-symbol="fused-switch"><rect x="{x:.1}" y="{y:.1}" width="16" height="10" fill="#0A0C10" stroke="#E2E8F0" stroke-width="1.3"/><line x1="{x1:.1}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" stroke="#E2E8F0" stroke-width="1.2"/></g>"##,
        x = x - 8.0,
        y = y - 5.0,
        x1 = x - 8.0,
        x2 = x + 8.0,
    )
}

fn transformer_symbol(x: f64, y: f64) -> String {
    format!(
        r##"<g data-symbol="transformer"><circle cx="{x:.1}" cy="{y1:.1}" r="12" class="fm-winding"/><circle cx="{x:.1}" cy="{y2:.1}" r="12" class="fm-winding"/></g>"##,
        y1 = y - 10.0,
        y2 = y + 10.0,
    )
}

fn utility_symbol(x: f64, y: f64) -> String {
    format!(
        r##"<g data-symbol="utility"><circle cx="{x:.1}" cy="{y:.1}" r="16" class="fm-winding"/><path d="M{x0:.1},{y:.1} q 4,-7 8,0 t 8,0 t 8,0" fill="none" stroke="#E2E8F0" stroke-width="1.3"/></g>"##,
        x0 = x - 12.0,
    )
}

fn grid(width: f64, height: f64) -> String {
    let mut s = String::from(r#"<g id="alignment-grid">"#);
    let mut y = 40.0;
    while y < height {
        s.push_str(&format!(r#"<line x1="0" y1="{y:.1}" x2="{width:.1}" y2="{y:.1}" class="fm-grid"/>"#));
        y += 40.0;
    }
    let mut x = 40.0;
    while x < width {
        s.push_str(&format!(r#"<line x1="{x:.1}" y1="0" x2="{x:.1}" y2="{height:.1}" class="fm-grid"/>"#));
        x += 40.0;
    }
    s.push_str("</g>");
    s
}

fn focal_bus(diagram: &Diagram) -> Option<String> {
    diagram
        .buses
        .iter()
        .filter_map(|bus| bus.arcs.iter().map(|arc| arc.cal_cm2).reduce(f64::max).map(|cal| (cal, bus.id.clone())))
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, id)| id)
}

fn short_label(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.chars().count() <= 46 {
        trimmed.to_string()
    } else {
        let mut end = 46;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &trimmed[..end])
    }
}

fn bus_svg(bus: &BusGlyph, focal: bool) -> String {
    let x = bus.x;
    let y = bus.y;
    let mut body = String::new();
    if bus.source {
        body.push_str(&utility_symbol(x - 112.0, y));
        body.push_str(&format!(
            r##"<line x1="{x1:.1}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" class="fm-branch-line"/>"##,
            x1 = x - 96.0,
            x2 = x - 80.0,
        ));
    }
    if focal {
        body.push_str(&format!(
            r##"<circle cx="{x:.1}" cy="{y:.1}" r="8" class="fm-focal-node"/><circle cx="{x:.1}" cy="{y:.1}" r="14" class="fm-focal-pulse"/>"##,
        ));
    }
    body.push_str(&format!(
        r##"<line x1="{x1:.1}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" class="fm-bus-bar"/>"##,
        x1 = x - 80.0,
        x2 = x + 80.0,
    ));
    let top = y - 16.0;
    let left = x + 96.0;
    let box_class = if focal { "fm-focal-node" } else { "fm-border" };
    body.push_str(&format!(
        r##"<rect x="{left:.1}" y="{top:.1}" width="210" height="{h:.1}" class="{box_class}"/>"##,
        h = bus.card_h,
    ));
    let mut ty = top + 16.0;
    let name_class = if focal { "fm-focal-text" } else { "fm-text-lbl" };
    body.push_str(&format!(
        r##"<text x="{x:.1}" y="{ty:.1}" class="{name_class}">{name}</text>"##,
        x = left + 8.0,
        name = esc(&tag(&bus.name)),
    ));
    ty += 16.0;
    if focal {
        body.push_str(&format!(
            r##"<text x="{x:.1}" y="{ty:.1}" class="fm-focal-text">&gt;&gt; FAULT LOCUS</text>"##,
            x = left + 8.0,
        ));
        ty += 14.0;
    }
    let volts = if bus.kv >= 1.0 { format!("{:.2} KV", bus.kv) } else { format!("{:.0} V", bus.kv * 1000.0) };
    let vpu = bus.v_pu.map(|v| format!("{v:.3}")).unwrap_or_else(|| "--".into());
    let ang = bus.angle_deg.map(|v| format!("{v:.2}")).unwrap_or_else(|| "--".into());
    body.push_str(&dat(left + 8.0, ty, &format!("V: {vpu} PU")));
    ty += 14.0;
    body.push_str(&dat(left + 8.0, ty, &format!("ANG: {ang} DEG")));
    ty += 14.0;
    body.push_str(&dat(left + 8.0, ty, &format!("{volts}")));
    ty += 14.0;
    body.push_str(&dat(left + 8.0, ty, &format!("I_SC: {}", ka(bus.fault_3p_ka))));
    ty += 14.0;
    body.push_str(&dat(left + 8.0, ty, &format!("I_LG: {}", ka(bus.fault_lg_ka))));
    for arc in &bus.arcs {
        ty += 14.0;
        let cls = if arc.cal_cm2 >= 8.0 { "fm-warn" } else { "fm-text-dat" };
        body.push_str(&format!(
            r##"<text x="{x:.1}" y="{ty:.1}" class="{cls}">{:.1} CAL/CM2</text>"##,
            arc.cal_cm2,
            x = left + 8.0,
        ));
        ty += 14.0;
        body.push_str(&dat(
            left + 8.0,
            ty,
            &format!("AFB {:.0} IN  {:.3} S{}", arc.afb_in, arc.time_s, if arc.assumed { "  ASSUMED" } else { "" }),
        ));
    }
    if !bus.arc_failures.is_empty() {
        ty += 18.0;
        body.push_str(&format!(r##"<g><title>{}</title>{}</g>"##,
            esc(&bus.arc_failures.join("; ")), dat(left + 8.0, ty, "ARC FLASH FAILED")));
    }
    body
}

fn dat(x: f64, y: f64, value: &str) -> String {
    format!(r##"<text x="{x:.1}" y="{y:.1}" class="fm-text-dat">{}</text>"##, esc(value))
}

fn tag(name: &str) -> String {
    name.trim().to_uppercase().replace(' ', "_")
}

fn ka(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.2} kA"),
        None => "—".into(),
    }
}

pub fn energy_color(cal: f64) -> &'static str {
    if cal < 1.2 {
        "#94A3B8"
    } else if cal < 8.0 {
        "#E2E8F0"
    } else {
        "#FFB300"
    }
}

fn esc(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::study::{self, Studies};

    #[test]
    fn sample_sld_shows_fault_and_arc_flash_on_each_bus() {
        let project = crate::model::Project::sample();
        let study = study::run(&project, Studies::all()).unwrap();
        let svg = to_svg(&diagram(&project, &study));
        assert!(svg.contains("MCC-1"), "{svg}");
        assert!(svg.contains("kA"));
        assert!(svg.contains("CAL/CM2"));
        assert!(svg.contains("fm-bus-bar"));
        assert!(svg.contains("#0A0C10"));
        assert!(svg.contains("IEEE 1584-2018"));
        assert!(svg.contains("<svg"));
        assert!(svg.contains("data-symbol=\"transformer\""));
        assert!(svg.contains("data-symbol=\"breaker\""));
        assert!(svg.contains("data-symbol=\"utility\""));
        let drawing = diagram(&project, &study);
        let util = drawing.buses.iter().find(|b| b.id == "util").unwrap();
        let mcc = drawing.buses.iter().find(|b| b.id == "mcc").unwrap();
        assert!(util.kv > mcc.kv);
        assert!(util.y < mcc.y, "utility y {} should be above mcc y {}", util.y, mcc.y);
    }
}
