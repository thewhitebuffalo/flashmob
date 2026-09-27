//! Export a Flashmob model in the formats SKM Power*Tools accepts through
//! Data Exchange: SKM XML, CSV, and tab-delimited text, plus a Revit
//! electrical schedule the Revit–SKM importer can map.
//!
//! SKM does not publish the Data Exchange XSD. Element and column names
//! follow the PTW Component Editor (Nominal System Voltage, Nominal kVA,
//! %Z, X/R, connection, from bus, to bus) and the PTW V9 equipment-data
//! fields (Equipment Type SWG/PNL/MCC/CBL/AIR, gap, working distance,
//! electrode configuration).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::{BranchKind, CurveSpec, Electrode, Project, XfmrConn};

#[derive(Clone, Debug, Serialize)]
pub struct SkmExport {
    pub files: Vec<SkmFile>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SkmFile {
    pub name: String,
    pub content: String,
}

impl SkmExport {
    pub fn xml(&self) -> &str {
        self.files.iter().find(|f| f.name.ends_with(".xml")).map(|f| f.content.as_str()).unwrap_or("")
    }
}

pub fn export(project: &Project) -> SkmExport {
    let names = Names::new(project);
    let mut files = vec![
        SkmFile { name: "project.xml".into(), content: xml(project, &names) },
        SkmFile { name: "buses.csv".into(), content: buses_csv(project, &names) },
        SkmFile { name: "cables.csv".into(), content: cables_csv(project, &names) },
        SkmFile { name: "transformers.csv".into(), content: transformers_csv(project, &names) },
        SkmFile { name: "utilities.csv".into(), content: utilities_csv(project, &names) },
        SkmFile { name: "loads.csv".into(), content: loads_csv(project, &names) },
        SkmFile { name: "motors.csv".into(), content: motors_csv(project, &names) },
        SkmFile { name: "protective-devices.csv".into(), content: devices_csv(project, &names) },
        SkmFile { name: "connections.csv".into(), content: connections_csv(project, &names) },
        SkmFile { name: "revit-panels.csv".into(), content: revit_panels_csv(project, &names) },
        SkmFile { name: "revit-circuits.csv".into(), content: revit_circuits_csv(project, &names) },
        SkmFile { name: "components.tab".into(), content: components_tab(project, &names) },
        SkmFile { name: "IMPORT.txt".into(), content: import_notes(project) },
    ];
    files.retain(|f| !f.content.is_empty());
    SkmExport { files }
}

pub fn write_dir(project: &Project, output: &Path) -> Result<PathBuf, String> {
    let exported = export(project);
    let dir = if output.extension().and_then(|ext| ext.to_str()) == Some("xml") {
        let dir = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;
        if let Some(xml) = exported.files.iter().find(|f| f.name == "project.xml") {
            std::fs::write(output, &xml.content).map_err(|err| format!("could not write {}: {err}", output.display()))?;
        }
        for file in &exported.files {
            if file.name == "project.xml" {
                continue;
            }
            let path = dir.join(&file.name);
            std::fs::write(&path, &file.content).map_err(|err| format!("could not write {}: {err}", path.display()))?;
        }
        dir.to_path_buf()
    } else {
        std::fs::create_dir_all(output).map_err(|err| format!("could not create {}: {err}", output.display()))?;
        for file in &exported.files {
            let path = output.join(&file.name);
            std::fs::write(&path, &file.content).map_err(|err| format!("could not write {}: {err}", path.display()))?;
        }
        output.to_path_buf()
    };
    Ok(dir)
}

struct Names {
    bus: Vec<(String, String)>,
}

impl Names {
    fn new(project: &Project) -> Self {
        let mut used = HashSet::new();
        let bus = project
            .buses
            .iter()
            .map(|bus| {
                let name = unique_name(&bus.name, &bus.id, &mut used);
                (bus.id.clone(), name)
            })
            .collect();
        Self { bus }
    }

    fn bus(&self, id: &str) -> String {
        self.bus.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()).unwrap_or_else(|| sanitize(id))
    }
}

