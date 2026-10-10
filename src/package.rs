//! Engineering data package for mapping. No target tool version has been round-tripped.
//! XML is a Flashmob representation, not a vendor schema or verified import format.
//!
//! Every exporter assumes a project that has passed `Project::validate()`;
//! callers (CLI, exec, GUI) validate before exporting.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::{Branch, BranchKind, CurveSpec, Electrode, GroundingMethod, Project, Winding, XfmrConn};

#[derive(Clone, Debug, Serialize)]
pub struct PackageExport {
    pub files: Vec<PackageFile>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageFile {
    pub name: String,
    pub content: String,
}

impl PackageExport {
    pub fn xml(&self) -> &str {
        self.file("project.xml").unwrap_or("")
    }

    pub fn file(&self, name: &str) -> Option<&str> {
        self.files.iter().find(|f| f.name == name).map(|f| f.content.as_str())
    }
}

pub fn export(project: &Project) -> PackageExport {
    let names = Names::new(project);
    let mut files = vec![
        PackageFile { name: "project.json".into(), content: serde_json::to_string_pretty(project).expect("project serializes") },
        PackageFile { name: "project.xml".into(), content: xml(project, &names) },
        PackageFile { name: "buses.csv".into(), content: buses_csv(project, &names) },
        PackageFile { name: "arc-equipment.csv".into(), content: arc_equipment_csv(project, &names) },
        PackageFile { name: "cables.csv".into(), content: cables_csv(project, &names) },
        PackageFile { name: "transformers.csv".into(), content: transformers_csv(project, &names) },
        PackageFile { name: "utilities.csv".into(), content: utilities_csv(project, &names) },
        PackageFile { name: "loads.csv".into(), content: loads_csv(project, &names) },
        PackageFile { name: "motors.csv".into(), content: motors_csv(project, &names) },
        PackageFile { name: "protective-devices.csv".into(), content: devices_csv(project, &names) },
        PackageFile { name: "switches.csv".into(), content: switches_csv(project, &names) },
        PackageFile { name: "connections.csv".into(), content: connections_csv(project, &names) },
        PackageFile { name: "revit-panels.csv".into(), content: revit_panels_csv(project, &names) },
        PackageFile { name: "revit-circuits.csv".into(), content: revit_circuits_csv(project, &names) },
        PackageFile { name: "components.tab".into(), content: components_tab(project, &names) },
        PackageFile { name: "IMPORT.txt".into(), content: import_notes(project) },
    ];
    files.retain(|f| !f.content.is_empty());
    PackageExport { files }
}

/// How `write_dir` treats files that already exist.
#[derive(Clone, Copy, Debug, Default)]
pub struct WriteOptions<'a> {
    /// Replace existing package files. Without it, any existing target file is an error.
    pub overwrite: bool,
    /// A file that must never be replaced, even with `overwrite` (the input project).
    pub protect: Option<&'a Path>,
}

/// Write the package. `output` is a directory, or a path ending in `.xml`; with an
/// `.xml` path, `project.xml` is written to that path and every other file is
/// written beside it in the same directory. Any other path with a file-like
/// extension (`.json`, `.csv`, `.txt`, `.tab`, `.svg`) or naming an existing
/// file is rejected rather than silently becoming a directory. No file is
/// written unless every target can be written without an unauthorized overwrite.
pub fn write_dir(project: &Project, output: &Path, options: WriteOptions) -> Result<PathBuf, String> {
    let exported = export(project);
    let ext = output.extension().and_then(|ext| ext.to_str()).map(|e| e.to_ascii_lowercase());
    let xml_path = ext.as_deref() == Some("xml");
    let dir = if xml_path {
        if output.is_dir() {
            return Err(format!("{} is a directory; give a directory or a new .xml file path", output.display()));
        }
        output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new(".")).to_path_buf()
    } else {
        if output.is_file() {
            return Err(format!("{} is an existing file; the output must be a directory or an .xml path", output.display()));
        }
        if matches!(ext.as_deref(), Some("json" | "csv" | "txt" | "tab" | "svg")) {
            return Err(format!("{} looks like a file; the output must be a directory or an .xml path", output.display()));
        }
        output.to_path_buf()
    };
    let targets: Vec<(PathBuf, &str)> = exported
        .files
        .iter()
        .map(|file| {
            let path = if xml_path && file.name == "project.xml" { output.to_path_buf() } else { dir.join(&file.name) };
            (path, file.content.as_str())
        })
        .collect();
    let protected = options.protect.and_then(|p| std::fs::canonicalize(p).ok());
    let mut existing = Vec::new();
    for (path, _) in &targets {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(format!("could not inspect {}: {err}", path.display())),
        };
        if !metadata.file_type().is_file() {
            return Err(format!("export target {} is not a regular file; choose another output", path.display()));
        }
        if protected.is_some() && std::fs::canonicalize(path).ok() == protected {
            return Err(format!("refusing to overwrite the input project file {}; choose another output", path.display()));
        }
        existing.push(path.display().to_string());
    }
    if !existing.is_empty() && !options.overwrite {
        return Err(format!(
            "refusing to overwrite existing file(s): {}. Choose an empty output or request overwrite (--force / \"overwrite\": true)",
            existing.join(", ")
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| format!("could not name export staging directory: {err}"))?.as_nanos();
    let staging = dir.join(format!(".flashmob-export-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|err| format!("could not create {}: {err}", staging.display()))?;
    let result = (|| {
        for (i, (path, content)) in targets.iter().enumerate() {
            std::fs::write(staging.join(i.to_string()), content)
                .map_err(|err| format!("could not stage {}: {err}", path.display()))?;
        }
        for (i, (path, _)) in targets.iter().enumerate() {
            std::fs::rename(staging.join(i.to_string()), path)
                .map_err(|err| format!("could not replace {}: {err}", path.display()))?;
        }
        Ok(())
    })();
    let cleanup = std::fs::remove_dir_all(&staging)
        .map_err(|err| format!("could not remove export staging directory {}: {err}", staging.display()));
    match (result, cleanup) {
        (Err(err), Err(cleanup)) => return Err(format!("{err}; {cleanup}")),
        (Err(err), _) | (_, Err(err)) => return Err(err),
        _ => {}
    }
    Ok(dir)
}

/// Unique, sanitized component names. One namespace covers every exported
/// component so a `ProtectedBranch` or bus reference resolves to exactly one row.
struct Names {
    by_id: HashMap<String, String>,
}

impl Names {
    fn new(project: &Project) -> Self {
        let mut used = HashSet::new();
        let mut by_id = HashMap::new();
        let mut add = |id: &str, name: &str| {
            let unique = unique_name(name, id, &mut used);
            by_id.entry(id.to_string()).or_insert(unique);
        };
        for x in &project.buses { add(&x.id, &x.name); }
        for x in &project.branches { add(&x.id, &x.name); }
        for x in &project.sources { add(&x.id, &x.name); }
        for x in &project.loads { add(&x.id, &x.name); }
        for x in &project.motors { add(&x.id, &x.name); }
        for x in &project.devices { add(&x.id, &x.name); }
        for x in &project.switches { add(&x.id, &x.name); }
        for x in &project.equipment { add(&x.id, &x.name); }
        Self { by_id }
    }

    fn of(&self, id: &str) -> String {
        self.by_id.get(id).cloned().unwrap_or_else(|| sanitize(id))
    }
}

/// Transformer terminals identified by winding voltage, independent of the
/// branch's drawing direction. Primary is always the HV winding.
struct XfmrEnds<'a> {
    pri_bus: &'a str,
    sec_bus: &'a str,
    pri_conn: &'static str,
    sec_conn: &'static str,
    pri_ground: GroundingFields,
    sec_ground: GroundingFields,
}

struct GroundingFields {
    method: &'static str,
    r: String,
    x: String,
}

fn grounding_fields(project: &Project, id: &str, winding: Option<Winding>, delta: bool) -> GroundingFields {
    if delta { return GroundingFields { method: "N/A", r: String::new(), x: String::new() }; }
    let (method, ohms) = crate::network::resolved_neutral(project, id, winding);
    let (method, r, x) = match method {
        GroundingMethod::Solid => ("Solid", String::new(), String::new()),
        GroundingMethod::Ungrounded => ("Ungrounded", String::new(), String::new()),
        GroundingMethod::Impedance => ("Impedance", fmt(ohms.re), fmt(ohms.im)),
    };
    GroundingFields { method, r, x }
}

fn grounded_connection(connection: &'static str, grounding: &GroundingFields) -> &'static str {
    if connection == "Delta" { return connection; }
    match grounding.method {
        "Ungrounded" => "Wye-Ungrounded",
        "Impedance" => "Wye-Impedance",
        _ => "Wye-Ground",
    }
}

fn xfmr_ends<'a>(project: &Project, branch: &'a Branch, hv_kv: f64, lv_kv: f64, connection: XfmrConn) -> XfmrEnds<'a> {
    let (from_conn, to_conn) = connections(connection);
    let from_kv = bus_kv(project, &branch.from);
    let to_kv = bus_kv(project, &branch.to);
    // Same rule as the network builder: each bus takes the winding nearest its kV.
    let near_hv = |kv: f64| (kv / hv_kv).ln().abs() <= (kv / lv_kv).ln().abs();
    let (from_hv, to_hv) = (near_hv(from_kv), near_hv(to_kv));
    let from_is_primary = if from_hv != to_hv { from_hv } else { from_kv >= to_kv };
    let (pri_bus, sec_bus, pri_conn, sec_conn) = if from_is_primary {
        (&branch.from, &branch.to, from_conn, to_conn)
    } else {
        (&branch.to, &branch.from, to_conn, from_conn)
    };
    let pri_ground = grounding_fields(project, &branch.id, Some(Winding::Hv), pri_conn == "Delta");
    let sec_ground = grounding_fields(project, &branch.id, Some(Winding::Lv), sec_conn == "Delta");
    XfmrEnds { pri_bus, sec_bus, pri_conn: grounded_connection(pri_conn, &pri_ground),
        sec_conn: grounded_connection(sec_conn, &sec_ground), pri_ground, sec_ground }
}

/// FromBus/ToBus as exported: transformers HV to LV, other branches as entered.
fn branch_ends<'a>(project: &Project, branch: &'a Branch) -> (&'a str, &'a str) {
    match &branch.kind {
        BranchKind::Transformer { hv_kv, lv_kv, connection, .. } => {
            let ends = xfmr_ends(project, branch, *hv_kv, *lv_kv, *connection);
            (ends.pri_bus, ends.sec_bus)
        }
        BranchKind::Line { .. } => (&branch.from, &branch.to),
    }
}

