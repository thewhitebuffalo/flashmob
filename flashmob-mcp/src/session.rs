//! One in-memory Flashmob project for the MCP server.
//!
//! Writes are allowed only in the opened file's directory, under ~/Documents,
//! or under /tmp. A path inside this repository is refused, including reads
//! through `open`, so a later one-line or curve export cannot land in the repo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use flashmob::exec::{self, Command};
use flashmob::library::{self, ApplyDevice};
use flashmob::model::Project;
use flashmob::study::{self, Studies, StudyOutput};
use serde_json::{json, Value};

/// Printed by every study tool. A result that trips any of these is not a label.
pub const LABEL_NOTE: &str = "Warnings, complete: false, an unresolved clearing path, or an assumption means the number is not label-ready. Feeder length and cable size must come from the model.";

const SEARCH_CAP: usize = 20;

pub struct State {
    session: Option<Session>,
    library: Option<CachedLibrary>,
}

struct Session {
    project: Project,
    /// Canonical path the project was opened from, or the path of the last save.
    path: PathBuf,
    dirty: bool,
    study: Option<StudyOutput>,
    study_full: bool,
}

struct CachedLibrary {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
    library: library::Library,
}

impl State {
    pub fn new() -> Self {
        Self {
            session: None,
            library: None,
        }
    }

    pub fn open(&mut self, path: &str) -> Result<Value, String> {
        let full = normalize(Path::new(path))?;
        if !full.is_file() {
            return Err(format!("could not read {}: not a file", full.display()));
        }
        refuse_repo(&full)?;
        let text = std::fs::read_to_string(&full)
            .map_err(|err| format!("could not read {}: {err}", full.display()))?;
        let project = exec::load_project(&text)?;
        let name = project.name.clone();
        let buses = project.buses.len();
        self.session = Some(Session {
            project,
            path: full.clone(),
            dirty: false,
            study: None,
            study_full: false,
        });
        Ok(json!({
            "path": full,
            "name": name,
            "buses": buses,
            "dirty": false,
        }))
    }

    pub fn save(&mut self, path: Option<&str>, overwrite: bool) -> Result<Value, String> {
        let session = self.session.as_mut().ok_or("no project is open")?;
        let requested = match path {
            Some(path) => PathBuf::from(path),
            None => session.path.clone(),
        };
        let full = ensure_write_allowed(&requested, Some(&session.path))?;
        if full.exists() && !overwrite {
            return Err(format!(
                "{} already exists; pass overwrite true to replace it",
                full.display()
            ));
        }
        if full.exists() && !full.is_file() {
            return Err(format!("{} is not a file", full.display()));
        }
        let text = serde_json::to_string_pretty(&session.project).map_err(|err| err.to_string())?;
        std::fs::write(&full, text + "\n")
            .map_err(|err| format!("could not write {}: {err}", full.display()))?;
        session.path = full.clone();
        session.dirty = false;
        Ok(json!({ "path": full, "dirty": false }))
    }

    pub fn summary(&self) -> Result<Value, String> {
        let session = self.session.as_ref().ok_or("no project is open")?;
        let project = &session.project;
        let component = |id: &str, name: &str| json!({ "id": id, "name": name });
        Ok(json!({
            "name": project.name,
            "path": session.path,
            "dirty": session.dirty,
            "counts": {
                "buses": project.buses.len(),
                "branches": project.branches.len(),
                "loads": project.loads.len(),
                "sources": project.sources.len(),
                "motors": project.motors.len(),
                "devices": project.devices.len(),
                "equipment": project.equipment.len(),
                "switches": project.switches.len(),
                "assumptions": project.assumptions.len(),
            },
            "buses": project.buses.iter().map(|bus| json!({
                "id": bus.id,
                "name": bus.name,
                "kv": bus.kv,
            })).collect::<Vec<_>>(),
            "branches": project.branches.iter().map(|item| component(&item.id, &item.name)).collect::<Vec<_>>(),
            "loads": project.loads.iter().map(|item| component(&item.id, &item.name)).collect::<Vec<_>>(),
            "sources": project.sources.iter().map(|item| component(&item.id, &item.name)).collect::<Vec<_>>(),
            "motors": project.motors.iter().map(|item| json!({
                "id": item.id,
                "name": item.name,
                "kv": item.kv,
            })).collect::<Vec<_>>(),
            "devices": project.devices.iter().map(|item| component(&item.id, &item.name)).collect::<Vec<_>>(),
            "equipment": project.equipment.iter().map(|item| component(&item.id, &item.name)).collect::<Vec<_>>(),
            "switches": project.switches.iter().map(|item| component(&item.id, &item.name)).collect::<Vec<_>>(),
            "assumptions": project.assumptions.iter().map(|item| component(&item.id, &item.statement)).collect::<Vec<_>>(),
        }))
    }