fn xml(project: &Project, names: &Names) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <SKMDataExchange Source=\"Flashmob\" Version=\"1.0\" Target=\"PTW Data Exchange\">\n\
         <Project Name=\"{}\" BaseMVA=\"{}\" FrequencyHz=\"60\" ArcDurationCapSec=\"{}\">\n\
         <Components>\n",
        xml_escape(&project.name),
        fmt(project.s_base_mva),
        fmt(project.arc_duration_cap_s),
    ));
    for bus in &project.buses {
        let equip = equipment_on(project, &bus.id);
        body.push_str(&format!(
            "  <Component Type=\"Bus\" Name=\"{}\">\n{}  </Component>\n",
            xml_escape(&names.bus(&bus.id)),
            attrs(&[
                ("NominalSystemVoltage", "V", fmt(bus.kv * 1000.0)),
                ("ShuntKVAR", "kvar", fmt(bus.shunt_kvar)),
                ("EquipmentType", "", equip.0.to_string()),
                ("ElectrodeConfiguration", "", equip.1.to_string()),
                ("Gap", "mm", equip.2),
                ("WorkingDistance", "mm", equip.3),
                ("EnclosureHeight", "mm", equip.4),
                ("EnclosureWidth", "mm", equip.5),
                ("EnclosureDepth", "mm", equip.6),
            ]),
        ));
    }
    for source in &project.sources {
        let kind = if source.is_slack { "Utility" } else { "Generator" };
        body.push_str(&format!(
            "  <Component Type=\"{kind}\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&sanitize(&source.name)),
            xml_escape(&names.bus(&source.bus)),
            attrs(&[
                ("NominalSystemVoltage", "V", fmt(bus_volts(project, &source.bus))),
                ("VoltagePU", "pu", fmt(source.v_pu)),
                ("Angle", "deg", fmt(source.angle_deg)),
                ("ThreePhaseShortCircuitMVA", "MVA", fmt(source.mva_sc)),
                ("XR", "", fmt(source.xr)),
                ("X0OverX1", "", fmt(source.x0_over_x1)),
                ("R0OverR1", "", fmt(source.r0_over_r1)),
                ("P", "MW", fmt(source.p_mw)),
            ]),
        ));
    }
    for branch in &project.branches {
        match &branch.kind {
            BranchKind::Line { r_ohm, x_ohm, r0_ohm, x0_ohm, ampacity_a, b_siemens } => {
                let (r0, x0, note) = zero_seq(*r_ohm, *x_ohm, *r0_ohm, *x0_ohm);
                body.push_str(&format!(
                    "  <Component Type=\"Cable\" Name=\"{}\" FromBus=\"{}\" ToBus=\"{}\">\n{}  </Component>\n",
                    xml_escape(&sanitize(&branch.name)),
                    xml_escape(&names.bus(&branch.from)),
                    xml_escape(&names.bus(&branch.to)),
                    attrs(&[
                        ("R1", "ohm", fmt(*r_ohm)),
                        ("X1", "ohm", fmt(*x_ohm)),
                        ("R0", "ohm", fmt(r0)),
                        ("X0", "ohm", fmt(x0)),
                        ("B", "S", fmt(*b_siemens)),
                        ("Ampacity", "A", ampacity_a.map(fmt).unwrap_or_default()),
                        ("ImpedanceUnit", "", "ohm".into()),
                        ("ZeroSequenceNote", "", note.into()),
                    ]),
                ));
            }
            BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, connection, tap_percent, x0_over_x1 } => {
                let (pct_r, pct_x) = percent_rx(*z_percent, *xr);
                let (pri, sec) = connections(*connection);
                body.push_str(&format!(
                    "  <Component Type=\"Transformer\" Name=\"{}\" FromBus=\"{}\" ToBus=\"{}\">\n{}  </Component>\n",
                    xml_escape(&sanitize(&branch.name)),
                    xml_escape(&names.bus(&branch.from)),
                    xml_escape(&names.bus(&branch.to)),
                    attrs(&[
                        ("NominalKVA", "kVA", fmt(*kva)),
                        ("PrimaryVoltage", "V", fmt(hv_kv * 1000.0)),
                        ("SecondaryVoltage", "V", fmt(lv_kv * 1000.0)),
                        ("PrimaryConnection", "", pri.into()),
                        ("SecondaryConnection", "", sec.into()),
                        ("PercentZ", "%", fmt(*z_percent)),
                        ("PercentR", "%", fmt(pct_r)),
                        ("PercentX", "%", fmt(pct_x)),
                        ("XR", "", fmt(*xr)),
                        ("X0OverX1", "", fmt(*x0_over_x1)),
                        ("Tap", "%", fmt(*tap_percent)),
                    ]),
                ));
            }
        }
    }
    for load in &project.loads {
        body.push_str(&format!(
            "  <Component Type=\"Load\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&sanitize(&load.name)),
            xml_escape(&names.bus(&load.bus)),
            attrs(&[("kW", "kW", fmt(load.kw)), ("kVAR", "kvar", fmt(load.kvar)), ("LoadType", "", "Constant kVA".into())]),
        ));
    }
    for motor in &project.motors {
        body.push_str(&format!(
            "  <Component Type=\"Motor\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&sanitize(&motor.name)),
            xml_escape(&names.bus(&motor.bus)),
            attrs(&[
                ("HP", "hp", fmt(motor.hp)),
                ("Voltage", "V", fmt(motor.kv * 1000.0)),
                ("PowerFactor", "", fmt(motor.pf)),
                ("Efficiency", "", fmt(motor.efficiency)),
                ("SubtransientReactance", "pu", fmt(motor.x_subtransient_pu)),
                ("XR", "", fmt(motor.xr)),
            ]),
        ));
    }
    for device in &project.devices {
        let (kind, curve, pickup, td, inst, inst_s) = device_fields(&device.curve);
        body.push_str(&format!(
            "  <Component Type=\"ProtectiveDevice\" Name=\"{}\" Bus=\"{}\">\n{}  </Component>\n",
            xml_escape(&sanitize(&device.name)),
            xml_escape(&names.bus(&device.bus)),
            attrs(&[
                ("DeviceType", "", kind.into()),
                ("Curve", "", curve.into()),
                ("Pickup", "A", pickup),
                ("TimeDial", "", td),
                ("InstantaneousPickup", "A", inst),
                ("InstantaneousTime", "s", inst_s),
            ]),
        ));
    }
    body.push_str("</Components>\n<Connections>\n");
    for branch in &project.branches {
        let kind = match branch.kind {
            BranchKind::Line { .. } => "Cable",
            BranchKind::Transformer { .. } => "Transformer",
        };
        body.push_str(&format!(
            "  <Connection Name=\"{}\" Type=\"{kind}\" FromBus=\"{}\" ToBus=\"{}\"/>\n",
            xml_escape(&sanitize(&branch.name)),
            xml_escape(&names.bus(&branch.from)),
            xml_escape(&names.bus(&branch.to)),
        ));
    }
    body.push_str("</Connections>\n</Project>\n</SKMDataExchange>\n");
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
    ]);
    for bus in &project.buses {
        let equip = equipment_on(project, &bus.id);
        s.push_str(&csv_line(&[
            &names.bus(&bus.id),
            &fmt(bus.kv * 1000.0),
            "V",
            &fmt(bus.shunt_kvar),
            equip.0,
            equip.1,
            &equip.2,
            "mm",
            &equip.3,
            "mm",
            &equip.4,
            &equip.5,
            &equip.6,
            "mm",
        ]));
    }
    s
}