fn branch_switch<'a>(project: &'a Project, branch: &Branch) -> Option<&'a crate::model::Switch> {
    project.switches.iter().find(|s| s.branch_id == branch.id)
}

fn normal_state(project: &Project, branch: &Branch) -> &'static str {
    if project.branch_closed(branch) { "Closed" } else { "Open" }
}

fn service_status(in_service: bool) -> &'static str {
    if in_service { "In Service" } else { "Out of Service" }
}

fn switch_kind(kind: crate::model::SwitchKind) -> &'static str {
    match kind {
        crate::model::SwitchKind::Breaker => "Breaker",
        crate::model::SwitchKind::Tie => "Tie",
        crate::model::SwitchKind::Ats => "ATS",
    }
}

const SOURCE_KIND_NOTE: &str = "Flashmob does not store utility vs generator; classify in the target tool. IsSlack is the load-flow reference role only.";

fn xml(project: &Project, names: &Names) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <FlashmobEngineeringData Source=\"Flashmob\" Version=\"1.1\" Purpose=\"Engineering mapping\">\n\
         <Project Name=\"{}\" BaseMVA=\"{}\" FrequencyHz=\"60\" ArcDurationCapSec=\"{}\" OperatingCases=\"{}\">\n\
         <Components>\n",
        xml_escape(&project.name),
        fmt(project.s_base_mva),
        fmt(project.arc_duration_cap_s),
        project.operating_cases.len(),
    ));
    for bus in &project.buses {
        let equip = bus_equipment(project, &bus.id);
        body.push_str(&format!(
            "  <Component Type=\"Bus\" Name=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.of(&bus.id)),
            attrs(&[
                ("NominalSystemVoltage", "V", fmt_scaled(bus.kv, 3)),
                ("ShuntKVAR", "kvar", fmt(bus.shunt_kvar)),
                ("EquipmentType", "", equip.kind.to_string()),
                ("ElectrodeConfiguration", "", equip.electrode.to_string()),
                ("Gap", "mm", equip.gap),
                ("WorkingDistance", "mm", equip.distance),
                ("EnclosureHeight", "mm", equip.height),
                ("EnclosureWidth", "mm", equip.width),
                ("EnclosureDepth", "mm", equip.depth),
                ("EquipmentBasis", "", equip.basis.to_string()),
                ("ArcEquipmentCount", "", equip.count.to_string()),
            ]),
        ));
    }
    for item in &project.equipment {
        body.push_str(&format!(
            "  <Component Type=\"ArcEquipment\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.of(&item.id)),
            xml_escape(&names.of(&item.bus)),
            attrs(&[
                ("EquipmentType", "", equipment_type(project, &item.bus, item.electrode).to_string()),
                ("ElectrodeConfiguration", "", item.electrode.as_str().to_string()),
                ("Gap", "mm", fmt(item.gap_mm)),
                ("WorkingDistance", "mm", fmt(item.distance_mm)),
                ("EnclosureHeight", "mm", fmt(item.height_mm)),
                ("EnclosureWidth", "mm", fmt(item.width_mm)),
                ("EnclosureDepth", "mm", fmt(item.depth_mm)),
                ("UpstreamDevice", "", item.upstream_device.as_deref().map(|d| names.of(d)).unwrap_or_default()),
                ("ClearingTime", "s", item.clearing_s.map(fmt).unwrap_or_default()),
                ("FallbackDuration", "s", item.fallback_duration_s.map(fmt).unwrap_or_default()),
            ]),
        ));
    }
    for source in &project.sources {
        let ground = grounding_fields(project, &source.id, None, false);
        body.push_str(&format!(
            "  <Component Type=\"Source\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.of(&source.id)),
            xml_escape(&names.of(&source.bus)),
            attrs(&[
                ("ServiceStatus", "", service_status(source.in_service).into()),
                ("IsSlack", "", source.is_slack.to_string()),
                ("SourceKindNote", "", SOURCE_KIND_NOTE.into()),
                ("GroundingMethod", "", ground.method.into()),
                ("NeutralR_ohm", "ohm", ground.r),
                ("NeutralX_ohm", "ohm", ground.x),
                ("NominalSystemVoltage", "V", fmt_scaled(bus_kv(project, &source.bus), 3)),
                ("VoltagePU", "pu", fmt(source.v_pu)),
                ("Angle", "deg", fmt(source.angle_deg)),
                ("ThreePhaseShortCircuitMVA", "MVA", fmt(source.mva_sc)),
                ("XR", "", fmt(source.xr)),
                ("X0OverX1", "", fmt(source.x0_over_x1)),
                ("R0OverR1", "", fmt(source.r0_over_r1)),
                ("P", "MW", fmt(source.p_mw)),
                ("DecrementCurvePoints", "", if source.decrement_curve.is_empty() { String::new() } else { source.decrement_curve.len().to_string() }),
            ]),
        ));
    }
    for branch in &project.branches {
        let state = normal_state(project, branch);
        match &branch.kind {
            BranchKind::Line { r_ohm, x_ohm, r0_ohm, x0_ohm, ampacity_a, b_siemens } => {
                let (r0, x0, note) = zero_seq(*r_ohm, *x_ohm, *r0_ohm, *x0_ohm);
                body.push_str(&format!(
                    "  <Component Type=\"Cable\" Name=\"{}\" FromBus=\"{}\" ToBus=\"{}\">\n{}  </Component>\n",
                    xml_escape(&names.of(&branch.id)),
                    xml_escape(&names.of(&branch.from)),
                    xml_escape(&names.of(&branch.to)),
                    attrs(&[
                        ("R1", "ohm", fmt(*r_ohm)),
                        ("X1", "ohm", fmt(*x_ohm)),
                        ("R0", "ohm", fmt(r0)),
                        ("X0", "ohm", fmt(x0)),
                        ("B", "S", fmt(*b_siemens)),
                        ("Ampacity", "A", ampacity_a.map(fmt).unwrap_or_default()),
                        ("ImpedanceUnit", "", "ohm total".into()),
                        ("ZeroSequenceNote", "", note.into()),
                        ("NormalState", "", state.into()),
                    ]),
                ));
            }
            BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, connection, tap_percent, x0_over_x1, r0_over_r1 } => {
                let (pct_r, pct_x) = percent_rx(*z_percent, *xr);
                let ends = xfmr_ends(project, branch, *hv_kv, *lv_kv, *connection);
                body.push_str(&format!(
                    "  <Component Type=\"Transformer\" Name=\"{}\" FromBus=\"{}\" ToBus=\"{}\">\n{}  </Component>\n",
                    xml_escape(&names.of(&branch.id)),
                    xml_escape(&names.of(ends.pri_bus)),
                    xml_escape(&names.of(ends.sec_bus)),
                    attrs(&[
                        ("NominalKVA", "kVA", fmt(*kva)),
                        ("PrimaryBus", "", names.of(ends.pri_bus)),
                        ("SecondaryBus", "", names.of(ends.sec_bus)),
                        ("PrimaryVoltage", "V", fmt_scaled(*hv_kv, 3)),
                        ("SecondaryVoltage", "V", fmt_scaled(*lv_kv, 3)),
                        ("PrimaryConnection", "", ends.pri_conn.into()),
                        ("SecondaryConnection", "", ends.sec_conn.into()),
                        ("PrimaryGroundingMethod", "", ends.pri_ground.method.into()),
                        ("PrimaryNeutralR_ohm", "ohm", ends.pri_ground.r),
                        ("PrimaryNeutralX_ohm", "ohm", ends.pri_ground.x),
                        ("SecondaryGroundingMethod", "", ends.sec_ground.method.into()),
                        ("SecondaryNeutralR_ohm", "ohm", ends.sec_ground.r),
                        ("SecondaryNeutralX_ohm", "ohm", ends.sec_ground.x),
                        ("PercentZ", "%", fmt(*z_percent)),
                        ("PercentR", "%", fmt(pct_r)),
                        ("PercentX", "%", fmt(pct_x)),
                        ("XR", "", fmt(*xr)),
                        ("X0OverX1", "", fmt(*x0_over_x1)),
                        ("R0OverR1", "", fmt(*r0_over_r1)),
                        ("PrimaryTap", "%", fmt(*tap_percent)),
                        ("NormalState", "", state.into()),
                    ]),
                ));
            }
        }
    }
    for switch in &project.switches {
        body.push_str(&format!(
            "  <Component Type=\"Switch\" Name=\"{}\" Branch=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.of(&switch.id)),
            xml_escape(&names.of(&switch.branch_id)),
            attrs(&[
                ("SwitchKind", "", switch_kind(switch.kind).into()),
                ("NormalState", "", if switch.closed { "Closed" } else { "Open" }.into()),
            ]),
        ));
    }
    for load in &project.loads {
        body.push_str(&format!(
            "  <Component Type=\"Load\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.of(&load.id)),
            xml_escape(&names.of(&load.bus)),
            attrs(&[("kW", "kW", fmt(load.kw)), ("kVAR", "kvar", fmt(load.kvar)), ("LoadType", "", "Constant kVA".into())]),
        ));
    }
    for motor in &project.motors {
        body.push_str(&format!(
            "  <Component Type=\"Motor\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.of(&motor.id)),
            xml_escape(&names.of(&motor.bus)),
            attrs(&[
                ("HP", "hp", fmt(motor.hp)),
                ("Voltage", "V", fmt_scaled(motor.kv, 3)),
                ("PowerFactor", "", fmt(motor.pf)),
                ("Efficiency", "", fmt(motor.efficiency)),
                ("SubtransientReactance", "pu", fmt(motor.x_subtransient_pu)),
                ("XR", "", fmt(motor.xr)),
            ]),
        ));
    }
    for device in &project.devices {
        let f = device_fields(&device.curve, device.fuse_total_clearing);
        body.push_str(&format!(
            "  <Component Type=\"ProtectiveDevice\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.of(&device.id)),
            xml_escape(&names.of(&device.bus)),
            attrs(&[
                ("DeviceType", "", f.kind.into()),
                ("Curve", "", f.curve),
                ("Pickup", "A", f.pickup),
                ("TimeDial", "", f.time_dial),
                ("Delay", "s", f.delay_s),
                ("DelayBasis", "", f.delay_basis.into()),
                ("ShortTimePickup", "A", f.st_pickup),
                ("ShortTimeDelay", "s", f.st_delay),
                ("InstantaneousPickup", "A", f.inst),
                ("InstantaneousTime", "s", f.inst_s),
                ("ProtectedBranch", "", device.protected_branch.as_deref().map(|b| names.of(b)).unwrap_or_default()),
                ("BreakerInterruptingTime", "s", device.breaker_interrupting_s.map(fmt).unwrap_or_default()),
                ("FuseTotalClearingCurve", "", device.fuse_total_clearing.to_string()),
            ]),
        ));
    }
    body.push_str("</Components>\n<Connections>\n");
    for branch in &project.branches {
        let kind = match branch.kind {
            BranchKind::Line { .. } => "Cable",
            BranchKind::Transformer { .. } => "Transformer",
        };
        let (from, to) = branch_ends(project, branch);
        let switch = branch_switch(project, branch);
        body.push_str(&format!(
            "  <Connection Name=\"{}\" Type=\"{kind}\" FromBus=\"{}\" ToBus=\"{}\" NormalState=\"{}\" Switch=\"{}\"/>\n",
            xml_escape(&names.of(&branch.id)),
            xml_escape(&names.of(from)),
            xml_escape(&names.of(to)),
            normal_state(project, branch),
            xml_escape(&switch.map(|s| names.of(&s.id)).unwrap_or_default()),
        ));
    }
    body.push_str("</Connections>\n</Project>\n</FlashmobEngineeringData>\n");
    body
}