    pub fn topology(&self) -> Result<Value, String> {
        let session = self.session.as_ref().ok_or("no project is open")?;
        let topology = flashmob::connectivity::topology(&session.project)?;
        serde_json::to_value(topology).map_err(|err| err.to_string())
    }

    /// Apply exec ops one at a time. Validation runs on the project after the
    /// batch, because an intermediate model may be incomplete. `export_package`
    /// is refused here until that tool is added on its own.
    pub fn apply(&mut self, ops: &[Value]) -> Result<Value, String> {
        let session = self.session.as_mut().ok_or("no project is open")?;
        let mut project = session.project.clone();
        let mut study = session.study.clone();
        let mut study_full = session.study_full;
        let mut rows = Vec::new();
        for (index, value) in ops.iter().enumerate() {
            let command: Command = match serde_json::from_value(value.clone()) {
                Ok(command) => command,
                Err(err) => {
                    rows.push(json!({
                        "index": index,
                        "op": value.get("op").and_then(Value::as_str).unwrap_or(""),
                        "ok": false,
                        "error": format!("could not read the op: {err}"),
                        "ids": [],
                    }));
                    break;
                }
            };
            let op = op_name(&command);
            if matches!(command, Command::ExportPackage { .. }) {
                rows.push(json!({
                    "index": index,
                    "op": op,
                    "ok": false,
                    "error": "export_package is not available on this server yet",
                    "ids": [],
                }));
                break;
            }
            let before = ids_of(&project);
            let response = exec::apply(project.clone(), std::slice::from_ref(&command));
            let command_failed = response
                .error
                .as_ref()
                .is_some_and(|err| err.starts_with("command "));
            let after_ids = touched(&before, &ids_of(&response.project), &command);
            if command_failed {
                // A failed run drops its results. A failed edit may have changed
                // the model before returning the error; keep that project.
                if matches!(command, Command::Run { .. }) {
                    study = None;
                    study_full = false;
                }
                project = response.project;
                rows.push(json!({
                    "index": index,
                    "op": op,
                    "ok": false,
                    "error": response.error,
                    "ids": after_ids,
                }));
                break;
            }
            if is_edit(&command) {
                study = None;
                study_full = false;
            }
            if let Some(results) = response.results {
                study = Some(results);
                study_full = match &command {
                    Command::Run { studies } => studies_are_full(studies),
                    Command::Sld | Command::Tcc { .. } => true,
                    _ => study_full,
                };
            }
            project = response.project;
            rows.push(json!({
                "index": index,
                "op": op,
                "ok": true,
                "error": null,
                "ids": after_ids,
            }));
        }
        let changed =
            serde_json::to_string(&project).ok() != serde_json::to_string(&session.project).ok();
        if changed {
            session.dirty = true;
        }
        session.project = project;
        session.study = study;
        session.study_full = study_full;
        let validation_errors = session.project.validate();
        let ok = rows
            .iter()
            .all(|row| row.get("ok").and_then(Value::as_bool) == Some(true))
            && validation_errors.is_empty();
        Ok(json!({
            "ok": ok,
            "ops": rows,
            "validation_errors": validation_errors,
            "dirty": session.dirty,
        }))
    }

    pub fn run(&mut self, studies: &[String]) -> Result<Value, String> {
        let session = self.session.as_mut().ok_or("no project is open")?;
        let which = Studies::parse(&studies.to_vec())?;
        match study::run(&session.project, which) {
            Ok(output) => {
                let report = study_rows(&session.project, &output);
                session.study = Some(output);
                session.study_full =
                    which.loadflow && which.fault && which.arcflash && which.coordination;
                Ok(report)
            }
            Err(err) => {
                session.study = None;
                session.study_full = false;
                Err(err)
            }
        }
    }

