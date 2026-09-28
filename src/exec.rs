use serde::{Deserialize, Serialize};

use crate::connectivity::{self, Topology};
use crate::model::{ArcEquipment, Branch, BranchKind, Bus, CurveSpec, DecrementPoint, Device, Electrode, EquipmentState, Load, Motor, OperatingCase, Project, Source, SourceState, Switch, SwitchGroup, SwitchKind, SwitchState, XfmrConn};
use crate::sld::{self, Diagram};
use crate::study::{self, Studies, StudyOutput};
use crate::tcc::{self, TccPlot};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    #[serde(default)]
    pub project: Option<Project>,
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Sample,
    Replace { project: Project },
    AddBus {
        id: String,
        #[serde(default)] name: String,
        kv: f64,
        #[serde(default)] shunt_kvar: f64,
        #[serde(default)] bracing_ka: Option<f64>,
        #[serde(default)] main_rating_a: Option<f64>,
    },
    AddAssumption { id: String, statement: String },
    SetOptions {
        #[serde(default)] name: Option<String>,
        #[serde(default)] s_base_mva: Option<f64>,
        #[serde(default)] arc_duration_cap_s: Option<f64>,
        #[serde(default)] prefault: Option<crate::model::Prefault>,
    },
    AddLine {
        id: String,
        #[serde(default)] name: String,
        from: String,
        to: String,
        r_ohm: f64,
        x_ohm: f64,
        #[serde(default)] b_siemens: f64,
        #[serde(default)] r0_ohm: f64,
        #[serde(default)] x0_ohm: f64,
        #[serde(default)] ampacity_a: Option<f64>,
    },
    AddTransformer {
        id: String,
        #[serde(default)] name: String,
        from: String,
        to: String,
        kva: f64,
        z_percent: f64,
        xr: f64,
        hv_kv: f64,
        lv_kv: f64,
        connection: XfmrConn,
        #[serde(default)] tap_percent: f64,
        #[serde(default = "one")] x0_over_x1: f64,
        #[serde(default = "one")] r0_over_r1: f64,
    },
    AddLoad { id: String, #[serde(default)] name: String, bus: String, kw: f64, kvar: f64, #[serde(default)] basis: String },
    AddSource {
        id: String,
        #[serde(default)] name: String,
        bus: String,
        #[serde(default = "one")] v_pu: f64,
        #[serde(default)] angle_deg: f64,
        mva_sc: f64,
        #[serde(default)] decrement_curve: Vec<DecrementPoint>,
        xr: f64,
        #[serde(default = "one")] x0_over_x1: f64,
        #[serde(default = "one")] r0_over_r1: f64,
        #[serde(default)] is_slack: bool,
        #[serde(default = "default_true")] in_service: bool,
        #[serde(default)] p_mw: f64,
    },
    AddMotor {
        id: String,
        #[serde(default)] name: String,
        bus: String,
        hp: f64,
        kv: f64,
        #[serde(default = "default_pf")] pf: f64,
        #[serde(default = "default_eff")] efficiency: f64,
        #[serde(default = "default_xd")] x_subtransient_pu: f64,
        #[serde(default = "default_xr")] xr: f64,
    },
    AddDevice { id: String, #[serde(default)] name: String, bus: String, curve: CurveSpec, #[serde(default)] basis: String, #[serde(default)] protected_branch: Option<String>, #[serde(default)] breaker_interrupting_s: Option<f64>, #[serde(default)] fuse_total_clearing: bool },
    AddSwitch { id: String, #[serde(default)] name: String, branch_id: String, kind: SwitchKind, closed: bool },
    AddSwitchGroup { id: String, switch_ids: Vec<String>, max_closed: usize },
    AddOperatingCase {
        id: String,
        #[serde(default)] name: String,
        #[serde(default)] switch_states: Vec<SwitchState>,
        #[serde(default)] source_states: Vec<SourceState>,
        #[serde(default)] equipment_states: Vec<EquipmentState>,
    },
    AddEquipment {
        id: String,
        #[serde(default)] name: String,
        bus: String,
        electrode: Electrode,
        gap_mm: f64,
        distance_mm: f64,
        height_mm: f64,
        width_mm: f64,
        depth_mm: f64,
        #[serde(default)] upstream_device: Option<String>,
        #[serde(default)] clearing_s: Option<f64>,
        #[serde(default)] fallback_duration_s: Option<f64>,
        #[serde(default)] basis: String,
    },
    Remove { id: String },
    SetBus {
        id: String,
        #[serde(default)] name: Option<String>,
        #[serde(default)] kv: Option<f64>,
        #[serde(default)] shunt_kvar: Option<f64>,
        #[serde(default)] bracing_ka: Option<f64>,
        #[serde(default)] main_rating_a: Option<f64>,
    },
    SetSource {
        id: String,
        #[serde(default)] mva_sc: Option<f64>,
        #[serde(default)] decrement_curve: Option<Vec<DecrementPoint>>,
        #[serde(default)] xr: Option<f64>,
        #[serde(default)] v_pu: Option<f64>,
        #[serde(default)] x0_over_x1: Option<f64>,
        #[serde(default)] is_slack: Option<bool>,
        #[serde(default)] in_service: Option<bool>,
    },
    Run { #[serde(default)] studies: Vec<String> },
    Topology,
    Sld,
    Tcc { #[serde(default)] ref_kv: Option<f64>, #[serde(default)] devices: Vec<String> },
    /// Write the engineering data package for mapping. `output` is a directory, or an `.xml` path.
    ExportSkm { #[serde(default)] output: Option<String> },
}

fn one() -> f64 { 1.0 }
fn default_true() -> bool { true }
fn default_pf() -> f64 { 0.85 }
fn default_eff() -> f64 { 0.94 }
fn default_xd() -> f64 { 0.17 }
fn default_xr() -> f64 { 6.0 }

#[derive(Clone, Debug, Serialize)]
pub struct ExecResponse {
    pub ok: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub project: Project,
    pub results: Option<StudyOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topology: Option<Topology>,
    pub sld_svg: Option<String>,
    pub tcc_svg: Option<String>,
    pub tcc_csv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skm: Option<crate::skm::SkmExport>,
}

pub fn exec_request(json: &str) -> ExecResponse {
    let request: ExecRequest = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(err) => {
            return ExecResponse {
                ok: false,
                error: Some(format!("could not read the request: {err}")),
                warnings: Vec::new(),
                project: Project::default(),
                results: None,
                topology: None,
                sld_svg: None,
                tcc_svg: None,
                tcc_csv: None,
                skm: None,
            };
        }
    };
    apply(request.project.unwrap_or_default(), &request.commands)
}

pub fn apply(mut project: Project, commands: &[Command]) -> ExecResponse {
    let mut results = None;
    let mut topology = None;
    let mut sld_svg = None;
    let mut tcc_svg = None;
    let mut tcc_csv = None;
    let mut skm = None;
    for (n, command) in commands.iter().enumerate() {
        let step = apply_one(&mut project, command, &mut results, &mut topology, &mut sld_svg, &mut tcc_svg, &mut tcc_csv, &mut skm);
        if let Err(err) = step {
            return ExecResponse {
                ok: false,
                error: Some(format!("command {n} ({}) : {err}", op_name(command))),
                warnings: Vec::new(),
                project,
                results,
                topology,
                sld_svg,
                tcc_svg,
                tcc_csv,
                skm,
            };
        }
    }
    let warnings = results.as_ref().map(|r| r.warnings.clone()).unwrap_or_default();
    ExecResponse { ok: true, error: None, warnings, project, results, topology, sld_svg, tcc_svg, tcc_csv, skm }
}

fn apply_one(
    project: &mut Project,
    command: &Command,
    results: &mut Option<StudyOutput>,
    topology: &mut Option<Topology>,
    sld_svg: &mut Option<String>,
    tcc_svg: &mut Option<String>,
    tcc_csv: &mut Option<String>,
    skm: &mut Option<crate::skm::SkmExport>,
) -> Result<(), String> {
    if !matches!(command, Command::Topology | Command::Sld | Command::Tcc { .. } | Command::ExportSkm { .. }) {
        *results = None;
        *sld_svg = None;
        *tcc_svg = None;
        *tcc_csv = None;
        *skm = None;
    }
    if !matches!(command, Command::Run { .. } | Command::Topology | Command::Sld | Command::Tcc { .. } | Command::ExportSkm { .. }) {
        *topology = None;
    }
    match command {
        Command::Sample => *project = Project::sample(),
        Command::Replace { project: next } => *project = next.clone(),
        Command::AddBus { id, name, kv, shunt_kvar, bracing_ka, main_rating_a } => {
            fresh(id, project)?;
            project.buses.push(Bus {
                id: id.clone(),
                name: named(name, id),
                kv: *kv,
                shunt_kvar: *shunt_kvar,
                bracing_ka: *bracing_ka,
                main_rating_a: *main_rating_a,
            });
        }
        Command::AddAssumption { id, statement } => {
            fresh(id, project)?;
            project.assumptions.push(crate::model::Assumption { id: id.clone(), statement: statement.clone() });
        }
        Command::SetOptions { name, s_base_mva, arc_duration_cap_s, prefault } => {
            if let Some(name) = name {
                project.name = name.clone();
            }
            if let Some(base) = s_base_mva {
                project.s_base_mva = *base;
            }
            if let Some(cap) = arc_duration_cap_s {
                project.arc_duration_cap_s = *cap;
            }
            if let Some(prefault) = prefault {
                project.prefault = *prefault;
            }
        }
        Command::AddLine { id, name, from, to, r_ohm, x_ohm, b_siemens, r0_ohm, x0_ohm, ampacity_a } => {
            fresh(id, project)?;
            project.branches.push(Branch {
                id: id.clone(),
                name: named(name, id),
                from: from.clone(),
                to: to.clone(),
                kind: BranchKind::Line {
                    r_ohm: *r_ohm,
                    x_ohm: *x_ohm,
                    b_siemens: *b_siemens,
                    r0_ohm: *r0_ohm,
                    x0_ohm: *x0_ohm,
                    ampacity_a: *ampacity_a,
                },
            });
        }
        Command::AddTransformer { id, name, from, to, kva, z_percent, xr, hv_kv, lv_kv, connection, tap_percent, x0_over_x1, r0_over_r1 } => {
            fresh(id, project)?;
            project.branches.push(Branch {
                id: id.clone(),
                name: named(name, id),
                from: from.clone(),
                to: to.clone(),
                kind: BranchKind::Transformer {
                    kva: *kva,
                    z_percent: *z_percent,
                    xr: *xr,
                    hv_kv: *hv_kv,
                    lv_kv: *lv_kv,
                    connection: *connection,
                    tap_percent: *tap_percent,
                    x0_over_x1: *x0_over_x1,
                    r0_over_r1: *r0_over_r1,
                },
            });
        }
        Command::AddLoad { id, name, bus, kw, kvar, basis } => {
            fresh(id, project)?;
            project.loads.push(Load { id: id.clone(), name: named(name, id), bus: bus.clone(), kw: *kw, kvar: *kvar, basis: basis.clone() });
        }
        Command::AddSource { id, name, bus, v_pu, angle_deg, mva_sc, decrement_curve, xr, x0_over_x1, r0_over_r1, is_slack, in_service, p_mw } => {
            fresh(id, project)?;
            project.sources.push(Source {
                id: id.clone(),
                name: named(name, id),
                bus: bus.clone(),
                v_pu: *v_pu,
                angle_deg: *angle_deg,
                mva_sc: *mva_sc,
                decrement_curve: decrement_curve.clone(),
                xr: *xr,
                x0_over_x1: *x0_over_x1,
                r0_over_r1: *r0_over_r1,
                is_slack: *is_slack,
                in_service: *in_service,
                p_mw: *p_mw,
                qmin_mvar: None,
                qmax_mvar: None,
            });
        }
        Command::AddMotor { id, name, bus, hp, kv, pf, efficiency, x_subtransient_pu, xr } => {
            fresh(id, project)?;
            project.motors.push(Motor {
                id: id.clone(),
                name: named(name, id),
                bus: bus.clone(),
                hp: *hp,
                kv: *kv,
                pf: *pf,
                efficiency: *efficiency,
                x_subtransient_pu: *x_subtransient_pu,
                xr: *xr,
            });
        }
        Command::AddDevice { id, name, bus, curve, basis, protected_branch, breaker_interrupting_s, fuse_total_clearing } => {
            fresh(id, project)?;
            project.devices.push(Device { id: id.clone(), name: named(name, id), bus: bus.clone(), curve: curve.clone(), basis: basis.clone(), protected_branch: protected_branch.clone(), breaker_interrupting_s: *breaker_interrupting_s, fuse_total_clearing: *fuse_total_clearing });
        }
        Command::AddSwitch { id, name, branch_id, kind, closed } => {
            fresh(id, project)?;
            project.switches.push(Switch { id: id.clone(), name: named(name, id), branch_id: branch_id.clone(), kind: kind.clone(), closed: *closed });
        }
        Command::AddSwitchGroup { id, switch_ids, max_closed } => {
            fresh(id, project)?;
            project.switch_groups.push(SwitchGroup { id: id.clone(), switch_ids: switch_ids.clone(), max_closed: *max_closed });
        }
        Command::AddOperatingCase { id, name, switch_states, source_states, equipment_states } => {
            fresh(id, project)?;
            project.operating_cases.push(OperatingCase {
                id: id.clone(), name: named(name, id),
                switch_states: switch_states.clone(), source_states: source_states.clone(), equipment_states: equipment_states.clone(),
            });
        }
        Command::AddEquipment { id, name, bus, electrode, gap_mm, distance_mm, height_mm, width_mm, depth_mm, upstream_device, clearing_s, fallback_duration_s, basis } => {
            fresh(id, project)?;
            project.equipment.push(ArcEquipment {
                id: id.clone(),
                name: named(name, id),
                bus: bus.clone(),
                electrode: *electrode,
                gap_mm: *gap_mm,
                distance_mm: *distance_mm,
                height_mm: *height_mm,
                width_mm: *width_mm,
                depth_mm: *depth_mm,
                upstream_device: upstream_device.clone(),
                clearing_s: *clearing_s,
                fallback_duration_s: *fallback_duration_s,
                basis: basis.clone(),
            });
        }
        Command::Remove { id } => remove(project, id)?,
        Command::SetBus { id, name, kv, shunt_kvar, bracing_ka, main_rating_a } => {
            let bus = project.buses.iter_mut().find(|b| b.id == *id).ok_or_else(|| format!("no bus '{id}'"))?;
            if let Some(name) = name { bus.name = name.clone(); }
            if let Some(kv) = kv { bus.kv = *kv; }
            if let Some(q) = shunt_kvar { bus.shunt_kvar = *q; }
            if let Some(ka) = bracing_ka { bus.bracing_ka = Some(*ka); }
            if let Some(amps) = main_rating_a { bus.main_rating_a = Some(*amps); }
        }
        Command::SetSource { id, mva_sc, decrement_curve, xr, v_pu, x0_over_x1, is_slack, in_service } => {
            let source = project.sources.iter_mut().find(|s| s.id == *id).ok_or_else(|| format!("no source '{id}'"))?;
            if let Some(v) = mva_sc { source.mva_sc = *v; }
            if let Some(v) = decrement_curve { source.decrement_curve = v.clone(); }
            if let Some(v) = xr { source.xr = *v; }
            if let Some(v) = v_pu { source.v_pu = *v; }
            if let Some(v) = x0_over_x1 { source.x0_over_x1 = *v; }
            if let Some(v) = is_slack { source.is_slack = *v; }
            if let Some(v) = in_service { source.in_service = *v; }
        }
        Command::Run { studies } => {
            let which = Studies::parse(studies)?;
            *results = Some(study::run(project, which)?);
        }
        Command::Topology => {
            *topology = Some(connectivity::topology(project)?);
        }
        Command::Sld => {
            let study = ensure_study(project, results)?;
            *sld_svg = Some(sld::to_svg(&sld::diagram(project, study)));
        }
        Command::Tcc { ref_kv, devices } => {
            let study = ensure_study(project, results)?;
            let drawn = tcc::plot(project, Some(study), *ref_kv, devices)?;
            *tcc_svg = Some(tcc::to_svg(&drawn));
            *tcc_csv = Some(tcc::to_csv(&drawn));
        }
        Command::ExportSkm { output } => {
            let exported = crate::skm::export(project);
            if let Some(path) = output {
                crate::skm::write_dir(project, std::path::Path::new(path))?;
            }
            *skm = Some(exported);
        }
    }
    Ok(())
}

fn ensure_study<'a>(project: &Project, results: &'a mut Option<StudyOutput>) -> Result<&'a StudyOutput, String> {
    if results.as_ref().map_or(true, |r| r.loadflow.is_none() || r.fault.is_none() || r.arc_flash.is_none() || r.coordination.is_none()) {
        *results = None;
        *results = Some(study::run(project, Studies::all())?);
    }
    Ok(results.as_ref().unwrap())
}

fn fresh(id: &str, project: &Project) -> Result<(), String> {
    if id.is_empty() {
        return Err("id is empty".into());
    }
    if project.ids().iter().any(|existing| *existing == id) {
        return Err(format!("id '{id}' is already used"));
    }
    Ok(())
}

fn named(name: &str, id: &str) -> String {
    if name.is_empty() { id.to_string() } else { name.to_string() }
}

fn remove(project: &mut Project, id: &str) -> Result<(), String> {
    let mut deps = Vec::new();
    for branch in &project.branches {
        if branch.from == id || branch.to == id {
            deps.push(format!("branch {}", branch.id));
        }
    }
    for item in &project.loads {
        if item.bus == id { deps.push(format!("load {}", item.id)); }
    }
    for item in &project.sources {
        if item.bus == id { deps.push(format!("source {}", item.id)); }
    }
    for item in &project.motors {
        if item.bus == id { deps.push(format!("motor {}", item.id)); }
    }
    for item in &project.devices {
        if item.bus == id || item.protected_branch.as_deref() == Some(id) {
            deps.push(format!("device {}", item.id));
        }
    }
    for item in &project.switches {
        if item.branch_id == id { deps.push(format!("switch {}", item.id)); }
    }
    for item in &project.switch_groups {
        if item.switch_ids.iter().any(|switch_id| switch_id == id) {
            deps.push(format!("switch group {}", item.id));
        }
    }
    for item in &project.operating_cases {
        if item.switch_states.iter().any(|state| state.switch_id == id)
            || item.source_states.iter().any(|state| state.source_id == id)
            || item.equipment_states.iter().any(|state| state.equipment_id == id || state.upstream_device.as_deref() == Some(id)) {
            deps.push(format!("operating case {}", item.id));
        }
    }
    for item in &project.equipment {
        if item.bus == id || item.upstream_device.as_deref() == Some(id) {
            deps.push(format!("equipment {}", item.id));
        }
    }
    if !deps.is_empty() {
        return Err(format!("'{id}' is still used by {}", deps.join(", ")));
    }
    let before = project.ids().len();
    project.buses.retain(|x| x.id != id);
    project.branches.retain(|x| x.id != id);
    project.loads.retain(|x| x.id != id);
    project.sources.retain(|x| x.id != id);
    project.motors.retain(|x| x.id != id);
    project.devices.retain(|x| x.id != id);
    project.switches.retain(|x| x.id != id);
    project.switch_groups.retain(|x| x.id != id);
    project.operating_cases.retain(|x| x.id != id);
    project.equipment.retain(|x| x.id != id);
    project.assumptions.retain(|x| x.id != id);
    if project.ids().len() == before {
        return Err(format!("no object with id '{id}'"));
    }
    Ok(())
}

fn op_name(command: &Command) -> &'static str {
    match command {
        Command::Sample => "sample",
        Command::Replace { .. } => "replace",
        Command::AddBus { .. } => "add_bus",
        Command::AddAssumption { .. } => "add_assumption",
        Command::SetOptions { .. } => "set_options",
        Command::AddLine { .. } => "add_line",
        Command::AddTransformer { .. } => "add_transformer",
        Command::AddLoad { .. } => "add_load",
        Command::AddSource { .. } => "add_source",
        Command::AddMotor { .. } => "add_motor",
        Command::AddDevice { .. } => "add_device",
        Command::AddSwitch { .. } => "add_switch",
        Command::AddSwitchGroup { .. } => "add_switch_group",
        Command::AddOperatingCase { .. } => "add_operating_case",
        Command::AddEquipment { .. } => "add_equipment",
        Command::Remove { .. } => "remove",
        Command::SetBus { .. } => "set_bus",
        Command::SetSource { .. } => "set_source",
        Command::Run { .. } => "run",
        Command::Topology => "topology",
        Command::Sld => "sld",
        Command::Tcc { .. } => "tcc",
        Command::ExportSkm { .. } => "export_skm",
    }
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "tool": "flashmob",
        "protocol": "exec-v1",
        "headless": true,
        "stdout": "JSON only. Do not expect prompts.",
        "studies": ["loadflow", "fault", "arcflash", "coordination"],
        "arc_flash_standard": "IEEE 1584-2018",
        "commands": {
            "schema": "flashmob schema",
            "sample": "flashmob sample",
            "validate": "flashmob validate project.json",
            "topology": "flashmob topology project.json    # or - for stdin",
            "run": "flashmob run project.json",
            "exec": "flashmob exec request.json    # or stdin",
            "sld": "flashmob sld project.json -o diagram.svg",
            "tcc": "flashmob tcc project.json -o curves.svg --csv curves.csv --ref-kv 0.48",
            "export_skm": "flashmob export-skm project.json -o skm-export"
        },
        "exec_request": {
            "project": null,
            "commands": [
                {"op": "sample"},
                {"op": "add_switch", "id": "feeder_switch", "branch_id": "feeder", "kind": "breaker", "closed": true},
                {"op": "add_switch_group", "id": "feeder_control", "switch_ids": ["feeder_switch"], "max_closed": 1},
                {"op": "add_operating_case", "id": "feeder_open", "switch_states": [{"switch_id": "feeder_switch", "closed": false}], "source_states": [], "equipment_states": []},
                {"op": "topology"},
                {"op": "run", "studies": ["loadflow", "fault", "arcflash", "coordination"]},
                {"op": "sld"},
                {"op": "tcc", "ref_kv": 0.48, "devices": []},
                {"op": "export_skm", "output": "skm-export"}
            ]
        },
        "exec_request_note": "The project field is optional; omit it or set it to null to start empty. The example commands build on the built-in sample.",
        "exec_response": ["ok", "error", "warnings", "project", "results", "topology", "sld_svg", "tcc_svg", "tcc_csv", "skm"],
        "topology": "Validated entered connections only: every bus lists branch terminal and neighbor plus attached source, load, motor, device, and arc equipment IDs. Global records give branch endpoints and switch link, source base in-service state, switch base closed state, group rules, named operating cases, and each device's entered protected branch terminal. Null placement means it was not entered. No study or protection inference is performed.",
        "source_decrement": "Optional add_source/set_source decrement_curve is an array of {time_s,current_ratio} for symmetrical three-phase RMS short-circuit current at source terminals relative to initial mva_sc. First point must be {time_s:0,current_ratio:1}; times increase strictly and ratios stay positive. Linear interpolation in time holds the first/last value outside entered points. An empty array clears the curve; omission retains the constant-current model. Arc energy uses at most 10 ms midpoint, quasi-steady network samples. For an isolating upstream breaker with an entered trip-only curve, protected terminal, and breaker_interrupting_s, relay trip elements integrate separately over full and reduced sampled arcing currents; breaker interruption is added after trip. Timers assume immediate reset below pickup and the result is marked assumed if a reset occurs. A fuse total-clearing curve does not establish dynamic clearing. Optional clearing_s overrides automatic clearing; fallback_duration_s applies only when clearing is unresolved. arc_duration_cap_s bounds the study, never supplies a field clearing setting.",
        "operating_cases": "Base switch.closed and source.in_service values define normal operation. Each named operating case overrides listed switch/source states. Its arc equipment starts with no inherited upstream_device, clearing_s, or fallback_duration_s; list case-specific values in equipment_states, or leave them absent for automatic protection selection or an explicit failure. At most one switch can control a branch. A switch group limits simultaneous closures using max_closed. `run` evaluates the base configuration and each named valid case; inspect per-case failures before using an arc-flash maximum.",
        "switch_model": "Opening a switch removes its entire referenced branch, including any transformer grounding shunt or line charging on that branch. For an independent breaker or ATS contact, use a separate short finite-impedance branch with separate bus nodes; put transformers and other equipment on their own branches. The closed field is required for every switch, especially normally open ties and ATS contacts.",
        "switch_kinds": ["breaker", "tie", "ats"],
        "skm": "Engineering data package for mapping; PTW import unverified: project.xml, per-component CSV, components.tab, and Revit panel/circuit schedules. See IMPORT.txt.",
        "sld": "SVG single-line diagram. Each bus card shows nominal voltage, load-flow pu voltage, symmetrical 3P and LG fault current, and the governing IEEE 1584-2018 incident energy and arc-flash boundary.",
        "tcc": "SVG log-log time-current curve plus CSV points. Current is referred to ref_kv. Fault and arcing-current markers are included when a study has been run.",
        "electrodes": ["VCB", "VCBB", "HCB", "VOA", "HOA"],
        "transformer_connections": ["dyn", "ynd", "ynyn", "dd"],
        "sample_project": serde_json::to_value(Project::sample()).unwrap(),
    })
}