fn attrs(rows: &[(&str, &str, String)]) -> String {
    let mut out = String::new();
    for (name, unit, value) in rows {
        if value.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "    <Attribute Name=\"{name}\" Unit=\"{}\">{}</Attribute>\n",
            xml_escape(unit),
            xml_escape(value),
        ));
    }
    out
}

fn buses_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name",
        "NominalSystemVoltage",
        "VoltageUnit",
        "ShuntKVAR",
        "EquipmentType",
        "ElectrodeConfiguration",
        "Gap",
        "GapUnit",
        "WorkingDistance",
        "WorkingDistanceUnit",
        "EnclosureHeight",
        "EnclosureWidth",
        "EnclosureDepth",
        "EnclosureUnit",
        "EquipmentBasis",
        "ArcEquipmentCount",
    ]);
    for bus in &project.buses {
        let equip = bus_equipment(project, &bus.id);
        s.push_str(&csv_line(&[
            &names.of(&bus.id),
            &fmt_scaled(bus.kv, 3),
            "V",
            &fmt(bus.shunt_kvar),
            equip.kind,
            equip.electrode,
            &equip.gap,
            "mm",
            &equip.distance,
            "mm",
            &equip.height,
            &equip.width,
            &equip.depth,
            "mm",
            equip.basis,
            &equip.count.to_string(),
        ]));
    }
    s
}

fn arc_equipment_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name", "Bus", "EquipmentType", "ElectrodeConfiguration", "GapMM", "WorkingDistanceMM", "EnclosureHeightMM", "EnclosureWidthMM", "EnclosureDepthMM",
        "UpstreamDevice", "ClearingTimeS", "FallbackDurationS", "Basis",
    ]);
    for item in &project.equipment {
        s.push_str(&csv_line(&[
            &names.of(&item.id),
            &names.of(&item.bus),
            equipment_type(project, &item.bus, item.electrode),
            item.electrode.as_str(),
            &fmt(item.gap_mm),
            &fmt(item.distance_mm),
            &fmt(item.height_mm),
            &fmt(item.width_mm),
            &fmt(item.depth_mm),
            &item.upstream_device.as_deref().map(|d| names.of(d)).unwrap_or_default(),
            &item.clearing_s.map(fmt).unwrap_or_default(),
            &item.fallback_duration_s.map(fmt).unwrap_or_default(),
            &item.basis,
        ]));
    }
    s
}

fn cables_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name", "FromBus", "ToBus", "R1", "X1", "R0", "X0", "B", "ImpedanceUnit", "BUnit", "Ampacity", "AmpacityUnit", "ZeroSequenceNote", "NormalState",
    ]);
    for branch in &project.branches {
        let BranchKind::Line { r_ohm, x_ohm, r0_ohm, x0_ohm, ampacity_a, b_siemens } = &branch.kind else { continue };
        let (r0, x0, note) = zero_seq(*r_ohm, *x_ohm, *r0_ohm, *x0_ohm);
        s.push_str(&csv_line(&[
            &names.of(&branch.id),
            &names.of(&branch.from),
            &names.of(&branch.to),
            &fmt(*r_ohm),
            &fmt(*x_ohm),
            &fmt(r0),
            &fmt(x0),
            &fmt(*b_siemens),
            "ohm total",
            "S",
            &ampacity_a.map(fmt).unwrap_or_default(),
            "A",
            note,
            normal_state(project, branch),
        ]));
    }
    s
}