    pub fn sld(&mut self, overwrite: bool) -> Result<Value, String> {
        let session = self.session.as_mut().ok_or("no project is open")?;
        let study = session
            .study
            .as_ref()
            .ok_or("run a study before writing an sld")?;
        let svg = flashmob::sld::to_svg(&flashmob::sld::diagram(&session.project, study));
        let path = derived_path(&session.path, ".sld.svg");
        let full = ensure_write_allowed(&path, Some(&session.path))?;
        write_text(&full, &svg, overwrite)?;
        let caption = format!(
            "One-line for {}, {} buses.",
            session.project.name,
            session.project.buses.len()
        );
        Ok(json!({
            "path": full,
            "caption": caption,
            "note": LABEL_NOTE,
        }))
    }

    pub fn tcc(
        &mut self,
        ref_kv: Option<f64>,
        devices: &[String],
        overwrite: bool,
    ) -> Result<Value, String> {
        let session = self.session.as_mut().ok_or("no project is open")?;
        let drawn = flashmob::tcc::plot(&session.project, session.study.as_ref(), ref_kv, devices)?;
        let svg = flashmob::tcc::to_svg(&drawn);
        let csv = flashmob::tcc::to_csv(&drawn);
        let svg_path = derived_path(&session.path, ".tcc.svg");
        let csv_path = derived_path(&session.path, ".tcc.csv");
        let svg_full = ensure_write_allowed(&svg_path, Some(&session.path))?;
        let csv_full = ensure_write_allowed(&csv_path, Some(&session.path))?;
        write_text(&svg_full, &svg, overwrite)?;
        if let Err(err) = write_text(&csv_full, &csv, overwrite) {
            // The SVG is already written. Leave it and report the CSV failure.
            return Err(format!(
                "{err}. The SVG was written at {}",
                svg_full.display()
            ));
        }
        let referred = ref_kv
            .map(|kv| format!("{kv} kV"))
            .unwrap_or_else(|| "the plotted reference".to_string());
        let caption = format!(
            "Time-current curves for {} at {referred}.",
            session.project.name
        );
        Ok(json!({
            "path": svg_full,
            "csv": csv_full,
            "caption": caption,
            "note": LABEL_NOTE,
        }))
    }

    pub fn library_search(&mut self, query: &str, limit: Option<u32>) -> Result<Value, String> {
        let library = self.configured_library()?;
        let terms: Vec<String> = query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .map(str::to_string)
            .collect();
        if terms.is_empty() {
            return Err("query is empty".into());
        }
        let limit = match limit {
            None | Some(0) => SEARCH_CAP,
            Some(value) => (value as usize).min(SEARCH_CAP),
        };
        let matched = library.search(&terms);
        let truncated = matched.len() > limit;
        let hits: Vec<Value> = matched
            .into_iter()
            .take(limit)
            .map(|device| {
                json!({
                    "id": device.id,
                    "manufacturer": device.manufacturer,
                    "category": device.category,
                    "type": device.device_type,
                    "style": device.style,
                    "catalog": device.catalog,
                    "reference": device.reference,
                })
            })
            .collect();
        let returned = hits.len();
        Ok(json!({
            "hits": hits,
            "returned": returned,
            "truncated": truncated,
            "limit": limit,
        }))
    }