pub fn load_project(text: &str) -> Result<Project, String> {
    serde_json::from_str(text).map_err(|err| format!("could not read the project: {err}"))
}

pub fn draw_sld(project: &Project, study: &StudyOutput) -> (Diagram, String) {
    let diagram = sld::diagram(project, study);
    let svg = sld::to_svg(&diagram);
    (diagram, svg)
}

pub fn draw_tcc(project: &Project, study: &StudyOutput, ref_kv: Option<f64>, devices: &[String]) -> Result<(TccPlot, String, String), String> {
    let plot = tcc::plot(project, Some(study), ref_kv, devices)?;
    let svg = tcc::to_svg(&plot);
    let csv = tcc::to_csv(&plot);
    Ok((plot, svg, csv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_exec_returns_studies_sld_and_tcc() {
        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"run"},{"op":"sld"},{"op":"tcc","ref_kv":0.48}]}"#);
        assert!(response.ok, "{:?}", response.error);
        let results = response.results.unwrap();
        assert!(results.loadflow.unwrap().converged);
        assert!(results.fault.unwrap().buses.len() == 3);
        let arcs = results.arc_flash.unwrap();
        assert!(arcs.iter().any(|r| r.bus_id == "mcc" && r.standard == "IEEE 1584-2018"));
        let svg = response.sld_svg.unwrap();
        assert!(svg.contains("MCC-1") && svg.contains("kA") && svg.contains("CAL/CM2"));
        let tcc = response.tcc_svg.unwrap();
        assert!(tcc.contains("MCC main") && tcc.contains("<path"));
        assert!(response.tcc_csv.unwrap().contains("time_s"));
    }

    #[test]
    fn topology_op_is_optional_and_does_not_discard_study_results() {
        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"run","studies":["fault"]},{"op":"topology"}]}"#);
        assert!(response.ok, "{:?}", response.error);
        assert!(response.results.unwrap().fault.is_some());
        let topology = response.topology.unwrap();
        assert_eq!(topology.buses.len(), 3);
        assert_eq!(topology.branches.len(), 2);

        let without = exec_request(r#"{"commands":[{"op":"sample"}]}"#);
        let json = serde_json::to_value(without).unwrap();
        assert!(json.get("topology").is_none());
    }

    #[test]
    fn topology_op_rejects_invalid_project_and_model_edits_clear_the_view() {
        let mut invalid = Project::sample();
        invalid.branches[0].to = "missing".into();
        let response = apply(invalid, &[Command::Topology]);
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("unknown bus 'missing'"));
        assert!(response.topology.is_none());

        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"topology"},{"op":"set_source","id":"grid","mva_sc":300}]}"#);
        assert!(response.ok, "{:?}", response.error);
        assert!(response.topology.is_none());
    }

    #[test]
    fn exec_builds_switch_cases_and_exposes_them_in_topology() {
        let response = exec_request(r#"{"commands":[
            {"op":"sample"},
            {"op":"add_switch","id":"feeder-switch","branch_id":"feeder","kind":"breaker","closed":true},
            {"op":"add_switch_group","id":"feeder-group","switch_ids":["feeder-switch"],"max_closed":1},
            {"op":"add_operating_case","id":"feeder-open","switch_states":[{"switch_id":"feeder-switch","closed":false}],"equipment_states":[{"equipment_id":"af-pnl","upstream_device":"brk-feeder","clearing_s":0.5}]},
            {"op":"topology"}
        ]}"#);
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.project.switches.len(), 1);
        assert_eq!(response.project.operating_cases.len(), 1);
        let topology = response.topology.unwrap();
        let branch = topology.branches.iter().find(|branch| branch.id == "feeder").unwrap();
        assert_eq!(branch.switch_id.as_deref(), Some("feeder-switch"));
        assert_eq!(branch.base_closed, Some(true));
        assert_eq!(topology.switches[0].branch_id, "feeder");
        assert_eq!(topology.switch_groups[0].switch_ids, ["feeder-switch"]);
        assert_eq!(topology.operating_cases[0].switch_states[0].switch_id, "feeder-switch");
        assert_eq!(topology.operating_cases[0].equipment_states[0].equipment_id, "af-pnl");
        assert_eq!(topology.operating_cases[0].equipment_states[0].clearing_s, Some(0.5));
    }

    #[test]
    fn source_service_edits_invalidate_outputs_and_remove_checks_case_references() {
        let response = exec_request(r#"{"commands":[
            {"op":"sample"},
            {"op":"run","studies":["fault"]},
            {"op":"set_source","id":"grid","in_service":false}
        ]}"#);
        assert!(response.ok, "{:?}", response.error);
        assert!(response.results.is_none());
        assert!(!response.project.sources[0].in_service);

        let response = exec_request(r#"{"commands":[
            {"op":"sample"},
            {"op":"add_switch","id":"feeder-switch","branch_id":"feeder","kind":"breaker","closed":true},
            {"op":"remove","id":"feeder"}
        ]}"#);
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("switch feeder-switch"));
    }

    #[test]
    fn exec_add_and_set_source_decrement_curve_with_topology_visibility() {
        let added = exec_request(r#"{"commands":[
            {"op":"sample"},
            {"op":"add_source","id":"standby","bus":"util","mva_sc":35,"xr":8,"in_service":false,
             "decrement_curve":[{"time_s":0,"current_ratio":1},{"time_s":0.1,"current_ratio":0.6}]},
            {"op":"topology"}
        ]}"#);
        assert!(added.ok, "{:?}", added.error);
        assert!((added.project.sources[1].decrement_ratio_at(0.05) - 0.8).abs() < 1e-12);
        let view = serde_json::to_value(added.topology.unwrap()).unwrap();
        assert_eq!(view["sources"][1]["decrement_curve"][1]["current_ratio"], 0.6);

        let changed = exec_request(r#"{"commands":[
            {"op":"sample"},
            {"op":"set_source","id":"grid","decrement_curve":[{"time_s":0,"current_ratio":1},{"time_s":0.5,"current_ratio":0.4}]},
            {"op":"topology"}
        ]}"#);
        assert!(changed.ok, "{:?}", changed.error);
        assert!((changed.project.sources[0].decrement_ratio_at(0.25) - 0.7).abs() < 1e-12);
        let cleared = exec_request(&serde_json::json!({
            "project": changed.project,
            "commands": [{"op":"set_source","id":"grid","decrement_curve":[]},{"op":"topology"}]
        }).to_string());
        assert!(cleared.ok, "{:?}", cleared.error);
        assert!(cleared.project.sources[0].decrement_curve.is_empty());
        let view = serde_json::to_value(cleared.topology.unwrap()).unwrap();
        assert!(view["sources"][0].get("decrement_curve").is_none());
    }

    #[test]
    fn topology_command_rejects_invalid_source_decrement_curve() {
        let response = exec_request(r#"{"commands":[
            {"op":"sample"},
            {"op":"set_source","id":"grid","decrement_curve":[{"time_s":0,"current_ratio":1},{"time_s":0.1,"current_ratio":0}]},
            {"op":"topology"}
        ]}"#);
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("decrement_curve point 1"));
    }
}