fn transformers_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name",
        "FromBus",
        "ToBus",
        "PrimaryBus",
        "SecondaryBus",
        "NominalKVA",
        "PrimaryVoltage",
        "SecondaryVoltage",
        "VoltageUnit",
        "PrimaryConnection",
        "SecondaryConnection",
        "PrimaryGroundingMethod", "PrimaryNeutralR_ohm", "PrimaryNeutralX_ohm",
        "SecondaryGroundingMethod", "SecondaryNeutralR_ohm", "SecondaryNeutralX_ohm",
        "PercentZ",
        "PercentR",
        "PercentX",
        "XR",
        "X0OverX1",
        "R0OverR1",
        "PrimaryTapPercent",
        "NormalState",
    ]);
    for branch in &project.branches {
        let BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, connection, tap_percent, x0_over_x1, r0_over_r1 } = &branch.kind else {
            continue;
        };
        let (pct_r, pct_x) = percent_rx(*z_percent, *xr);
        let ends = xfmr_ends(project, branch, *hv_kv, *lv_kv, *connection);
        let (pri, sec) = (names.of(ends.pri_bus), names.of(ends.sec_bus));
        s.push_str(&csv_line(&[
            &names.of(&branch.id),
            &pri,
            &sec,
            &pri,
            &sec,
            &fmt(*kva),
            &fmt_scaled(*hv_kv, 3),
            &fmt_scaled(*lv_kv, 3),
            "V",
            ends.pri_conn,
            ends.sec_conn,
            ends.pri_ground.method, &ends.pri_ground.r, &ends.pri_ground.x,
            ends.sec_ground.method, &ends.sec_ground.r, &ends.sec_ground.x,
            &fmt(*z_percent),
            &fmt(pct_r),
            &fmt(pct_x),
            &fmt(*xr),
            &fmt(*x0_over_x1),
            &fmt(*r0_over_r1),
            &fmt(*tap_percent),
            normal_state(project, branch),
        ]));
    }
    s
}

fn utilities_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name", "Type", "ServiceStatus", "IsSlack", "Bus", "NominalSystemVoltage", "VoltageUnit", "VoltagePU", "AngleDeg", "ThreePhaseShortCircuitMVA", "XR",
        "X0OverX1", "R0OverR1", "PMW", "DecrementCurvePoints", "SourceKindNote",
        "GroundingMethod", "NeutralR_ohm", "NeutralX_ohm",
    ]);
    for source in &project.sources {
        let ground = grounding_fields(project, &source.id, None, false);
        s.push_str(&csv_line(&[
            &names.of(&source.id),
            "Source",
            service_status(source.in_service),
            &source.is_slack.to_string(),
            &names.of(&source.bus),
            &fmt_scaled(bus_kv(project, &source.bus), 3),
            "V",
            &fmt(source.v_pu),
            &fmt(source.angle_deg),
            &fmt(source.mva_sc),
            &fmt(source.xr),
            &fmt(source.x0_over_x1),
            &fmt(source.r0_over_r1),
            &fmt(source.p_mw),
            &source.decrement_curve.len().to_string(),
            SOURCE_KIND_NOTE,
            ground.method, &ground.r, &ground.x,
        ]));
    }
    s
}

fn loads_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Name", "Bus", "kW", "kVAR", "LoadType"]);
    for load in &project.loads {
        s.push_str(&csv_line(&[&names.of(&load.id), &names.of(&load.bus), &fmt(load.kw), &fmt(load.kvar), "Constant kVA"]));
    }
    s
}

fn motors_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Name", "Bus", "HP", "Voltage", "VoltageUnit", "PowerFactor", "Efficiency", "SubtransientReactancePU", "XR"]);
    for motor in &project.motors {
        s.push_str(&csv_line(&[
            &names.of(&motor.id),
            &names.of(&motor.bus),
            &fmt(motor.hp),
            &fmt_scaled(motor.kv, 3),
            "V",
            &fmt(motor.pf),
            &fmt(motor.efficiency),
            &fmt(motor.x_subtransient_pu),
            &fmt(motor.xr),
        ]));
    }
    s
}

fn devices_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name",
        "Bus",
        "DeviceType",
        "Curve",
        "PickupA",
        "TimeDial",
        "DelayS",
        "DelayBasis",
        "ShortTimePickupA",
        "ShortTimeDelayS",
        "InstantaneousPickupA",
        "InstantaneousTimeS",
        "ProtectedBranch",
        "BreakerInterruptingTimeS",
        "FuseTotalClearingCurve",
    ]);
    for device in &project.devices {
        let f = device_fields(&device.curve, device.fuse_total_clearing);
        s.push_str(&csv_line(&[
            &names.of(&device.id),
            &names.of(&device.bus),
            f.kind,
            &f.curve,
            &f.pickup,
            &f.time_dial,
            &f.delay_s,
            f.delay_basis,
            &f.st_pickup,
            &f.st_delay,
            &f.inst,
            &f.inst_s,
            &device.protected_branch.as_deref().map(|b| names.of(b)).unwrap_or_default(),
            &device.breaker_interrupting_s.map(fmt).unwrap_or_default(),
            &device.fuse_total_clearing.to_string(),
        ]));
    }
    s
}

fn switches_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Name", "Branch", "SwitchKind", "NormalState"]);
    for switch in &project.switches {
        s.push_str(&csv_line(&[
            &names.of(&switch.id),
            &names.of(&switch.branch_id),
            switch_kind(switch.kind),
            if switch.closed { "Closed" } else { "Open" },
        ]));
    }
    s
}

fn connections_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Name", "Type", "FromBus", "ToBus", "NormalState", "Switch", "SwitchKind"]);
    for branch in &project.branches {
        let kind = match branch.kind {
            BranchKind::Line { .. } => "Cable",
            BranchKind::Transformer { .. } => "Transformer",
        };
        let (from, to) = branch_ends(project, branch);
        let switch = branch_switch(project, branch);
        s.push_str(&csv_line(&[
            &names.of(&branch.id),
            kind,
            &names.of(from),
            &names.of(to),
            normal_state(project, branch),
            &switch.map(|s| names.of(&s.id)).unwrap_or_default(),
            switch.map(|s| switch_kind(s.kind)).unwrap_or(""),
        ]));
    }
    s
}

/// The LV winding connection that supplies a bus: the transformer secondary on
/// the bus itself, or reached through cables only (same voltage system).
fn supplying_lv_connection(project: &Project, bus: &str) -> Option<&'static str> {
    let mut seen = HashSet::from([bus.to_string()]);
    let mut queue = std::collections::VecDeque::from([bus.to_string()]);
    while let Some(at) = queue.pop_front() {
        for branch in &project.branches {
            match &branch.kind {
                BranchKind::Transformer { hv_kv, lv_kv, connection, .. } => {
                    let ends = xfmr_ends(project, branch, *hv_kv, *lv_kv, *connection);
                    if ends.sec_bus == at {
                        return Some(ends.sec_conn);
                    }
                }
                BranchKind::Line { .. } => {
                    let next = if branch.from == at { &branch.to } else if branch.to == at { &branch.from } else { continue };
                    if seen.insert(next.clone()) {
                        queue.push_back(next.clone());
                    }
                }
            }
        }
    }
    None
}

/// Revit distribution system, phases, and wires from the supplying winding.
fn distribution_system(project: &Project, bus: &crate::model::Bus) -> (String, &'static str, &'static str) {
    let volts = (bus.kv * 1000.0).round();
    match supplying_lv_connection(project, &bus.id) {
        Some("Wye-Ground" | "Wye-Ungrounded" | "Wye-Impedance") => (format!("{volts:.0}Y/{:.0} V", (volts / 3f64.sqrt()).round()), "3", "4"),
        Some(_) => (format!("{volts:.0} V Delta"), "3", "3"),
        // No transformer secondary supplies this bus; the grounding is not modeled.
        None => (format!("{volts:.0} V"), "3", ""),
    }
}