    pub fn apply_library(&mut self, request: LibraryApply) -> Result<Value, String> {
        let library_name = configured_library_path()?.display().to_string();
        let library = self.configured_library()?.clone();
        let session = self.session.as_mut().ok_or("no project is open")?;
        if session.project.ids().iter().any(|id| *id == request.id) {
            return Err(format!("id '{}' is already in the project", request.id));
        }
        let fuse_total_clearing = request.fuse_total_clearing;
        let device = library::apply_device(
            &library,
            &ApplyDevice {
                device_id: request.device_id,
                id: request.id.clone(),
                name: request.name.unwrap_or_default(),
                bus: request.bus,
                protected_branch: request.protected_branch,
                rating_a: request.rating_a,
                current_base_a: request.current_base_a,
                voltage_v: request.voltage_v,
                function_name: request.function_name,
                library_name,
                fuse_total_clearing,
                dial: request.dial,
                ct_ratio: request.ct_ratio,
                tap_a: request.tap_a,
                sensor_a: request.sensor_a,
                lt_pickup: pair(
                    request.lt_pickup_function,
                    request.lt_pickup_label,
                    "lt_pickup",
                )?,
                lt_delay: pair(
                    request.lt_delay_function,
                    request.lt_delay_label,
                    "lt_delay",
                )?,
                lt_delay_multiple: request.lt_delay_multiple,
                st_pickup: pair(
                    request.st_pickup_function,
                    request.st_pickup_label,
                    "st_pickup",
                )?,
                st_delay: pair(
                    request.st_delay_function,
                    request.st_delay_label,
                    "st_delay",
                )?,
                inst: pair(request.inst_function, request.inst_label, "inst")?,
                inst_s: request.inst_s,
                pickup_base: request.pickup_base,
            },
        )?;
        let notes = device
            .library
            .as_ref()
            .map(|link| link.unsupported_elements.clone())
            .unwrap_or_default();
        let id = device.id.clone();
        let basis = device.basis.clone();
        session.project.devices.push(device);
        session.dirty = true;
        session.study = None;
        session.study_full = false;
        Ok(json!({
            "id": id,
            "fuse_total_clearing": fuse_total_clearing,
            "basis": basis,
            "notes": notes,
            "note": "The copied curve is a snapshot marked assumed. The slower band is not total clearing unless fuse_total_clearing was set on this call. In the current range where only the fast band has started, clearing is unresolved and that range is named. A fuse category may estimate clearing from the melting curve by IEEE 1584-2018 Clause 6.9.2 (10% of the melting time plus 0.004 s, or 0.01 s when that total is shorter); that estimate is assumed and is not an arc-flash label.",
        }))
    }

    fn configured_library(&mut self) -> Result<&library::Library, String> {
        let path = configured_library_path()?;
        let meta = std::fs::metadata(&path)
            .map_err(|err| format!("could not read {}: {err}", path.display()))?;
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let reuse = self.library.as_ref().is_some_and(|cached| {
            cached.path == path && cached.modified == modified && cached.len == meta.len()
        });
        if !reuse {
            let library = library::library_from_path(&path)?;
            self.library = Some(CachedLibrary {
                path,
                modified,
                len: meta.len(),
                library,
            });
        }
        Ok(&self
            .library
            .as_ref()
            .expect("library was just stored")
            .library)
    }
}

pub struct LibraryApply {
    pub device_id: u32,
    pub id: String,
    pub bus: String,
    pub name: Option<String>,
    pub fuse_total_clearing: bool,
    pub protected_branch: Option<String>,
    pub rating_a: Option<f64>,
    pub current_base_a: Option<f64>,
    pub voltage_v: Option<f64>,
    pub function_name: Option<String>,
    pub dial: Option<f64>,
    pub ct_ratio: Option<f64>,
    pub tap_a: Option<f64>,
    pub sensor_a: Option<f64>,
    pub lt_pickup_function: Option<String>,
    pub lt_pickup_label: Option<String>,
    pub lt_delay_function: Option<String>,
    pub lt_delay_label: Option<String>,
    pub lt_delay_multiple: Option<f64>,
    pub st_pickup_function: Option<String>,
    pub st_pickup_label: Option<String>,
    pub st_delay_function: Option<String>,
    pub st_delay_label: Option<String>,
    pub inst_function: Option<String>,
    pub inst_label: Option<String>,
    pub inst_s: Option<f64>,
    pub pickup_base: Option<String>,
}

fn pair(
    function: Option<String>,
    label: Option<String>,
    name: &str,
) -> Result<Option<flashmob::library_apply::SettingRef>, String> {
    flashmob::library_apply::setting_ref(&function, &label, name)
}

fn configured_library_path() -> Result<PathBuf, String> {
    match std::env::var_os("FLASHMOB_LIBRARY") {
        Some(value) if !value.is_empty() => {
            let path = PathBuf::from(value);
            if path.is_file() {
                Ok(path)
            } else {
                Err(format!("could not read {}: not a file", path.display()))
            }
        }
        _ => Err("FLASHMOB_LIBRARY is not set. This server reads the device library only from that file.".into()),
    }
}

