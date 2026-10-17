//! Format-neutral mapping layer for importing projects from other study
//! programs. A format reader fills an [`ImportedNetwork`] with the values as
//! entered (ohms, kV, kVA, percents) and [`build_project`] turns it into a
//! Flashmob [`Project`], linking devices to the device library where an exact
//! match exists and reporting everything it could not resolve.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::library::Library;
use crate::library_apply::{apply_device, ApplyDevice};
use crate::model::{typical_equipment, ArcEquipment, Assumption, Branch, BranchKind, Bus, CurveSpec, Device, Motor, Project, Source, XfmrConn};

/// Transformer impedance used when a project leaves it blank, so the network
/// can still be solved. Always reported as an assumption.
pub const ASSUMED_TRANSFORMER_Z_PERCENT: f64 = 5.75;
/// Motor efficiency used when the source file stores none.
pub const ASSUMED_MOTOR_EFFICIENCY: f64 = 0.9;

#[derive(Clone, Debug, Default)]
pub struct ImportedNetwork {
    pub name: String,
    pub buses: Vec<ImportBus>,
    pub sources: Vec<ImportSource>,
    pub branches: Vec<ImportBranch>,
    pub motors: Vec<ImportMotor>,
    pub devices: Vec<ImportDevice>,
    /// Reader-level findings (skipped records, unmodeled elements).
    pub warnings: Vec<ImportWarning>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportBus {
    pub key: String,
    pub name: String,
    pub kv: f64,
    pub bracing_ka: Option<f64>,
    /// Enclosure height, width, depth in millimetres.
    pub enclosure_mm: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportSource {
    pub key: String,
    pub name: String,
    pub bus: String,
    /// Voltage entered on the source itself, kV.
    pub kv: Option<f64>,
    pub mva_sc: f64,
    pub xr: f64,
    pub z0_over_z1: f64,
    pub v_pu: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindingConn {
    Delta,
    Wye,
    WyeGrounded,
    Unknown,
}

#[derive(Clone, Debug)]
pub enum ImportBranchKind {
    Cable { r_ohm: f64, x_ohm: f64, r0_ohm: f64, x0_ohm: f64, ampacity_a: Option<f64> },
    Transformer {
        kva: f64,
        z_percent: Option<f64>,
        z0_percent: Option<f64>,
        xr: f64,
        from_kv: f64,
        to_kv: f64,
        from_conn: WindingConn,
        to_conn: WindingConn,
    },
}

#[derive(Clone, Debug)]
pub struct ImportBranch {
    pub key: String,
    pub name: String,
    pub from: String,
    pub to: String,
    pub kind: ImportBranchKind,
}

#[derive(Clone, Debug, Default)]
pub struct ImportMotor {
    pub key: String,
    pub name: String,
    pub bus: String,
    pub hp: f64,
    pub kv: f64,
    pub pf: f64,
    pub efficiency: Option<f64>,
    pub x_subtransient_pu: f64,
    pub xr: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportDeviceKind {
    Breaker,
    Fuse,
}

#[derive(Clone, Debug)]
pub struct ImportDevice {
    pub key: String,
    pub name: String,
    pub kind: ImportDeviceKind,
    /// Bus key at the device's line terminal. None when not connected to a bus.
    pub bus: Option<String>,
    /// Branch key the device protects. None when the device feeds nothing.
    pub branch: Option<String>,
    pub manufacturer: String,
    pub device_type: String,
    pub style: String,
    pub frame: String,
    /// Trip or fuse rating, amperes.
    pub rating_a: Option<f64>,
    pub interrupting_ka: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImportWarning {
    pub code: String,
    pub element: String,
    pub message: String,
}

impl ImportWarning {
    pub fn new(code: &str, element: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), element: element.into(), message: message.into() }
    }
}

#[derive(Debug)]
pub struct ImportResult {
    pub project: Project,
    pub warnings: Vec<ImportWarning>,
}

/// Lowercase alphanumerics, used to compare names across programs.
fn norm(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Common manufacturer abbreviations seen in project files.
fn manufacturer_matches(entered: &str, library: &str) -> bool {
    let (e, l) = (norm(entered), norm(library));
    if e.is_empty() {
        return false;
    }
    if e == l {
        return true;
    }
    let alias = match e.as_str() {
        "sqd" | "sqrd" => "squared",
        "ge" => "generalelectric",
        "ch" | "cutlerhammer" => "eaton",
        "ab" => "allenbradley",
        "westinghouse" => "eaton",
        _ => return false,
    };
    l == alias || l.starts_with(alias)
}

/// Entered breaker styles that the device library stores under another style
/// name. Each entry is an assumption, not a manufacturer cross-reference: a
/// match through it is reported as an assumed alias in the import warnings and
/// in the project assumptions, so the result never reads as an exact match.
///
/// Columns: manufacturer (normalized as in [`manufacturer_matches`]), entered
/// style, library style, reason.
pub const STYLE_ALIASES: &[(&str, &str, &str, &str)] = &[
    ("squared", "JL", "GJL", "JL entered; library curves for the GJL family are used"),
    ("squared", "EGB", "EG", "EGB (bolt-on EG) entered; library curves for the EG family are used"),
];

/// Library match for one entered device.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryMatch {
    pub device_id: u32,
    /// Set when the match went through [`STYLE_ALIASES`]: (entered, library, reason).
    pub alias: Option<(&'static str, &'static str, &'static str)>,
}

fn alias_for(manufacturer: &str, style: &str) -> Option<(&'static str, &'static str, &'static str)> {
    STYLE_ALIASES.iter()
        .find(|(m, entered, _, _)| manufacturer_matches(manufacturer, m) && entered.eq_ignore_ascii_case(style))
        .map(|(_, entered, library, why)| (*entered, *library, *why))
}

/// The library device that matches the entered manufacturer and style exactly
/// and has a trip unit at the entered rating. The entered style is compared on
/// its first word (a pole-count suffix such as "3P" is ignored). When the style
/// has an entry in [`STYLE_ALIASES`], the aliased library style is used and the
/// match says so. Only a single match is accepted; anything else is reported,
/// never guessed.
pub fn match_library(library: &Library, device: &ImportDevice) -> Result<LibraryMatch, String> {
    let entered = device.style.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    if entered.is_empty() {
        return Err("no style entered".into());
    }
    let rating = device.rating_a.ok_or("no trip rating entered")?;
    let alias = alias_for(&device.manufacturer, &entered);
    let style = alias.map(|(_, library, _)| library.to_string()).unwrap_or_else(|| entered.clone());
    let via = alias.map(|(e, l, _)| format!(" (assumed alias {e} -> {l})")).unwrap_or_default();
    let candidates: Vec<_> = library.devices.iter()
        .filter(|d| manufacturer_matches(&device.manufacturer, &d.manufacturer))
        .filter(|d| d.style.trim().to_ascii_uppercase() == style)
        .collect();
    if candidates.is_empty() {
        return Err(format!("no library style {style}{via} for manufacturer {}", device.manufacturer));
    }
    let rated: Vec<_> = candidates.iter()
        .filter(|d| d.trip_units.iter().any(|u| (u.rating_a - rating).abs() <= 1e-6 * rating.max(1.0)))
        .collect();
    match rated.len() {
        1 => Ok(LibraryMatch { device_id: rated[0].id, alias }),
        0 => Err(format!("library style {style}{via} has no {} A trip unit", round4(rating))),
        n => Err(format!("{n} library devices match style {style}{via} at {} A", round4(rating))),
    }
}

fn round4(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

fn unique_id(name: &str, prefix: &str, used: &mut HashSet<String>) -> String {
    let mut base: String = name.trim().chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    while base.contains("--") {
        base = base.replace("--", "-");
    }
    let base = base.trim_matches('-').to_string();
    let base = if base.is_empty() { prefix.to_string() } else { base };
    let mut id = base.clone();
    let mut n = 2;
    while used.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    used.insert(id.clone());
    id
}

fn xfmr_conn(hv: WindingConn, lv: WindingConn) -> Option<XfmrConn> {
    use WindingConn::*;
    match (hv, lv) {
        (Delta, WyeGrounded) => Some(XfmrConn::Dyn),
        (WyeGrounded, Delta) => Some(XfmrConn::Ynd),
        (WyeGrounded, WyeGrounded) => Some(XfmrConn::Ynyn),
        (Delta, Delta) => Some(XfmrConn::Dd),
        _ => None,
    }
}

/// A library curve may separate a vertical segment by one floating-point step,
/// which does not survive a JSON round trip. Widen such steps to a relative
/// 1e-9 so the saved project reloads with the same shape, then re-check it.
fn json_safe_curve(curve: &mut CurveSpec) -> Result<(), String> {
    if let CurveSpec::Points { points, max_points, .. } = curve {
        let widen = |band: &mut Vec<[f64; 2]>| {
            for i in 1..band.len() {
                let floor = band[i - 1][0] * (1.0 + 1e-9);
                if band[i][0] < floor {
                    band[i][0] = floor;
                }
            }
        };
        widen(points);
        if let Some(max) = max_points.as_mut() {
            widen(max);
        }
        crate::curves::check_point_curve(points, max_points.as_deref())?;
    }
    Ok(())
}

/// Build a Flashmob project. `library` links breakers to library parts where
/// one matches exactly; without a library every breaker is reported unmatched.
pub fn build_project(net: &ImportedNetwork, library: Option<(&Library, &str)>) -> ImportResult {
    let mut warnings = net.warnings.clone();
    let mut project = Project { name: net.name.clone(), ..Project::sample() };
    project.buses.clear();
    project.branches.clear();
    project.loads.clear();
    project.sources.clear();
    project.motors.clear();
    project.devices.clear();
    project.equipment.clear();
    project.switches.clear();
    project.switch_groups.clear();
    project.operating_cases.clear();
    project.assumptions.clear();
    project.s_base_mva = 100.0;
    let motor_template = Project::sample().motors.remove(0);
    let source_template = Project::sample().sources.remove(0);

    let mut used = HashSet::new();
    let mut bus_id: HashMap<&str, String> = HashMap::new();
    let mut bus_kv: HashMap<&str, f64> = HashMap::new();
    for b in &net.buses {
        let id = unique_id(&b.name, "bus", &mut used);
        bus_id.insert(&b.key, id.clone());
        bus_kv.insert(&b.key, b.kv);
        project.buses.push(Bus {
            id, name: b.name.clone(), kv: b.kv, shunt_kvar: 0.0, bracing_ka: b.bracing_ka, main_rating_a: None,
            bracing_test_xr: None, equipment_class: None, prefault_pu: None,
        });
    }
    let bus_of = |key: &str| bus_id.get(key).cloned();

    for s in &net.sources {
        let Some(bus) = bus_of(&s.bus) else {
            warnings.push(ImportWarning::new("element_unconnected", &s.name, "source is not connected to a bus; skipped"));
            continue;
        };
        let bkv = bus_kv[s.bus.as_str()];
        if let Some(kv) = s.kv {
            if bkv > 0.0 && ((kv - bkv) / bkv).abs() > 0.01 {
                warnings.push(ImportWarning::new("source_kv_mismatch", &s.name,
                    format!("source voltage is entered as {} kV but its bus is {} kV;", round4(kv), round4(bkv)) + " the source was imported at the bus voltage with its short-circuit MVA as entered"));
            }
        }
        let mut src: Source = source_template.clone();
        src.id = unique_id(&s.name, "source", &mut used);
        src.name = s.name.clone();
        src.bus = bus;
        src.mva_sc = s.mva_sc;
        src.xr = s.xr;
        src.x0_over_x1 = s.z0_over_z1;
        src.r0_over_r1 = s.z0_over_z1;
        src.v_pu = s.v_pu;
        src.is_slack = project.sources.is_empty();
        project.sources.push(src);
    }

    let mut branch_id: HashMap<&str, String> = HashMap::new();
    let mut branch_ends: HashMap<String, (String, String)> = HashMap::new();
    for br in &net.branches {
        let (Some(from), Some(to)) = (bus_of(&br.from), bus_of(&br.to)) else {
            warnings.push(ImportWarning::new("element_unconnected", &br.name, "branch does not connect two buses; skipped"));
            continue;
        };
        let kind = match &br.kind {
            ImportBranchKind::Cable { r_ohm, x_ohm, r0_ohm, x0_ohm, ampacity_a } => BranchKind::Line {
                r_ohm: *r_ohm, x_ohm: *x_ohm, b_siemens: 0.0, r0_ohm: *r0_ohm, x0_ohm: *x0_ohm, ampacity_a: *ampacity_a,
            },
            ImportBranchKind::Transformer { kva, z_percent, z0_percent, xr, from_kv, to_kv, from_conn, to_conn } => {
                let z = match z_percent {
                    Some(z) if *z > 0.0 => *z,
                    _ => {
                        warnings.push(ImportWarning::new("transformer_z_missing", &br.name,
                            format!("impedance is blank in the source file; {ASSUMED_TRANSFORMER_Z_PERCENT}% was assumed so the network solves. Enter the nameplate value")));
                        project.assumptions.push(Assumption {
                            id: format!("import-z-{}", project.assumptions.len() + 1),
                            statement: format!("{}: transformer impedance assumed {ASSUMED_TRANSFORMER_Z_PERCENT}% (blank in imported file)", br.name),
                        });
                        ASSUMED_TRANSFORMER_Z_PERCENT
                    }
                };
                let (hv, lv, hv_conn, lv_conn) = if from_kv >= to_kv { (*from_kv, *to_kv, *from_conn, *to_conn) } else { (*to_kv, *from_kv, *to_conn, *from_conn) };
                let connection = xfmr_conn(hv_conn, lv_conn).unwrap_or_else(|| {
                    warnings.push(ImportWarning::new("transformer_connection_unknown", &br.name,
                        format!("winding connection {hv_conn:?}-{lv_conn:?} has no direct equivalent; imported as delta-wye grounded")));
                    XfmrConn::Dyn
                });
                let x0 = z0_percent.filter(|z0| *z0 > 0.0).map(|z0| z0 / z).unwrap_or(1.0);
                BranchKind::Transformer { kva: *kva, z_percent: z, xr: *xr, hv_kv: hv, lv_kv: lv, connection, tap_percent: 0.0, x0_over_x1: x0, r0_over_r1: x0 }
            }
        };
        let id = unique_id(&br.name, "branch", &mut used);
        branch_id.insert(&br.key, id.clone());
        branch_ends.insert(id.clone(), (from.clone(), to.clone()));
        project.branches.push(Branch { id, name: br.name.clone(), from, to, kind });
    }

    for m in &net.motors {
        let Some(bus) = bus_of(&m.bus) else {
            warnings.push(ImportWarning::new("element_unconnected", &m.name, "motor is not connected to a bus; skipped"));
            continue;
        };
        let mut motor: Motor = motor_template.clone();
        motor.id = unique_id(&m.name, "motor", &mut used);
        motor.name = m.name.clone();
        motor.bus = bus;
        motor.hp = m.hp;
        motor.kv = m.kv;
        motor.pf = m.pf;
        motor.x_subtransient_pu = m.x_subtransient_pu;
        motor.xr = m.xr;
        motor.efficiency = m.efficiency.unwrap_or_else(|| {
            warnings.push(ImportWarning::new("motor_efficiency_assumed", &m.name, format!("no efficiency stored; {ASSUMED_MOTOR_EFFICIENCY} assumed")));
            ASSUMED_MOTOR_EFFICIENCY
        });
        project.motors.push(motor);
    }

    let mut devices_on_branch: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for d in &net.devices {
        let (Some(bus), Some(branch)) = (d.bus.as_deref().and_then(bus_of), d.branch.as_deref().and_then(|k| branch_id.get(k).cloned())) else {
            warnings.push(ImportWarning::new("device_unconnected", &d.name, "device is not between a bus and an imported branch; skipped"));
            continue;
        };
        let id = unique_id(&d.name, "device", &mut used);
        let unresolved = |why: String| Device {
            id: id.clone(), name: d.name.clone(), bus: bus.clone(),
            curve: CurveSpec::SettingsNotCollected { note: why },
            protected_branch: Some(branch.clone()), breaker_interrupting_s: None,
            fuse_total_clearing: false, curve_is_total_clearing: false, basis: "assumed".into(),
            library: None, interrupting: None, current_limiting: None,
        };
        let device = if d.kind == ImportDeviceKind::Fuse && d.manufacturer.trim().is_empty() && d.device_type.trim().is_empty() {
            warnings.push(ImportWarning::new("device_unresolved", &d.name, "fuse has no manufacturer or type in the source file; no curve, so buses it clears have no arc duration"));
            unresolved("fuse type not entered in imported file".into())
        } else {
            match library {
                None => {
                    warnings.push(ImportWarning::new("device_unmatched", &d.name, "no device library available; curve not linked"));
                    unresolved(format!("{} {} {} not linked", d.manufacturer, d.style, d.frame))
                }
                Some((lib, lib_name)) => match match_library(lib, d) {
                    Err(why) => {
                        warnings.push(ImportWarning::new("device_unmatched", &d.name, format!("{} {} {} ({}): {why}", d.manufacturer, d.device_type, d.style, d.frame)));
                        unresolved(format!("{} {} {}: {why}", d.manufacturer, d.style, d.frame))
                    }
                    Ok(LibraryMatch { device_id, alias }) => {
                        let request = ApplyDevice {
                            device_id, id: id.clone(), name: d.name.clone(), bus: bus.clone(), protected_branch: Some(branch.clone()),
                            rating_a: d.rating_a, current_base_a: None, voltage_v: None, function_name: None,
                            library_name: lib_name.to_string(), fuse_total_clearing: false, dial: None, ct_ratio: None,
                            tap_a: None, sensor_a: None, lt_pickup: None, lt_delay: None, lt_delay_multiple: None,
                            st_pickup: None, st_delay: None, inst: None, inst_s: None, pickup_base: None,
                        };
                        match apply_device(lib, &request) {
                            Ok(mut dev) => match json_safe_curve(&mut dev.curve) {
                              Err(why) => {
                                warnings.push(ImportWarning::new("device_unmatched", &d.name, format!("library device {device_id} curve is not usable: {why}")));
                                unresolved(why)
                              }
                              Ok(()) => {
                                let mut note = String::new();
                                if dev.breaker_interrupting_s.is_none() && !dev.curve_includes_clearing() {
                                    // Published molded-case and low-voltage breaker bands include clearing.
                                    dev.curve_is_total_clearing = true;
                                    note = "; slower band taken as total clearing".into();
                                }
                                dev.basis = "assumed".into();
                                if let Some((entered, library_style, why)) = alias {
                                    warnings.push(ImportWarning::new("device_library_alias", &d.name, format!("linked to library device {device_id} at {} A through the assumed alias {entered} -> {library_style}: {why}{note}", round4(d.rating_a.unwrap_or(0.0)))));
                                    project.assumptions.push(Assumption {
                                        id: format!("import-alias-{}", project.assumptions.len() + 1),
                                        statement: format!("{}: library style {library_style} used for entered style {entered} (assumed alias, not a verified cross-reference)", d.name),
                                    });
                                    if let Some(link) = dev.library.as_mut() {
                                        link.unsupported_elements.push(format!("assumed style alias {entered} -> {library_style}"));
                                    }
                                } else {
                                    warnings.push(ImportWarning::new("device_library_match", &d.name, format!("linked to library device {device_id} at {} A{note}", round4(d.rating_a.unwrap_or(0.0)))));
                                }
                                dev
                              }
                            },
                            Err(why) => {
                                warnings.push(ImportWarning::new("device_unmatched", &d.name, format!("library device {device_id} could not be applied: {why}")));
                                unresolved(why)
                            }
                        }
                    }
                },
            }
        };
        devices_on_branch.entry(branch).or_default().push(device.id.clone());
        project.devices.push(device);
    }

    // Upstream device for each bus: walk from the sources and take the first
    // device found on the feeding branches, nearest the bus first.
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut queue: VecDeque<String> = project.sources.iter().map(|s| s.bus.clone()).collect();
    let mut seen: HashSet<String> = queue.iter().cloned().collect();
    while let Some(b) = queue.pop_front() {
        for branch in &project.branches {
            let (id, f, t) = (&branch.id, &branch.from, &branch.to);
            let other = if *f == b { t } else if *t == b { f } else { continue };
            if seen.insert(other.clone()) {
                parent.insert(other.clone(), id.clone());
                queue.push_back(other.clone());
            }
        }
    }
    for b in &net.buses {
        let id = bus_id[b.key.as_str()].clone();
        let mut upstream = None;
        let mut at = id.clone();
        while let Some(br) = parent.get(&at) {
            if let Some(devs) = devices_on_branch.get(br) {
                upstream = devs.first().cloned();
                break;
            }
            let (f, t) = &branch_ends[br];
            at = if *t == at { f.clone() } else { t.clone() };
        }
        if upstream.is_none() {
            warnings.push(ImportWarning::new("no_upstream_device", &b.name, "no protective device found between the bus and the source"));
        }
        let (electrode, gap, distance, h, w, dpt, _) = typical_equipment(b.kv);
        let [h, w, dpt] = b.enclosure_mm.unwrap_or([h, w, dpt]);
        project.equipment.push(ArcEquipment {
            id: format!("af-{id}"), name: b.name.clone(), bus: id, electrode, gap_mm: gap, distance_mm: distance,
            height_mm: h, width_mm: w, depth_mm: dpt, upstream_device: upstream, clearing_s: None,
            fallback_duration_s: None, basis: "assumed".into(),
        });
    }
    ImportResult { project, warnings }
}