fn revit_panels_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Panel Name",
        "Distribution System",
        "Voltage",
        "Number of Phases",
        "Wires",
        "Apparent Load",
        "Load Unit",
        "Study_EquipmentType",
        "Study_ElectrodeConfiguration",
        "Study_Gap_mm",
        "Study_WorkingDistance_mm",
    ]);
    for bus in &project.buses {
        let (kw, kvar) = bus_load(project, &bus.id);
        let kva = (kw * kw + kvar * kvar).sqrt();
        let equip = bus_equipment(project, &bus.id);
        let (system, phases, wires) = distribution_system(project, bus);
        s.push_str(&csv_line(&[
            &names.of(&bus.id),
            &system,
            &fmt_scaled(bus.kv, 3),
            phases,
            wires,
            &fmt(kva),
            "kVA",
            equip.kind,
            equip.electrode,
            &equip.gap,
            &equip.distance,
        ]));
    }
    s
}

fn revit_circuits_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Circuit", "Panel", "Load Name", "From", "To", "Type", "kVA", "Primary Voltage", "Secondary Voltage", "NormalState"]);
    for branch in &project.branches {
        let (from, to) = branch_ends(project, branch);
        match &branch.kind {
            BranchKind::Transformer { kva, hv_kv, lv_kv, .. } => {
                s.push_str(&csv_line(&[
                    &names.of(&branch.id),
                    &names.of(from),
                    &names.of(&branch.id),
                    &names.of(from),
                    &names.of(to),
                    "Transformer",
                    &fmt(*kva),
                    &fmt_scaled(*hv_kv, 3),
                    &fmt_scaled(*lv_kv, 3),
                    normal_state(project, branch),
                ]));
            }
            BranchKind::Line { .. } => {
                s.push_str(&csv_line(&[
                    &names.of(&branch.id),
                    &names.of(from),
                    &names.of(&branch.id),
                    &names.of(from),
                    &names.of(to),
                    "Cable",
                    "",
                    &fmt_scaled(bus_kv(project, from), 3),
                    &fmt_scaled(bus_kv(project, to), 3),
                    normal_state(project, branch),
                ]));
            }
        }
    }
    s
}

const TAB_COLUMNS: [&str; 24] = [
    "ComponentType",
    "Name",
    "FromBus",
    "ToBus",
    "NominalSystemVoltage",
    "NominalKVA",
    "ShortCircuitKVA",
    "PercentZ",
    "XR",
    "kW",
    "kVAR",
    "R1",
    "X1",
    "NormalState",
    "ServiceStatus",
    "GroundingMethod", "NeutralR_ohm", "NeutralX_ohm",
    "PrimaryGroundingMethod", "PrimaryNeutralR_ohm", "PrimaryNeutralX_ohm",
    "SecondaryGroundingMethod", "SecondaryNeutralR_ohm", "SecondaryNeutralX_ohm",
];

fn components_tab(project: &Project, names: &Names) -> String {
    let row = |cols: &[&str]| {
        let mut line = cols.iter().map(|c| tab_cell(c)).collect::<Vec<_>>().join("\t");
        for _ in cols.len()..TAB_COLUMNS.len() { line.push('\t'); }
        line.push('\n');
        line
    };
    let mut s = row(&TAB_COLUMNS);
    for bus in &project.buses {
        s.push_str(&row(&["Bus", &names.of(&bus.id), "", "", &fmt_scaled(bus.kv, 3), "", "", "", "", "", "", "", "", "", ""]));
    }
    for source in &project.sources {
        let ground = grounding_fields(project, &source.id, None, false);
        s.push_str(&row(&[
            "Source",
            &names.of(&source.id),
            &names.of(&source.bus),
            "",
            &fmt_scaled(bus_kv(project, &source.bus), 3),
            "",
            &fmt_scaled(source.mva_sc, 3),
            "",
            &fmt(source.xr),
            "",
            "",
            "",
            "",
            "",
            service_status(source.in_service),
            ground.method, &ground.r, &ground.x,
        ]));
    }
    for branch in &project.branches {
        let state = normal_state(project, branch);
        let (from, to) = branch_ends(project, branch);
        match &branch.kind {
            BranchKind::Line { r_ohm, x_ohm, .. } => s.push_str(&row(&[
                "Cable",
                &names.of(&branch.id),
                &names.of(from),
                &names.of(to),
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                &fmt(*r_ohm),
                &fmt(*x_ohm),
                state,
                "",
            ])),
            BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, connection, .. } => {
                let ends = xfmr_ends(project, branch, *hv_kv, *lv_kv, *connection);
                s.push_str(&row(&[
                "Transformer",
                &names.of(&branch.id),
                &names.of(from),
                &names.of(to),
                "",
                &fmt(*kva),
                "",
                &fmt(*z_percent),
                &fmt(*xr),
                "",
                "",
                "",
                "",
                state,
                "",
                "", "", "",
                ends.pri_ground.method, &ends.pri_ground.r, &ends.pri_ground.x,
                ends.sec_ground.method, &ends.sec_ground.r, &ends.sec_ground.x,
            ]));
            }
        }
    }
    for load in &project.loads {
        s.push_str(&row(&[
            "Load",
            &names.of(&load.id),
            &names.of(&load.bus),
            "",
            "",
            "",
            "",
            "",
            "",
            &fmt(load.kw),
            &fmt(load.kvar),
            "",
            "",
            "",
            "",
        ]));
    }
    s
}

fn import_notes(project: &Project) -> String {
    let cases = if project.operating_cases.is_empty() {
        "This project has no named operating cases.".to_string()
    } else {
        format!(
            "This project has {} named operating case(s): {}. They are stored only in\n\
             project.json (operating_cases); no CSV or XML row represents them.",
            project.operating_cases.len(),
            project.operating_cases.iter().map(|c| strip_controls(&c.id)).collect::<Vec<_>>().join(", ")
        )
    };
    format!(
        "\
Flashmob engineering data package for mapping
Project: {name}

This package has not been round-tripped through any target study tool.
It is not a verified import. The XML is a Flashmob representation;
no vendor XML schema or compatibility is claimed. An engineer must map and
verify the data against the selected target tool version.

  project.json              complete Flashmob model, including all protection metadata
  project.xml               one file with every component and connection
  buses.csv                 NominalSystemVoltage in volts, IEEE 1584 enclosure
  arc-equipment.csv         every arc-flash equipment record (several per bus allowed)
  cables.csv                R and X in total ohms, B in siemens
  transformers.csv          NominalKVA, %Z, %R, %X, X/R, winding connection
  utilities.csv             every source: service status, slack role, short-circuit MVA, X/R
  loads.csv                 constant kVA loads
  motors.csv                horsepower and subtransient reactance
  protective-devices.csv    pickup, time dial, delays, short time, instantaneous
  switches.csv              switches, their branch, kind, and normal state
  connections.csv           from bus, to bus, normal state, and controlling switch
  components.tab            summary, tab delimited
  revit-panels.csv          Revit panel schedule columns plus Study_ fields
  revit-circuits.csv        Revit circuit connections

Units
  Voltages are in volts. Transformer percent impedance is on the transformer
  kVA base. Cable impedance is the total ohms entered in Flashmob, not ohms
  per thousand feet. Numbers use the shortest text that round-trips the
  stored value; very small or large values use exponent notation (2.5e-7).

Names
  Every component name is unique across the package (case-insensitive).
  When names collide, later components get a -2, -3 ... suffix. Bus,
  ProtectedBranch, UpstreamDevice, Switch, and Branch references use these
  same names. Text cells that begin with = + - @ are prefixed with a single
  quote so spreadsheets do not evaluate them.

Transformers
  Primary is always the HV winding and Secondary the LV winding, regardless
  of the Flashmob branch drawing direction. For transformers, FromBus is the
  primary (HV) bus and ToBus the secondary (LV) bus; PrimaryConnection and
  SecondaryConnection belong to those buses. PrimaryTapPercent changes HV
  winding turns.

Neutral grounding
  GroundingMethod is Solid, Impedance, or Ungrounded for sources. Transformer
  winding columns/attributes use Primary (HV) and Secondary (LV) prefixes:
  PrimaryGroundingMethod, PrimaryNeutralR_ohm, PrimaryNeutralX_ohm and the
  corresponding Secondary fields. Delta grounding is N/A. NeutralR_ohm and
  NeutralX_ohm are physical ohms (not 3Zn or per unit), blank for solid,
  ungrounded, and delta. Omitted neutral_grounding entries resolve to Solid,
  as in the study network. Wye connections are Wye-Ground, Wye-Ungrounded,
  or Wye-Impedance according to the resolved neutral method.

Normal operating state
  NormalState is Closed or Open for each branch. A branch with no switch is
  always Closed. Opening a Flashmob switch removes its entire branch from the
  model. ServiceStatus marks sources that are out of service in the normal
  state. Flashmob does not record whether a source is a utility or a
  generator: Type is Source and IsSlack is only its load-flow reference role.
  Generator decrement curves are counted in DecrementCurvePoints and stored
  only in project.json.

Operating cases
  {cases}
  Switch groups (interlocks) and per-case equipment states are likewise only
  in project.json. Map alternate cases to target-tool scenarios manually.

Protective devices
  A protective device is exported as data, not as a vendor library curve.
  TimeDial is dimensionless (IEC TMS or IEEE TD). DelayS is seconds: the
  definite-time delay, or for a thermal-magnetic breaker the long-time trip
  time at 6x long-time pickup (DelayBasis says which). ShortTimePickupA and
  ShortTimeDelayS describe the short-time element. Fuse curves are marked
  FuseTotalClearingCurve = true.

Arc-flash equipment
  buses.csv carries the first equipment record entered on the bus, or a
  typical assumed enclosure when none is entered (EquipmentBasis says which,
  ArcEquipmentCount gives the number of records). arc-equipment.csv lists all
  records.

Revit panels
  Distribution System is derived from the LV connection of the transformer
  that supplies the bus through cables: wye-ground gives V_LL Y / V_LN with
  4 wires, delta gives 3 wires. Wires is blank when no transformer supplies
  the bus.

Map Name to the component name and FromBus / ToBus to the connected buses.
",
        name = strip_controls(&project.name),
    )
}