fn study_rows(project: &Project, output: &StudyOutput) -> Value {
    let fault = output.fault.as_ref();
    let envelope = output.worst_case.as_ref();
    let mut buses = Vec::new();
    let mut bus_ids: Vec<String> = project.buses.iter().map(|bus| bus.id.clone()).collect();
    if let Some(fault) = fault {
        for bus in &fault.buses {
            if !bus_ids.iter().any(|id| id == &bus.id) {
                bus_ids.push(bus.id.clone());
            }
        }
    }
    for bus_id in bus_ids {
        let point = fault.and_then(|fault| fault.buses.iter().find(|bus| bus.id == bus_id));
        let model = project.buses.iter().find(|bus| bus.id == bus_id);
        let case =
            envelope.and_then(|envelope| envelope.buses.iter().find(|bus| bus.bus_id == bus_id));
        let complete = match (point, case) {
            (None, _) => false,
            (Some(point), Some(case)) => {
                point.error.is_none() && case.complete && fault.is_some_and(|fault| fault.valid)
            }
            (Some(point), None) => point.error.is_none() && fault.is_some_and(|fault| fault.valid),
        };
        let governing_case =
            case.and_then(|case| case.three_phase_ka.as_ref().map(|max| max.case_id.clone()));
        let mut warnings = Vec::new();
        if let Some(note) = point.and_then(|point| point.note.clone()) {
            warnings.push(note);
        }
        if let Some(error) = point.and_then(|point| point.error.clone()) {
            warnings.push(error);
        }
        buses.push(json!({
            "id": bus_id,
            "name": point.map(|point| point.name.clone()).or_else(|| model.map(|bus| bus.name.clone())).unwrap_or_default(),
            "kv": point.map(|point| point.kv).or(model.map(|bus| bus.kv)).unwrap_or(0.0),
            "three_phase_ka": point.and_then(|point| point.three_phase.as_ref().map(|item| item.symmetrical_ka)),
            "line_to_ground_ka": point.and_then(|point| point.line_to_ground.as_ref().map(|item| item.symmetrical_ka)),
            "line_to_line_ka": point.and_then(|point| point.line_to_line.as_ref().map(|item| item.symmetrical_ka)),
            "line_to_line_ground_ka": point.and_then(|point| point.line_to_line_ground.as_ref().map(|item| item.symmetrical_ka)),
            "governing_case": governing_case,
            "assumed": false,
            "duration_capped": false,
            "warnings": warnings,
            "complete": complete,
        }));
    }
    let mut equipment = Vec::new();
    if let Some(rows) = &output.arc_flash {
        for row in rows {
            let case = envelope.and_then(|envelope| {
                envelope
                    .arc_equipment
                    .iter()
                    .find(|item| item.equipment_id == row.equipment_id)
            });
            let failed = output
                .arc_flash_failures
                .iter()
                .any(|failure| failure.equipment_id == row.equipment_id);
            let complete = match case {
                Some(case) => case.complete && !failed,
                None => !failed,
            };
            equipment.push(json!({
                "equipment_id": row.equipment_id,
                "name": row.name,
                "bus_id": row.bus_id,
                "fault_ka": row.bolted_ka,
                "incident_energy_cal_cm2": row.governing_cal_cm2,
                "boundary_mm": row.afb_mm,
                "governing": row.governing,
                "governing_case": case.and_then(|case| case.incident_energy_cal_cm2.as_ref().map(|max| max.case_id.clone())),
                "assumed": row.assumed,
                "duration_capped": row.duration_capped,
                "warnings": row.warnings,
                "complete": complete,
            }));
        }
    }
    for failure in &output.arc_flash_failures {
        if equipment.iter().any(|row| {
            row.get("equipment_id").and_then(Value::as_str) == Some(failure.equipment_id.as_str())
        }) {
            continue;
        }
        equipment.push(json!({
            "equipment_id": failure.equipment_id,
            "name": failure.name,
            "bus_id": failure.bus_id,
            "fault_ka": null,
            "incident_energy_cal_cm2": null,
            "boundary_mm": null,
            "governing": null,
            "governing_case": null,
            "assumed": false,
            "duration_capped": false,
            "warnings": [failure.error],
            "complete": false,
        }));
    }
    json!({
        "case_id": output.case_id,
        "warnings": output.warnings,
        "buses": buses,
        "arc_flash": equipment,
        "note": LABEL_NOTE,
    })
}

fn studies_are_full(studies: &[String]) -> bool {
    Studies::parse(&studies.to_vec())
        .is_ok_and(|which| which.loadflow && which.fault && which.arcflash && which.coordination)
}

fn is_edit(command: &Command) -> bool {
    !matches!(
        command,
        Command::Run { .. }
            | Command::Topology
            | Command::Sld
            | Command::Tcc { .. }
            | Command::ExportPackage { .. }
    )
}

