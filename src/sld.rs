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

const COL: f64 = 320.0;
const ROW: f64 = 250.0;

pub fn diagram(project: &Project, study: &StudyOutput) -> Diagram {
    let layers = layers(project);
    let mut buses = Vec::new();
    let mut max_x: f64 = 240.0;
    let mut max_y: f64 = 180.0;
    for (col, layer) in layers.iter().enumerate() {
        for (row, &idx) in layer.iter().enumerate() {
            let bus = &project.buses[idx];
            let x = 180.0 + col as f64 * COL;
            let y = 150.0 + row as f64 * ROW;
            let arcs = study
                .arc_flash
                .as_ref()
                .map(|rows| notes_for(rows, &bus.id))
                .unwrap_or_default();
            let card_h = 112.0 + arcs.len() as f64 * 30.0;
            let fault = study.fault.as_ref().and_then(|f| f.buses.iter().find(|b| b.id == bus.id));
            let flow = study.loadflow.as_ref().and_then(|lf| lf.buses.iter().find(|b| b.id == bus.id));
            let v_pu = flow.map(|b| b.v_pu);
            let angle_deg = flow.map(|b| b.angle_deg);
            let source = project.sources.iter().any(|s| s.bus == bus.id);
            max_x = max_x.max(x + 180.0);
            max_y = max_y.max(y + card_h + 40.0);
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
            x1: a.x + 70.0,
            y1: a.y,
            x2: b.x - 70.0,
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

fn layers(project: &Project) -> Vec<Vec<usize>> {
    let n = project.buses.len();
    if n == 0 {
        return Vec::new();
    }
    let mut index = std::collections::HashMap::new();
    for (i, bus) in project.buses.iter().enumerate() {
        index.insert(bus.id.as_str(), i);
    }
    let mut adj = vec![Vec::new(); n];
    for branch in &project.branches {
        if let (Some(&a), Some(&b)) = (index.get(branch.from.as_str()), index.get(branch.to.as_str())) {
            adj[a].push(b);
            adj[b].push(a);
        }
    }
    let root = project
        .sources
        .iter()
        .find(|s| s.is_slack)
        .and_then(|s| index.get(s.bus.as_str()).copied())
        .unwrap_or(0);
    let mut depth = vec![None; n];
    let mut queue = std::collections::VecDeque::from([root]);
    depth[root] = Some(0);
    while let Some(i) = queue.pop_front() {
        let d = depth[i].unwrap();
        for &j in &adj[i] {
            if depth[j].is_none() {
                depth[j] = Some(d + 1);
                queue.push_back(j);
            }
        }
    }
    let max_d = depth.iter().filter_map(|d| *d).max().unwrap_or(0);
    let mut layers = vec![Vec::new(); max_d + 1];
    let mut orphans = Vec::new();
    for i in 0..n {
        match depth[i] {
            Some(d) => layers[d].push(i),
            None => orphans.push(i),
        }
    }
    if !orphans.is_empty() {
        layers.push(orphans);
    }
    layers
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
        let mid_x = (branch.x1 + branch.x2) / 2.0;
        let mid_y = (branch.y1 + branch.y2) / 2.0;
        s.push_str(&format!(
            r##"<polyline class="fm-branch-line" points="{x1:.1},{y1:.1} {mx:.1},{y1:.1} {mx:.1},{y2:.1} {x2:.1},{y2:.1}"/>"##,
            x1 = branch.x1,
            y1 = branch.y1,
            mx = mid_x,
            y2 = branch.y2,
            x2 = branch.x2,
        ));
        if branch.protector == "breaker" {
            s.push_str(&breaker_symbol(branch.x1 + 18.0, branch.y1));
        } else if branch.protector == "switch" {
            s.push_str(&switch_symbol(branch.x1 + 18.0, branch.y1));
        }
        if branch.transformer {
            s.push_str(&transformer_symbol(mid_x, mid_y));
        }
        let label = short_label(&branch.name);
        s.push_str(&format!(
            r##"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" class="fm-text-dat">{name}</text>"##,
            x = mid_x + 14.0,
            y = mid_y,
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
        body.push_str(&utility_symbol(x - 96.0, y));
        body.push_str(&format!(
            r##"<line x1="{x1:.1}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" class="fm-branch-line"/>"##,
            x1 = x - 80.0,
            x2 = x - 70.0,
        ));
    }
    if focal {
        body.push_str(&format!(
            r##"<circle cx="{x:.1}" cy="{y:.1}" r="8" class="fm-focal-node"/><circle cx="{x:.1}" cy="{y:.1}" r="14" class="fm-focal-pulse"/>"##,
        ));
    }
    body.push_str(&format!(
        r##"<line x1="{x1:.1}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" class="fm-bus-bar"/>"##,
        x1 = x - 70.0,
        x2 = x + 70.0,
    ));
    let top = y + 18.0;
    let left = x - 84.0;
    let box_class = if focal { "fm-focal-node" } else { "fm-border" };
    body.push_str(&format!(
        r##"<rect x="{left:.1}" y="{top:.1}" width="188" height="{h:.1}" class="{box_class}"/>"##,
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
    }
}