struct BusEquipment {
    kind: &'static str,
    electrode: &'static str,
    gap: String,
    distance: String,
    height: String,
    width: String,
    depth: String,
    basis: &'static str,
    count: usize,
}

/// Bus-level enclosure columns: the first entered record, or a typical assumed enclosure.
fn bus_equipment(project: &Project, bus: &str) -> BusEquipment {
    let count = project.equipment.iter().filter(|item| item.bus == bus).count();
    if let Some(item) = project.equipment.iter().find(|item| item.bus == bus) {
        BusEquipment {
            kind: equipment_type(project, bus, item.electrode),
            electrode: item.electrode.as_str(),
            gap: fmt(item.gap_mm),
            distance: fmt(item.distance_mm),
            height: fmt(item.height_mm),
            width: fmt(item.width_mm),
            depth: fmt(item.depth_mm),
            basis: if count > 1 { "entered (first of several; see arc-equipment.csv)" } else { "entered" },
            count,
        }
    } else {
        let (electrode, gap, distance, height, width, depth, _) = crate::model::typical_equipment(bus_kv(project, bus));
        BusEquipment {
            kind: equipment_type(project, bus, electrode),
            electrode: electrode.as_str(),
            gap: fmt(gap),
            distance: fmt(distance),
            height: fmt(height),
            width: fmt(width),
            depth: fmt(depth),
            basis: "typical (assumed)",
            count,
        }
    }
}

fn equipment_type(project: &Project, bus: &str, electrode: Electrode) -> &'static str {
    let name = project.bus(bus).map(|bus| bus.name.to_ascii_uppercase()).unwrap_or_default();
    match electrode {
        Electrode::VOA | Electrode::HOA => "AIR",
        _ if name.contains("MCC") => "MCC",
        _ if name.contains("MDP") || name.contains("SWG") || name.contains("SWITCH") || bus_kv(project, bus) > 1.0 || electrode == Electrode::VCBB => "SWG",
        _ => "PNL",
    }
}

struct DeviceFields {
    kind: &'static str,
    curve: String,
    pickup: String,
    /// Dimensionless multiplier (IEC TMS or IEEE TD).
    time_dial: String,
    /// Seconds (definite time, or thermal-magnetic long-time delay at 6x pickup).
    delay_s: String,
    delay_basis: &'static str,
    st_pickup: String,
    st_delay: String,
    inst: String,
    inst_s: String,
}

fn device_fields(curve: &CurveSpec, fuse: bool) -> DeviceFields {
    let opt = |v: &Option<f64>| v.map(fmt).unwrap_or_default();
    let mut f = match curve {
        CurveSpec::Iec { kind, pickup_a, tms, inst_a, inst_s } => DeviceFields {
            kind: "IEC Relay",
            curve: format!("{kind:?}"),
            pickup: fmt(*pickup_a),
            time_dial: fmt(*tms),
            delay_s: String::new(),
            delay_basis: "",
            st_pickup: String::new(),
            st_delay: String::new(),
            inst: opt(inst_a),
            inst_s: opt(inst_s),
        },
        CurveSpec::Ieee { kind, pickup_a, td, inst_a, inst_s } => DeviceFields {
            kind: "IEEE Relay",
            curve: format!("{kind:?}"),
            pickup: fmt(*pickup_a),
            time_dial: fmt(*td),
            delay_s: String::new(),
            delay_basis: "",
            st_pickup: String::new(),
            st_delay: String::new(),
            inst: opt(inst_a),
            inst_s: opt(inst_s),
        },
        CurveSpec::Definite { pickup_a, time_s } => DeviceFields {
            kind: "Definite Time",
            curve: "Definite".into(),
            pickup: fmt(*pickup_a),
            time_dial: String::new(),
            delay_s: fmt(*time_s),
            delay_basis: "definite time",
            st_pickup: String::new(),
            st_delay: String::new(),
            inst: String::new(),
            inst_s: String::new(),
        },
        CurveSpec::ThermalMagnetic { lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s } => DeviceFields {
            kind: "LV Breaker",
            curve: "Thermal-Magnetic".into(),
            pickup: fmt(*lt_pickup_a),
            time_dial: String::new(),
            delay_s: fmt(*lt_delay_s),
            delay_basis: "long-time trip time at 6x pickup",
            st_pickup: opt(st_pickup_a),
            st_delay: opt(st_delay_s),
            inst: opt(inst_a),
            inst_s: opt(inst_s),
        },
        CurveSpec::Points { points, max_points, .. } => DeviceFields {
            kind: "Point Curve",
            curve: match max_points {
                Some(max) => format!("Points (min {} / max {}; points in project.json)", points.len(), max.len()),
                None => format!("Points ({}; points in project.json)", points.len()),
            },
            pickup: max_points.as_deref().unwrap_or(points).first().map(|p| fmt(p[0])).unwrap_or_default(),
            time_dial: String::new(),
            delay_s: String::new(),
            delay_basis: "",
            st_pickup: String::new(),
            st_delay: String::new(),
            inst: String::new(),
            inst_s: String::new(),
        },
        CurveSpec::SettingsNotCollected { .. } => DeviceFields {
            kind: "Collected device",
            curve: "Settings not collected".into(),
            pickup: String::new(),
            time_dial: String::new(),
            delay_s: String::new(),
            delay_basis: "",
            st_pickup: String::new(),
            st_delay: String::new(),
            inst: String::new(),
            inst_s: String::new(),
        },
    };
    if fuse && !matches!(curve, CurveSpec::SettingsNotCollected { .. }) {
        f.kind = "Fuse";
    }
    f
}

/// (from-side, to-side) winding connection as entered on the branch.
fn connections(connection: XfmrConn) -> (&'static str, &'static str) {
    match connection {
        XfmrConn::Dyn => ("Delta", "Wye-Ground"),
        XfmrConn::Ynd => ("Wye-Ground", "Delta"),
        XfmrConn::Ynyn => ("Wye-Ground", "Wye-Ground"),
        XfmrConn::Dd => ("Delta", "Delta"),
    }
}

fn percent_rx(z_percent: f64, xr: f64) -> (f64, f64) {
    let den = (1.0 + xr * xr).sqrt();
    let r = z_percent / den;
    (r, r * xr)
}

fn zero_seq(r: f64, x: f64, r0: f64, x0: f64) -> (f64, f64, &'static str) {
    if r0.abs() < 1e-12 && x0.abs() < 1e-12 {
        (3.0 * r, 3.0 * x, "R0 and X0 were blank; exported as 3x the positive-sequence ohms")
    } else {
        (r0, x0, "")
    }
}

fn bus_kv(project: &Project, id: &str) -> f64 {
    project.bus(id).map(|bus| bus.kv).unwrap_or(0.0)
}

fn bus_load(project: &Project, id: &str) -> (f64, f64) {
    project.loads.iter().filter(|load| load.bus == id).fold((0.0, 0.0), |(p, q), load| (p + load.kw, q + load.kvar))
}

