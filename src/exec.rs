use serde::{Deserialize, Serialize};

use crate::connectivity::{self, Topology};
use crate::model::{ArcEquipment, GeneratorKind, GeneratorRating, MotorKind, Branch, BranchKind, Bus, CurrentLimiting, CurveSpec, DecrementPoint, Device, Electrode, EquipmentClass, EquipmentState, InterruptingRating, LibraryLink, Load, Motor, OperatingCase, Project, Source, SourceState, Switch, SwitchGroup, SwitchKind, SwitchState, XfmrConn};
use crate::sld;
use crate::study::{self, Studies, StudyOutput};
use crate::tcc;

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
        #[serde(default)] equipment_class: Option<EquipmentClass>,
        #[serde(default)] prefault_pu: Option<f64>,
    },
    AddAssumption { id: String, statement: String },
    SetOptions {
        #[serde(default)] name: Option<String>,
        #[serde(default)] s_base_mva: Option<f64>,
        #[serde(default)] arc_duration_cap_s: Option<f64>,
        #[serde(default)] escape_possible: Option<bool>,
        #[serde(default)] motor_contribution_s: Option<f64>,
        #[serde(default)] prefault: Option<crate::model::Prefault>,
        #[serde(default)] fault_method: Option<crate::model::FaultMethod>,
        #[serde(default)] prefault_pu: Option<f64>,
        #[serde(default)] cleared_fault_fraction: Option<f64>,
        #[serde(default)] machine_step_down: Option<crate::model::MachineStepDown>,
        #[serde(default)] disable_machine_step_down: bool,
        #[serde(default)] retime_on_reduced_current: Option<bool>,
        #[serde(default)] upstream_backup: Option<crate::model::UpstreamBackup>,
        #[serde(default)] disable_upstream_backup: bool,
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
        #[serde(default)] x0_over_x1: Option<f64>,
        #[serde(default)] r0_over_r1: Option<f64>,
        #[serde(default)] is_slack: bool,
        #[serde(default = "default_true")] in_service: bool,
        #[serde(default)] p_mw: f64,
        #[serde(default)] machine_kind: Option<GeneratorKind>,
        #[serde(default)] transient_mva_sc: Option<f64>,
        #[serde(default)] generator: Option<GeneratorRating>,
    },
    AddMotor {
        id: String,
        #[serde(default)] name: String,
        bus: String,
        hp: f64,
        kv: f64,
        #[serde(default)] pf: Option<f64>,
        #[serde(default)] efficiency: Option<f64>,
        #[serde(default)] x_subtransient_pu: Option<f64>,
        #[serde(default)] xr: Option<f64>,
        #[serde(default)] machine_kind: Option<MotorKind>,
        #[serde(default)] synchronous_rpm: Option<f64>,
        #[serde(default)] phases: Option<u8>,
        #[serde(default)] decay_time_constant_s: Option<f64>,
    },
    AddDevice { id: String, #[serde(default)] name: String, bus: String, curve: CurveSpec, #[serde(default)] basis: String, #[serde(default)] protected_branch: Option<String>, #[serde(default)] breaker_interrupting_s: Option<f64>, #[serde(default)] fuse_total_clearing: bool, #[serde(default)] curve_is_total_clearing: bool, #[serde(default)] library: Option<LibraryLink>, #[serde(default)] current_limiting: Option<CurrentLimiting> },
    /// Copy a library point curve, dial curve, or trip setting onto a new device. The result is a snapshot.
    ApplyLibraryDevice {
        library_path: String,
        device_id: u32,
        id: String,
        bus: String,
        #[serde(default)] name: String,
        #[serde(default)] rating_a: Option<f64>,
        #[serde(default)] current_base_a: Option<f64>,
        #[serde(default)] voltage_v: Option<f64>,
        #[serde(default)] function_name: Option<String>,
        #[serde(default)] protected_branch: Option<String>,
        #[serde(default)] library_name: String,
        /// Set only when the governing band is already total clearing.
        #[serde(default)] fuse_total_clearing: bool,
        #[serde(default)] dial: Option<f64>,
        #[serde(default)] ct_ratio: Option<f64>,
        #[serde(default)] tap_a: Option<f64>,
        #[serde(default)] sensor_a: Option<f64>,
        #[serde(default)] lt_pickup_function: Option<String>,
        #[serde(default)] lt_pickup_label: Option<String>,
        #[serde(default)] lt_delay_function: Option<String>,
        #[serde(default)] lt_delay_label: Option<String>,
        #[serde(default)] lt_delay_multiple: Option<f64>,
        #[serde(default)] st_pickup_function: Option<String>,
        #[serde(default)] st_pickup_label: Option<String>,
        #[serde(default)] st_delay_function: Option<String>,
        #[serde(default)] st_delay_label: Option<String>,
        #[serde(default)] inst_function: Option<String>,
        #[serde(default)] inst_label: Option<String>,
        #[serde(default)] inst_s: Option<f64>,
        /// "sensor" or "long_time" when a pickup function does not state its base.
        #[serde(default)] pickup_base: Option<String>,
    },
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
        #[serde(default)] bracing_test_xr: Option<f64>,
        #[serde(default)] equipment_class: Option<EquipmentClass>,
    },
    SetSource {
        id: String,
        #[serde(default)] mva_sc: Option<f64>,
        #[serde(default)] decrement_curve: Option<Vec<DecrementPoint>>,
        #[serde(default)] xr: Option<f64>,
        #[serde(default)] v_pu: Option<f64>,
        #[serde(default)] x0_over_x1: Option<f64>,
        #[serde(default)] r0_over_r1: Option<f64>,
        #[serde(default)] is_slack: Option<bool>,
        #[serde(default)] in_service: Option<bool>,
        #[serde(default)] machine_kind: Option<GeneratorKind>,
        #[serde(default)] transient_mva_sc: Option<f64>,
        #[serde(default)] generator: Option<GeneratorRating>,
    },
    /// Change fields of an existing protective device. Omitted fields are kept;
    /// `clear` lists optional fields to remove (protected_branch,
    /// breaker_interrupting_s, library, interrupting, current_limiting).
    SetDevice {
        id: String,
        #[serde(default)] name: Option<String>,
        #[serde(default)] bus: Option<String>,
        #[serde(default)] curve: Option<CurveSpec>,
        #[serde(default)] basis: Option<String>,
        #[serde(default)] protected_branch: Option<String>,
        #[serde(default)] breaker_interrupting_s: Option<f64>,
        #[serde(default)] fuse_total_clearing: Option<bool>,
        #[serde(default)] curve_is_total_clearing: Option<bool>,
        #[serde(default)] library: Option<LibraryLink>,
        #[serde(default)] interrupting: Option<InterruptingRating>,
        #[serde(default)] current_limiting: Option<CurrentLimiting>,
        #[serde(default)] clear: Vec<String>,
    },
    Run { #[serde(default)] studies: Vec<String> },
    Topology,
    Sld,
    Tcc { #[serde(default)] ref_kv: Option<f64>, #[serde(default)] devices: Vec<String> },
    /// Write the engineering data package for mapping. `output` is a directory, or an `.xml`
    /// path (other files are written beside it). Existing files are an error unless `overwrite`.
    ExportPackage { #[serde(default)] output: Option<String>, #[serde(default)] overwrite: bool },
}

fn one() -> f64 { 1.0 }
fn default_true() -> bool { true }

#[derive(Clone, Debug, Serialize)]
pub struct ExecResponse {
    pub ok: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    /// `Project::validate()` errors for the final project. Any entry makes `ok` false.
    pub validation_errors: Vec<String>,
    pub project: Project,
    pub results: Option<StudyOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topology: Option<Topology>,
    pub sld_svg: Option<String>,
    pub tcc_svg: Option<String>,
    pub tcc_csv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<crate::package::PackageExport>,
}

pub fn exec_request(json: &str) -> ExecResponse {
    let request: ExecRequest = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(err) => {
            return ExecResponse {
                ok: false,
                error: Some(format!("could not read the request: {err}")),
                warnings: Vec::new(),
                validation_errors: Vec::new(),
                project: Project::default(),
                results: None,
                topology: None,
                sld_svg: None,
                tcc_svg: None,
                tcc_csv: None,
                package: None,
            };
        }
    };
    apply(request.project.unwrap_or_default(), &request.commands)
}

/// Study outputs and derived views carried between commands.
#[derive(Default)]
struct Outputs {
    results: Option<StudyOutput>,
    /// `results` came from a full study run. sld/tcc reuse it even when one
    /// study (such as load flow) failed, rather than rerunning every study.
    full: bool,
    topology: Option<Topology>,
    sld_svg: Option<String>,
    tcc_svg: Option<String>,
    tcc_csv: Option<String>,
    package: Option<crate::package::PackageExport>,
}

/// Apply commands in order. Intermediate states may be incomplete; the final
/// project is validated, and any validation error makes the response `ok: false`.
pub fn apply(mut project: Project, commands: &[Command]) -> ExecResponse {
    let mut out = Outputs::default();
    let mut failure = None;
    for (n, command) in commands.iter().enumerate() {
        if let Err(err) = apply_one(&mut project, command, &mut out) {
            failure = Some(format!("command {n} ({}) : {err}", op_name(command)));
            break;
        }
    }
    let validation_errors = project.validate();
    let mut warnings = out.results.as_ref().map(|r| r.warnings.clone()).unwrap_or_default();
    if !validation_errors.is_empty() {
        let message = format!("project is invalid: {}", validation_errors.join("; "));
        if failure.is_none() { failure = Some(message); } else { warnings.push(message); }
    }
    ExecResponse {
        ok: failure.is_none(),
        error: failure,
        warnings,
        validation_errors,
        project,
        results: out.results,
        topology: out.topology,
        sld_svg: out.sld_svg,
        tcc_svg: out.tcc_svg,
        tcc_csv: out.tcc_csv,
        package: out.package,
    }
}

/// Commands that only read the model. Every other command is a model edit
/// and invalidates results and every derived view.
fn is_view(command: &Command) -> bool {
    matches!(command, Command::Run { .. } | Command::Topology | Command::Sld | Command::Tcc { .. } | Command::ExportPackage { .. })
}

fn apply_one(project: &mut Project, command: &Command, out: &mut Outputs) -> Result<(), String> {
    if !is_view(command) {
        *out = Outputs::default();
    }
    match command {
        Command::Sample => *project = Project::sample(),
        Command::Replace { project: next } => *project = next.clone(),
        Command::AddBus { id, name, kv, shunt_kvar, bracing_ka, main_rating_a, equipment_class, prefault_pu } => {
            fresh(id, project)?;
            project.buses.push(Bus {
                id: id.clone(),
                name: named(name, id),
                kv: *kv,
                shunt_kvar: *shunt_kvar,
                bracing_ka: *bracing_ka,
                main_rating_a: *main_rating_a,
                bracing_test_xr: None, equipment_class: *equipment_class, prefault_pu: *prefault_pu,
            });
        }
        Command::AddAssumption { id, statement } => {
            fresh(id, project)?;
            project.assumptions.push(crate::model::Assumption { id: id.clone(), statement: statement.clone() });
        }
        Command::SetOptions { name, s_base_mva, arc_duration_cap_s, escape_possible, motor_contribution_s, prefault, prefault_pu, cleared_fault_fraction, machine_step_down, disable_machine_step_down, retime_on_reduced_current, upstream_backup, disable_upstream_backup, fault_method } => {
            if let Some(name) = name {
                project.name = name.clone();
            }
            if let Some(base) = s_base_mva {
                project.s_base_mva = *base;
            }
            if let Some(cap) = arc_duration_cap_s {
                project.arc_duration_cap_s = *cap;
            }
            if let Some(escape) = escape_possible {
                project.escape_possible = *escape;
            }
            if let Some(window) = motor_contribution_s {
                project.motor_contribution_s = *window;
            }
            if let Some(prefault) = prefault {
                project.prefault = *prefault;
            }
            if let Some(method) = fault_method {
                project.fault_method = *method;
            }
            if let Some(value) = prefault_pu { project.prefault_pu = *value; }
            if let Some(value) = cleared_fault_fraction { project.cleared_fault_fraction = *value; }
            if let Some(step) = machine_step_down { project.machine_step_down = Some(step.clone()); }
            if *disable_machine_step_down { project.machine_step_down = None; }
            if let Some(value) = retime_on_reduced_current { project.retime_on_reduced_current = *value; }
            if let Some(backup) = upstream_backup { project.upstream_backup = Some(backup.clone()); }
            if *disable_upstream_backup { project.upstream_backup = None; }
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
        Command::AddSource { id, name, bus, v_pu, angle_deg, mva_sc, decrement_curve, xr, x0_over_x1, r0_over_r1, is_slack, in_service, p_mw, machine_kind, transient_mva_sc, generator } => {
            fresh(id, project)?;
            let mut defaulted = Vec::new();
            let mut entered = |value: &Option<f64>, field: &str| value.unwrap_or_else(|| { defaulted.push(field.to_string()); Source::default_value(field).unwrap() });
            let (x0_over_x1, r0_over_r1) = (entered(x0_over_x1, "x0_over_x1"), entered(r0_over_r1, "r0_over_r1"));
            project.sources.push(Source {
                id: id.clone(),
                name: named(name, id),
                bus: bus.clone(),
                v_pu: *v_pu,
                angle_deg: *angle_deg,
                mva_sc: *mva_sc,
                decrement_curve: decrement_curve.clone(),
                xr: *xr,
                x0_over_x1,
                r0_over_r1,
                is_slack: *is_slack,
                in_service: *in_service,
                p_mw: *p_mw,
                qmin_mvar: None,
                qmax_mvar: None,
                machine_kind: *machine_kind,
                transient_mva_sc: *transient_mva_sc,
                generator: generator.clone(),
                defaulted,
            });
        }
        Command::AddMotor { id, name, bus, hp, kv, pf, efficiency, x_subtransient_pu, xr, machine_kind, synchronous_rpm, phases, decay_time_constant_s } => {
            fresh(id, project)?;
            let mut defaulted = Vec::new();
            let mut entered = |value: &Option<f64>, field: &str| value.unwrap_or_else(|| { defaulted.push(field.to_string()); Motor::default_value(field).unwrap() });
            let (pf, efficiency, x_subtransient_pu, xr) = (entered(pf, "pf"), entered(efficiency, "efficiency"), entered(x_subtransient_pu, "x_subtransient_pu"), entered(xr, "xr"));
            project.motors.push(Motor {
                id: id.clone(),
                name: named(name, id),
                bus: bus.clone(),
                hp: *hp,
                kv: *kv,
                pf,
                efficiency,
                x_subtransient_pu,
                xr,
                machine_kind: *machine_kind,
                synchronous_rpm: *synchronous_rpm,
                phases: *phases,
                decay_time_constant_s: *decay_time_constant_s,
                defaulted,
            });
        }
        Command::AddDevice { id, name, bus, curve, basis, protected_branch, breaker_interrupting_s, fuse_total_clearing, curve_is_total_clearing, library, current_limiting } => {
            fresh(id, project)?;
            project.devices.push(Device { id: id.clone(), name: named(name, id), bus: bus.clone(), curve: curve.clone(), basis: basis.clone(), protected_branch: protected_branch.clone(), breaker_interrupting_s: *breaker_interrupting_s, fuse_total_clearing: *fuse_total_clearing, curve_is_total_clearing: *curve_is_total_clearing, library: library.clone(), interrupting: None, current_limiting: current_limiting.clone() });
        }
        Command::ApplyLibraryDevice { library_path, device_id, id, bus, name, rating_a, current_base_a, voltage_v, function_name, protected_branch, library_name, fuse_total_clearing, dial, ct_ratio, tap_a, sensor_a, lt_pickup_function, lt_pickup_label, lt_delay_function, lt_delay_label, lt_delay_multiple, st_pickup_function, st_pickup_label, st_delay_function, st_delay_label, inst_function, inst_label, inst_s, pickup_base } => {
            fresh(id, project)?;
            let lt_pickup = crate::library_apply::setting_ref(lt_pickup_function, lt_pickup_label, "lt_pickup")?;
            let lt_delay = crate::library_apply::setting_ref(lt_delay_function, lt_delay_label, "lt_delay")?;
            let st_pickup = crate::library_apply::setting_ref(st_pickup_function, st_pickup_label, "st_pickup")?;
            let st_delay = crate::library_apply::setting_ref(st_delay_function, st_delay_label, "st_delay")?;
            let inst = crate::library_apply::setting_ref(inst_function, inst_label, "inst")?;
            let (library, default_name) = if library_path.is_empty() {
                crate::library::open_default_library()?
            } else {
                let path = std::path::Path::new(library_path);
                let library = crate::library::library_from_path(path)?;
                (library, library_path.clone())
            };
            let library_name = if library_name.is_empty() { default_name } else { library_name.clone() };
            let device = crate::library::apply_device(&library, &crate::library::ApplyDevice {
                device_id: *device_id,
                id: id.clone(),
                name: name.clone(),
                bus: bus.clone(),
                protected_branch: protected_branch.clone(),
                rating_a: *rating_a,
                current_base_a: *current_base_a,
                voltage_v: *voltage_v,
                function_name: function_name.clone(),
                library_name,
                fuse_total_clearing: *fuse_total_clearing,
                dial: *dial,
                ct_ratio: *ct_ratio,
                tap_a: *tap_a,
                sensor_a: *sensor_a,
                lt_pickup,
                lt_delay,
                lt_delay_multiple: *lt_delay_multiple,
                st_pickup,
                st_delay,
                inst,
                inst_s: *inst_s,
                pickup_base: pickup_base.clone(),
            })?;
            project.devices.push(device);
        }
        Command::SetDevice { id, name, bus, curve, basis, protected_branch, breaker_interrupting_s, fuse_total_clearing, curve_is_total_clearing, library, interrupting, current_limiting, clear } => {
            for field in clear {
                let settable = match field.as_str() {
                    "protected_branch" => protected_branch.is_some(),
                    "breaker_interrupting_s" => breaker_interrupting_s.is_some(),
                    "library" => library.is_some(),
                    "interrupting" => interrupting.is_some(),
                    "current_limiting" => current_limiting.is_some(),
                    other => return Err(format!("set_device cannot clear '{other}'; clearable fields are protected_branch, breaker_interrupting_s, library, interrupting, current_limiting")),
                };
                if settable { return Err(format!("set_device both sets and clears '{field}'")); }
            }
            let device = project.devices.iter_mut().find(|d| d.id == *id).ok_or_else(|| format!("no device '{id}'"))?;
            if let Some(v) = name { device.name = v.clone(); }
            if let Some(v) = bus { device.bus = v.clone(); }
            if let Some(v) = curve { device.curve = v.clone(); }
            if let Some(v) = basis { device.basis = v.clone(); }
            if let Some(v) = protected_branch { device.protected_branch = Some(v.clone()); }
            if let Some(v) = breaker_interrupting_s { device.breaker_interrupting_s = Some(*v); }
            if let Some(v) = fuse_total_clearing { device.fuse_total_clearing = *v; }
            if let Some(v) = curve_is_total_clearing { device.curve_is_total_clearing = *v; }
            if let Some(v) = library { device.library = Some(v.clone()); }
            if let Some(v) = interrupting { device.interrupting = Some(v.clone()); }
            if let Some(v) = current_limiting { device.current_limiting = Some(v.clone()); }
            for field in clear {
                match field.as_str() {
                    "protected_branch" => device.protected_branch = None,
                    "breaker_interrupting_s" => device.breaker_interrupting_s = None,
                    "library" => device.library = None,
                    "interrupting" => device.interrupting = None,
                    "current_limiting" => device.current_limiting = None,
                    _ => {}
                }
            }
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
        Command::SetBus { id, name, kv, shunt_kvar, bracing_ka, main_rating_a, bracing_test_xr, equipment_class } => {
            let bus = project.buses.iter_mut().find(|b| b.id == *id).ok_or_else(|| format!("no bus '{id}'"))?;
            if let Some(name) = name { bus.name = name.clone(); }
            if let Some(kv) = kv { bus.kv = *kv; }
            if let Some(q) = shunt_kvar { bus.shunt_kvar = *q; }
            if let Some(ka) = bracing_ka { bus.bracing_ka = Some(*ka); }
            if let Some(amps) = main_rating_a { bus.main_rating_a = Some(*amps); }
            if let Some(xr) = bracing_test_xr { bus.bracing_test_xr = Some(*xr); }
            if let Some(class) = equipment_class { bus.equipment_class = Some(*class); }
        }
        Command::SetSource { id, mva_sc, decrement_curve, xr, v_pu, x0_over_x1, r0_over_r1, is_slack, in_service, machine_kind, transient_mva_sc, generator } => {
            let source = project.sources.iter_mut().find(|s| s.id == *id).ok_or_else(|| format!("no source '{id}'"))?;
            if let Some(v) = mva_sc { source.mva_sc = *v; }
            if let Some(v) = decrement_curve { source.decrement_curve = v.clone(); }
            if let Some(v) = xr { source.xr = *v; }
            if let Some(v) = v_pu { source.v_pu = *v; }
            if let Some(v) = x0_over_x1 { source.x0_over_x1 = *v; source.defaulted.retain(|f| f != "x0_over_x1"); }
            if let Some(v) = r0_over_r1 { source.r0_over_r1 = *v; source.defaulted.retain(|f| f != "r0_over_r1"); }
            if let Some(v) = is_slack { source.is_slack = *v; }
            if let Some(v) = in_service { source.in_service = *v; }
            if let Some(v) = machine_kind { source.machine_kind = Some(*v); }
            if let Some(v) = transient_mva_sc { source.transient_mva_sc = Some(*v); }
            if let Some(v) = generator { source.generator = Some(v.clone()); }
        }
        Command::Run { studies } => {
            // A successful run does not change the model, so sld/tcc/package views
            // stay valid. A failed run leaves no results and no derived views.
            match Studies::parse(studies).and_then(|which| study::run(project, which).map(|r| (which, r))) {
                Ok((which, results)) => {
                    out.results = Some(results);
                    out.full = which.loadflow && which.fault && which.arcflash && which.coordination;
                }
                Err(err) => {
                    let topology = out.topology.take();
                    *out = Outputs { topology, ..Outputs::default() };
                    return Err(err);
                }
            }
        }
        Command::Topology => {
            out.topology = Some(connectivity::topology(project)?);
        }
        Command::Sld => {
            let study = ensure_study(project, out)?;
            let svg = sld::to_svg(&sld::diagram(project, study));
            out.sld_svg = Some(svg);
        }
        Command::Tcc { ref_kv, devices } => {
            let study = ensure_study(project, out)?;
            let drawn = tcc::plot(project, Some(study), *ref_kv, devices)?;
            out.tcc_svg = Some(tcc::to_svg(&drawn));
            out.tcc_csv = Some(tcc::to_csv(&drawn));
        }
        Command::ExportPackage { output, overwrite } => {
            let errors = project.validate();
            if !errors.is_empty() {
                return Err(format!("project is invalid; nothing exported: {}", errors.join("; ")));
            }
            let exported = crate::package::export(project);
            if let Some(path) = output {
                let options = crate::package::WriteOptions { overwrite: *overwrite, protect: None };
                crate::package::write_dir(project, std::path::Path::new(path), options)?;
            }
            out.package = Some(exported);
        }
    }
    Ok(())
}

/// Reuse the last full study. A partial `run` (or none) triggers one full run.
/// A full run in which a study failed (for example, load flow without a slack
/// source) is kept, so repeated sld/tcc commands do not recompute everything.
fn ensure_study<'a>(project: &Project, out: &'a mut Outputs) -> Result<&'a StudyOutput, String> {
    if out.results.is_none() || !out.full {
        out.results = None;
        out.full = false;
        out.results = Some(study::run(project, Studies::all())?);
        out.full = true;
    }
    Ok(out.results.as_ref().expect("study was just stored"))
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
        Command::ApplyLibraryDevice { .. } => "apply_library_device",
        Command::AddSwitch { .. } => "add_switch",
        Command::AddSwitchGroup { .. } => "add_switch_group",
        Command::AddOperatingCase { .. } => "add_operating_case",
        Command::AddEquipment { .. } => "add_equipment",
        Command::Remove { .. } => "remove",
        Command::SetBus { .. } => "set_bus",
        Command::SetSource { .. } => "set_source",
        Command::SetDevice { .. } => "set_device",
        Command::Run { .. } => "run",
        Command::Topology => "topology",
        Command::Sld => "sld",
        Command::Tcc { .. } => "tcc",
        Command::ExportPackage { .. } => "export_package",
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
            "export_package": "flashmob export-package project.json -o package-export    # --force to replace existing files",
            "apply_library": "flashmob apply-library [device-library.json] --device-id 1 --id fuse-1 --bus mcc [--project project.json] [-o out.json]. Omit the library path to use the library compiled into the program. FLASHMOB_LIBRARY names a different file."
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
                {"op": "export_package", "output": "package-export"},
                {"op": "apply_library_device", "library_path": "device-library.json", "device_id": 1, "id": "fuse-1", "bus": "mcc"}
            ]
        },
        "exec_request_note": "The project field is optional; omit it or set it to null to start empty. The example commands build on the built-in sample.",
        "exec_response": ["ok", "error", "warnings", "validation_errors", "project", "results", "topology", "sld_svg", "tcc_svg", "tcc_csv", "package"],
        "exec_semantics": "Commands run in order; intermediate states may be incomplete. The final project is validated: any validation error is listed in validation_errors and makes ok false (the CLI exits 1). Model edits clear results and every derived view; run, topology, sld, tcc, and export_package do not. sld and tcc reuse the last full study, including one whose load flow failed, instead of rerunning.",
        "export_package_options": "output: directory, or an .xml path whose siblings are written beside it; other file-like paths (.json, .csv, .txt, .tab, .svg) are rejected. Existing files are refused unless \"overwrite\": true. The project must validate.",
        "topology": "Validated entered connections only: every bus lists branch terminal and neighbor plus attached source, load, motor, device, and arc equipment IDs. Global records give branch endpoints and switch link, source base in-service state, switch base closed state, group rules, named operating cases, and each device's entered protected branch terminal. Null placement means it was not entered. No study or protection inference is performed.",
        "source_decrement": "Optional add_source/set_source decrement_curve is an array of {time_s,current_ratio} for symmetrical three-phase RMS short-circuit current at source terminals relative to initial mva_sc. First point must be {time_s:0,current_ratio:1}; times increase strictly and ratios stay positive. Linear interpolation in time holds the first/last value outside entered points. An empty array clears the curve; omission retains the constant-current model. For arc flash with decrement, Flashmob scales the curved source's contribution from the one fault solution by the entered ratio in time slices of at most 10 ms, split at entered curve points, and sums the IEEE 1584 incident-energy contributions; the boundary comes from accumulated energy. The network is not re-solved. By default the device is timed once at the initial arcing current. With retime_on_reduced_current: true, an isolating upstream breaker with an entered trip-only curve, protected branch terminal, and breaker_interrupting_s advances each relay trip element on its scaled protected-terminal arcing current, separately for full and reduced arcing-current cases; between samples it interpolates current, splitting at each entered pickup threshold. Breaker interruption time starts after relay operation. An entered total clearing_s overrides this calculation and its energy still follows the scaled profile. When automatic clearing cannot be established, that bus is an arc-flash failure naming the missing data. fallback_duration_s is not used as an arc duration. arc_duration_cap_s bounds the study when escape_possible is true (IEEE 1584-2018 Clause 6.9.1, a person can move away) and that capped result is assumed. The cap never supplies a field clearing setting.",
        "point_curves": "A device curve {\"type\":\"points\"} stores [amperes, seconds] pairs in points (the faster / minimum band) and optional max_points (the slower / maximum or total-clearing band). Currents increase strictly and times never increase; max_points is never faster than points where they overlap. Time is interpolated in log(current)-log(time). Below the first point the device does not operate. Current from the fast band's first point up to the slow band's first point is unresolved. The failure names that ampere range. The fast time is not used, and the slow band's first time is not held. beyond_last is required: hold_last_time keeps the last time above the last point and marks results assumed; unavailable gives no time there (clearing is unresolved). Trip, clearing, coordination, and arc-flash times use the max band when present; TCC draws both bands. band_interpretation \"assumed\" (default \"entered\") marks results that rely on an inferred band meaning as assumed. Clearing adds breaker_interrupting_s to the governing band, unless curve_is_total_clearing (or fuse_total_clearing for a fuse) says that band already includes total clearing. A library fuse (library category containing \"fuse\") with a point curve, and with neither total-clearing flag nor breaker_interrupting_s, estimates clearing by IEEE 1584-2018 Clause 6.9.2. Add 10% of the melting time plus 0.004 s. If that total is under 0.01 s, use 0.01 s. The same adjustment applies to one average-melt band and to the slower band of a tolerance pair. A two-band library fuse stays a melt-tolerance pair. fuse_total_clearing stays false unless the caller sets it. A published total-clearing curve is not adjusted. Current above a last point of 0.01 s uses 0.01 s and does not reduce the arc current unless manufacturer let-through is entered on current_limiting. current_limiting.cycle_table uses a 60 Hz half-cycle from the threshold up to twice that current and a quarter-cycle at or above twice the threshold. A stored total-clearing time already faster than one half-cycle and drawn below 0.01 s is kept. A melting time is not kept. Annex H incident-energy equations are not applied. Energy stays on the IEEE 1584-2018 model at that duration. An entered manufacturer clearing time is used when cycle_table is omitted or false. The estimate is assumed and is not an arc-flash label. A point curve without that fuse category still needs breaker_interrupting_s or a total-clearing flag. Current below the first point does not operate.",
        "total_clearing_flags": "Device clearing is the curve time plus breaker_interrupting_s, unless the curve already includes total clearing. curve_is_total_clearing: true says so for any device (for example a breaker whose point-curve max band is total clearing). fuse_total_clearing: true is kept unchanged for fuses: it implies curve_is_total_clearing and also marks the device as a fuse (SLD fuse symbol, package device type Fuse). Existing projects are not migrated; both flags are read as entered. Either flag together with breaker_interrupting_s is a validation error. add_device and set_device accept both. A library fuse melting curve can also estimate clearing by the IEEE 1584-2018 Clause 6.9.2 adjustment described under point_curves. That adjustment does not set fuse_total_clearing.",
        "apply_library_device": "Copies a library point curve onto a new project device. The curve is a snapshot (studies do not read the library file). Multiples are scaled to amperes. Two bands are ordered faster then slower and marked assumed. fuse_total_clearing stays false unless set: a slower library band is often a current-tolerance side, not total clearing. For a fuse category, arc flash may still estimate clearing from that melting curve by IEEE 1584-2018 Clause 6.9.2 (10% of the melting time plus 0.004 s, and 0.01 s when that total is shorter). The estimate is assumed and is not the published total-clearing curve. A time_dial_curve_family is scaled by dial, tap_a (secondary amperes), and ct_ratio to one primary-ampere band. Trip settings named by function and label (lt_pickup, lt_delay, optional st_pickup, st_delay, and inst) build a thermal-magnetic curve from sensor_a; lt_delay_multiple says the current multiple of the long-time delay. library_path is the library JSON; an empty library_name stores that path. One agreeing symmetrical interrupting rating is copied onto the device; several different duties are left unset and named. An empty library_path uses the device library built into the program. A missing file or an invalid library does not change the project.",
        "device_library_link": "Optional device.library records which device-library entry a device came from: device_id (required), library, manufacturer, type, style, category, size, rating_a, voltage_v, modified, unsupported_elements. Studies use the curve and the device fields. The category is also read to recognize a library fuse for the IEEE 1584-2018 Clause 6.9.2 melting-time adjustment. add_device accepts it; set_device changes it, and set_device clear [\"library\"] removes it.",
        "voltage_factor": "Each fault current also reports c_min, c_max, symmetrical_min_ka, and symmetrical_max_ka from IEC 60909-0 Table 1. The symmetrical current is multiplied by c / |prefault pu|. At or below 1 kV, cmin is 0.95 and cmax is 1.05 unless low_voltage_tolerance_plus_10 is true, which selects cmax 1.10. Above 1 kV, cmin is 1.00 and cmax is 1.10. The maximum symmetrical current uses a network with IEC 60909-0 impedance correction factors: K_T on network transformers (clause 6.3.3), K_G on generators with an entered generator rating (clause 6.6.1), and K_S or K_SO on a generator and its unit transformer when generator.unit is entered (clause 6.7). Each is evaluated in closed form from entered data, and the corrected impedance is the entered impedance times the factor. The minimum current is uncorrected. A rotating-machine source without a generator rating is not corrected and is reported. iec_max_corrected marks a corrected maximum, and fault.iec_corrections lists each factor. add_source and set_source accept generator {rated_mva, rated_pf, optional rated_kv (omitted uses the bus kV and is reported), optional unit {transformer, on_load_tap_changer, optional p_g, optional p_t (omitted is 0 and reported; the K_SO tap term is 1 + p_t)}}. IEC peak current is kappa times sqrt(2) times the maximum initial symmetrical current (cmax and, where applied, the impedance correction factors), with kappa from the faulted-bus R/X (IEC 60909-0 clause 8.1). C37 half-cycle and duty values stay on the uncorrected symmetrical current. A bus whose closed, energized branch graph contains a loop is meshed and uses method B, kappa times 1.15 limited to 1.8 at or below 1 kV and 2.0 above, unless every branch in that loop group has R/X below 0.3. A radial bus uses kappa without 1.15; several radial infeeds are combined through the Thevenin R/X, not a sum of partial peak currents. Each fault point reports peak_basis. The C37 half-cycle and duty values stay on the unscaled symmetrical current.",
        "interrupting_duty": "After a fault study is solved, including when that fault result is kept only for arc flash and then omitted from the output, each braced bus and each device with an interrupting rating is checked. The half-cycle factor is sqrt(1 + 2*exp(-2*pi/(X/R))); no resistance (infinite X/R) uses sqrt(3), and X/R of 0 uses 1. A bus with bracing_ka uses the three-phase or line-to-ground fault with the greater half-cycle rms. A device with protected_branch uses the bolted three-phase current through that branch at each terminal and keeps the larger one. Line-to-ground current through the branch is not stored, so it is not compared. Without protected_branch the greater of the device bus three-phase and line-to-ground faults is used. System X/R is the faulted bus X/R from separate networks: the Thevenin reactance of a reactance-only network over the Thevenin resistance of a resistance-only network, built for that duty with its machine multipliers and without shunt capacitance. A part that is zero in an element is floored at 1e-6 of its impedance magnitude, and a Thevenin part under 1e-4 of the other is treated as zero (R gives infinite X/R, X gives 0). Line-to-ground X/R uses the sums of the three sequence parts. Symmetrical current stays from the complex network, and the half-cycle rms uses the separate-network X/R. The fault study output keeps the complex-network X/R. With a test X/R, adjusted symmetrical kA is rated symmetrical kA times factor(test X/R) / factor(system X/R). Half-cycle rms at or below 90% of the rated half-cycle rms is within_rating, above 90% and at or below that capability is marginal, and above it is exceeds_rating. Bracing without a test X/R is not an asymmetrical pass: symmetrical current above the bracing is exceeds_rating, above 90% and at or below the bracing is marginal, otherwise not_adjusted. exceeds_rating, marginal, and not_adjusted each add one warning; within_rating does not. If interrupting voltage_v is positive and the device bus nominal kV differs by more than 10 percent, the row notes the mismatch and the current comparison still stands. An invalid fault result produces no rows. set_bus accepts bracing_test_xr. set_device accepts interrupting (symmetrical_ka, test_xr, voltage_v, optional contact_parting_cycles) and can clear interrupting. Above 1 kV, contact_parting_cycles reports a remote multiplier: asymmetrical rms at that parting time, sqrt(1 + 2*exp(-4*pi*cycles/(X/R))), divided by the breaker asymmetry capability (1.4, 1.2, 1.1, and 1.0 at 1, 2, 3, and 4 cycles, linear between those times, and 1 after 4 cycles), and not below 1. The half-cycle status is unchanged. The remote check adds its own warning. NACD is the three-phase current from sources with no decrement curve divided by the total fault current. Motors and sources with a decrement curve are local. When NACD is below 1 and a local source has an entered decrement curve, the reported multiplier blends the remote factor with the local factor. The local symmetrical-current ratio is taken from that curve at the contact parting time converted to seconds at 60 Hz, combined with the DC term, divided by the breaker asymmetry capability, and not taken below 1. Motor contribution without a decrement curve is not reduced. NACD of 1, a missing NACD, or local contribution that is only motors keeps the remote factor. At or below 1 kV the parting time is ignored. Above 1 kV, closing and latching duty is 1.6 times the symmetrical current already used for the device. Capability is 1.6 times the voltage range factor K times the rated symmetrical current. Omitted K is 1. interrupting.voltage_range_factor stores K, and it must be at least 1 when set. The entered symmetrical rating is the rated short-circuit current. It does not replace the half-cycle comparison. At or below 1 kV closing and latching is not applied.",
        "machine_duty_reactance": "IEEE C37.010-1979 5.4.1 per-duty machine reactance. Duty rows solve a first-cycle network (half-cycle comparison, bus bracing, closing and latching) and an interrupting network (remote or blended interrupting multiplier). Each scales a classified machine's whole impedance, so its X/R is kept. Each duty network is then reduced separately for resistance and reactance to give the duty X/R. add_motor accepts machine_kind (induction or synchronous), synchronous_rpm, and phases (1 or 3). Synchronous motor: 1.0 first-cycle, 1.5 interrupting. Three-phase induction motor above 1000 hp, or above 250 hp at a synchronous speed above 1800 r/min (3600 r/min at 60 Hz): 1.0 and 1.5. Induction motor from 50 hp to those limits: 1.2 and 3.0. Three-phase induction motors below 50 hp and every single-phase motor are left out of both duty networks. An induction motor above 250 hp and up to 1000 hp with no synchronous_rpm takes 1.0 and 1.5 and is reported. A motor with no machine_kind keeps x_subtransient_pu for both duties and is reported. Omitted phases means three-phase. add_source and set_source accept machine_kind (utility, turbogenerator, hydro_with_amortisseur, hydro_without_amortisseur, synchronous_condenser) and transient_mva_sc. All of them keep mva_sc for both duties except hydro_without_amortisseur, which uses 0.75 times the transient reactance behind transient_mva_sc; without transient_mva_sc it keeps mva_sc and is reported. Load flow, the main fault study, and arc flash keep the entered reactances. When no machine is scaled or omitted, duty rows use the main fault study.",
        "arc_flash_label": "Each arc-flash row reports clearing_basis and clearing_basis_reduced (manual_duration, assumed_exposure_duration, total_clearing_curve, trip_curve_plus_interrupting_time, fuse_melt_estimate, current_limiting_manufacturer_time, current_limiting_fixed_table, current_limiting_stored_curve, staged_multi_device, staged_decaying_current), label_ready, and label_reasons (duration_capped, assumed_exposure_duration, fuse_melt_estimate, current_limiting_fixed_table, assumed_curve_band, point_curve_tail_held, relay_reset_assumed, staged_clearing_assumed, staged_decaying_current, static_device_timing, motor_decay_assumed, remaining_current_excluded, machine_step_down_assumed, assumed_equipment, bolted_out_of_range, model_range_exceeded, assumed_clearing). label_ready is true only when no reason applies to either arcing-current case; it is a data-basis screen, not an engineering review. Arc-flash failures report clearing_basis unresolved, label_ready false, and label_reasons [unresolved]. Times and energies are unchanged by this classification.",
        "motor_backfeed": "The arc lasts until the devices that have operated have cleared cleared_fault_fraction (default 0.80) of the total bus fault current, or until every contribution has ended, or until the study cap when escape is possible. Everything comes from one prefault and machine-stage fault solutions (base; induction motors removed after the motor window; optional generator and synchronous-motor step-down): each contribution into the faulted bus is cleared by its own fastest located device. When a device opens, the contributions it carries are removed; a device opening does not re-solve the network. Incident energy accumulates in trip order. When the threshold is crossed with current still flowing, the arc is taken as extinguished, the remaining contribution is excluded, the row reports remaining_current_excluded, and governing_device names the device that crossed the threshold (otherwise the last device to open). One device that isolates every in-service source and leaves no motor keeps that device's time. Machine stages: an induction motor (or a motor with no machine_kind) is removed after motor_contribution_s (default five 60 Hz cycles) by a separate fault solution, including motors behind a feeder that has not opened; optional decay_time_constant_s scales it by exp(-t / tau) inside that window (motor_decay_assumed). Optional machine_step_down {percent_of_rated, after_cycles, apply_to_generators, apply_to_synchronous_motors, synchronous_motor_percent_of_rated} steps generators and synchronous motors to an entered percent of rated after after_cycles (must exceed the induction window) using another fault solution (machine_step_down_assumed); a machine with zero rating or 0% drops out at the step; omit it, or send disable_machine_step_down, to keep the initial contribution. A source with an entered decrement curve follows that curve and is not stepped down. By default each device is timed once at its initial arcing current (staged_multi_device, staged_clearing_assumed). With retime_on_reduced_current: true, non-fuse devices are re-timed once at the motor window and once at the step-down on that stage's currents, and continuously under a source decrement curve (staged_decaying_current). A fuse, total-clearing curve, or current-limiting time is read once (static_device_timing). Optional upstream_backup {levels 1-5, ratio} lets a faster upstream device time a branch when it carries at least ratio of the near device's current; fuses and current-limiting devices are excluded from that search. A utility or generator path with no isolating device fails. A de-energized bus with zero fault current is reported out of service; an energized bus with zero fault current is reported as such; no voltage is substituted. Fault method: comprehensive (default, entered reactances) or ansi (first-cycle C37.010 duty network; step-down off). Prefault: flat (prefault_pu), per_bus, no_load_tap, or loadflow. If loadflow is requested and the load flow errors (missing or not converged), the study run switches to no_load_tap and warns; the project file is not changed. Load-flow warnings alone do not switch. A flat 1.0 pu is used only when flat is chosen. An explicit equipment clearing_s overrides staging only when it represents total exposure from all paths. Do not invent feeder length or cable size. set_options accepts fault_method, cleared_fault_fraction, machine_step_down, disable_machine_step_down, retime_on_reduced_current, upstream_backup, disable_upstream_backup, prefault, and prefault_pu; add_bus accepts prefault_pu.",
        "operating_cases": "Base switch.closed and source.in_service values define normal operation. Each named operating case overrides listed switch/source states. Its arc equipment starts with no inherited upstream_device, clearing_s, or fallback_duration_s; list case-specific values in equipment_states, or leave them absent for automatic protection selection or an explicit failure. At most one switch can control a branch. A switch group limits simultaneous closures using max_closed. `run` evaluates the base configuration and each named valid case; inspect per-case failures before using an arc-flash maximum.",
        "switch_model": "Opening a switch removes its entire referenced branch, including any transformer grounding shunt or line charging on that branch. For an independent breaker or ATS contact, use a separate short finite-impedance branch with separate bus nodes; put transformers and other equipment on their own branches. The closed field is required for every switch, especially normally open ties and ATS contacts.",
        "switch_kinds": ["breaker", "tie", "ats"],
        "package": "Engineering data package for mapping; third-party import unverified: project.xml, per-component CSV (including switch normal state, source service status, and interrupting symmetrical kA, test X/R, and voltage on protective devices), interrupting-duty.csv from a fault study of the exported project (header only when that fault produces no rows), components.tab, and Revit panel/circuit schedules. Operating cases remain only in project.json. See IMPORT.txt.",
        "sld": "SVG single-line diagram. Each bus card shows nominal voltage, load-flow pu voltage, symmetrical 3P and LG fault current, and the governing IEEE 1584-2018 incident energy and arc-flash boundary; with operating cases, also the worst-case incident energy and its governing case. Normally open branches are dashed with an open switch symbol; out-of-service sources are grey.",
        "tcc": "SVG log-log time-current curve plus CSV points. Current is referred to ref_kv. Fault and arcing-current markers are included when a study has been run; arcing markers from assumed inputs are labeled assumed.",
        "electrodes": ["VCB", "VCBB", "HCB", "VOA", "HOA"],
        "transformer_connections": ["dyn", "ynd", "ynyn", "dd"],
        "sample_project": serde_json::to_value(Project::sample()).unwrap(),
    })
}