fn cables_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name", "FromBus", "ToBus", "R1", "X1", "R0", "X0", "B", "ImpedanceUnit", "Ampacity", "AmpacityUnit", "ZeroSequenceNote",
    ]);
    for branch in &project.branches {
        let BranchKind::Line { r_ohm, x_ohm, r0_ohm, x0_ohm, ampacity_a, b_siemens } = &branch.kind else { continue };
        let (r0, x0, note) = zero_seq(*r_ohm, *x_ohm, *r0_ohm, *x0_ohm);
        s.push_str(&csv_line(&[
            &sanitize(&branch.name),
            &names.bus(&branch.from),
            &names.bus(&branch.to),
            &fmt(*r_ohm),
            &fmt(*x_ohm),
            &fmt(r0),
            &fmt(x0),
            &fmt(*b_siemens),
            "ohm",
            &ampacity_a.map(fmt).unwrap_or_default(),
            "A",
            note,
        ]));
    }
    s
}

fn transformers_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name",
        "FromBus",
        "ToBus",
        "NominalKVA",
        "PrimaryVoltage",
        "SecondaryVoltage",
        "VoltageUnit",
        "PrimaryConnection",
        "SecondaryConnection",
        "PercentZ",
        "PercentR",
        "PercentX",
        "XR",
        "X0OverX1",
        "TapPercent",
    ]);
    for branch in &project.branches {
        let BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, connection, tap_percent, x0_over_x1 } = &branch.kind else {
            continue;
        };
        let (pct_r, pct_x) = percent_rx(*z_percent, *xr);
        let (pri, sec) = connections(*connection);
        s.push_str(&csv_line(&[
            &sanitize(&branch.name),
            &names.bus(&branch.from),
            &names.bus(&branch.to),
            &fmt(*kva),
            &fmt(hv_kv * 1000.0),
            &fmt(lv_kv * 1000.0),
            "V",
            pri,
            sec,
            &fmt(*z_percent),
            &fmt(pct_r),
            &fmt(pct_x),
            &fmt(*xr),
            &fmt(*x0_over_x1),
            &fmt(*tap_percent),
        ]));
    }
    s
}