fn unique_name(name: &str, id: &str, used: &mut HashSet<String>) -> String {
    let base = sanitize(if name.trim().is_empty() { id } else { name });
    let mut candidate = base.clone();
    let mut n = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

fn sanitize(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ',' | '\n' | '\r' | '\t' => out.push('-'),
            c if c.is_control() => {}
            _ => out.push(ch),
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() { "UNNAMED".into() } else { trimmed.to_string() }
}

/// Remove C0 control characters other than tab, newline, and carriage return;
/// they are not allowed in XML 1.0 and confuse spreadsheet imports.
pub fn strip_controls(value: &str) -> String {
    value.chars().filter(|&c| !(c < '\u{20}' && c != '\t' && c != '\n' && c != '\r') && c != '\u{7f}').collect()
}

/// Spreadsheet formula-injection guard for text cells. Numbers (including
/// negative numbers) are left unchanged.
pub fn formula_guard(value: &str) -> String {
    let risky = match value.chars().next() {
        Some('=' | '+' | '@' | '\t' | '\r') => true,
        Some('-') => value.parse::<f64>().is_err(),
        _ => false,
    };
    if risky { format!("'{value}") } else { value.to_string() }
}

fn csv_line(fields: &[&str]) -> String {
    let mut line = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        let field = formula_guard(&strip_controls(field));
        if field.contains([',', '"', '\n', '\r']) {
            line.push('"');
            line.push_str(&field.replace('"', "\"\""));
            line.push('"');
        } else {
            line.push_str(&field);
        }
    }
    line.push('\n');
    line
}

fn tab_cell(value: &str) -> String {
    formula_guard(&strip_controls(value).replace(['\t', '\n', '\r'], " "))
}

