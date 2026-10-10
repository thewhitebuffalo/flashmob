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
    /// Worst case across operating cases, per equipment item; empty without cases.
    pub worst: Vec<WorstNote>,
    pub card_h: f64,
    pub source: bool,
    /// Every source on this bus is out of service in the normal state.
    pub source_out_of_service: bool,
}

#[derive(Clone, Debug)]
pub struct ArcNote {
    pub name: String,
    pub cal_cm2: f64,
    pub afb_in: f64,
    pub time_s: f64,
    pub assumed: bool,
}

/// Envelope maximum incident energy and its governing operating case.
#[derive(Clone, Debug)]
pub struct WorstNote {
    pub equipment_id: String,
    pub cal_cm2: Option<f64>,
    pub case_id: Option<String>,
    pub complete: bool,
    pub failed_case_ids: Vec<String>,
}

impl WorstNote {
    /// One line for a bus card, such as "WORST 4.2 CAL/CM2 [on_generator]".
    pub fn label(&self) -> String {
        let value = match (self.cal_cm2, &self.case_id) {
            (Some(cal), Some(case)) => format!("WORST {cal:.1} CAL/CM2 [{case}]"),
            (Some(cal), None) => format!("WORST {cal:.1} CAL/CM2"),
            _ => "WORST CASE UNAVAILABLE".into(),
        };
        if self.complete { value } else { format!("{value} INCOMPLETE") }
    }
}