pub fn load_project(text: &str) -> Result<Project, String> {
    serde_json::from_str(text).map_err(|err| format!("could not read the project: {err}"))
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
        assert!(arcs.iter().any(|r| r.bus_id == "pnl" && r.standard == "IEEE 1584-2018"));
        assert!(arcs.iter().any(|row| row.bus_id == "mcc" && row.assumed && row.duration_note.contains("m1")));
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

    #[test]
    fn final_project_is_validated_and_invalid_models_are_not_ok() {
        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"set_bus","id":"pnl","kv":-1}]}"#);
        assert!(!response.ok);
        assert!(response.validation_errors.iter().any(|e| e.contains("pnl")), "{:?}", response.validation_errors);
        assert!(response.error.unwrap().contains("project is invalid"));

        // An incomplete intermediate state is fine when the final model validates.
        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"set_bus","id":"pnl","kv":-1},{"op":"set_bus","id":"pnl","kv":0.48}]}"#);
        assert!(response.ok, "{:?}", response.error);
        assert!(response.validation_errors.is_empty());
        let json = serde_json::to_value(&response).unwrap();
        assert!(json["validation_errors"].as_array().unwrap().is_empty());
    }

    #[test]
    fn export_package_validates_and_refuses_to_overwrite_without_permission() {
        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"set_bus","id":"pnl","kv":0},{"op":"export_package"}]}"#);
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("nothing exported"));
        assert!(response.package.is_none());

        let dir = std::env::temp_dir().join(format!("flashmob-exec-package-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let request = |overwrite: bool| serde_json::json!({"commands":[{"op":"sample"},{"op":"export_package","output":dir.display().to_string(),"overwrite":overwrite}]}).to_string();
        let first = exec_request(&request(false));
        assert!(first.ok, "{:?}", first.error);
        assert!(dir.join("buses.csv").is_file());
        let second = exec_request(&request(false));
        assert!(!second.ok);
        assert!(second.error.unwrap().contains("refusing to overwrite"));
        let third = exec_request(&request(true));
        assert!(third.ok, "{:?}", third.error);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_keeps_views_and_only_edits_invalidate() {
        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"sld"},{"op":"tcc"},{"op":"export_package"},{"op":"run","studies":["fault"]}]}"#);
        assert!(response.ok, "{:?}", response.error);
        assert!(response.sld_svg.is_some() && response.tcc_svg.is_some() && response.package.is_some());
        let response = exec_request(r#"{"commands":[{"op":"sample"},{"op":"sld"},{"op":"set_options","name":"Edited"}]}"#);
        assert!(response.ok);
        assert!(response.sld_svg.is_none() && response.results.is_none());
    }

    #[test]
    fn views_reuse_a_full_study_even_when_load_flow_failed() {
        let project = Project::sample();
        let mut out = Outputs::default();
        let mut cached = study::run(&project, Studies::all()).unwrap();
        cached.loadflow = None; // as when load flow could not be solved
        cached.case_id = "cached".into();
        out.results = Some(cached);
        out.full = true;
        assert_eq!(ensure_study(&project, &mut out).unwrap().case_id, "cached");
        assert_eq!(ensure_study(&project, &mut out).unwrap().case_id, "cached");
        // A partial run is replaced by one full run.
        out.full = false;
        assert_eq!(ensure_study(&project, &mut out).unwrap().case_id, "normal");
        assert!(out.full);
    }
}