fn xml_escape(value: &str) -> String {
    strip_controls(value)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Shortest text that parses back to exactly the stored value.
fn fmt(value: f64) -> String {
    fmt_scaled(value, 0)
}

/// `value × 10^shift` written by moving the decimal point of the shortest
/// round-trip text, so 12.47 kV becomes exactly "12470" V with no binary
/// multiplication error. Magnitudes below 1e-4 or at least 1e15 use exponent form.
fn fmt_scaled(value: f64, shift: i32) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if !value.is_finite() {
        return value.to_string();
    }
    let sci = format!("{value:e}");
    let (mantissa, exp) = sci.split_once('e').expect("exponent form");
    let exp: i32 = exp.parse::<i32>().expect("exponent") + shift;
    let sign = if mantissa.starts_with('-') { "-" } else { "" };
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let n = digits.len() as i32;
    if !(-4..15).contains(&exp) {
        let rest = &digits[1..];
        return if rest.is_empty() { format!("{sign}{}e{exp}", &digits[..1]) } else { format!("{sign}{}.{rest}e{exp}", &digits[..1]) };
    }
    if exp >= n - 1 {
        format!("{sign}{digits}{}", "0".repeat((exp - (n - 1)) as usize))
    } else if exp >= 0 {
        let split = (exp + 1) as usize;
        format!("{sign}{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(exported: &'a PackageExport, name: &str) -> &'a str {
        exported.file(name).unwrap_or_else(|| panic!("{name} missing"))
    }

    /// Parse a CSV without quoted commas (sufficient for these fixtures).
    fn rows(text: &str) -> Vec<HashMap<String, String>> {
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap().split(',').collect();
        lines.map(|line| header.iter().zip(line.split(',')).map(|(h, v)| (h.to_string(), v.to_string())).collect()).collect()
    }

    #[test]
    fn sample_export_has_package_components() {
        let exported = export(&Project::sample());
        let xml = exported.xml();
        assert!(xml.contains("<Component Type=\"Bus\""));
        assert!(xml.contains("<Component Type=\"Transformer\""));
        assert!(xml.contains("NominalSystemVoltage"));
        assert!(xml.contains("Wye-Ground"));
        let buses = file(&exported, "buses.csv");
        assert!(buses.lines().next().unwrap().contains("NominalSystemVoltage"));
        assert!(buses.contains("MCC-1"));
        let revit = file(&exported, "revit-panels.csv");
        assert!(revit.contains("Panel Name"));
        assert!(revit.contains("Study_EquipmentType"));
        let xfmr = file(&exported, "transformers.csv");
        assert!(xfmr.contains("PercentZ"));
        assert!(xfmr.contains("NominalKVA"));
    }

    #[test]
    fn transformer_primary_is_hv_regardless_of_branch_direction() {
        let normal = export(&Project::sample());
        let mut reversed = Project::sample();
        let t1 = &mut reversed.branches[0];
        std::mem::swap(&mut t1.from, &mut t1.to);
        // Reversing a branch reverses its connection code: from=mcc wye-ground, to=util delta.
        if let BranchKind::Transformer { connection, .. } = &mut t1.kind { *connection = XfmrConn::Ynd; }
        assert!(reversed.validate().is_empty(), "{:?}", reversed.validate());
        let reversed = export(&reversed);
        for exported in [&normal, &reversed] {
            let row = &rows(file(exported, "transformers.csv"))[0];
            assert_eq!(row["PrimaryBus"], "Utility");
            assert_eq!(row["FromBus"], "Utility");
            assert_eq!(row["SecondaryBus"], "MCC-1");
            assert_eq!(row["PrimaryVoltage"], "12470");
            assert_eq!(row["SecondaryVoltage"], "480");
            assert_eq!(row["PrimaryConnection"], "Delta");
            assert_eq!(row["SecondaryConnection"], "Wye-Ground");
            let xml = exported.xml();
            assert!(xml.contains("<Attribute Name=\"PrimaryConnection\" Unit=\"\">Delta</Attribute>"));
            assert!(xml.contains("<Attribute Name=\"SecondaryConnection\" Unit=\"\">Wye-Ground</Attribute>"));
            assert!(xml.contains("Type=\"Transformer\" FromBus=\"Utility\" ToBus=\"MCC-1\""));
            let conn = &rows(file(exported, "connections.csv"))[0];
            assert_eq!((conn["FromBus"].as_str(), conn["ToBus"].as_str()), ("Utility", "MCC-1"));
        }
    }

    #[test]
    fn values_and_units_volts_total_ohms_and_percent_on_kva_base() {
        let exported = export(&Project::sample());
        let bus = rows(file(&exported, "buses.csv"));
        assert_eq!(bus[0]["NominalSystemVoltage"], "12470");
        assert_eq!(bus[0]["VoltageUnit"], "V");
        assert_eq!(bus[1]["NominalSystemVoltage"], "480");
        let cable = &rows(file(&exported, "cables.csv"))[0];
        assert_eq!(cable["R1"], "0.0044");
        assert_eq!(cable["X1"], "0.0072");
        assert_eq!(cable["R0"], "0.013");
        assert_eq!(cable["ImpedanceUnit"], "ohm total");
        let xfmr = &rows(file(&exported, "transformers.csv"))[0];
        assert_eq!(xfmr["NominalKVA"], "2500");
        assert_eq!(xfmr["PercentZ"], "5.75");
        let (r, x): (f64, f64) = (xfmr["PercentR"].parse().unwrap(), xfmr["PercentX"].parse().unwrap());
        assert!(((r * r + x * x).sqrt() - 5.75).abs() < 1e-12);
        assert!((x / r - 7.5).abs() < 1e-12);
        let motor = &rows(file(&exported, "motors.csv"))[0];
        assert_eq!(motor["Voltage"], "480");
        let tab = file(&exported, "components.tab");
        let header: Vec<&str> = tab.lines().next().unwrap().split('\t').collect();
        let source = tab.lines().find(|l| l.starts_with("Source\t")).unwrap();
        let cols: Vec<&str> = source.split('\t').collect();
        let at = |name: &str| cols[header.iter().position(|h| *h == name).unwrap()];
        assert_eq!(at("NominalKVA"), "", "source short-circuit strength is not a nominal kVA");
        assert_eq!(at("ShortCircuitKVA"), "250000");
        let xfmr_line = tab.lines().find(|l| l.starts_with("Transformer\t")).unwrap();
        let cols: Vec<&str> = xfmr_line.split('\t').collect();
        assert_eq!(cols[header.iter().position(|h| *h == "NominalKVA").unwrap()], "2500");
    }

    #[test]
    fn numbers_round_trip_including_tiny_values() {
        for v in [2.5e-7, 1.4e-6, 0.1 + 0.2, 123456.789, 1e20, -0.0044, 5.75, 1.0 / 3.0, -2.5e-9, 9.99e14, 1e15] {
            let text = fmt(v);
            assert_eq!(text.parse::<f64>().unwrap(), v, "{v} -> {text}");
        }
        assert_eq!(fmt(2.5e-7), "2.5e-7");
        assert_eq!(fmt(1.4e-6), "1.4e-6");
        assert_eq!(fmt(0.0), "0");
        assert_eq!(fmt(-0.0), "0");
        assert_eq!(fmt(480.0), "480");
        assert_eq!(fmt_scaled(12.47, 3), "12470");
        assert_eq!(fmt_scaled(0.208, 3), "208");
        assert_eq!(fmt_scaled(0.0001, 3), "0.1");
        assert_eq!(fmt_scaled(250.0, 3), "250000");
        let mut p = Project::sample();
        if let BranchKind::Line { b_siemens, r_ohm, .. } = &mut p.branches[1].kind { *b_siemens = 2.5e-7; *r_ohm = 1.4e-6; }
        let cable = &rows(file(&export(&p), "cables.csv"))[0];
        assert_eq!(cable["B"].parse::<f64>().unwrap(), 2.5e-7);
        assert_eq!(cable["R1"].parse::<f64>().unwrap(), 1.4e-6);
    }

    #[test]
    fn operating_state_switches_and_sources_are_exported() {
        let mut p = Project::sample();
        p.switches.push(crate::model::Switch { id: "fs".into(), name: "Feeder switch".into(), branch_id: "feeder".into(), kind: crate::model::SwitchKind::Breaker, closed: false });
        let mut standby = p.sources[0].clone();
        standby.id = "gen".into();
        standby.name = "Standby".into();
        standby.in_service = false;
        standby.is_slack = false;
        p.sources.push(standby);
        let exported = export(&p);
        let conns = rows(file(&exported, "connections.csv"));
        let feeder = conns.iter().find(|r| r["Name"] == "Feeder to Panel A").unwrap();
        assert_eq!(feeder["NormalState"], "Open");
        assert_eq!(feeder["Switch"], "Feeder switch");
        assert_eq!(conns.iter().find(|r| r["Name"] == "T1").unwrap()["NormalState"], "Closed");
        let sources = rows(file(&exported, "utilities.csv"));
        assert!(sources.iter().all(|r| r["Type"] == "Source"));
        let standby = sources.iter().find(|r| r["Name"] == "Standby").unwrap();
        assert_eq!(standby["ServiceStatus"], "Out of Service");
        assert_eq!(standby["IsSlack"], "false");
        let switches = rows(file(&exported, "switches.csv"));
        assert_eq!(switches[0]["NormalState"], "Open");
        let xml = exported.xml();
        assert!(xml.contains("NormalState=\"Open\" Switch=\"Feeder switch\""));
        assert!(xml.contains("<Attribute Name=\"ServiceStatus\" Unit=\"\">Out of Service</Attribute>"));
        assert!(!xml.contains("Type=\"Utility\""));
        let notes = file(&exported, "IMPORT.txt");
        assert!(notes.contains("NormalState") && notes.contains("Operating cases") && notes.contains("project.json"));
    }

    #[test]
    fn devices_have_unit_separated_columns_and_unique_branch_keys() {
        let mut p = Project::sample();
        // Same display name as the transformer; the reference must still resolve uniquely.
        p.branches[1].name = "T1".into();
        let exported = export(&p);
        let devices = rows(file(&exported, "protective-devices.csv"));
        let main = devices.iter().find(|r| r["Name"] == "MCC main").unwrap();
        assert_eq!(main["PickupA"], "3200");
        assert_eq!(main["TimeDial"], "");
        assert_eq!(main["DelayS"], "8");
        assert_eq!(main["ShortTimePickupA"], "16000");
        assert_eq!(main["ShortTimeDelayS"], "0.3");
        assert_eq!(main["InstantaneousPickupA"], "36000");
        assert_eq!(main["ProtectedBranch"], "T1");
        let feeder = devices.iter().find(|r| r["Name"] == "Panel feeder").unwrap();
        assert_eq!(feeder["TimeDial"], "0.15");
        assert_eq!(feeder["DelayS"], "");
        assert_eq!(feeder["ProtectedBranch"], "T1-2");
        let cables = rows(file(&exported, "cables.csv"));
        assert_eq!(cables[0]["Name"], "T1-2");
        // The sample source shares the utility bus name.
        let sources = rows(file(&exported, "utilities.csv"));
        assert_eq!(sources[0]["Name"], "Utility-2");
        assert_eq!(sources[0]["Bus"], "Utility");
    }

    #[test]
    fn every_equipment_item_is_exported_and_revit_system_follows_winding() {
        let mut p = Project::sample();
        let mut second = p.equipment[0].clone();
        second.id = "af-mcc-2".into();
        second.name = "MCC-1 section 2".into();
        p.equipment.push(second);
        let exported = export(&p);
        let equipment = rows(file(&exported, "arc-equipment.csv"));
        assert_eq!(equipment.len(), 3);
        assert!(equipment.iter().any(|r| r["Name"] == "MCC-1 section 2" && r["Bus"] == "MCC-1"));
        assert_eq!(exported.xml().matches("Type=\"ArcEquipment\"").count(), 3);
        let buses = rows(file(&exported, "buses.csv"));
        assert_eq!(buses[1]["ArcEquipmentCount"], "2");
        let panels = rows(file(&exported, "revit-panels.csv"));
        let mcc = panels.iter().find(|r| r["Panel Name"] == "MCC-1").unwrap();
        assert_eq!(mcc["Distribution System"], "480Y/277 V");
        assert_eq!(mcc["Wires"], "4");
        let pnl = panels.iter().find(|r| r["Panel Name"] == "Panel A").unwrap();
        assert_eq!(pnl["Distribution System"], "480Y/277 V", "cable-fed panel inherits the winding");

        let mut delta = Project::sample();
        if let BranchKind::Transformer { connection, .. } = &mut delta.branches[0].kind { *connection = XfmrConn::Dd; }
        let panels = rows(file(&export(&delta), "revit-panels.csv"));
        let mcc = panels.iter().find(|r| r["Panel Name"] == "MCC-1").unwrap();
        assert_eq!(mcc["Distribution System"], "480 V Delta");
        assert_eq!(mcc["Wires"], "3");
    }

    #[test]
    fn csv_formula_injection_and_control_characters_are_neutralized() {
        let mut p = Project::sample();
        p.loads[0].name = "=HYPERLINK(\"x\")".into();
        p.loads[1].name = "-cmd".into();
        p.name = "Plant\u{1}One".into();
        p.equipment[0].basis = "@SUM(A1)".into();
        let exported = export(&p);
        let loads = file(&exported, "loads.csv");
        assert!(loads.contains("'=HYPERLINK(-x-)"), "{loads}");
        assert!(loads.contains("'-cmd"));
        assert!(file(&exported, "arc-equipment.csv").contains("'@SUM(A1)"));
        // Negative numbers are numbers, not formulas.
        assert_eq!(formula_guard("-0.5"), "-0.5");
        assert_eq!(formula_guard("+1"), "'+1");
        assert!(file(&exported, "components.tab").contains("'=HYPERLINK"));
        let xml = exported.xml();
        assert!(xml.contains("Name=\"PlantOne\""));
        assert!(!xml.chars().any(|c| c < ' ' && c != '\n'));
        assert!(!file(&exported, "IMPORT.txt").contains('\u{1}'));
    }

    #[test]
    fn write_dir_refuses_overwrite_and_never_replaces_the_input() {
        let root = std::env::temp_dir().join(format!("flashmob-package-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let p = Project::sample();
        let out = root.join("pkg");
        write_dir(&p, &out, WriteOptions::default()).unwrap();
        let err = write_dir(&p, &out, WriteOptions::default()).unwrap_err();
        assert!(err.contains("refusing to overwrite"), "{err}");
        write_dir(&p, &out, WriteOptions { overwrite: true, protect: None }).unwrap();
        // The input project in the output folder is never replaced, even with overwrite.
        let input = out.join("project.json");
        std::fs::write(&input, "original").unwrap();
        let err = write_dir(&p, &out, WriteOptions { overwrite: true, protect: Some(&input) }).unwrap_err();
        assert!(err.contains("input project"), "{err}");
        assert_eq!(std::fs::read_to_string(&input).unwrap(), "original");
        // An .xml path writes project.xml there and siblings beside it.
        let xml = root.join("single").join("plant.xml");
        let dir = write_dir(&p, &xml, WriteOptions::default()).unwrap();
        assert!(xml.is_file() && dir.join("buses.csv").is_file() && !dir.join("project.xml").exists());
        // File-like non-XML outputs are rejected rather than becoming directories.
        assert!(write_dir(&p, &root.join("out.csv"), WriteOptions::default()).is_err());
        assert!(write_dir(&p, &input, WriteOptions { overwrite: true, protect: None }).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