fn utilities_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&[
        "Name", "Type", "Bus", "NominalSystemVoltage", "VoltageUnit", "VoltagePU", "AngleDeg", "ThreePhaseShortCircuitMVA", "XR", "X0OverX1", "R0OverR1", "PMW",
    ]);
    for source in &project.sources {
        let kind = if source.is_slack { "Utility" } else { "Generator" };
        s.push_str(&csv_line(&[
            &sanitize(&source.name),
            kind,
            &names.bus(&source.bus),
            &fmt(bus_volts(project, &source.bus)),
            "V",
            &fmt(source.v_pu),
            &fmt(source.angle_deg),
            &fmt(source.mva_sc),
            &fmt(source.xr),
            &fmt(source.x0_over_x1),
            &fmt(source.r0_over_r1),
            &fmt(source.p_mw),
        ]));
    }
    s
}

fn loads_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Name", "Bus", "kW", "kVAR", "LoadType"]);
    for load in &project.loads {
        s.push_str(&csv_line(&[&sanitize(&load.name), &names.bus(&load.bus), &fmt(load.kw), &fmt(load.kvar), "Constant kVA"]));
    }
    s
}

fn motors_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Name", "Bus", "HP", "Voltage", "VoltageUnit", "PowerFactor", "Efficiency", "SubtransientReactancePU", "XR"]);
    for motor in &project.motors {
        s.push_str(&csv_line(&[
            &sanitize(&motor.name),
            &names.bus(&motor.bus),
            &fmt(motor.hp),
            &fmt(motor.kv * 1000.0),
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
    let mut s = csv_line(&["Name", "Bus", "DeviceType", "Curve", "PickupA", "TimeDial", "InstantaneousPickupA", "InstantaneousTimeS"]);
    for device in &project.devices {
        let (kind, curve, pickup, td, inst, inst_s) = device_fields(&device.curve);
        s.push_str(&csv_line(&[&sanitize(&device.name), &names.bus(&device.bus), kind, &curve, &pickup, &td, &inst, &inst_s]));
    }
    s
}

fn connections_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Name", "Type", "FromBus", "ToBus"]);
    for branch in &project.branches {
        let kind = match branch.kind {
            BranchKind::Line { .. } => "Cable",
            BranchKind::Transformer { .. } => "Transformer",
        };
        s.push_str(&csv_line(&[&sanitize(&branch.name), kind, &names.bus(&branch.from), &names.bus(&branch.to)]));
    }
    s
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
        "SKM_EquipmentType",
        "SKM_ElectrodeConfiguration",
        "SKM_Gap_mm",
        "SKM_WorkingDistance_mm",
    ]);
    for bus in &project.buses {
        let (kw, kvar) = bus_load(project, &bus.id);
        let kva = (kw * kw + kvar * kvar).sqrt();
        let equip = equipment_on(project, &bus.id);
        let system = if bus.kv >= 1.0 {
            format!("{:.0} V", bus.kv * 1000.0)
        } else if (bus.kv - 0.48).abs() < 0.02 {
            "480Y/277 V".into()
        } else if (bus.kv - 0.208).abs() < 0.02 {
            "208Y/120 V".into()
        } else {
            format!("{:.0} V", bus.kv * 1000.0)
        };
        s.push_str(&csv_line(&[
            &names.bus(&bus.id),
            &system,
            &fmt(bus.kv * 1000.0),
            "3",
            "4",
            &fmt(kva),
            "kVA",
            equip.0,
            equip.1,
            &equip.2,
            &equip.3,
        ]));
    }
    s
}

