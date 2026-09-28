use crate::model::Project;
use crate::study::{ArcRow, StudyOutput};

#[derive(Clone, Debug)]
pub struct Diagram {
    pub title: String,
    pub has_assumptions: bool,
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
    pub protector_at_source: bool,
    /// Routed separately from symbols so folded feeder groups stay connected.
    pub route: Vec<(f64, f64)>,
}

pub fn diagram(project: &Project, study: &StudyOutput) -> Diagram {
    let layout = place_by_topology(project, study);
    let placed = &layout.positions;
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
    for (branch_idx, branch) in project.branches.iter().enumerate() {
        let Some(a) = buses.iter().find(|b| b.id == branch.from) else { continue };
        let Some(b) = buses.iter().find(|b| b.id == branch.to) else { continue };
        let transformer = matches!(branch.kind, crate::model::BranchKind::Transformer { .. });
        let protector = protector_on(project, &branch.id);
        branches.push(BranchGlyph {
            id: branch.id.clone(),
            name: branch.name.clone(),
            x1: a.x,
            y1: a.y,
            x2: b.x,
            y2: b.y,
            transformer,
            protector,
            protector_at_source: project.devices.iter().any(|d| d.protected_branch.as_deref() == Some(&branch.id) && d.bus == branch.from),
            route: layout.routes[branch_idx].clone(),
        });
    }
    for branch in &branches {
        for &(x, y) in &branch.route {
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    Diagram {
        title: project.name.clone(),
        has_assumptions: !project.assumptions.is_empty(),
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

/// Build a deterministic spanning forest. A common incoming chain stays at the
/// top; downstream feeder subtrees fold into at most three vertical columns.
/// Voltage alone cannot express feeder hierarchy (most LV buses share a voltage).
struct Layout {
    positions: Vec<(f64, f64)>,
    routes: Vec<Vec<(f64, f64)>>,
}

fn place_by_topology(project: &Project, study: &StudyOutput) -> Layout {
    let n = project.buses.len();
    let mut positions = vec![(260.0, 100.0); n];
    let index: std::collections::HashMap<&str, usize> = project.buses.iter()
        .enumerate().map(|(i, b)| (b.id.as_str(), i)).collect();
    let mut outgoing = vec![Vec::new(); n];
    let mut indegree = vec![0; n];
    for (edge, b) in project.branches.iter().enumerate() {
        if let (Some(&a), Some(&z)) = (index.get(b.from.as_str()), index.get(b.to.as_str())) {
            outgoing[a].push((z, edge));
            indegree[z] += 1;
        }
    }
    let mut candidates = Vec::new();
    for s in &project.sources {
        if let Some(&i) = index.get(s.bus.as_str()) { candidates.push(i); }
    }
    candidates.extend((0..n).filter(|&i| indegree[i] == 0));
    candidates.extend(0..n);
    let mut visited = vec![false; n];
    let mut children = vec![Vec::new(); n];
    let mut tree_edges = std::collections::HashSet::new();
    let mut roots = Vec::new();
    for root in candidates {
        if visited[root] { continue; }
        roots.push(root);
        visited[root] = true;
        let mut queue = std::collections::VecDeque::from([root]);
        while let Some(a) = queue.pop_front() {
            for &(z, edge) in &outgoing[a] {
                if !visited[z] {
                    visited[z] = true;
                    children[a].push(z);
                    tree_edges.insert(edge);
                    queue.push_back(z);
                }
            }
        }
    }
    let max_arcs = project.buses.iter().map(|b| study.arc_flash.as_ref()
        .map_or(0, |rows| rows.iter().filter(|r| r.bus_id == b.id).count())).max().unwrap_or(0);
    // Every row includes enough room for the tallest result card and feeder label.
    let pitch = (112.0 + max_arcs as f64 * 30.0 + 120.0).max(240.0);
    let mut groups = Vec::new();
    let mut group_roots = std::collections::HashSet::new();
    let mut top_rows = 0;
    if roots.len() == 1 {
        let mut a = roots[0];
        loop {
            positions[a] = (260.0, 100.0 + top_rows as f64 * pitch);
            top_rows += 1;
            if children[a].len() == 1 { a = children[a][0]; }
            else { groups.extend(children[a].iter().copied()); break; }
        }
    } else { groups = roots; }
    let mut subtrees = Vec::new();
    let mut max_depth = 0;
    for root in groups {
        group_roots.insert(root);
        let mut nodes = Vec::new();
        let mut stack = vec![(root, 0)];
        while let Some((a, depth)) = stack.pop() {
            nodes.push((a, depth));
            max_depth = max_depth.max(depth);
            stack.extend(children[a].iter().rev().map(|&z| (z, depth + 1)));
        }
        subtrees.push(nodes);
    }
    // Large subtrees first gives stable, balanced sheets without widening a star.
    subtrees.sort_by_key(|nodes| std::cmp::Reverse(nodes.len()));
    let columns = subtrees.len().clamp(1, 3);
    let lane_width = max_depth as f64 * 140.0 + 540.0;
    let mut heights = vec![top_rows; columns];
    let mut gutters = vec![150.0; n];
    for nodes in subtrees {
        let col = (0..columns).min_by_key(|&c| heights[c]).unwrap();
        let base = 260.0 + col as f64 * lane_width;
        for (a, depth) in nodes {
            positions[a] = (base + depth as f64 * 140.0, 100.0 + heights[col] as f64 * pitch);
            gutters[a] = base - 110.0;
            heights[col] += 1;
        }
        heights[col] += 1;
    }
    let routes = project.branches.iter().enumerate().map(|(edge, b)| {
        let (Some(&a), Some(&z)) = (index.get(b.from.as_str()), index.get(b.to.as_str())) else { return Vec::new() };
        let (ax, ay) = positions[a];
        let (zx, zy) = positions[z];
        if !tree_edges.contains(&edge) {
            // Ties and cycles use the outside margin; never overwrite a tree parent.
            vec![(ax, ay), (ax, ay + pitch - 60.0), (40.0, ay + pitch - 60.0),
                 (40.0, zy - 90.0), (zx, zy - 90.0), (zx, zy)]
        } else if group_roots.contains(&z) {
            vec![(ax, ay), (ax, ay + pitch - 90.0), (gutters[z], ay + pitch - 90.0),
                 (gutters[z], zy - 90.0), (zx, zy - 90.0), (zx, zy)]
        } else {
            vec![(ax, ay), (ax, zy - 90.0), (zx, zy - 90.0), (zx, zy)]
        }
    }).collect();
    Layout { positions, routes }
}

impl BranchGlyph {
    pub fn points(&self) -> Vec<(f64, f64)> { self.route.clone() }

    pub fn device_at(&self) -> (f64, f64) {
        if self.protector_at_source { (self.x1, self.y1 + 26.0) }
        else { (self.x2, self.y2 - 16.0) }
    }

    pub fn winding_at(&self) -> (f64, f64) { (self.x2, self.y2 - 60.0) }
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
<text x="24" y="46" class="fm-text-dat">ONE-LINE  IEEE 1584-2018  SYMMETRICAL RMS{basis}</text>
"##,
        basis = if diagram.has_assumptions { "  |  PRELIMINARY - ASSUMPTIONS APPLY" } else { "" },
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

fn protector_on(project: &Project, branch: &str) -> String {
    let mut breaker = false;
    for device in project.devices.iter().filter(|d| d.protected_branch.as_deref() == Some(branch)) {
        match &device.curve {
            crate::model::CurveSpec::ThermalMagnetic { .. } => breaker = true,
            crate::model::CurveSpec::SettingsNotCollected { .. } => {},
            _ => breaker = true,
        }
    }
    if breaker {
        "breaker".into()
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
    if trimmed.chars().count() <= 26 {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(26).collect::<String>())
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
        name = esc(&tag(&bus.name).chars().take(23).collect::<String>()),
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
    fn wide_same_voltage_system_folds_without_overlapping_cards() {
        let mut p = Project::default();
        for i in 0..41 {
            p.buses.push(crate::model::Bus { id: format!("b{i}"), name: format!("Bus {i}"), kv: 0.48,
                shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None });
            if i > 0 {
                p.branches.push(crate::model::Branch { id: format!("f{i}"), name: format!("Feeder {i}"),
                    from: "b0".into(), to: format!("b{i}"),
                    kind: crate::model::BranchKind::Line { r_ohm: 0.01, x_ohm: 0.01,
                        b_siemens: 0.0, r0_ohm: 0.03, x0_ohm: 0.03, ampacity_a: None } });
            }
        }
        let study = study::run(&Project::sample(), Studies::all()).unwrap();
        let d = diagram(&p, &study);
        assert!(d.width < 2000.0, "a star must not become a 40-column strip");
        assert_eq!(d.buses.len(), 41);
        for (i, a) in d.buses.iter().enumerate() {
            for b in &d.buses[i + 1..] {
                assert!((a.x - b.x).abs() >= 400.0 || (a.y - b.y).abs() >= a.card_h.max(b.card_h) + 40.0);
            }
        }
        for edge in &d.branches {
            assert!(edge.y2 > edge.y1, "same-voltage child must be below its parent");
            assert_eq!(edge.points().first(), Some(&(edge.x1, edge.y1)));
            assert_eq!(edge.points().last(), Some(&(edge.x2, edge.y2)));
            for segment in edge.points().windows(2) {
                assert!(segment[0].0 == segment[1].0 || segment[0].1 == segment[1].1);
                // No feeder may run through an unrelated result card.
                for bus in &d.buses {
                    let left = bus.x + 96.0;
                    let right = left + 210.0;
                    let top = bus.y - 16.0;
                    let bottom = top + bus.card_h;
                    let lo_x = segment[0].0.min(segment[1].0);
                    let hi_x = segment[0].0.max(segment[1].0);
                    let lo_y = segment[0].1.min(segment[1].1);
                    let hi_y = segment[0].1.max(segment[1].1);
                    assert!(!(hi_x > left && lo_x < right && hi_y > top && lo_y < bottom));
                }
            }
        }
        // A reverse tie and an isolated bus do not collapse the forest or loop.
        let mut tie = p.branches[0].clone();
        tie.id = "tie".into(); tie.from = "b1".into(); tie.to = "b0".into();
        p.branches.push(tie);
        let mut isolated = p.buses[0].clone(); isolated.id = "island".into(); p.buses.push(isolated);
        let d = diagram(&p, &study);
        assert_eq!(d.buses.len(), 42);
        assert_eq!(d.branches.len(), 41);
        let positions: std::collections::HashSet<_> = d.buses.iter().map(|b| (b.x as i64, b.y as i64)).collect();
        assert_eq!(positions.len(), 42);
        for edge in &d.branches {
            assert!(edge.points().iter().all(|&(x, y)| x >= 0.0 && y >= 0.0 && x < d.width && y < d.height));
        }
    }

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