fn ids_of(project: &Project) -> BTreeSet<String> {
    project.ids().into_iter().map(str::to_string).collect()
}

fn touched(before: &BTreeSet<String>, after: &BTreeSet<String>, command: &Command) -> Vec<String> {
    let mut ids: BTreeSet<String> = before.symmetric_difference(after).cloned().collect();
    if let Some(id) = command_id(command) {
        ids.insert(id.to_string());
    }
    if matches!(command, Command::Sample | Command::Replace { .. }) {
        ids.extend(after.iter().cloned());
    }
    ids.into_iter().collect()
}

fn command_id(command: &Command) -> Option<&str> {
    Some(match command {
        Command::AddBus { id, .. }
        | Command::AddAssumption { id, .. }
        | Command::AddLine { id, .. }
        | Command::AddTransformer { id, .. }
        | Command::AddLoad { id, .. }
        | Command::AddSource { id, .. }
        | Command::AddMotor { id, .. }
        | Command::AddDevice { id, .. }
        | Command::ApplyLibraryDevice { id, .. }
        | Command::AddSwitch { id, .. }
        | Command::AddSwitchGroup { id, .. }
        | Command::AddOperatingCase { id, .. }
        | Command::AddEquipment { id, .. }
        | Command::Remove { id }
        | Command::SetBus { id, .. }
        | Command::SetSource { id, .. }
        | Command::SetDevice { id, .. } => id.as_str(),
        Command::Sample
        | Command::Replace { .. }
        | Command::SetOptions { .. }
        | Command::Run { .. }
        | Command::Topology
        | Command::Sld
        | Command::Tcc { .. }
        | Command::ExportPackage { .. } => return None,
    })
}

fn op_name(command: &Command) -> String {
    serde_json::to_value(command)
        .ok()
        .and_then(|value| value.get("op")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn repo_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .map_err(|err| format!("could not resolve the Flashmob repo: {err}"))
}

fn refuse_repo(path: &Path) -> Result<(), String> {
    let root = repo_root()?;
    if path.starts_with(&root) {
        Err(format!(
            "refused: {} is inside the Flashmob repo",
            path.display()
        ))
    } else {
        Ok(())
    }
}

fn normalize(path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| err.to_string())?
            .join(path)
    };
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|err| format!("could not resolve {}: {err}", path.display()));
    }
    let name = path
        .file_name()
        .ok_or_else(|| format!("path has no file name: {}", path.display()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        return Err(format!("directory does not exist: {}", parent.display()));
    }
    let parent = parent
        .canonicalize()
        .map_err(|err| format!("could not resolve {}: {err}", parent.display()))?;
    Ok(parent.join(name))
}

fn documents_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let documents = home.join("Documents");
    documents.canonicalize().ok()
}

fn tmp_dir() -> Option<PathBuf> {
    Path::new("/tmp").canonicalize().ok()
}