fn revit_circuits_csv(project: &Project, names: &Names) -> String {
    let mut s = csv_line(&["Circuit", "Panel", "Load Name", "From", "To", "Type", "kVA", "Primary Voltage", "Secondary Voltage"]);
    for branch in &project.branches {
        match &branch.kind {
            BranchKind::Transformer { kva, hv_kv, lv_kv, .. } => {
                s.push_str(&csv_line(&[
                    &sanitize(&branch.name),
                    &names.bus(&branch.from),
                    &sanitize(&branch.name),
                    &names.bus(&branch.from),
                    &names.bus(&branch.to),
                    "Transformer",
                    &fmt(*kva),
                    &fmt(hv_kv * 1000.0),
                    &fmt(lv_kv * 1000.0),
                ]));
            }
            BranchKind::Line { .. } => {
                s.push_str(&csv_line(&[
                    &sanitize(&branch.name),
                    &names.bus(&branch.from),
                    &sanitize(&branch.name),
                    &names.bus(&branch.from),
                    &names.bus(&branch.to),
                    "Cable",
                    "",
                    &fmt(bus_volts(project, &branch.from)),
                    &fmt(bus_volts(project, &branch.to)),
                ]));
            }
        }
    }
    s
}

fn components_tab(project: &Project, names: &Names) -> String {
    let mut s = [
        "ComponentType",
        "Name",
        "FromBus",
        "ToBus",
        "NominalSystemVoltage",
        "NominalKVA",
        "PercentZ",
        "XR",
        "kW",
        "kVAR",
        "R1",
        "X1",
    ]
    .join("\t");
    s.push('\n');
    let row = |cols: [&str; 12]| {
        let mut line = cols.join("\t");
        line.push('\n');
        line
    };
    for bus in &project.buses {
        s.push_str(&row(["Bus", &names.bus(&bus.id), "", "", &fmt(bus.kv * 1000.0), "", "", "", "", "", "", ""]));
    }
    for source in &project.sources {
        s.push_str(&row([
            "Utility",
            &sanitize(&source.name),
            &names.bus(&source.bus),
            "",
            &fmt(bus_volts(project, &source.bus)),
            &fmt(source.mva_sc * 1000.0),
            "",
            &fmt(source.xr),
            "",
            "",
            "",
            "",
        ]));
    }
    for branch in &project.branches {
        match &branch.kind {
            BranchKind::Line { r_ohm, x_ohm, .. } => s.push_str(&row([
                "Cable",
                &sanitize(&branch.name),
                &names.bus(&branch.from),
                &names.bus(&branch.to),
                "",
                "",
                "",
                "",
                "",
                "",
                &fmt(*r_ohm),
                &fmt(*x_ohm),
            ])),
            BranchKind::Transformer { kva, z_percent, xr, .. } => s.push_str(&row([
                "Transformer",
                &sanitize(&branch.name),
                &names.bus(&branch.from),
                &names.bus(&branch.to),
                "",
                &fmt(*kva),
                &fmt(*z_percent),
                &fmt(*xr),
                "",
                "",
                "",
                "",
            ])),
        }
    }
    for load in &project.loads {
        s.push_str(&row(["Load", &sanitize(&load.name), &names.bus(&load.bus), "", "", "", "", "", &fmt(load.kw), &fmt(load.kvar), "", ""]));
    }
    s
}

fn import_notes(project: &Project) -> String {
    format!(
        "\
Flashmob export for SKM Power*Tools Data Exchange
Project: {name}

SKM imports project data through the Data Exchange module as SKM XML, CSV,
or tab-delimited text, and through the Autodesk Revit exchange. SKM does
not publish the XML schema. These files use the PTW Component Editor field
names and the PTW V9 equipment-data fields.

In PTW, use Project > Import, or Data Exchange, and map the columns once:

  project.xml               one file with every component and connection
  buses.csv                 NominalSystemVoltage in volts, IEEE 1584 enclosure
  cables.csv                R and X in ohms (ImpedanceUnit = ohm)
  transformers.csv          NominalKVA, %Z, %R, %X, X/R, winding connection
  utilities.csv             utility short-circuit MVA and X/R
  loads.csv                 constant kVA loads
  motors.csv                horsepower and subtransient reactance
  protective-devices.csv    pickup, time dial, instantaneous
  connections.csv           from bus and to bus
  components.tab            the same data, tab delimited
  revit-panels.csv          Revit panel schedule columns plus SKM_ fields
  revit-circuits.csv        Revit circuit connections

Voltages are in volts. Transformer percent impedance is on the transformer
kVA base. Cable impedance is the total ohms entered in Flashmob, not ohms
per thousand feet. A protective device is exported as data, not as an SKM
library curve.

Map Name to the component name and FromBus / ToBus to the connected buses.
",
        name = project.name,
    )
}