/// Card height for a bus with these result rows; used for layout and drawing.
pub fn card_height(arcs: usize, has_failures: bool, worst: usize) -> f64 {
    112.0 + arcs as f64 * 30.0 + if has_failures { 20.0 } else { 0.0 } + worst as f64 * 30.0
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
    /// Normal state. An open branch is drawn dashed with an open switch.
    pub closed: bool,
    /// Name of the switch controlling this branch, if any.
    pub switch_name: Option<String>,
    /// One-line mark on the feeder: "breaker", "fuse", "switch", or "none".
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
            let worst = worst_for(project, study, &bus.id);
            let card_h = card_height(arcs.len(), !arc_failures.is_empty(), worst.len());
            let fault = study.fault.as_ref().and_then(|f| f.buses.iter().find(|b| b.id == bus.id));
            let flow = study.loadflow.as_ref().filter(|lf| lf.converged).and_then(|lf| lf.buses.iter().find(|b| b.id == bus.id));
            let v_pu = flow.map(|b| b.v_pu);
            let angle_deg = flow.map(|b| b.angle_deg);
            let source = project.sources.iter().any(|s| s.bus == bus.id);
            let source_out_of_service = source && project.sources.iter().filter(|s| s.bus == bus.id).all(|s| !s.in_service);
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
                worst,
                card_h,
                source,
                source_out_of_service,
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
            closed: project.branch_closed(branch),
            switch_name: project.switches.iter().find(|s| s.branch_id == branch.id).map(|s| s.name.clone()),
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

/// Worst-case incident energy per equipment item on this bus. Only shown when
/// the project has named operating cases (the envelope then differs from normal).
fn worst_for(project: &Project, study: &StudyOutput, bus: &str) -> Vec<WorstNote> {
    if project.operating_cases.is_empty() {
        return Vec::new();
    }
    let Some(envelope) = &study.worst_case else { return Vec::new() };
    envelope
        .arc_equipment
        .iter()
        .filter(|e| e.bus_id == bus)
        .map(|e| WorstNote {
            equipment_id: e.equipment_id.clone(),
            cal_cm2: e.incident_energy_cal_cm2.as_ref().map(|m| m.value),
            case_id: e.incident_energy_cal_cm2.as_ref().map(|m| m.case_id.clone()),
            complete: e.complete,
            failed_case_ids: e.failed_case_ids.clone(),
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
    let max_card = project.buses.iter().map(|b| card_height(
        study.arc_flash.as_ref().map_or(0, |rows| rows.iter().filter(|r| r.bus_id == b.id).count()),
        study.arc_flash_failures.iter().any(|f| f.bus_id == b.id),
        worst_for(project, study, &b.id).len(),
    )).fold(112.0, f64::max);
    // Every row includes enough room for the tallest result card and feeder label.
    let pitch = (max_card + 120.0).max(240.0);
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

    /// Where the open-switch mark goes: the device slot when it is free,
    /// otherwise just above a load-end device.
    pub fn switch_at(&self) -> (f64, f64) {
        if self.protector == "switch" || self.protector == "none" || self.protector_at_source { (self.x2, self.y2 - 16.0) }
        else { (self.x2, self.y2 - 31.0) }
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
.fm-branch-open {{ stroke-dasharray:8 6; opacity:0.75; }}
.fm-oos {{ stroke:#64748B; fill:none; stroke-width:1.5; stroke-dasharray:3 3; }}
.fm-oos-text {{ fill:#64748B; font-family:'JetBrains Mono','Fira Code','SF Mono',monospace; font-size:10px; font-weight:700; }}
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
        let class = if branch.closed { "fm-branch-line" } else { "fm-branch-line fm-branch-open" };
        s.push_str(&format!(r##"<polyline class="{class}" data-state="{}" points="{}"/>"##,
            if branch.closed { "closed" } else { "open" }, coords.join(" ")));
        let (dx, dy) = branch.device_at();
        match branch.protector.as_str() {
            "breaker" => s.push_str(&breaker_symbol(dx, dy)),
            "fuse" => s.push_str(&fuse_symbol(dx, dy)),
            "switch" => s.push_str(&switch_symbol(dx, dy, branch.closed)),
            _ => {}
        }
        if !branch.closed {
            // An open switch is always marked, even where a breaker or fuse is also drawn.
            let (sx, sy) = branch.switch_at();
            if branch.protector != "switch" {
                s.push_str(&switch_symbol(sx, sy, false));
            }
            s.push_str(&format!(
                r##"<text x="{x:.1}" y="{y:.1}" class="fm-warn">N.O.{name}</text>"##,
                x = sx + 14.0,
                y = sy + 4.0,
                name = branch.switch_name.as_deref().map(|n| format!(" {}", esc(&short_label(n).to_uppercase()))).unwrap_or_default(),
            ));
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

/// The mark drawn on a branch: a fuse-type device (entered fuse total-clearing
/// curve) is a fuse; another device with settings is a breaker; a branch with
/// only a switch gets a switch; otherwise nothing.
fn protector_on(project: &Project, branch: &str) -> String {
    let mut breaker = false;
    let mut fuse = false;
    for device in project.devices.iter().filter(|d| d.protected_branch.as_deref() == Some(branch)) {
        match &device.curve {
            crate::model::CurveSpec::SettingsNotCollected { .. } => {}
            _ if device.fuse_total_clearing => fuse = true,
            _ => breaker = true,
        }
    }
    let switched = project.switches.iter().any(|s| s.branch_id == branch);
    if fuse {
        "fuse".into()
    } else if breaker {
        "breaker".into()
    } else if switched {
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

/// Fuse: a vertical rectangle with the conductor running through it.
fn fuse_symbol(x: f64, y: f64) -> String {
    format!(
        r##"<g data-symbol="fuse"><rect x="{rx:.1}" y="{ry:.1}" width="8" height="18" fill="#0A0C10" stroke="#E2E8F0" stroke-width="1.3"/><line x1="{x:.1}" y1="{ry:.1}" x2="{x:.1}" y2="{y2:.1}" stroke="#E2E8F0" stroke-width="1.2"/></g>"##,
        rx = x - 4.0,
        ry = y - 9.0,
        y2 = y + 9.0,
    )
}

/// Disconnect switch: two contacts and a blade, closed in line or open at an angle.
fn switch_symbol(x: f64, y: f64, closed: bool) -> String {
    let (bx, by) = if closed { (x, y - 7.0) } else { (x + 9.0, y - 5.0) };
    format!(
        r##"<g data-symbol="switch" data-state="{state}"><rect x="{x0:.1}" y="{y0:.1}" width="14" height="18" fill="#0A0C10" stroke="none"/><circle cx="{x:.1}" cy="{ya:.1}" r="2" fill="#E2E8F0"/><circle cx="{x:.1}" cy="{yb:.1}" r="2" fill="#E2E8F0"/><line x1="{x:.1}" y1="{yb:.1}" x2="{bx:.1}" y2="{by:.1}" stroke="{ink}" stroke-width="1.6"/></g>"##,
        state = if closed { "closed" } else { "open" },
        ink = if closed { "#E2E8F0" } else { "#FFB300" },
        x0 = x - 7.0,
        y0 = y - 9.0,
        ya = y - 7.0,
        yb = y + 7.0,
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
    if bus.source && bus.source_out_of_service {
        body.push_str(&format!(
            r##"<g data-symbol="utility" data-state="out-of-service"><circle cx="{cx:.1}" cy="{y:.1}" r="16" class="fm-oos"/><line x1="{x1:.1}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" class="fm-oos"/><text x="{tx:.1}" y="{ty:.1}" class="fm-oos-text">SOURCE OUT OF SERVICE</text></g>"##,
            cx = x - 112.0,
            x1 = x - 96.0,
            x2 = x - 80.0,
            tx = x - 150.0,
            ty = y + 32.0,
        ));
    } else if bus.source {
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
    for worst in &bus.worst {
        ty += 14.0;
        let cls = if !worst.complete || worst.cal_cm2.is_some_and(|c| c >= 8.0) { "fm-warn" } else { "fm-text-dat" };
        let title = if worst.failed_case_ids.is_empty() { format!("Worst case for {}", worst.equipment_id) }
            else { format!("Worst case for {}; failed cases: {}", worst.equipment_id, worst.failed_case_ids.join(", ")) };
        body.push_str(&format!(r##"<g data-envelope="{}"><title>{}</title><text x="{x:.1}" y="{ty:.1}" class="{cls}">{}</text></g>"##,
            esc(&worst.equipment_id), esc(&title), esc(&worst.label()), x = left + 8.0));
        ty += 16.0;
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

/// XML text/attribute escape that also drops control characters XML 1.0 forbids.
fn esc(value: &str) -> String {
    crate::package::strip_controls(value)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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

    #[test]
    fn operating_state_fuses_and_worst_case_are_drawn() {
        use crate::model::{Branch, BranchKind, Bus, OperatingCase, Switch, SwitchKind, SwitchState};
        let mut project = Project::sample();
        project.devices[1].fuse_total_clearing = true;
        project.devices[1].breaker_interrupting_s = None;
        project.buses.push(Bus { id: "gen_bus".into(), name: "Gen bus".into(), kv: 12.47, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None });
        project.branches.push(Branch { id: "gen_tie".into(), name: "Gen tie".into(), from: "gen_bus".into(), to: "util".into(),
            kind: BranchKind::Line { r_ohm: 0.01, x_ohm: 0.02, b_siemens: 0.0, r0_ohm: 0.03, x0_ohm: 0.06, ampacity_a: None } });
        project.switches.push(Switch { id: "gen_sw".into(), name: "Gen breaker".into(), branch_id: "gen_tie".into(), kind: SwitchKind::Breaker, closed: false });
        let mut generator = project.sources[0].clone();
        generator.id = "gen".into();
        generator.name = "Generator".into();
        generator.bus = "gen_bus".into();
        generator.in_service = false;
        generator.is_slack = false;
        project.sources.push(generator);
        project.operating_cases.push(OperatingCase { id: "gen_closed".into(), name: "Generator breaker closed".into(),
            switch_states: vec![SwitchState { switch_id: "gen_sw".into(), closed: true }], source_states: Vec::new(), equipment_states: Vec::new() });
        assert!(project.validate().is_empty(), "{:?}", project.validate());
        let study = study::run(&project, Studies::all()).unwrap();
        let drawing = diagram(&project, &study);
        let tie = drawing.branches.iter().find(|b| b.id == "gen_tie").unwrap();
        assert!(!tie.closed);
        assert_eq!(tie.protector, "switch");
        assert_eq!(drawing.branches.iter().find(|b| b.id == "feeder").unwrap().protector, "fuse");
        let gen_bus = drawing.buses.iter().find(|b| b.id == "gen_bus").unwrap();
        assert!(gen_bus.source && gen_bus.source_out_of_service);
        let pnl = drawing.buses.iter().find(|b| b.id == "pnl").unwrap();
        assert_eq!(pnl.worst.len(), 1);
        assert!(pnl.worst[0].case_id.is_some());
        assert_eq!(pnl.card_h, card_height(pnl.arcs.len(), !pnl.arc_failures.is_empty(), 1));
        let svg = to_svg(&drawing);
        assert!(svg.contains("fm-branch-open") && svg.contains("data-state=\"open\""));
        assert!(svg.contains("data-symbol=\"switch\" data-state=\"open\""));
        assert!(svg.contains("N.O. GEN BREAKER"));
        assert!(svg.contains("data-symbol=\"fuse\""));
        assert!(svg.contains("SOURCE OUT OF SERVICE"));
        assert!(svg.contains("WORST "), "{svg}");
        assert!(svg.contains("data-envelope=\"af-pnl\""));
        // Without operating cases there is no envelope line.
        let plain = Project::sample();
        let plain_svg = to_svg(&diagram(&plain, &study::run(&plain, Studies::all()).unwrap()));
        assert!(!plain_svg.contains("WORST "));
        assert!(!plain_svg.contains("fm-branch-open\""));
    }

    #[test]
    fn svg_text_drops_control_characters() {
        let mut project = Project::sample();
        project.name = "Plant\u{0}\u{1b}<A>".into();
        let svg = to_svg(&diagram(&project, &study::run(&project, Studies::all()).unwrap()));
        assert!(svg.contains("PLANT&lt;A&gt;"));
        assert!(!svg.chars().any(|c| c < ' ' && c != '\n'));
    }
}