fn ensure_write_allowed(path: &Path, opened: Option<&Path>) -> Result<PathBuf, String> {
    let full = normalize(path)?;
    refuse_repo(&full)?;
    let mut roots = Vec::new();
    if let Some(opened) = opened {
        if let Some(parent) = opened.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if let Some(documents) = documents_dir() {
        roots.push(documents);
    }
    if let Some(tmp) = tmp_dir() {
        roots.push(tmp);
    }
    if roots.iter().any(|root| full.starts_with(root)) {
        Ok(full)
    } else {
        Err(format!(
            "refused: {} is outside the opened project directory, ~/Documents, and /tmp",
            full.display()
        ))
    }
}

fn derived_path(project: &Path, suffix: &str) -> PathBuf {
    let mut name = project.file_stem().unwrap_or_default().to_os_string();
    name.push(suffix);
    project.with_file_name(name)
}

fn write_text(path: &Path, text: &str, overwrite: bool) -> Result<(), String> {
    if path.exists() && !overwrite {
        return Err(format!(
            "{} already exists; pass overwrite true to replace it",
            path.display()
        ));
    }
    if path.exists() && !path.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    std::fs::write(path, text).map_err(|err| format!("could not write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn scratch() -> PathBuf {
        let dir = PathBuf::from(format!("/tmp/flashmob-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn open_apply_and_run_use_the_built_in_sample() {
        let dir = scratch();
        let path = dir.join("sample.json");
        let project = Project::sample();
        std::fs::write(&path, serde_json::to_string_pretty(&project).unwrap()).unwrap();

        let mut state = State::new();
        let opened = state.open(path.to_str().unwrap()).unwrap();
        assert_eq!(opened["name"], "Sample");
        assert_eq!(opened["buses"], 3);

        let ops = vec![
            json!({"op": "add_bus", "id": "extra", "name": "Extra", "kv": 0.48}),
            json!({"op": "add_assumption", "id": "note-1", "statement": "Feeder length and cable size come from the model."}),
        ];
        let applied = state.apply(&ops).unwrap();
        assert_eq!(applied["ok"], true, "{applied}");
        assert!(applied["validation_errors"].as_array().unwrap().is_empty());
        let ids: Vec<&str> = applied["ops"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row["ids"].as_array().unwrap())
            .filter_map(Value::as_str)
            .collect();
        assert!(ids.contains(&"extra"));
        assert!(ids.contains(&"note-1"));

        let summary = state.summary().unwrap();
        assert_eq!(summary["counts"]["buses"], 4);
        assert!(summary["buses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|bus| bus["id"] == "extra" && bus["kv"] == 0.48));

        let report = state.run(&["fault".into()]).unwrap();
        assert_eq!(report["note"], LABEL_NOTE);
        let util = report["buses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|bus| bus["id"] == "util")
            .unwrap();
        assert!(util["three_phase_ka"].as_f64().unwrap() > 0.0);
        assert_eq!(util["complete"], true);
    }

    #[test]
    fn a_path_inside_the_repo_is_refused() {
        let mut state = State::new();
        let readme = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../README.md");
        let err = state.open(readme.to_str().unwrap()).unwrap_err();
        assert!(err.contains("inside the Flashmob repo"), "{err}");

        let dir = scratch();
        let path = dir.join("sample.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&Project::sample()).unwrap(),
        )
        .unwrap();
        state.open(path.to_str().unwrap()).unwrap();
        let repo_target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/lib.rs");
        let err = state
            .save(Some(repo_target.to_str().unwrap()), true)
            .unwrap_err();
        assert!(err.contains("inside the Flashmob repo"), "{err}");
        let text = std::fs::read_to_string(&repo_target).unwrap();
        assert!(text.contains("pub mod exec"));
    }

    #[test]
    fn library_search_uses_a_tiny_fixture_and_caps_at_twenty() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("FLASHMOB_LIBRARY");
        let dir = scratch();
        let path = dir.join("fixture-library.json");
        std::fs::write(&path, fixture_library()).unwrap();
        std::env::set_var("FLASHMOB_LIBRARY", &path);

        let mut state = State::new();
        let lpj = state.library_search("lpj", None).unwrap();
        assert_eq!(lpj["returned"], 1);
        assert_eq!(lpj["hits"][0]["catalog"], "LPJ-100SP");
        assert_eq!(lpj["truncated"], false);

        let many = state.library_search("widget", Some(100)).unwrap();
        assert_eq!(many["returned"], 20);
        assert_eq!(many["limit"], 20);
        assert_eq!(many["truncated"], true);

        match previous {
            Some(value) => std::env::set_var("FLASHMOB_LIBRARY", value),
            None => std::env::remove_var("FLASHMOB_LIBRARY"),
        }
    }

    fn fixture_library() -> String {
        let mut devices = vec![json!({
            "id": 1,
            "category": "fuse",
            "manufacturer": "Example",
            "type": "100 A",
            "style": "LPJ",
            "catalog": "LPJ-100SP",
            "reference": "sheet",
            "notes": "",
            "created": "",
            "modified": "",
            "ratings": [],
            "frames": [],
            "trip_units": []
        })];
        for id in 2..=26 {
            devices.push(json!({
                "id": id,
                "category": "breaker",
                "manufacturer": "Example",
                "type": "frame",
                "style": "WIDGET",
                "catalog": format!("W-{id}"),
                "reference": "",
                "notes": "",
                "created": "",
                "modified": "",
                "ratings": [],
                "frames": [],
                "trip_units": []
            }));
        }
        serde_json::to_string(&json!({
            "format": "flashmob-device-library",
            "version": 1,
            "source": { "description": "fixture", "bytes": 0 },
            "summary": {
                "devices": devices.len(),
                "devices_by_category": {},
                "functions_by_kind": {},
                "skipped_records_by_class": {},
                "warnings": []
            },
            "devices": devices
        }))
        .unwrap()
    }
}