fn equipment_on(project: &Project, bus: &str) -> (&'static str, &'static str, String, String, String, String, String) {
    if let Some(item) = project.equipment.iter().find(|item| item.bus == bus) {
        let kind = equipment_type(project, bus, item.electrode);
        (
            kind,
            item.electrode.as_str(),
            fmt(item.gap_mm),
            fmt(item.distance_mm),
            fmt(item.height_mm),
            fmt(item.width_mm),
            fmt(item.depth_mm),
        )
    } else {
        let (electrode, gap, distance, height, width, depth, _) = crate::model::typical_equipment(bus_kv(project, bus));
        let kind = equipment_type(project, bus, electrode);
        (kind, electrode.as_str(), fmt(gap), fmt(distance), fmt(height), fmt(width), fmt(depth))
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

fn device_fields(curve: &CurveSpec) -> (&'static str, String, String, String, String, String) {
    match curve {
        CurveSpec::Iec { kind, pickup_a, tms, inst_a, inst_s } => (
            "IEC Relay",
            format!("{kind:?}"),
            fmt(*pickup_a),
            fmt(*tms),
            inst_a.map(fmt).unwrap_or_default(),
            inst_s.map(fmt).unwrap_or_default(),
        ),
        CurveSpec::Ieee { kind, pickup_a, td, inst_a, inst_s } => (
            "IEEE Relay",
            format!("{kind:?}"),
            fmt(*pickup_a),
            fmt(*td),
            inst_a.map(fmt).unwrap_or_default(),
            inst_s.map(fmt).unwrap_or_default(),
        ),
        CurveSpec::Definite { pickup_a, time_s } => ("Definite Time", "Definite".into(), fmt(*pickup_a), fmt(*time_s), String::new(), String::new()),
        CurveSpec::ThermalMagnetic { lt_pickup_a, lt_delay_s, inst_a, inst_s, .. } => (
            "LV Breaker",
            "Thermal-Magnetic".into(),
            fmt(*lt_pickup_a),
            fmt(*lt_delay_s),
            inst_a.map(fmt).unwrap_or_default(),
            inst_s.map(fmt).unwrap_or_default(),
        ),
        CurveSpec::SettingsNotCollected { .. } => (
            "Collected device",
            "Settings not collected".into(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
    }
}

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

fn bus_volts(project: &Project, id: &str) -> f64 {
    bus_kv(project, id) * 1000.0
}

fn bus_kv(project: &Project, id: &str) -> f64 {
    project.bus(id).map(|bus| bus.kv).unwrap_or(0.0)
}

fn bus_load(project: &Project, id: &str) -> (f64, f64) {
    project.loads.iter().filter(|load| load.bus == id).fold((0.0, 0.0), |(p, q), load| (p + load.kw, q + load.kvar))
}

fn unique_name(name: &str, id: &str, used: &mut HashSet<String>) -> String {
    let base = sanitize(if name.is_empty() { id } else { name });
    let mut candidate = base.clone();
    let mut n = 2;
    while !used.insert(candidate.clone()) {
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
            _ => out.push(ch),
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() { "UNNAMED".into() } else { trimmed.to_string() }
}

fn csv_line(fields: &[&str]) -> String {
    let mut line = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        if field.contains([',', '"', '\n']) {
            line.push('"');
            line.push_str(&field.replace('"', "\"\""));
            line.push('"');
        } else {
            line.push_str(field);
        }
    }
    line.push('\n');
    line
}

fn xml_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn fmt(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        let text = format!("{value:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_export_has_skm_components() {
        let exported = export(&Project::sample());
        let xml = exported.xml();
        assert!(xml.contains("<Component Type=\"Bus\""));
        assert!(xml.contains("<Component Type=\"Transformer\""));
        assert!(xml.contains("NominalSystemVoltage"));
        assert!(xml.contains("Wye-Ground"));
        let buses = &exported.files.iter().find(|f| f.name == "buses.csv").unwrap().content;
        assert!(buses.lines().next().unwrap().contains("NominalSystemVoltage"));
        assert!(buses.contains("MCC-1"));
        let revit = &exported.files.iter().find(|f| f.name == "revit-panels.csv").unwrap().content;
        assert!(revit.contains("Panel Name"));
        assert!(revit.contains("SKM_EquipmentType"));
        let xfmr = &exported.files.iter().find(|f| f.name == "transformers.csv").unwrap().content;
        assert!(xfmr.contains("PercentZ"));
        assert!(xfmr.contains("NominalKVA"));
    }
}
