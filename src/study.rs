use serde::{Deserialize, Serialize};

use crate::arcflash::{self, EnergySegment, IeeeInput};
use crate::curves::{self, curve_label};
use crate::duty;
use crate::fault::{self, FaultResult};
use crate::loadflow::{self, LoadflowResult};
use crate::model::{FaultMethod, classes_for_assumed_bus, typical_equipment, ArcEquipment, CurveSpec, Device, EquipmentSpec, Prefault, Project};

#[derive(Clone, Copy, Debug)]
pub struct Studies {
    pub loadflow: bool,
    pub fault: bool,
    pub arcflash: bool,
    pub coordination: bool,
}

impl Studies {
    pub fn all() -> Self {
        Self { loadflow: true, fault: true, arcflash: true, coordination: true }
    }

    pub fn parse(names: &[String]) -> Result<Self, String> {
        if names.is_empty() {
            return Ok(Self::all());
        }
        let mut out = Self { loadflow: false, fault: false, arcflash: false, coordination: false };
        for name in names {
            match name.as_str() {
                "loadflow" | "load_flow" => out.loadflow = true,
                "fault" | "short_circuit" | "shortcircuit" => out.fault = true,
                "arcflash" | "arc_flash" => out.arcflash = true,
                "coordination" | "tcc" => out.coordination = true,
                "all" => return Ok(Self::all()),
                other => return Err(format!("unknown study '{other}'")),
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudyOutput {
    /// `normal` is the entered default state; named cases override it.
    #[serde(default)]
    pub case_id: String,
    pub methods: Methods,
    pub elapsed_ms: f64,
    pub warnings: Vec<String>,
    pub loadflow: Option<LoadflowResult>,
    pub fault: Option<FaultResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interrupting: Vec<InterruptingDuty>,
    pub arc_flash: Option<Vec<ArcRow>>,
    pub coordination: Option<Vec<CoordRow>>,
    pub arc_flash_failures: Vec<ArcFailure>,
    #[serde(default)]
    pub protection: Vec<crate::protection::Protection>,
    /// Results for each explicitly entered alternative operating case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operating_cases: Vec<OperatingCaseResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worst_case: Option<ScenarioEnvelope>,
    /// Buses electrically connected to an in-service source in this case.
    #[serde(default)]
    pub energized_bus_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperatingCaseResult {
    pub id: String,
    pub name: String,
    pub switch_states: Vec<crate::model::SwitchState>,
    pub source_states: Vec<crate::model::SourceState>,
    pub equipment_states: Vec<crate::model::EquipmentState>,
    pub result: Option<Box<StudyOutput>>,
    pub error: Option<String>,
    pub energized_bus_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaseMaximum {
    pub case_id: String,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusCaseEnvelope {
    pub bus_id: String,
    pub three_phase_ka: Option<CaseMaximum>,
    pub three_phase_iec_peak_ka: Option<CaseMaximum>,
    pub three_phase_half_cycle_rms_ka: Option<CaseMaximum>,
    pub line_to_ground_ka: Option<CaseMaximum>,
    pub line_to_ground_iec_peak_ka: Option<CaseMaximum>,
    pub line_to_ground_half_cycle_rms_ka: Option<CaseMaximum>,
    pub line_to_line_ka: Option<CaseMaximum>,
    pub line_to_line_iec_peak_ka: Option<CaseMaximum>,
    pub line_to_line_half_cycle_rms_ka: Option<CaseMaximum>,
    pub line_to_line_ground_ka: Option<CaseMaximum>,
    pub line_to_line_ground_iec_peak_ka: Option<CaseMaximum>,
    pub line_to_line_ground_half_cycle_rms_ka: Option<CaseMaximum>,
    /// False if a source-connected case lacked a valid fault result.
    pub complete: bool,
    pub failed_case_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArcCaseEnvelope {
    pub equipment_id: String,
    pub bus_id: String,
    pub incident_energy_cal_cm2: Option<CaseMaximum>,
    pub boundary_mm: Option<CaseMaximum>,
    /// False if a source-connected case had no valid result for this equipment.
    pub complete: bool,
    pub failed_case_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioEnvelope {
    pub buses: Vec<BusCaseEnvelope>,
    pub arc_equipment: Vec<ArcCaseEnvelope>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArcFailure {
    pub equipment_id: String,
    pub name: String,
    pub bus_id: String,
    pub error: String,
    /// Always "unresolved": no arc duration was established.
    #[serde(default = "unresolved_basis")]
    pub clearing_basis: String,
    /// Always false for a failure.
    #[serde(default)]
    pub label_ready: bool,
    #[serde(default)]
    pub label_reasons: Vec<String>,
}

fn unresolved_basis() -> String { "unresolved".into() }

impl ArcFailure {
    fn new(case: &ArcEquipment, error: String) -> Self {
        Self { equipment_id: case.id.clone(), name: case.name.clone(), bus_id: case.bus.clone(), error, clearing_basis: unresolved_basis(), label_ready: false, label_reasons: vec!["unresolved".into()] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Methods {
    pub load_flow: String,
    pub short_circuit: String,
    pub arc_flash: String,
    pub coordination: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArcRow {
    pub equipment_id: String,
    pub name: String,
    pub bus_id: String,
    pub bus_name: String,
    pub assumed: bool,
    /// The enclosure gap, working distance, or electrode was not collected.
    #[serde(default)]
    pub enclosure_assumed: bool,
    pub electrode: String,
    /// Bolted and arcing currents at fault inception (t = 0). Integrated
    /// energy below uses the full entered decrement profile when
    /// decrement_applied is true; such rows say so in `warnings`.
    pub bolted_ka: f64,
    pub arcing_ka: f64,
    pub arcing_min_ka: f64,
    pub time_s: f64,
    pub time_min_s: f64,
    pub energy_cal_cm2: f64,
    pub energy_min_cal_cm2: f64,
    pub governing_cal_cm2: f64,
    pub afb_full_mm: f64,
    pub afb_reduced_mm: f64,
    pub afb_in: f64,
    pub afb_mm: f64,
    pub governing: String,
    pub duration_capped: bool,
    /// States whether the printed seconds are the upstream device time or the 2.0 s cap.
    pub duration_note: String,
    pub upstream_device: Option<String>,
    pub standard: String,
    pub warnings: Vec<String>,
    /// True when supplied source-current decrement points were used to
    /// integrate the incident energy and boundary over the arc duration.
    #[serde(default)]
    pub decrement_applied: bool,
    /// Device whose operation ends the governing case's arc: the selected
    /// upstream device, or in a staged arc the device that brought the cleared
    /// share to the threshold (else the last device to open). None for an
    /// entered duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governing_device: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decrement_source_ids: Vec<String>,
    /// With decrement only: bolted current in the last integration slice of
    /// the full-arcing-current duration (slice midpoint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_bolted_ka: Option<f64>,
    /// With decrement only: arcing current in the last slice of the full case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_arcing_ka: Option<f64>,
    /// With decrement only: reduced arcing current in the last slice of the
    /// reduced-current duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_arcing_min_ka: Option<f64>,
    /// How the full-arcing-current duration was obtained: manual_duration,
    /// assumed_exposure_duration, total_clearing_curve,
    /// trip_curve_plus_interrupting_time, fuse_melt_estimate,
    /// current_limiting_manufacturer_time, current_limiting_fixed_table,
    /// current_limiting_stored_curve, or staged_multi_device.
    #[serde(default)]
    pub clearing_basis: String,
    /// The same for the reduced-arcing-current duration.
    #[serde(default)]
    pub clearing_basis_reduced: String,
    /// True when no reason below applies to either arcing-current case or to
    /// the equipment inputs. This is a data-basis screen, not an engineering
    /// review or approval of a label.
    #[serde(default)]
    pub label_ready: bool,
    /// Reason codes that keep this result off a label. See `LABEL_REASONS`.
    #[serde(default)]
    pub label_reasons: Vec<String>,
}

/// Every label-readiness reason code and what it means.
pub const LABEL_REASONS: &[(&str, &str)] = &[
    ("duration_capped", "arc duration is the study cap that applies only when a person can move away"),
    ("assumed_exposure_duration", "the entered duration is marked assumed"),
    ("fuse_melt_estimate", "a melting-curve fuse uses the assumed IEEE 1584-2018 Clause 6.9.2 clearing estimate"),
    ("current_limiting_fixed_table", "the fixed 60 Hz half-cycle or quarter-cycle let-through time is used"),
    ("assumed_curve_band", "the point curve's band interpretation is assumed"),
    ("point_curve_tail_held", "the current is beyond the point curve's last point and its last time is held"),
    ("relay_reset_assumed", "relay reset was assumed immediate and changed the result"),
    ("staged_clearing_assumed", "more than one clearing event; each device is timed at the initial arcing current"),
    ("staged_decaying_current", "more than one clearing event; relays are re-timed on their reduced contributions from the one fault solution"),
    ("static_device_timing", "with retiming on, a fuse, total-clearing curve, or current-limiting time is still read once at the initial arcing current"),
    ("remaining_current_excluded", "the arc was taken as extinguished at the cleared-fault threshold; the remaining contribution is excluded"),
    ("machine_step_down_assumed", "generator or synchronous-motor contribution steps down to an entered percent of rated current"),
    ("motor_decay_assumed", "a motor contribution decays with an entered, assumed time constant"),
    ("assumed_equipment", "equipment class, gap, working distance, or electrode was not collected"),
    ("bolted_out_of_range", "bolted current is outside the IEEE 1584-2018 model range"),
    ("model_range_exceeded", "an IEEE 1584-2018 model input is outside its range"),
    ("assumed_clearing", "the clearing result is marked assumed for another reason given in the duration note"),
    ("lee_method", "the bus is above 15 kV, so the conservative Lee (1982) theoretical model with arc current equal to bolted current is used instead of IEEE 1584-2018"),
    ("unresolved", "no arc duration was established (arc-flash failure rows)"),
];

fn label_reasons(case_assumed: bool, full: &Clearing, reduced: &Clearing, bolted_out_of_range: bool, model_range_exceeded: bool, assumed: bool) -> Vec<String> {
    let mut codes: Vec<&'static str> = Vec::new();
    for code in full.reasons.iter().chain(&reduced.reasons) {
        if !codes.contains(code) { codes.push(code); }
    }
    if case_assumed { codes.push("assumed_equipment"); }
    if bolted_out_of_range { codes.push("bolted_out_of_range"); }
    if model_range_exceeded { codes.push("model_range_exceeded"); }
    if assumed && codes.is_empty() { codes.push("assumed_clearing"); }
    codes.into_iter().map(String::from).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoordRow {
    pub device_id: String,
    pub name: String,
    pub bus_id: String,
    pub bus_kv: f64,
    pub curve: String,
    pub bolted_ka: Option<f64>,
    pub trip_at_bolted_s: Option<f64>,
    pub total_clearing_s: Option<f64>,
    /// Point curves with two bands: operating time on the faster (min) band.
    /// `trip_at_bolted_s` and `total_clearing_s` use the slower (max) band.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_band_trip_s: Option<f64>,
    /// Bus faulted to obtain `bolted_ka`: the protected-side terminal of the
    /// device's protected branch.
    #[serde(default)]
    pub fault_bus_id: String,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterruptingDuty {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub fault: String,
    pub bus_id: String,
    pub symmetrical_ka: f64,
    pub system_xr: Option<f64>,
    pub half_cycle_rms_ka: f64,
    pub rated_symmetrical_ka: f64,
    pub test_xr: Option<f64>,
    pub rated_half_cycle_rms_ka: Option<f64>,
    pub adjusted_symmetrical_ka: Option<f64>,
    pub status: String,
    pub note: String,
    /// Entered contact parting time, in cycles, when the remote multiplier was applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_parting_cycles: Option<f64>,
    /// Applied interrupting multiplier. This is the remote DC-decrement factor
    /// unless `local_ac_applied` says an entered generator decrement was blended in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_multiplying_factor: Option<f64>,
    /// Symmetrical current times the applied multiplier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplied_symmetrical_ka: Option<f64>,
    /// Three-phase no-AC-decrement ratio at the fault bus used for this row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nacd: Option<f64>,
    /// within_rating, marginal, or exceeds_rating for the multiplied current.
    /// The half-cycle `status` is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_status: Option<String>,
    /// True when the stored multiplier blends an entered generator decrement by NACD.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_ac_applied: bool,
    /// 1.6 times the symmetrical current. Set above 1 kV.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_latch_ka: Option<f64>,
    /// 1.6 times K times the rated symmetrical current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_latch_capability_ka: Option<f64>,
    /// Voltage range factor K used for the closing and latching capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_range_factor: Option<f64>,
    /// within_rating, marginal, or exceeds_rating for closing and latching.
    /// The half-cycle `status` is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_latch_status: Option<String>,
}

pub fn run(project: &Project, studies: Studies) -> Result<StudyOutput, String> {
    let started = std::time::Instant::now();
    let mut normal = run_one(project, studies)?;
    for case in &project.operating_cases {
        let resolved = project.apply_operating_case(case)?;
        let (result, error, energized_bus_ids) = match run_one(&resolved, studies) {
            Ok(mut result) => {
                result.case_id = case.id.clone();
                let energized = result.energized_bus_ids.clone();
                (Some(Box::new(result)), None, energized)
            }
            Err(error) => {
                // Keep failed cases visible. A failed calculation cannot be
                // silently omitted from a claimed worst-case envelope.
                let energized = crate::network::build_validated(&resolved)
                    .map(|model| model.energized.iter().enumerate().filter(|(_, on)| **on)
                        .map(|(i, _)| model.index.id_of[i].clone()).collect())
                    .unwrap_or_else(|_| resolved.buses.iter().map(|b| b.id.clone()).collect());
                (None, Some(error), energized)
            }
        };
        normal.operating_cases.push(OperatingCaseResult {
            id: case.id.clone(),
            name: case.name.clone(),
            switch_states: resolved.switches.iter().map(|switch| crate::model::SwitchState {
                switch_id: switch.id.clone(), closed: switch.closed,
            }).collect(),
            source_states: resolved.sources.iter().map(|source| crate::model::SourceState {
                source_id: source.id.clone(), in_service: source.in_service,
            }).collect(),
            equipment_states: resolved.equipment.iter().map(|equipment| crate::model::EquipmentState {
                equipment_id: equipment.id.clone(),
                upstream_device: equipment.upstream_device.clone(),
                clearing_s: equipment.clearing_s,
                fallback_duration_s: equipment.fallback_duration_s,
            }).collect(),
            result,
            error,
            energized_bus_ids,
        });
    }
    if !project.operating_cases.is_empty() {
        normal.worst_case = Some(scenario_envelope(project, &normal, studies));
        normal.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    }
    Ok(normal)
}

/// Project for fault and arc flash under the selected fault method.
/// Comprehensive leaves the project unchanged. ANSI clears the generator /
/// synchronous-motor step-down; duty networks are chosen per bus voltage.
fn fault_method_project(project: &Project) -> Project {
    match project.fault_method {
        FaultMethod::Comprehensive => project.clone(),
        FaultMethod::Ansi => {
            let mut p = project.clone();
            p.machine_step_down = None;
            p
        }
    }
}

/// Bolted fault study under the selected fault method. ANSI solves the
/// first-cycle and interrupting C37.010 networks and keeps each bus from the
/// network selected by its nominal voltage (≤ 1 kV first-cycle, above that
/// interrupting).
fn fault_for_method(project: &Project, loadflow: Option<&crate::loadflow::LoadflowResult>) -> Result<crate::fault::FaultResult, String> {
    if project.fault_method != FaultMethod::Ansi {
        let model = crate::network::build_validated(project)?;
        return crate::fault::solve_with_model(project, loadflow, &model);
    }
    let first = crate::machine::duty_project(project, crate::machine::Duty::FirstCycle)
        .unwrap_or_else(|| project.clone());
    let interrupting = crate::machine::duty_project(project, crate::machine::Duty::Interrupting)
        .unwrap_or_else(|| project.clone());
    let first_model = crate::network::build_validated(&first)?;
    let int_model = crate::network::build_validated(&interrupting)?;
    let first_fault = crate::fault::solve_with_model(&first, loadflow, &first_model)?;
    let int_fault = crate::fault::solve_with_model(&interrupting, loadflow, &int_model)?;
    Ok(merge_ansi_bus_faults(project, first_fault, int_fault))
}

fn merge_ansi_bus_faults(
    project: &Project,
    first_cycle: crate::fault::FaultResult,
    interrupting: crate::fault::FaultResult,
) -> crate::fault::FaultResult {
    let mut out = first_cycle.clone();
    out.buses = project.buses.iter().filter_map(|bus| {
        let source = if crate::machine::ansi_duty_for_kv(bus.kv) == crate::machine::Duty::FirstCycle {
            &first_cycle
        } else {
            &interrupting
        };
        source.buses.iter().find(|row| row.id == bus.id).cloned()
    }).collect();
    for warning in &interrupting.iec_warnings {
        if !out.iec_warnings.iter().any(|existing| existing == warning) {
            out.iec_warnings.push(warning.clone());
        }
    }
    for correction in &interrupting.iec_corrections {
        if !out.iec_corrections.iter().any(|existing| existing == correction) {
            out.iec_corrections.push(correction.clone());
        }
    }
    out
}

fn run_one(project: &Project, studies: Studies) -> Result<StudyOutput, String> {
    let errors = project.validate();
    if !errors.is_empty() { return Err(errors.join("; ")); }
    let started = std::time::Instant::now();
    let model = crate::network::build_validated(project);
    let mut warnings = model.as_ref().map(|m| m.warnings.clone()).unwrap_or_default();
    let need_lf = studies.loadflow || project.prefault == crate::model::Prefault::Loadflow;
    let need_fault = studies.fault || studies.arcflash || studies.coordination;

    let loadflow = if need_lf {
        let result = model.as_ref().map_err(Clone::clone)
            .and_then(|model| loadflow::solve_with_model(project, model));
        match result {
            Ok(result) => {
                if let Some(msg) = &result.message {
                    warnings.push(msg.clone());
                }
                if result.converged {
                    warnings.extend(voltage_screens(project, &result));
                }
                Some(result)
            }
            Err(err) => {
                warnings.push(err);
                None
            }
        }
    } else {
        None
    };

    let switched;
    let project = match prefault_fallback(project, loadflow.as_ref()) {
        Some((changed, warning)) => {
            warnings.push(warning);
            switched = changed;
            &switched
        }
        None => project,
    };
    // Fault and arc flash use the network selected by fault_method. Load flow
    // keeps the entered reactances. Under ANSI each bus uses the first-cycle
    // C37.010 network at or below 1 kV and the interrupting network above that;
    // the generator / synchronous-motor step-down is off. Post-motor arc-flash
    // stages use the same duty with induction motors removed.
    let method_held;
    let method_project = {
        let p = fault_method_project(project);
        if project.fault_method == FaultMethod::Ansi {
            warnings.extend(crate::machine::warnings(project));
            warnings.push("Fault method is ANSI: bolted currents use the first-cycle C37.010 duty network at buses through 1 kV and the interrupting network above 1 kV. The generator and synchronous-motor step-down is not applied.".into());
        }
        method_held = p;
        &method_held
    };
    let fault = if need_fault {
        Some(fault_for_method(method_project, loadflow.as_ref())?)
    } else {
        None
    };

    let protection = crate::protection::identify_all(project);
    let mut arc_flash_failures = Vec::new();
    let arc_flash = if studies.arcflash {
        Some(arc_flash(method_project, loadflow.as_ref(), fault.as_ref().unwrap(), &protection, &mut warnings, &mut arc_flash_failures))
    } else {
        None
    };

    let coordination = if studies.coordination {
        Some(coordination(project, fault.as_ref().unwrap()))
    } else {
        None
    };
    if let Some(error) = fault.as_ref().and_then(|f| f.error.as_ref()) { warnings.push(error.clone()); }
    if let Some(f) = fault.as_ref() { warnings.extend(f.iec_warnings.iter().cloned()); }
    let interrupting = if let Some(solved) = fault.as_ref().filter(|result| result.valid) {
        let rows = interrupting_duties(project, loadflow.as_ref(), solved)?;
        if !rows.is_empty() { warnings.extend(crate::machine::warnings(project)); }
        warnings.extend(duty_warnings(&rows));
        rows
    } else {
        Vec::new()
    };
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));

    Ok(StudyOutput {
        case_id: "normal".into(),
        methods: Methods {
            load_flow: "Newton-Raphson positive-sequence power flow".into(),
            short_circuit: "Symmetrical components. Peak current uses the IEC 60909 factor. IEC peak current is kappa times sqrt(2) times the maximum initial symmetrical current (cmax and, where applied, the impedance correction factors), with kappa from the faulted-bus R/X (IEC 60909-0 clause 8.1). C37 half-cycle and duty values stay on the uncorrected symmetrical current. A bus whose closed, energized branch graph contains a loop is meshed and uses method B, kappa times 1.15 limited to 1.8 at or below 1 kV and 2.0 above, unless every branch in that loop group has R/X below 0.3. A radial bus uses kappa without 1.15; several radial infeeds are combined through the Thevenin R/X, not a sum of partial peak currents. Each fault point reports peak_basis. Half-cycle rms is the first-half-cycle asymmetrical value. Minimum and maximum symmetrical currents apply IEC 60909-0 Table 1 voltage factor c to that current. The maximum symmetrical current uses a network with IEC 60909-0 impedance correction factors: K_T on network transformers (clause 6.3.3), K_G on generators with an entered generator rating (clause 6.6.1), and K_S or K_SO on a generator and its unit transformer when generator.unit is entered (clause 6.7). Each is evaluated in closed form from entered data, and the corrected impedance is the entered impedance times the factor. The minimum current is uncorrected. A rotating-machine source without a generator rating is not corrected and is reported. A breaker above 1 kV with contact_parting_cycles reports an IEEE C37.010 multiplier: asymmetrical rms at that parting time divided by the breaker asymmetry capability, and not below 1. NACD is reported. When NACD is below 1 and a local source has an entered decrement curve, that multiplier blends the remote DC-decrement factor with the local factor from the curve at 60 Hz. Motor contribution without a decrement curve is not reduced. A source with no decrement curve stays remote. Above 1 kV, closing and latching duty is 1.6 times that symmetrical current, and capability is 1.6 times K times the rated symmetrical current. Omitted K is 1. Duty rows use IEEE C37.010 5.4.1 machine reactance multipliers for classified motors and generators: the half-cycle comparison, bracing, and closing and latching use the first-cycle network, and the interrupting multiplier uses the interrupting network. Unclassified machines keep their entered reactance for both duties. Duty X/R comes from separate networks: the faulted-bus Thevenin reactance of a reactance-only network over the Thevenin resistance of a resistance-only network, each with that duty's machine multipliers and without shunt capacitance. Duty symmetrical current stays from the complex network. The fault study table keeps the complex-network X/R.".into(),
            arc_flash: "IEEE 1584-2018 Each arc-flash row reports clearing_basis and clearing_basis_reduced (manual_duration, assumed_exposure_duration, total_clearing_curve, trip_curve_plus_interrupting_time, fuse_melt_estimate, current_limiting_manufacturer_time, current_limiting_fixed_table, current_limiting_stored_curve, staged_multi_device, staged_decaying_current), label_ready, and label_reasons (duration_capped, assumed_exposure_duration, fuse_melt_estimate, current_limiting_fixed_table, assumed_curve_band, point_curve_tail_held, relay_reset_assumed, staged_clearing_assumed, staged_decaying_current, static_device_timing, motor_decay_assumed, remaining_current_excluded, machine_step_down_assumed, assumed_equipment, bolted_out_of_range, model_range_exceeded, assumed_clearing). label_ready is true only when no reason applies to either arcing-current case; it is a data-basis screen, not an engineering review. Arc-flash failures report clearing_basis unresolved, label_ready false, and label_reasons [unresolved]. Times and energies are unchanged by this classification.".into(),
            coordination: "IEC 60255, IEEE C37.112, definite time, and a simplified thermal-magnetic breaker".into(),
        },
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        warnings,
        loadflow: if studies.loadflow { loadflow } else { None },
        fault: if studies.fault { fault } else { None },
        interrupting,
        arc_flash,
        coordination,
        arc_flash_failures,
        protection,
        operating_cases: Vec::new(),
        worst_case: None,
        energized_bus_ids: model.as_ref().map(|model| model.energized.iter().enumerate()
            .filter(|(_, on)| **on)
            .map(|(i, _)| model.index.id_of[i].clone()).collect()).unwrap_or_default(),
    })
}

struct CaseView<'a> {
    id: &'a str,
    energized_bus_ids: &'a [String],
    result: Option<&'a StudyOutput>,
}

fn update_maximum(maximum: &mut Option<CaseMaximum>, case_id: &str, value: f64) {
    if value.is_finite() && maximum.as_ref().map_or(true, |prior| value > prior.value) {
        *maximum = Some(CaseMaximum { case_id: case_id.into(), value });
    }
}

fn scenario_envelope(project: &Project, normal: &StudyOutput, studies: Studies) -> ScenarioEnvelope {
    let mut cases = vec![CaseView {
        id: "normal",
        energized_bus_ids: &normal.energized_bus_ids,
        result: Some(normal),
    }];
    cases.extend(normal.operating_cases.iter().map(|case| CaseView {
        id: &case.id,
        energized_bus_ids: &case.energized_bus_ids,
        result: case.result.as_deref(),
    }));

    let mut buses = Vec::new();
    if studies.fault {
        for bus in &project.buses {
            let mut envelope = BusCaseEnvelope {
                bus_id: bus.id.clone(),
                three_phase_ka: None, three_phase_iec_peak_ka: None, three_phase_half_cycle_rms_ka: None,
                line_to_ground_ka: None, line_to_ground_iec_peak_ka: None, line_to_ground_half_cycle_rms_ka: None,
                line_to_line_ka: None, line_to_line_iec_peak_ka: None, line_to_line_half_cycle_rms_ka: None,
                line_to_line_ground_ka: None, line_to_line_ground_iec_peak_ka: None, line_to_line_ground_half_cycle_rms_ka: None,
                complete: true, failed_case_ids: Vec::new(),
            };
            for case in &cases {
                // A completed, deenergized case is not applicable. A failed
                // case with no usable energized mask must remain visible as
                // incomplete rather than disappearing from the envelope.
                if !case.energized_bus_ids.contains(&bus.id)
                    && (case.result.is_some() || !case.energized_bus_ids.is_empty()) {
                    continue;
                }
                let point = case.result.and_then(|out| out.fault.as_ref())
                    .filter(|fault| fault.valid)
                    .and_then(|fault| fault.buses.iter().find(|row| row.id == bus.id));
                if let Some((Some(three), Some(ground), Some(line), Some(double))) = point.map(|row| (
                    row.three_phase.as_ref(), row.line_to_ground.as_ref(),
                    row.line_to_line.as_ref(), row.line_to_line_ground.as_ref(),
                )) {
                    update_maximum(&mut envelope.three_phase_ka, case.id, three.symmetrical_ka);
                    update_maximum(&mut envelope.three_phase_iec_peak_ka, case.id, three.iec_peak_ka);
                    update_maximum(&mut envelope.three_phase_half_cycle_rms_ka, case.id, three.half_cycle_rms_ka);
                    update_maximum(&mut envelope.line_to_ground_ka, case.id, ground.symmetrical_ka);
                    update_maximum(&mut envelope.line_to_ground_iec_peak_ka, case.id, ground.iec_peak_ka);
                    update_maximum(&mut envelope.line_to_ground_half_cycle_rms_ka, case.id, ground.half_cycle_rms_ka);
                    update_maximum(&mut envelope.line_to_line_ka, case.id, line.symmetrical_ka);
                    update_maximum(&mut envelope.line_to_line_iec_peak_ka, case.id, line.iec_peak_ka);
                    update_maximum(&mut envelope.line_to_line_half_cycle_rms_ka, case.id, line.half_cycle_rms_ka);
                    update_maximum(&mut envelope.line_to_line_ground_ka, case.id, double.symmetrical_ka);
                    update_maximum(&mut envelope.line_to_line_ground_iec_peak_ka, case.id, double.iec_peak_ka);
                    update_maximum(&mut envelope.line_to_line_ground_half_cycle_rms_ka, case.id, double.half_cycle_rms_ka);
                } else {
                    envelope.complete = false;
                    envelope.failed_case_ids.push(case.id.into());
                }
            }
            buses.push(envelope);
        }
    }

    let mut arc_equipment = Vec::new();
    if studies.arcflash {
        let mut equipment: Vec<(String, String)> = project.equipment.iter()
            .map(|item| (item.id.clone(), item.bus.clone())).collect();
        for bus in &project.buses {
            if !project.equipment.iter().any(|item| item.bus == bus.id) {
                equipment.push((format!("assumed:{}", bus.id), bus.id.clone()));
            }
        }
        for (equipment_id, bus_id) in equipment {
            let mut envelope = ArcCaseEnvelope {
                equipment_id: equipment_id.clone(), bus_id: bus_id.clone(),
                incident_energy_cal_cm2: None, boundary_mm: None,
                complete: true, failed_case_ids: Vec::new(),
            };
            for case in &cases {
                if !case.energized_bus_ids.contains(&bus_id)
                    && (case.result.is_some() || !case.energized_bus_ids.is_empty()) {
                    continue;
                }
                let row = case.result.and_then(|out| out.arc_flash.as_ref())
                    .and_then(|rows| rows.iter().find(|row| row.equipment_id == equipment_id));
                if let Some(row) = row {
                    update_maximum(&mut envelope.incident_energy_cal_cm2, case.id, row.governing_cal_cm2);
                    update_maximum(&mut envelope.boundary_mm, case.id, row.afb_mm);
                } else {
                    envelope.complete = false;
                    envelope.failed_case_ids.push(case.id.into());
                }
            }
            arc_equipment.push(envelope);
        }
    }
    ScenarioEnvelope { buses, arc_equipment }
}

fn arc_flash(project: &Project, _loadflow: Option<&LoadflowResult>, fault: &FaultResult, protection: &[crate::protection::Protection], warnings: &mut Vec<String>, failures: &mut Vec<ArcFailure>) -> Vec<ArcRow> {
    let mut rows = Vec::new();
    let equipment_cases = cases(project, protection);
    for case in equipment_cases {
        // A bus without collected equipment is evaluated once per IEEE 1584
        // equipment class. Each class brings its own gap, enclosure, working
        // distance, and electrode. The governing energy and boundary are kept
        // independently.
        let assumed_enclosure = case.id.starts_with("assumed:");
        let specs = if assumed_enclosure {
            project.bus(&case.bus).map(classes_for_assumed_bus).unwrap_or_default()
                .into_iter().map(|class| class.spec(project.bus(&case.bus).map(|b| b.kv).unwrap_or(0.48))).collect()
        } else {
            Vec::new()
        };
        if assumed_enclosure && specs.is_empty() {
            let error = format!("no IEEE 1584 equipment class applies at this voltage");
            warnings.push(format!("{}: {error}", case.name));
            failures.push(ArcFailure::new(&case, error));
            continue;
        }
        let variants: Vec<(ArcEquipment, Option<EquipmentSpec>)> = if assumed_enclosure {
            specs.iter().map(|spec| {
                let mut variant = case.clone();
                variant.name = format!("{} ({}, assumed)", case.name.trim_end_matches(" (assumed)"), spec.label);
                variant.electrode = spec.electrode;
                variant.gap_mm = spec.gap_mm;
                variant.distance_mm = spec.distance_mm;
                variant.height_mm = spec.height_mm;
                variant.width_mm = spec.width_mm;
                variant.depth_mm = spec.depth_mm;
                variant.basis = "assumed".into();
                (variant, Some(*spec))
            }).collect()
        } else {
            vec![(case.clone(), None)]
        };
        let mut evaluated = Vec::new();
        let mut error = None;
        for (variant, spec) in variants {
            match evaluate_equipment(project, fault, protection, &variant) {
                Ok((row, warnings_for_case)) => evaluated.push((row, warnings_for_case, spec)),
                Err(e) => {
                    error = Some(if let Some(spec) = spec {
                        format!("assumed {}: {e}; the maximum over assumed equipment classes is unavailable", spec.label)
                    } else { e });
                    break;
                }
            }
        }
        if let Some(error) = error {
            warnings.push(format!("{}: {error}", case.name));
            failures.push(ArcFailure::new(&case, error));
            continue;
        }
        let mut ieee_warnings: Vec<String> = Vec::new();
        for (_, items, _) in &evaluated {
            for w in items {
                if !ieee_warnings.contains(w) { ieee_warnings.push(w.clone()); }
            }
        }
        let row = if assumed_enclosure {
            let specs: Vec<EquipmentSpec> = evaluated.iter().filter_map(|(_, _, spec)| *spec).collect();
            let (row, note) = governing_assumed_classes(evaluated.into_iter().map(|(row, _, _)| row).collect(), &specs);
            warnings.push(format!(
                "{} enclosure dimensions were not collected; the values used are an assumption, not a field measurement. {note}",
                case.name
            ));
            row
        } else {
            let row = evaluated.pop().expect("one equipment item evaluated").0;
            if case.basis == "assumed" {
                warnings.push(format!(
                    "{} enclosure dimensions were not collected; the values used are an assumption, not a field measurement.",
                    case.name
                ));
            }
            row
        };
        for w in &ieee_warnings {
            warnings.push(format!("{}: {w}", case.name));
        }
        rows.push(row);
    }
    rows
}

/// Greatest governing incident energy, and the greatest boundary independently.
fn governing_assumed_classes(rows: Vec<ArcRow>, specs: &[EquipmentSpec]) -> (ArcRow, String) {
    let energies = rows.iter().zip(specs).map(|(row, spec)| format!("{} {:.2} cal/cm2", spec.label, row.governing_cal_cm2))
        .collect::<Vec<_>>().join(", ");
    let boundary_index = rows.iter().enumerate().max_by(|a, b| a.1.afb_mm.total_cmp(&b.1.afb_mm)).map(|(i, _)| i).unwrap_or(0);
    let energy_index = rows.iter().enumerate().max_by(|a, b| a.1.governing_cal_cm2.total_cmp(&b.1.governing_cal_cm2)).map(|(i, _)| i).unwrap_or(0);
    let boundary_label = specs.get(boundary_index).map(|spec| spec.label).unwrap_or("unknown");
    let energy_label = specs.get(energy_index).map(|spec| spec.label).unwrap_or("unknown");
    let afb_full_mm = rows.iter().map(|row| row.afb_full_mm).fold(f64::MIN, f64::max);
    let afb_reduced_mm = rows.iter().map(|row| row.afb_reduced_mm).fold(f64::MIN, f64::max);
    let mut governing = rows.into_iter().nth(energy_index).expect("assumed classes");
    let classes = specs.iter().map(|spec| spec.label).collect::<Vec<_>>().join("; ");
    let gap_note = if specs.iter().any(|spec| spec.gap_not_collected) {
        " Open-air gap was not collected; the enclosed-class gap at this voltage was used."
    } else {
        ""
    };
    let note = format!(
        "Assumed equipment class: {classes} were evaluated, each with its own gap, enclosure, working distance, and electrode. {energy_label} governs incident energy ({energies}) and {boundary_label} governs the arc-flash boundary. The maximum of each is reported.{gap_note}"
    );
    governing.afb_full_mm = afb_full_mm;
    governing.afb_reduced_mm = afb_reduced_mm;
    governing.afb_mm = afb_full_mm.max(afb_reduced_mm);
    governing.afb_in = governing.afb_mm / 25.4;
    governing.assumed = true;
    governing.enclosure_assumed = true;
    if !governing.label_reasons.iter().any(|r| r == "assumed_equipment") { governing.label_reasons.push("assumed_equipment".into()); }
    governing.label_ready = false;
    governing.warnings.insert(0, note.clone());
    (governing, note)
}

/// One IEEE 1584 evaluation of an equipment item (or one assumed electrode
/// configuration). Returns the row and the IEEE model warnings for the study.
fn evaluate_equipment(project: &Project, fault: &FaultResult,
    protection: &[crate::protection::Protection], case: &ArcEquipment) -> Result<(ArcRow, Vec<String>), String> {
    let assumed = case.id.starts_with("assumed:") || case.basis == "assumed";
    let Some(bus) = project.bus(&case.bus) else { return Err("unknown bus".into()) };
    let decrement_source_ids = curved_sources_reaching(project, &case.bus);
    let Some(bus_fault) = fault.buses.iter().find(|b| b.id == bus.id) else { return Err("missing fault result".into()) };
    let Some(bolted) = bus_fault.three_phase.as_ref().map(|p| p.symmetrical_ka) else {
        let deenergized = !project.sources.iter().any(|s| s.in_service && reachable_buses(project, &case.bus).contains(&s.bus));
        return Err(if deenergized {
            format!("bus '{}' is out of service: zero three-phase fault current ({})", case.bus, bus_fault.note.clone().unwrap_or_else(|| "de-energized".into()))
        } else {
            format!("bus '{}' fault current is zero ({})", case.bus, bus_fault.note.clone().unwrap_or_else(|| "no source".into()))
        });
    };
    if bus.kv < 0.208 {
        return Err(format!("{:.2} kV is outside the IEEE 1584-2018 range of 208 V to 15 kV", bus.kv));
    }
    let template = preview_input(case, bus.kv, bolted);
    let preview = arcflash::calculate(&template)?;
    let protection = protection.iter().find(|p| p.bus_id == case.bus).expect("validated bus has protection status");
    let manual = case.clearing_s.is_some();
    let single = single_device_duration(project, case);
    let limiting = !manual && single && (
        current_limiting_at(project, case, bus_fault, preview.i_arc_ka, bolted)
        || current_limiting_at(project, case, bus_fault, preview.i_arc_min_ka, bolted)
    );
    let reachable = reachable_buses(project, &case.bus);
    let time_varying_data = !decrement_source_ids.is_empty()
        || project.motors.iter().any(|motor| motor.decay_time_constant_s.is_some() && reachable.contains(&motor.bus))
        || project.machine_step_down.is_some();
    let placed = case.upstream_device.as_deref().is_some_and(|id| located_devices(project).iter().any(|device| device.id == id));
    let use_one_solution = !manual && (!single || (time_varying_data && !limiting && placed));
    let (full_clear, min_clear, staged) = if use_one_solution {
        let staged = one_solution_exposure(project, case, bus_fault, bolted, &template, &decrement_source_ids, protection, None)?;
        (staged.full.clone(), staged.reduced.clone(), Some(staged))
    } else {
        let full_clear = duration(project, case, bus_fault, preview.i_arc_ka, protection)?;
        let min_clear = duration(project, case, bus_fault, preview.i_arc_min_ka, protection)?;
        (full_clear, min_clear, None)
    };
    let mut full_clear = full_clear;
    let mut min_clear = min_clear;
    if limiting && !decrement_source_ids.is_empty() {
        let extra = " Source decrement was not applied because the current-limiting let-through time is used.";
        full_clear.note.push_str(extra);
        min_clear.note.push_str(extra);
    }
    let mut input = preview_input(case, bus.kv, bolted);
    input.time_s = full_clear.seconds;
    input.time_min_s = min_clear.seconds;
    input.arc_ka_override = full_clear.effective_arc_ka;
    input.arc_min_ka_override = min_clear.effective_arc_ka;
    let fixed_profile_exposure = if manual && time_varying_data {
        Some(one_solution_exposure(project, case, bus_fault, bolted, &template, &decrement_source_ids, protection, Some((full_clear.seconds, min_clear.seconds)))?)
    } else {
        None
    };
    let integrate_decrement = fixed_profile_exposure.as_ref().is_some_and(|e| e.time_varying) && !decrement_source_ids.is_empty();
    let staged_decrement = staged.as_ref().is_some_and(|exposure| exposure.time_varying) && !decrement_source_ids.is_empty();
    let (out, final_slice) = if let Some(staged) = staged.as_ref().or(fixed_profile_exposure.as_ref().filter(|e| e.time_varying)) {
        let mut out = arcflash::calculate(&input)?;
        let full = arcflash::integrate_profile(&input, &staged.full_segments, false)?;
        let reduced = arcflash::integrate_profile(&input, &staged.reduced_segments, true)?;
        apply_profile(&mut out, full, reduced);
        let last = if staged.time_varying {
            out.warnings.push("Contributions into the bus were scaled in time from the one fault solution (source decrement, motor decay, or machine step-down); the network was not re-solved. Source X/R was held constant.".into());
            Some(staged_final_slice(&input, &staged.full_segments, &staged.reduced_segments)?)
        } else {
            None
        };
        (out, last)
    } else {
        (arcflash::calculate(&input)?, None)
    };
    let governing_clear = if out.governing == "reduced_arcing" { &min_clear } else { &full_clear };
    let ieee_warnings = out.warnings.clone();
    let mut notes = out.warnings;
    for clear in [&full_clear, &min_clear] {
        if clear.assumed && !notes.contains(&clear.note) { notes.push(clear.note.clone()); }
    }
    if final_slice.is_some() {
        notes.push("Arcing currents shown are initial (t=0) values; incident energy and boundary integrate the time-varying decrement profile. final_* fields give the last integration slice.".into());
    }
    let row = ArcRow {
        equipment_id: case.id.clone(),
        name: case.name.clone(),
        bus_id: bus.id.clone(),
        bus_name: bus.name.clone(),
        assumed: assumed || full_clear.assumed || min_clear.assumed || out.bolted_out_of_range || out.model_range_exceeded,
        enclosure_assumed: assumed,
        electrode: case.electrode.as_str().into(),
        bolted_ka: bolted,
        arcing_ka: out.i_arc_ka,
        arcing_min_ka: out.i_arc_min_ka,
        time_s: out.time_s,
        time_min_s: out.time_min_s,
        energy_cal_cm2: out.energy_cal_cm2,
        energy_min_cal_cm2: out.energy_min_cal_cm2,
        governing_cal_cm2: out.governing_cal_cm2,
        afb_full_mm: out.afb_full_mm,
        afb_reduced_mm: out.afb_reduced_mm,
        afb_in: out.afb_in,
        afb_mm: out.afb_mm,
        governing: out.governing.into(),
        duration_capped: governing_clear.capped,
        duration_note: governing_clear.note.clone(),
        upstream_device: case.upstream_device.clone(),
        governing_device: match &staged {
            Some(exposure) => if out.governing == "reduced_arcing" { exposure.reduced_device.clone() } else { exposure.full_device.clone() },
            None if manual => None,
            None => case.upstream_device.clone(),
        },
        standard: out.standard.into(),
        decrement_applied: integrate_decrement || staged_decrement,
        decrement_source_ids: if integrate_decrement || staged_decrement { decrement_source_ids } else { Vec::new() },
        final_bolted_ka: final_slice.as_ref().map(|last| last.bolted_ka),
        final_arcing_ka: final_slice.as_ref().map(|last| last.arcing_ka),
        final_arcing_min_ka: final_slice.as_ref().map(|last| last.arcing_min_ka),
        warnings: notes,
        clearing_basis: full_clear.basis.into(),
        clearing_basis_reduced: min_clear.basis.into(),
        label_ready: false,
        label_reasons: Vec::new(),
    };
    let mut row = row;
    row.label_reasons = label_reasons(assumed, &full_clear, &min_clear, out.bolted_out_of_range, out.model_range_exceeded, row.assumed);
    if template.lee {
        row.label_reasons.push("lee_method".into());
        row.warnings.insert(0, format!(
            "{:.2} kV is above the IEEE 1584-2018 range of 208 V to 15 kV; incident energy and boundary use the Lee (1982) theoretical model with the arc carrying the full bolted current. Device timing, staging, and the cleared-fault threshold use that current. Lee is conservative for most medium-voltage equipment; compare against an open-air medium-voltage method where one applies.",
            bus.kv));
    }
    row.label_ready = row.label_reasons.is_empty();
    Ok((row, ieee_warnings))
}

fn apply_profile(out: &mut arcflash::IeeeOutput, full: arcflash::ProfileEnergy, reduced: arcflash::ProfileEnergy) {
    out.bolted_out_of_range |= full.bolted_out_of_range || reduced.bolted_out_of_range;
    out.model_range_exceeded |= full.model_range_exceeded || reduced.model_range_exceeded;
    out.energy_cal_cm2 = full.energy_cal_cm2;
    out.energy_min_cal_cm2 = reduced.energy_cal_cm2;
    out.energy_j_cm2 = full.energy_cal_cm2 * 4.184;
    out.energy_min_j_cm2 = reduced.energy_cal_cm2 * 4.184;
    out.afb_full_mm = full.boundary_mm;
    out.afb_reduced_mm = reduced.boundary_mm;
    out.afb_mm = full.boundary_mm.max(reduced.boundary_mm);
    out.afb_in = out.afb_mm / 25.4;
    if out.governing == "lee" {
        // Lee has one arcing current; the full case governs.
        out.governing_cal_cm2 = full.energy_cal_cm2;
    } else if reduced.energy_cal_cm2 >= full.energy_cal_cm2 {
        out.governing = "reduced_arcing";
        out.governing_cal_cm2 = reduced.energy_cal_cm2;
    } else {
        out.governing = "arcing";
        out.governing_cal_cm2 = full.energy_cal_cm2;
    }
    out.governing_j_cm2 = out.governing_cal_cm2 * 4.184;
    for warning in full.warnings.into_iter().chain(reduced.warnings) {
        if !out.warnings.contains(&warning) { out.warnings.push(warning); }
    }
}


/// How long the decrement relay is allowed to time before the study stops it.
/// Escape uses the duration cap. Without escape, the relay may run through the
/// last entered decrement point and then finish at the held current.
fn exposure_limit_s(project: &Project, decrement_source_ids: &[String]) -> f64 {
    if project.escape_possible {
        return project.arc_duration_cap_s;
    }
    project.sources.iter()
        .filter(|source| decrement_source_ids.contains(&source.id))
        .filter_map(|source| source.decrement_curve.last().map(|point| point.time_s))
        .fold(project.arc_duration_cap_s, f64::max)
}

/// Interpolate between adjacent network-current samples and split at each
/// entered element pickup. A stage only accrues the portion of a slice above
/// its pickup; a midpoint alone could otherwise falsely complete a short delay.
fn advance_relay_over_ramp(relay: &mut curves::DynamicRelay, start_amps: f64, end_amps: f64,
    duration_s: f64) -> Result<Option<f64>, String> {
    if !start_amps.is_finite() || !end_amps.is_finite() || !duration_s.is_finite() || duration_s <= 0.0 {
        return Err("sampled relay current or interval is invalid".into());
    }
    let mut fractions = vec![0.0, 1.0];
    for pickup in relay.pickup_thresholds() {
        if (start_amps - pickup) * (end_amps - pickup) < 0.0 {
            fractions.push((pickup - start_amps) / (end_amps - start_amps));
        }
    }
    fractions.sort_by(f64::total_cmp);
    let mut elapsed_s = 0.0;
    for pair in fractions.windows(2) {
        let width_s = duration_s * (pair[1] - pair[0]);
        let midpoint_fraction = (pair[0] + pair[1]) / 2.0;
        let amps = start_amps + (end_amps - start_amps) * midpoint_fraction;
        if let Some(within_s) = relay.advance(amps, width_s)? {
            return Ok(Some(elapsed_s + within_s));
        }
        elapsed_s += width_s;
    }
    Ok(None)
}

#[cfg(test)]
mod relay_ramp_regression {
    use super::advance_relay_over_ramp;
    use crate::{curves::DynamicRelay, model::CurveSpec};

    #[test]
    fn pickup_crossing_does_not_falsely_complete_a_short_definite_delay() {
        let curve = CurveSpec::Definite { pickup_a: 100.0, time_s: 0.008 };
        let mut relay = DynamicRelay::new(&curve).unwrap();
        // Pickup lasts only 5.03 ms; treating the 100.5 A midpoint as a
        // constant 10 ms would incorrectly claim an 8 ms trip.
        assert_eq!(advance_relay_over_ramp(&mut relay, 200.0, 1.0, 0.010).unwrap(), None);
        assert!(relay.reset_occurred());

        let mut fast = DynamicRelay::new(&CurveSpec::Definite { pickup_a: 100.0, time_s: 0.004 }).unwrap();
        let trip = advance_relay_over_ramp(&mut fast, 200.0, 1.0, 0.010).unwrap().unwrap();
        assert!((trip - 0.004).abs() < 1e-12, "trip at {trip}");
    }
}

/// Currents in the last integration slice of a decrement profile.
struct FinalSlice {
    bolted_ka: f64,
    arcing_ka: f64,
    arcing_min_ka: f64,
}

fn staged_final_slice(input: &IeeeInput, full: &[EnergySegment], reduced: &[EnergySegment]) -> Result<FinalSlice, String> {
    let last = |segments: &[EnergySegment]| segments.last().map(|segment| segment.ibf_ka)
        .ok_or_else(|| "decrement profile has no slices".to_string());
    let final_bolted_ka = last(full)?;
    let final_reduced_bolted_ka = last(reduced)?;
    Ok(FinalSlice {
        bolted_ka: final_bolted_ka,
        arcing_ka: arcflash::calculate(&IeeeInput { ibf_ka: final_bolted_ka, ..input.clone() })?.i_arc_ka,
        arcing_min_ka: arcflash::calculate(&IeeeInput { ibf_ka: final_reduced_bolted_ka, ..input.clone() })?.i_arc_min_ka,
    })
}


fn curved_sources_reaching(project: &Project, bus_id: &str) -> Vec<String> {
    let mut connected = std::collections::HashSet::from([bus_id.to_string()]);
    loop {
        let prior = connected.len();
        for branch in project.branches.iter().filter(|branch| project.branch_closed(branch)) {
            if connected.contains(&branch.from) { connected.insert(branch.to.clone()); }
            if connected.contains(&branch.to) { connected.insert(branch.from.clone()); }
        }
        if connected.len() == prior { break; }
    }
    project.sources.iter()
        .filter(|source| source.in_service && !source.decrement_curve.is_empty() && connected.contains(&source.bus))
        .map(|source| source.id.clone()).collect()
}

const DECREMENT_STEP_S: f64 = 0.01;





fn next_decrement_end(project: &Project, decrement_source_ids: &[String], time_s: f64, duration_s: f64) -> f64 {
    let mut end_s = (time_s + DECREMENT_STEP_S).min(duration_s);
    // A recorded profile point is an integration boundary for both relay
    // operation and incident energy.
    for source in project.sources.iter().filter(|source| decrement_source_ids.contains(&source.id)) {
        if let Some(next) = source.decrement_curve.iter()
            .find(|point| point.time_s > time_s + 1e-12 && point.time_s < end_s - 1e-12) {
            end_s = next.time_s;
        }
    }
    end_s
}

fn preview_input(case: &ArcEquipment, kv: f64, bolted: f64) -> IeeeInput {
    IeeeInput {
        voc_kv: kv,
        ibf_ka: bolted,
        gap_mm: case.gap_mm,
        distance_mm: case.distance_mm,
        height_mm: case.height_mm,
        width_mm: case.width_mm,
        depth_mm: case.depth_mm,
        electrode: case.electrode,
        time_s: 0.1,
        time_min_s: 0.1,
        arc_ka_override: None,
        arc_min_ka_override: None,
        lee: kv > 15.0,
    }
}

fn cases(project: &Project, protection: &[crate::protection::Protection]) -> Vec<ArcEquipment> {
    let mut cases = project.equipment.clone();
    for bus in &project.buses {
        if cases.iter().any(|e| e.bus == bus.id) {
            continue;
        }
        let (electrode, gap, distance, height, width, depth, label) = typical_equipment(bus.kv);
        let upstream = None;
        let name = if bus.name.is_empty() { bus.id.clone() } else { bus.name.clone() };
        cases.push(ArcEquipment {
            id: format!("assumed:{}", bus.id),
            name: format!("{name} ({label}, assumed)"),
            bus: bus.id.clone(),
            electrode,
            gap_mm: gap,
            distance_mm: distance,
            height_mm: height,
            width_mm: width,
            depth_mm: depth,
            upstream_device: upstream,
            clearing_s: None,
            fallback_duration_s: None,
            basis: "assumed".into(),
        });
    }
    for case in &mut cases {
        if case.upstream_device.is_none() {
            case.upstream_device = protection.iter().find(|p| p.bus_id == case.bus).and_then(|p| p.device_id.clone());
        }
    }
    cases
}

#[derive(Clone)]
struct Clearing {
    seconds: f64,
    capped: bool,
    note: String,
    assumed: bool,
    /// Manufacturer effective arc current for a sole-feed current-limiting fuse.
    effective_arc_ka: Option<f64>,
    /// How the duration was obtained; see `ArcRow::clearing_basis`.
    basis: &'static str,
    /// Label-readiness reason codes; see `ArcRow::label_reasons`.
    reasons: Vec<&'static str>,
}

impl Clearing {
    fn reason(&mut self, code: &'static str) {
        if !self.reasons.contains(&code) { self.reasons.push(code); }
    }
}

/// Reason codes for a point-curve note sampled over a time-varying current,
/// where the current at which the note was made is not kept.
fn point_note_reasons(note: &str) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if note.contains("is held (assumed)") { reasons.push("point_curve_tail_held"); }
    if note.contains("interpretation") && note.contains("is assumed") { reasons.push("assumed_curve_band"); }
    reasons
}

/// Missing clearing data is a per-bus error. `fallback_duration_s` is not an arc-flash result.
fn missing_arc_flash_message(bus: &str, missing: &str) -> String {
    format!("Arc flash cannot be calculated for bus {bus}. Missing: {missing}")
}

fn missing_arc_flash(bus: &str, missing: &str) -> Result<Clearing, String> {
    Err(missing_arc_flash_message(bus, missing))
}

fn clearing_gap(device: &Device, amps: f64, point_note: Option<(bool, String)>) -> String {
    let name = &device.name;
    let id = &device.id;
    let extra = point_note.map(|(_, note)| format!(" {note}")).unwrap_or_default();
    match &device.curve {
        CurveSpec::SettingsNotCollected { note } => {
            let recorded = if note.is_empty() { String::new() } else { format!(" Recorded note: {note}") };
            format!("a trip curve for {name} [{id}]. No pickup, delay, fuse size, or instantaneous setting is stored.{recorded}")
        }
        CurveSpec::Points { .. } => {
            if curves::trip_time(&device.curve, amps).is_none() {
                if curves::fast_only_range(&device.curve, amps).is_some() {
                    let melt = if curves::average_melt_fuse(device) {
                        " The slower band of a two-band library fuse is a melt-tolerance pair, not the published total-clearing curve. IEEE 1584-2018 Clause 6.9.2 is not applied because the governing band has no time in this range. fuse_total_clearing stays off unless the caller sets it."
                    } else {
                        ""
                    };
                    format!("clearing for {name} [{id}] at {amps:.0} A.{melt}{extra}")
                } else if curves::average_melt_fuse(device) {
                    format!("an operating time for {name} [{id}] at {amps:.0} A. IEEE 1584-2018 Clause 6.9.2 is not applied: the melting curve has no time at this current. Below the first point the fuse does not operate. Above the last point, 0.01 s is used only when that point is the 0.01 s bottom of the curve.{extra}")
                } else {
                    format!("an operating time for {name} [{id}] at {amps:.0} A. The point curve does not produce a time at this current.{extra}")
                }
            } else if !device.curve_includes_clearing() && device.breaker_interrupting_s.is_none() {
                format!("total clearing for {name} [{id}] at {amps:.0} A. The curve has an operating time, but it is not marked total clearing and breaker_interrupting_s is absent. For a fuse, set fuse_total_clearing only when the governing band is the published total-clearing curve. For a breaker, enter breaker_interrupting_s.{extra}")
            } else {
                format!("a usable total-clearing time for {name} [{id}] at {amps:.0} A.{extra}")
            }
        }
        _ => {
            if curves::trip_time(&device.curve, amps).is_none() {
                format!("an operating time for {name} [{id}] at {amps:.0} A. A pickup, delay, or instantaneous time is missing, or the current is below pickup.{extra}")
            } else if device.breaker_interrupting_s.is_none() && !device.curve_includes_clearing() {
                format!("breaker_interrupting_s for {name} [{id}]. The trip curve operates at {amps:.0} A, and total clearing is that operating time plus the breaker interrupting time.{extra}")
            } else {
                format!("a usable total-clearing time for {name} [{id}] at {amps:.0} A.{extra}")
            }
        }
    }
}

fn duration(project: &Project, case: &ArcEquipment, fault: &crate::fault::BusFault, i_ka: f64, protection: &crate::protection::Protection) -> Result<Clearing, String> {
    let not_a_label = "This cap is not a field trip setting and is not for an arc-flash label.";
    if let Some(manual) = case.clearing_s {
        let basis = if case.basis == "assumed" { "assumed exposure duration" } else { "manual duration" };
        let mut clear = finish_clearing(manual, project.arc_duration_cap_s, project.escape_possible, &format!("{basis} of {manual:.3} s"), not_a_label);
        clear.assumed = case.basis == "assumed" || clear.assumed;
        if case.basis == "assumed" {
            clear.basis = "assumed_exposure_duration";
            clear.reason("assumed_exposure_duration");
        } else {
            clear.basis = "manual_duration";
        }
        clear.note.push_str(&format!(" {} Manual duration overrides device-based clearing.", protection.detail));
        return Ok(clear);
    }
    let Some(id) = case.upstream_device.as_ref() else {
        return missing_arc_flash(&case.bus, &format!("an upstream device that isolates this bus. {}", protection.detail));
    };
    let device = project.device(id).ok_or_else(|| missing_arc_flash_message(&case.bus, &format!("the upstream device [{id}] named on this equipment")))?;
    if let Some(branch) = device.protected_branch.as_deref()
        .and_then(|id| project.branches.iter().find(|branch| branch.id == id)) {
        if !project.branch_closed(branch) {
            return missing_arc_flash(&case.bus, &format!("a closed protected branch for upstream device {} [{}]. Branch '{}' is open in this operating case, so that device does not clear this fault", device.name, device.id, branch.id));
        }
    }
    let bolted = fault.three_phase.as_ref()
        .ok_or_else(|| missing_arc_flash_message(&case.bus, "a three-phase fault current at this bus"))?
        .symmetrical_ka;
    let Some(amps) = device_current(project, device, fault, i_ka / bolted) else {
        return missing_arc_flash(&case.bus, &format!("protected_branch on upstream device {} [{}], naming the branch whose terminal current the device interrupts", device.name, device.id));
    };
    if !isolates_sources(project, device, &case.bus) {
        return Err("opening the selected device does not isolate all utility/generator paths to the fault; a multi-device clearing study is required".into());
    }
    reject_connected_motors(project, device, &case.bus)?;
    device_clearing(project, device, amps, true).map_err(|gap| missing_arc_flash_message(&case.bus, &gap))
}

/// True when the existing single-device duration should be used.
/// A named device with a missing or open branch stays on that path so the
/// original missing-data error is kept. Staging is only for a device that
/// leaves another source or a motor connected.
fn single_device_duration(project: &Project, equipment: &ArcEquipment) -> bool {
    let Some(id) = equipment.upstream_device.as_ref() else { return false };
    let Some(device) = project.device(id) else { return true };
    let Some(branch) = device.protected_branch.as_deref().and_then(|id| project.branches.iter().find(|branch| branch.id == id)) else {
        return true;
    };
    if !project.branch_closed(branch) || (device.bus != branch.from && device.bus != branch.to) {
        return true;
    }
    isolates_sources(project, device, &equipment.bus) && connected_motor_ids(project, device, &equipment.bus).is_empty()
}

fn current_limiting_at(project: &Project, equipment: &ArcEquipment, fault: &crate::fault::BusFault, i_ka: f64, bolted: f64) -> bool {
    let Some(device) = equipment.upstream_device.as_deref().and_then(|id| project.device(id)) else { return false };
    let Some(limit) = &device.current_limiting else { return false };
    if bolted <= 0.0 { return false; }
    device_current(project, device, fault, i_ka / bolted).is_some_and(|amps| amps >= limit.threshold_a)
}

struct DeviceExposure {
    seconds: f64,
    assumed: bool,
    /// Appended after the duration sentence.
    extra: String,
    effective_arc_ka: Option<f64>,
    what: String,
    basis: &'static str,
    reasons: Vec<&'static str>,
}

/// Uncapped device time at one terminal current. `sole_feed` is true only when
/// this device isolates every utility, generator, and motor.
fn device_exposure(device: &Device, amps: f64, sole_feed: bool) -> Result<DeviceExposure, String> {
    let what_at = |kind: &str| format!("{kind} of {} at {amps:.0} A through its protected terminal (network terminal current; loadflow prefault includes initial branch flow)", device.name);
    if let Some(limit) = &device.current_limiting {
        if amps >= limit.threshold_a {
            let (seconds, assumed, mut extra, basis, reasons) = if limit.cycle_table {
                let (seconds, assumed, extra, stored) = let_through_table(device, limit, amps);
                if stored {
                    let reasons = if assumed { curves::point_curve_reasons(&device.curve, amps) } else { Vec::new() };
                    (seconds, assumed, extra, "current_limiting_stored_curve", reasons)
                } else {
                    (seconds, assumed, extra, "current_limiting_fixed_table", vec!["current_limiting_fixed_table"])
                }
            } else {
                (
                    limit.clearing_s,
                    false,
                    format!(" Manufacturer current-limiting clearing time is used at {amps:.0} A, at or above the {:.0} A peak let-through threshold (IEEE 1584-2018 Clause 6.9.2).", limit.threshold_a),
                    "current_limiting_manufacturer_time",
                    Vec::new(),
                )
            };
            let effective = if sole_feed {
                if limit.effective_arc_ka.is_some() {
                    extra.push_str(" Manufacturer effective arc current replaces the calculated arcing current for this sole feed. The variation factor is not applied again.");
                } else {
                    extra.push_str(" No manufacturer effective arc current was entered, so the calculated arcing current is kept.");
                }
                limit.effective_arc_ka
            } else if limit.effective_arc_ka.is_some() {
                extra.push_str(" Manufacturer effective arc current was not substituted because more than one source feeds the arc.");
                None
            } else {
                extra.push_str(" No manufacturer effective arc current was entered, so the calculated arcing current is kept.");
                None
            };
            let kind = if limit.cycle_table { "current-limiting let-through time" } else { "manufacturer current-limiting clearing time" };
            return Ok(DeviceExposure {
                seconds,
                assumed,
                extra,
                effective_arc_ka: effective,
                what: what_at(kind),
                basis,
                reasons,
            });
        }
    }
    let point_note = curves::point_curve_note(&device.curve, amps);
    if let Some(melt) = curves::average_melt_clearing(device, amps) {
        let mut extra = format!(" {}", melt.detail(amps));
        let curve_bottom = matches!(melt.adjustment, curves::MeltAdjustment::CurveBottom { .. });
        let mut assumed = true;
        let mut reasons = vec!["fuse_melt_estimate"];
        let point_reasons = curves::point_curve_reasons(&device.curve, amps);
        if !curve_bottom {
            if let Some((band_assumed, note)) = &point_note {
                assumed |= *band_assumed;
                extra.push(' ');
                extra.push_str(note);
                if *band_assumed { reasons.extend(point_reasons); }
            }
        } else if point_note.as_ref().is_some_and(|(band_assumed, _)| *band_assumed) {
            extra.push_str(" The point curve's band interpretation is assumed.");
            if point_reasons.contains(&"assumed_curve_band") { reasons.push("assumed_curve_band"); }
        }
        return Ok(DeviceExposure {
            seconds: melt.seconds,
            assumed,
            extra,
            effective_arc_ka: None,
            what: what_at("estimated clearing time"),
            basis: "fuse_melt_estimate",
            reasons,
        });
    }
    let Some(raw) = curves::clearing_time(device, amps) else {
        return Err(clearing_gap(device, amps, point_note));
    };
    let mut extra = String::new();
    let mut assumed = false;
    let mut reasons = Vec::new();
    if let Some((band_assumed, note)) = point_note {
        assumed |= band_assumed;
        extra.push(' ');
        extra.push_str(&note);
        if band_assumed { reasons = curves::point_curve_reasons(&device.curve, amps); }
    }
    let basis = if device.curve_includes_clearing() { "total_clearing_curve" } else { "trip_curve_plus_interrupting_time" };
    Ok(DeviceExposure {
        seconds: raw,
        assumed,
        extra,
        effective_arc_ka: None,
        what: what_at("total clearing time"),
        basis,
        reasons,
    })
}

/// 60 Hz half-cycle and quarter-cycle durations. The model has no frequency field.
const HALF_CYCLE_S: f64 = 1.0 / 120.0;
const QUARTER_CYCLE_S: f64 = 1.0 / 240.0;

/// Duration from the half-cycle / quarter-cycle let-through table.
///
/// From `threshold_a` up to twice that current the time is one half-cycle.
/// At or above twice the threshold the time is one quarter-cycle. A stored
/// total-clearing time already faster than one half-cycle and drawn below
/// 0.01 s is kept. A melting time is not kept. Annex H incident-energy
/// equations are not applied.
fn let_through_table(device: &Device, limit: &crate::model::CurrentLimiting, amps: f64) -> (f64, bool, String, bool) {
    let twice = 2.0 * limit.threshold_a;
    let annex = " Annex H incident-energy equations are not applied. Energy uses the IEEE 1584-2018 model at this duration.";
    let ignored = if limit.clearing_s > 0.0 {
        " The entered manufacturer clearing time is not used."
    } else {
        ""
    };
    if device.curve_includes_clearing() {
        if let Some(stored) = curves::time_drawn_below_bottom(&device.curve, amps) {
            if stored < HALF_CYCLE_S {
                let mut assumed = false;
                let mut extra = format!(
                    " Current-limiting let-through at {amps:.0} A is at or above the {:.0} A threshold. The stored total-clearing time {stored:.6} s is already faster than one half-cycle and is drawn below 0.01 s, so that time is kept. The half-cycle and quarter-cycle table is not used.{ignored}{annex}",
                    limit.threshold_a
                );
                if let Some((band_assumed, note)) = curves::point_curve_note(&device.curve, amps) {
                    assumed |= band_assumed;
                    extra.push(' ');
                    extra.push_str(&note);
                }
                return (stored, assumed, extra, true);
            }
        }
    }
    let (seconds, band) = if amps < twice {
        (HALF_CYCLE_S, format!("from the {:.0} A threshold up to {twice:.0} A. The 60 Hz half-cycle time {HALF_CYCLE_S:.6} s is used.", limit.threshold_a))
    } else {
        (QUARTER_CYCLE_S, format!("at or above {twice:.0} A, twice the {:.0} A threshold. The 60 Hz quarter-cycle time {QUARTER_CYCLE_S:.6} s is used.", limit.threshold_a))
    };
    let extra = format!(" Current-limiting let-through at {amps:.0} A is {band} Clause 6.9.2 is not applied in this range.{ignored}{annex}");
    (seconds, true, extra, false)
}

fn device_clearing(project: &Project, device: &Device, amps: f64, sole_feed: bool) -> Result<Clearing, String> {
    let exposure = device_exposure(device, amps, sole_feed)?;
    let not_a_label = "This cap is not a field trip setting and is not for an arc-flash label.";
    let mut clear = finish_clearing(exposure.seconds, project.arc_duration_cap_s, project.escape_possible, &exposure.what, not_a_label);
    clear.assumed |= exposure.assumed;
    clear.effective_arc_ka = exposure.effective_arc_ka;
    clear.basis = exposure.basis;
    for code in exposure.reasons { clear.reason(code); }
    clear.note.push_str(&exposure.extra);
    Ok(clear)
}


fn device_current(project: &Project, device: &crate::model::Device, fault: &crate::fault::BusFault, fraction: f64) -> Option<f64> {
    device_current_from_terminals(project, device, &fault.terminal_currents, fraction)
}

fn device_current_from_terminals(project: &Project, device: &crate::model::Device,
    terminals: &[crate::fault::TerminalCurrent], fraction: f64) -> Option<f64> {
    let id = device.protected_branch.as_ref()?;
    let branch = project.branches.iter().find(|b| b.id == *id)?;
    let current = terminals.iter().find(|c| c.branch_id == *id)?;
    let (pre, inc) = if device.bus == branch.from { (current.from_prefault_a, current.from_increment_a) }
        else if device.bus == branch.to { (current.to_prefault_a, current.to_increment_a) }
        else { return None; };
    Some((pre[0] - fraction*inc[0]).hypot(pre[1] - fraction*inc[1]))
}

fn isolates_sources(project: &Project, device: &crate::model::Device, fault_bus: &str) -> bool {
    let seen = buses_after_device_opens(project, device, fault_bus);
    !project.sources.iter().any(|s| s.in_service && seen.contains(&s.bus))
}

/// A motor on the fault side of the selected breaker can keep feeding the arc
/// until its own feeder opens or its current decays. The present single-device
/// duration cannot claim that the main breaker's opening ends exposure.
fn connected_motor_ids(project: &Project, device: &crate::model::Device, fault_bus: &str) -> Vec<String> {
    let seen = buses_after_device_opens(project, device, fault_bus);
    let mut motors: Vec<String> = project.motors.iter()
        .filter(|motor| seen.contains(&motor.bus))
        .map(|motor| motor.id.clone()).collect();
    motors.sort_unstable();
    motors
}

fn reject_connected_motors(project: &Project, device: &crate::model::Device, fault_bus: &str) -> Result<(), String> {
    let motors = connected_motor_ids(project, device, fault_bus);
    if motors.is_empty() { return Ok(()); }
    Err(format!("opening upstream device '{}' leaves motor(s) {} connected to the fault at bus '{}'; motor feeder breaker operation and motor-current decay are not modeled as clearing events, so total arc duration is unresolved",
        device.id, motors.join(", "), fault_bus))
}

fn buses_after_device_opens(project: &Project, device: &crate::model::Device, fault_bus: &str) -> std::collections::HashSet<String> {
    let mut seen = std::collections::HashSet::from([fault_bus.to_string()]);
    loop {
        let old = seen.len();
        for b in &project.branches {
            if !project.branch_closed(b) { continue; }
            if device.protected_branch.as_ref() == Some(&b.id) { continue; }
            if seen.contains(&b.from) { seen.insert(b.to.clone()); }
            if seen.contains(&b.to) { seen.insert(b.from.clone()); }
        }
        if old == seen.len() { break; }
    }
    seen
}

fn finish_clearing(raw: f64, cap: f64, escape: bool, what: &str, not_a_label: &str) -> Clearing {
    if escape && raw > cap {
        Clearing {
            seconds: cap,
            capped: true,
            assumed: true,
            effective_arc_ka: None,
            basis: "",
            reasons: vec!["duration_capped"],
            note: format!("Arc duration is the {cap:.1} s cap. IEEE 1584-2018 Clause 6.9.1 allows this limit only when a person can move away. The {what} exceeds the cap. {not_a_label}"),
        }
    } else if !escape && raw > cap {
        Clearing {
            seconds: raw,
            capped: false,
            assumed: false,
            effective_arc_ka: None,
            basis: "",
            reasons: Vec::new(),
            note: format!("Arc duration {raw:.3} s is the {what}. The {cap:.1} s cap was not applied because escape is not assumed possible (IEEE 1584-2018 Clause 6.9.1)."),
        }
    } else {
        Clearing { seconds: raw, capped: false, assumed: false, effective_arc_ka: None, basis: "", reasons: Vec::new(), note: format!("Arc duration {raw:.3} s is the {what}.") }
    }
}




/// How one machine's share of a stage solution changes inside that stage.
/// The induction-motor window and the generator/synchronous-motor step-down
/// are separate fault solutions (see `MachineStages`); this rule only covers
/// what varies inside a stage: a source decrement curve and an entered motor
/// decay time constant.
#[derive(Clone, Copy)]
enum MachineRule {
    Constant,
    Decrement(usize),
    Synchronous { tau_s: Option<f64> },
    Induction { tau_s: Option<f64> },
}

struct StagedMachine {
    rule: MachineRule,
    label: String,
}

fn machine_scale(project: &Project, machine: &StagedMachine, time_s: f64) -> f64 {
    match machine.rule {
        MachineRule::Constant => 1.0,
        MachineRule::Decrement(index) => project.sources[index].decrement_ratio_at(time_s),
        MachineRule::Synchronous { tau_s } => tau_s.map_or(1.0, |tau| (-time_s / tau).exp()),
        MachineRule::Induction { tau_s } => if time_s >= project.motor_contribution_s - 1e-12 { 0.0 } else { tau_s.map_or(1.0, |tau| (-time_s / tau).exp()) },
    }
}

/// Machine-stage fault solutions, all from the same prefault and never
/// re-solved for a device opening: the base network; the network with every
/// induction motor removed (used from the motor window on); and, when the
/// step-down is enabled, the network with induction motors removed and each
/// stepped generator or synchronous motor reduced to its entered percent of
/// rated current (used from the step-down instant on). Each is solved once
/// at a flat 1.0 pu prefault; a fault increment is linear in the faulted
/// bus's prefault voltage, so every bus is scaled by its own study prefault.
struct AnsiDutyStages {
    base: crate::fault::FaultResult,
    no_induction: Option<crate::fault::FaultResult>,
}

struct MachineStages {
    base: crate::fault::FaultResult,
    no_induction: Option<crate::fault::FaultResult>,
    stepped: Option<crate::fault::FaultResult>,
    stepped_labels: Vec<String>,
    dropped_labels: Vec<String>,
    warnings: Vec<String>,
    /// ANSI only: first-cycle and interrupting stage pairs. When set, bus
    /// currents are taken from the pair selected by nominal voltage.
    ansi: Option<(AnsiDutyStages, AnsiDutyStages)>,
}

impl MachineStages {
    fn for_bus(&self, kv: f64) -> (&crate::fault::FaultResult, Option<&crate::fault::FaultResult>) {
        if let Some((first_cycle, interrupting)) = &self.ansi {
            let pair = if crate::machine::ansi_duty_for_kv(kv) == crate::machine::Duty::FirstCycle {
                first_cycle
            } else {
                interrupting
            };
            (&pair.base, pair.no_induction.as_ref())
        } else {
            (&self.base, self.no_induction.as_ref())
        }
    }
}

thread_local! {
    static STAGE_CACHE: std::cell::RefCell<Option<(String, std::rc::Rc<MachineStages>)>> = const { std::cell::RefCell::new(None) };
}

fn is_induction(motor: &crate::model::Motor) -> bool {
    motor.machine_kind != Some(crate::model::MotorKind::Synchronous)
}

fn solve_stage_pair(project: &Project) -> Result<(crate::fault::FaultResult, Option<crate::fault::FaultResult>), String> {
    let mut base_project = project.clone();
    base_project.prefault = Prefault::Flat;
    base_project.prefault_pu = 1.0;
    for bus in &mut base_project.buses { bus.prefault_pu = None; }
    let base = crate::fault::solve_uncorrected(&base_project, None)?;
    let mut without = base_project.clone();
    without.motors.retain(|motor| !is_induction(motor));
    let no_induction = if base_project.motors.iter().any(is_induction) {
        Some(crate::fault::solve_uncorrected(&without, None)?)
    } else {
        None
    };
    Ok((base, no_induction))
}

fn machine_stages(project: &Project) -> Result<std::rc::Rc<MachineStages>, String> {
    let key = serde_json::to_string(project).map_err(|error| error.to_string())?;
    if let Some(hit) = STAGE_CACHE.with(|cache| cache.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, stages)| stages.clone())) {
        return Ok(hit);
    }
    if project.fault_method == FaultMethod::Ansi {
        let first = crate::machine::duty_project(project, crate::machine::Duty::FirstCycle)
            .unwrap_or_else(|| project.clone());
        let interrupting = crate::machine::duty_project(project, crate::machine::Duty::Interrupting)
            .unwrap_or_else(|| project.clone());
        let (fc_base, fc_ni) = solve_stage_pair(&first)?;
        let (int_base, int_ni) = solve_stage_pair(&interrupting)?;
        let stages = std::rc::Rc::new(MachineStages {
            base: fc_base.clone(),
            no_induction: fc_ni.clone(),
            stepped: None,
            stepped_labels: Vec::new(),
            dropped_labels: Vec::new(),
            warnings: Vec::new(),
            ansi: Some((
                AnsiDutyStages { base: fc_base, no_induction: fc_ni },
                AnsiDutyStages { base: int_base, no_induction: int_ni },
            )),
        });
        STAGE_CACHE.with(|cache| *cache.borrow_mut() = Some((key, stages.clone())));
        return Ok(stages);
    }
    let mut base_project = project.clone();
    base_project.prefault = Prefault::Flat;
    base_project.prefault_pu = 1.0;
    for bus in &mut base_project.buses { bus.prefault_pu = None; }
    let base = crate::fault::solve_uncorrected(&base_project, None)?;
    let mut without = base_project.clone();
    without.motors.retain(|motor| !is_induction(motor));
    let no_induction = if base_project.motors.iter().any(is_induction) { Some(crate::fault::solve_uncorrected(&without, None)?) } else { None };
    let mut stepped = None;
    let mut stepped_labels = Vec::new();
    let mut dropped_labels = Vec::new();
    let mut warnings = Vec::new();
    if let Some(step) = &project.machine_step_down {
        let mut reduced = without.clone();
        let generators: Vec<usize> = reduced.sources.iter().enumerate()
            .filter(|(_, source)| source.in_service && source.decrement_curve.is_empty()
                && matches!(source.machine_kind, Some(kind) if kind != crate::model::GeneratorKind::Utility))
            .map(|(index, _)| index).collect();
        // A switch is off when no machine of that kind exists.
        if step.apply_to_generators && !generators.is_empty() {
            for index in generators {
                let source = &mut reduced.sources[index];
                let rated = source.generator.as_ref().map_or(0.0, |g| g.rated_mva);
                let target = step.percent_of_rated / 100.0 * rated;
                if target <= 1e-9 {
                    // Zero rating or 0%: the machine drops out at the step.
                    source.mva_sc = 1e-9;
                    dropped_labels.push(format!("source {}", source.id));
                } else {
                    if target > source.mva_sc {
                        warnings.push(format!("Source {}: the stepped-down contribution ({:.0}% of rated) is larger than its initial contribution; the entered value is used.", source.id, step.percent_of_rated));
                    }
                    source.mva_sc = target;
                }
                stepped_labels.push(format!("source {}", source.id));
            }
        }
        if step.apply_to_synchronous_motors {
            let percent = step.synchronous_percent();
            for motor in &mut reduced.motors {
                if percent <= 1e-9 {
                    motor.x_subtransient_pu = 1e9;
                    dropped_labels.push(format!("motor {}", motor.id));
                } else {
                    let x = 100.0 / percent * motor.x_subtransient_pu;
                    if x < motor.x_subtransient_pu {
                        warnings.push(format!("Motor {}: the stepped-down contribution ({:.0}% of rated) is larger than its initial contribution; the entered value is used.", motor.id, percent));
                    }
                    motor.x_subtransient_pu = x;
                }
                stepped_labels.push(format!("motor {}", motor.id));
            }
        }
        if !stepped_labels.is_empty() {
            stepped = Some(crate::fault::solve_uncorrected(&reduced, None)?);
        }
    }
    let stages = std::rc::Rc::new(MachineStages { base, no_induction, stepped, stepped_labels, dropped_labels, warnings, ansi: None });
    STAGE_CACHE.with(|cache| *cache.borrow_mut() = Some((key, stages.clone())));
    Ok(stages)
}

/// One contribution into the faulted bus: a connected branch, or a machine
/// on the faulted bus itself. Currents are the study-prefault-scaled phasors
/// from each machine-stage solution.
struct Stream<'a> {
    label: String,
    /// Base, no-induction, and stepped-down kA phasors into the faulted bus.
    base: [f64; 2],
    no_induction: [f64; 2],
    stepped: [f64; 2],
    /// Machines whose continuous rules (decrement, tau) still scale this stream.
    machines: Vec<(usize, [f64; 2])>,
    device: Option<&'a Device>,
    has_source: bool,
}

impl Stream<'_> {
    fn phasor_at(&self, project: &Project, machines: &[StagedMachine], time_s: f64) -> [f64; 2] {
        let stage = if project.machine_step_down.as_ref().is_some_and(|s| time_s >= s.after_cycles / 60.0 - 1e-12) {
            self.stepped
        } else if time_s >= project.motor_contribution_s - 1e-12 {
            self.no_induction
        } else {
            self.base
        };
        // Continuous rules (decrement curves, entered decay time constants)
        // still scale relative to the stage's t = 0 machines.
        let mut total = [0.0, 0.0];
        let mut scaled = [0.0, 0.0];
        for (index, current) in &self.machines {
            let k = machine_scale(project, &machines[*index], time_s);
            total = [total[0] + current[0], total[1] + current[1]];
            scaled = [scaled[0] + k * current[0], scaled[1] + k * current[1]];
        }
        let norm = total[0] * total[0] + total[1] * total[1];
        if norm < 1e-18 { return stage; }
        let factor = [(scaled[0] * total[0] + scaled[1] * total[1]) / norm, (scaled[1] * total[0] - scaled[0] * total[1]) / norm];
        cmul(stage, factor)
    }
}

fn cmul(a: [f64; 2], b: [f64; 2]) -> [f64; 2] { [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]] }
fn cabs(a: [f64; 2]) -> f64 { a[0].hypot(a[1]) }

struct OneSolutionExposure {
    full: Clearing,
    reduced: Clearing,
    full_segments: Vec<EnergySegment>,
    reduced_segments: Vec<EnergySegment>,
    full_device: Option<String>,
    reduced_device: Option<String>,
    time_varying: bool,
}

fn one_solution_exposure(project: &Project, case: &ArcEquipment, fault: &crate::fault::BusFault, bolted_ka: f64, template: &IeeeInput, decrement_source_ids: &[String], protection: &crate::protection::Protection, fixed: Option<(f64, f64)>) -> Result<OneSolutionExposure, String> {
    let gap = |detail: String| missing_arc_flash_message(&case.bus, &detail);
    let bus_kv = project.bus(&case.bus).map(|bus| bus.kv).ok_or_else(|| "unknown bus".to_string())?;
    let stages = machine_stages(project)?;
    let (stage_base, stage_no_induction) = stages.for_bus(bus_kv);
    let stage_bus = stage_base.buses.iter().find(|b| b.id == case.bus)
        .ok_or_else(|| format!("missing stage fault for bus '{}'", case.bus))?;
    // Machines listed on the base solution.
    let mut machines: Vec<StagedMachine> = Vec::new();
    let mut machine_bus: Vec<(String, [f64; 2], bool)> = Vec::new();
    for current in &stage_bus.machine_currents {
        let (rule, label, is_source) = if let Some(id) = &current.source_id {
            let index = project.sources.iter().position(|source| &source.id == id).ok_or_else(|| format!("unknown source '{id}'"))?;
            let source = &project.sources[index];
            let generator = matches!(source.machine_kind, Some(kind) if kind != crate::model::GeneratorKind::Utility);
            let rule = if !source.decrement_curve.is_empty() {
                MachineRule::Decrement(index)
            } else if generator {
                MachineRule::Synchronous { tau_s: None }
            } else {
                MachineRule::Constant
            };
            (rule, format!("source {id}"), true)
        } else {
            let id = current.motor_id.clone().unwrap_or_default();
            let motor = project.motors.iter().find(|motor| motor.id == id).ok_or_else(|| format!("unknown motor '{id}'"))?;
            let rule = if motor.machine_kind == Some(crate::model::MotorKind::Synchronous) {
                MachineRule::Synchronous { tau_s: motor.decay_time_constant_s }
            } else {
                MachineRule::Induction { tau_s: motor.decay_time_constant_s }
            };
            (rule, format!("motor {id}"), false)
        };
        machines.push(StagedMachine { rule, label });
        machine_bus.push((current.bus_id.clone(), current.current_pu, is_source));
    }
    let scale = fault.prefault_pu; // stages solved at 1.0 pu
    let located = located_devices(project);
    let mut streams: Vec<Stream> = Vec::new();
    for terminal in &stage_bus.terminal_currents {
        let Some(branch) = project.branches.iter().find(|branch| branch.id == terminal.branch_id) else { continue };
        let (near_end, far) = if branch.from == case.bus { (true, &branch.to) }
            else if branch.to == case.bus { (false, &branch.from) }
            else { continue };
        if !project.branch_closed(branch) { continue; }
        let take = |stage: &crate::fault::BusFault| -> [f64; 2] {
            let Some(t) = stage.terminal_currents.iter().find(|t| t.branch_id == branch.id) else { return [0.0, 0.0] };
            let a = if near_end { t.from_increment_a } else { t.to_increment_a };
            [a[0] / 1000.0 * scale, a[1] / 1000.0 * scale]
        };
        let base = take(stage_bus);
        let no_induction = stage_no_induction.and_then(|f| f.buses.iter().find(|b| b.id == case.bus)).map(take).unwrap_or(base);
        let stepped = stages.stepped.as_ref().and_then(|f| f.buses.iter().find(|b| b.id == case.bus)).map(take).unwrap_or(no_induction);
        let component = component_beyond(project, &case.bus, far);
        let behind: Vec<(usize, [f64; 2])> = machine_bus.iter().enumerate()
            .filter(|(_, (bus, _, _))| component.contains(bus))
            .map(|(index, (_, current, _))| (index, *current)).collect();
        let has_source = machine_bus.iter().any(|(bus, _, source)| *source && component.contains(bus));
        streams.push(Stream { label: format!("branch {}", branch.id), base, no_induction, stepped, machines: behind, device: None, has_source });
    }
    let ibase = crate::model::i_base_ka(bus_kv, project.s_base_mva);
    for (index, (bus, current, source)) in machine_bus.iter().enumerate() {
        if bus != &case.bus { continue; }
        let base = [current[0] * ibase * scale, current[1] * ibase * scale];
        // On-bus machines: their stage currents come from the stage's machine list.
        let take_m = |stage: &crate::fault::FaultResult| -> [f64; 2] {
            let Some(b) = stage.buses.iter().find(|b| b.id == case.bus) else { return [0.0, 0.0] };
            let Some(m) = b.machine_currents.iter().find(|m| {
                (m.source_id.as_ref().map(|s| format!("source {s}")) == Some(machines[index].label.clone()))
                || (m.motor_id.as_ref().map(|s| format!("motor {s}")) == Some(machines[index].label.clone()))
            }) else { return [0.0, 0.0] };
            [m.current_pu[0] * ibase * scale, m.current_pu[1] * ibase * scale]
        };
        let no_induction = stage_no_induction.map(|f| take_m(f)).unwrap_or(base);
        let stepped = stages.stepped.as_ref().map(|f| take_m(f)).unwrap_or(no_induction);  // falls back to no_induction/base
        streams.push(Stream { label: machines[index].label.clone(), base, no_induction, stepped, machines: vec![(index, *current)], device: None, has_source: *source });
    }
    let total = streams.iter().fold([0.0, 0.0], |sum, stream| [sum[0] + stream.base[0], sum[1] + stream.base[1]]);
    let total_abs = cabs(total);
    if !(total_abs.is_finite() && total_abs > 0.0) {
        let deenergized = !project.sources.iter().any(|s| s.in_service && reachable_buses(project, &case.bus).contains(&s.bus));
        return Err(if deenergized {
            format!("bus '{}' is out of service: zero three-phase fault current (de-energized)", case.bus)
        } else {
            format!("bus '{}' fault current is zero", case.bus)
        });
    }
    streams.retain(|stream| cabs(stream.base) > 1e-9 * total_abs);

    // Assign each branch its fastest located device (lowest total clearing
    // time at the base arcing current; ties broken by the uncapped time).
    let arcing0 = {
        let out = arcflash::calculate(template)?;
        out.i_arc_ka
    };
    let fraction0 = arcing0 / bolted_ka;
    assign_stream_devices(project, case, fault, &mut streams, &located, fraction0, fixed.is_some())?;

    if fixed.is_none() && streams.iter().all(|stream| stream.device.is_none()) {
        return Err(missing_arc_flash_message(&case.bus, &format!("an upstream device that isolates this bus. {}", protection.detail)));
    }
    for stream in &streams {
        if fixed.is_none() && stream.has_source && stream.device.is_none() {
            return Err(format!("opening the located devices does not isolate every utility/generator path to bus '{}'; {} has no isolating device, so multiple-device clearing is unresolved", case.bus, stream.label));
        }
    }
    let time_varying = !decrement_source_ids.is_empty()
        || machines.iter().any(|m| matches!(m.rule, MachineRule::Induction { tau_s: Some(_) } | MachineRule::Synchronous { tau_s: Some(_) }))
        || (project.machine_step_down.is_some() && !stages.stepped_labels.is_empty())
        || stages.no_induction.is_some();
    if let Some((full_s, reduced_s)) = fixed {
        let profile = |duration_s: f64| fixed_profile(project, &streams, &machines, total, bolted_ka, decrement_source_ids, duration_s);
        let empty = || Clearing { seconds: 0.0, capped: false, assumed: false, effective_arc_ka: None, basis: "", reasons: Vec::new(), note: String::new() };
        return Ok(OneSolutionExposure {
            full: empty(), reduced: empty(),
            full_segments: profile(full_s)?, reduced_segments: profile(reduced_s)?,
            full_device: None, reduced_device: None, time_varying,
        });
    }
    let run = |reduced: bool| -> Result<(Clearing, Vec<EnergySegment>, Option<String>), String> {
        stage_streams(project, case, fault, bolted_ka, template, reduced, &streams, &machines, total, decrement_source_ids, &stages, gap)
    };
    let full = run(false)?;
    let preview = arcflash::calculate(template)?;
    let reduced = if (preview.i_arc_ka - preview.i_arc_min_ka).abs() < 1e-12 { full.clone() } else { run(true)? };
    Ok(OneSolutionExposure {
        full: full.0, reduced: reduced.0,
        full_segments: full.1, reduced_segments: reduced.1,
        full_device: full.2, reduced_device: reduced.2, time_varying,
    })
}

fn assign_stream_devices<'a>(project: &'a Project, case: &ArcEquipment, fault: &crate::fault::BusFault, streams: &mut [Stream<'a>], located: &[&'a Device], fraction0: f64, skip: bool) -> Result<(), String> {
    if skip { return Ok(()); }
    // Prefer the selected upstream only for the "isolates nothing" check: the
    // selection itself no longer prefers that device for timing.
    if let Some(id) = &case.upstream_device {
        if let Some(device) = located.iter().copied().find(|d| d.id == *id) {
            if !streams.iter().any(|s| s.label.starts_with("branch ") && {
                let far = project.branches.iter().find(|b| format!("branch {}", b.id) == s.label).map(|b| if b.from == case.bus { &b.to } else { &b.from });
                far.is_some_and(|far| device_disconnects(project, device, &case.bus, far))
            }) {
                return Err(format!("opening the selected device {} [{}] does not disconnect a utility, generator, or motor from bus '{}'; multiple-device clearing cannot use a device that isolates nothing", device.name, device.id, case.bus));
            }
        } else {
            return Err(missing_arc_flash_message(&case.bus, &format!("a closed protected branch for upstream device [{id}]")));
        }
    }
    for stream in streams.iter_mut() {
        if !stream.label.starts_with("branch ") { continue; }
        let branch_id = &stream.label["branch ".len()..];
        let Some(branch) = project.branches.iter().find(|b| b.id == branch_id) else { continue };
        let far = if branch.from == case.bus { &branch.to } else { &branch.from };
        let mut candidates: Vec<&Device> = located.iter().copied().filter(|d| device_disconnects(project, d, &case.bus, far)).collect();
        if candidates.is_empty() { continue; }
        // Sort by total clearing time (lowest first); ties by uncapped time, then id.
        let mut scored: Vec<(f64, f64, &Device)> = Vec::new();
        for device in candidates.drain(..) {
            let Some(amps) = device_current(project, device, fault, fraction0) else { continue };
            match device_exposure(device, amps, false) {
                Ok(exposure) if exposure.seconds.is_finite() && exposure.seconds > 0.0 => {
                    let uncapped = exposure.seconds; // same; finish_clearing not applied here
                    scored.push((exposure.seconds, uncapped, device));
                }
                _ => {}
            }
        }
        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.2.id.cmp(&b.2.id)));
        // When no candidate has a finite clearing time at the initial current
        // (for example a relay below pickup), keep the nearest hop so the
        // continuous retime path can still try to clear on the decaying current.
        let near = if let Some((_, _, device)) = scored.first().copied() {
            device
        } else {
            // A motor-only path drops out with the motor window, so a device
            // with no curve is not forced onto it.
            let has_source = stream.has_source;
            let mut by_hop: Vec<&Device> = located.iter().copied()
                .filter(|d| device_disconnects(project, d, &case.bus, far))
                .filter(|d| has_source || !matches!(d.curve, crate::model::CurveSpec::SettingsNotCollected { .. }))
                .collect();
            by_hop.sort_by(|a, b| device_hops(project, &case.bus, a).cmp(&device_hops(project, &case.bus, b)).then(a.id.cmp(&b.id)));
            let Some(device) = by_hop.first().copied() else { continue };
            device
        };
        let mut chosen = near;
        if let Some(backup) = &project.upstream_backup {
            if let Some(up) = upstream_backup_device(project, case, fault, near, far, located, fraction0, backup) {
                chosen = up;
            }
        }
        stream.device = Some(chosen);
    }
    Ok(())
}

fn upstream_backup_device<'a>(project: &'a Project, case: &ArcEquipment, fault: &crate::fault::BusFault, near: &'a Device, far: &str, located: &[&'a Device], fraction0: f64, backup: &crate::model::UpstreamBackup) -> Option<&'a Device> {
    let near_amps = device_current(project, near, fault, fraction0)?;
    let near_time = device_exposure(near, near_amps, false).ok()?.seconds;
    // Walk device-levels away from the fault bus along the near device's branch.
    let mut frontier = vec![near.bus.clone()];
    let mut seen = std::collections::HashSet::from([near.bus.clone(), case.bus.clone()]);
    let mut best: Option<(f64, &Device)> = None;
    for _ in 0..backup.levels {
        let mut next = Vec::new();
        for _bus in &frontier {
            for device in located.iter().copied() {
                if seen.contains(&device.bus) { continue; }
                if device.fuse_total_clearing || device.curve_is_total_clearing || curves::average_melt_fuse(device) { continue; }
                if device.current_limiting.is_some() { continue; }
                if !device_disconnects(project, device, &case.bus, far) { continue; }
                // Same branch level: only consider devices that sit further from the fault than `near`.
                if device_hops(project, &case.bus, device) <= device_hops(project, &case.bus, near) { continue; }
                let Some(amps) = device_current(project, device, fault, fraction0) else { continue };
                if amps + 1e-9 < backup.ratio * near_amps { continue; }
                let Ok(exposure) = device_exposure(device, amps, false) else { continue };
                if !(exposure.seconds.is_finite() && exposure.seconds > 0.0 && exposure.seconds < near_time - 1e-9) { continue; }
                if best.as_ref().is_none_or(|(t, d)| exposure.seconds < *t - 1e-12 || ((exposure.seconds - *t).abs() < 1e-12 && device.id < d.id)) {
                    best = Some((exposure.seconds, device));
                }
                next.push(device.bus.clone());
                seen.insert(device.bus.clone());
            }
        }
        if next.is_empty() { break; }
        frontier = next;
    }
    best.map(|(_, d)| d)
}

/// Bus current over an entered duration from the machine-stage solutions,
/// with continuous machine rules applied and no device opening.
fn fixed_profile(project: &Project, streams: &[Stream], machines: &[StagedMachine], total: [f64; 2], bolted_ka: f64, decrement_source_ids: &[String], duration_s: f64) -> Result<Vec<EnergySegment>, String> {
    let total_abs = cabs(total);
    let mut knots: Vec<f64> = Vec::new();
    if machines.iter().any(|m| matches!(m.rule, MachineRule::Induction { .. })) {
        knots.push(project.motor_contribution_s);
    }
    if let Some(step) = &project.machine_step_down { knots.push(step.after_cycles / 60.0); }
    let mut segments = Vec::new();
    let mut t = 0.0;
    while duration_s - t > 1e-12 {
        if segments.len() >= 10_000 {
            return Err("arc profile exceeds 10,000 slices; reduce the arc duration".into());
        }
        let knot = knots.iter().copied().filter(|k| *k > t + 1e-9).fold(duration_s, f64::min);
        let end = next_decrement_end(project, decrement_source_ids, t, knot);
        let midpoint = t + (end - t) / 2.0;
        let now = streams.iter().fold([0.0, 0.0], |sum, stream| { let p = stream.phasor_at(project, machines, midpoint); [sum[0] + p[0], sum[1] + p[1]] });
        segments.push(EnergySegment { ibf_ka: bolted_ka * cabs(now) / total_abs, duration_s: end - t });
        t = end;
    }
    Ok(segments)
}

/// Buses reached from `start` over closed branches without passing `fault_bus`.
fn component_beyond(project: &Project, fault_bus: &str, start: &str) -> std::collections::HashSet<String> {
    let mut seen = std::collections::HashSet::from([start.to_string()]);
    let mut queue = vec![start.to_string()];
    while let Some(bus) = queue.pop() {
        for branch in project.branches.iter().filter(|branch| project.branch_closed(branch)) {
            let next = if branch.from == bus { &branch.to } else if branch.to == bus { &branch.from } else { continue };
            if next == fault_bus || seen.contains(next) { continue; }
            seen.insert(next.clone());
            queue.push(next.clone());
        }
    }
    seen
}

struct StreamDevice<'a> {
    device: &'a Device,
    open_s: Option<f64>,
    /// True when this device was timed as a fuse / total-clearing / limiting
    /// device and must keep its base-solution time even when retiming.
    static_time: bool,
    /// Present when retime is on and the device follows a trip-only curve; used
    /// for continuous advance under a source decrement curve.
    relay: Option<curves::DynamicRelay>,
    interrupting_s: f64,
    prior_amps: f64,
    initial_amps: f64,
}

#[allow(clippy::too_many_arguments)]
fn stage_streams(project: &Project, case: &ArcEquipment, fault: &crate::fault::BusFault, bolted_ka: f64, template: &IeeeInput, reduced: bool,
    streams: &[Stream], machines: &[StagedMachine], total: [f64; 2], decrement_source_ids: &[String], stages: &MachineStages,
    gap: impl Fn(String) -> String) -> Result<(Clearing, Vec<EnergySegment>, Option<String>), String> {
    let total_abs = cabs(total);
    let arcing_of = |ibf_ka: f64| -> Result<f64, String> {
        let out = arcflash::calculate(&IeeeInput { ibf_ka, ..template.clone() })?;
        Ok(if reduced { out.i_arc_min_ka } else { out.i_arc_ka })
    };
    let current_case = if reduced { "reduced" } else { "full" };
    let fraction0 = arcing_of(bolted_ka)? / bolted_ka;
    let retime = project.retime_on_reduced_current;
    let mut reasons: Vec<&'static str> = Vec::new();
    let mut extras: Vec<String> = Vec::new();
    let mut assumed = false;
    let mut static_names: Vec<String> = Vec::new();
    let mut basis_single: Option<&'static str> = None;
    let mut devices: Vec<StreamDevice> = Vec::new();
    for stream in streams {
        let Some(device) = stream.device else { continue };
        if devices.iter().any(|d| d.device.id == device.id) { continue; }
        let amps = device_current(project, device, fault, fraction0).ok_or_else(|| gap(format!("the current through protected branch '{}' of {} [{}]", device.protected_branch.as_deref().unwrap_or(""), device.name, device.id)))?;
        let limiting = device.current_limiting.as_ref().is_some_and(|limit| amps >= limit.threshold_a);
        let is_static = device.fuse_total_clearing || device.curve_is_total_clearing || curves::average_melt_fuse(device) || limiting;
        let follows = retime && !is_static && device.breaker_interrupting_s.is_some();
        let relay = if follows { curves::DynamicRelay::new(&device.curve).ok() } else { None };
        let exposure = device_exposure(device, amps, false);
        match exposure {
            Ok(exposure) if exposure.seconds.is_finite() && exposure.seconds > 0.0 => {
                assumed |= exposure.assumed;
                for code in exposure.reasons { if !reasons.contains(&code) { reasons.push(code); } }
                if !exposure.extra.is_empty() { extras.push(exposure.extra); }
                basis_single = Some(exposure.basis);
                if retime && is_static { static_names.push(format!("{} [{}]", device.name, device.id)); }
                let open_s = if relay.is_some() && !decrement_source_ids.is_empty() { None } else { Some(exposure.seconds) };
                devices.push(StreamDevice {
                    device, open_s, static_time: is_static, relay,
                    interrupting_s: device.breaker_interrupting_s.unwrap_or(0.0),
                    prior_amps: amps, initial_amps: amps,
                });
            }
            Ok(_) | Err(_) if relay.is_some() => {
                // Below pickup at the initial current: continuous retime may still clear.
                devices.push(StreamDevice {
                    device, open_s: None, static_time: false, relay,
                    interrupting_s: device.breaker_interrupting_s.unwrap_or(0.0),
                    prior_amps: amps, initial_amps: amps,
                });
            }
            Ok(_) => {
                return Err(format!("clearing time of {} [{}] at bus '{}' is not a positive finite duration", device.name, device.id, case.bus));
            }
            Err(detail) => return Err(gap(detail)),
        }
    }
    // Two one-time retimes, ordered by time: at the motor window, then at the
    // step-down. Each uses that stage's solution currents and only retimes
    // devices that have not yet cleared. Fuses keep their base time.
    if retime {
        let mut events: Vec<(f64, fn(&Stream) -> [f64; 2])> = Vec::new();
        if stages.no_induction.is_some() {
            events.push((project.motor_contribution_s, |s: &Stream| s.no_induction));
        }
        if stages.stepped.is_some() {
            if let Some(step) = &project.machine_step_down {
                events.push((step.after_cycles / 60.0, |s: &Stream| s.stepped));
            }
        }
        events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (at, pick) in events {
            // Stage bolted = |sum of stage stream phasors|.
            let stage_total = streams.iter().fold([0.0, 0.0], |sum, s| { let p = pick(s); [sum[0] + p[0], sum[1] + p[1]] });
            let stage_abs = cabs(stage_total);
            if stage_abs <= 1e-12 * total_abs { continue; }
            let stage_bolted = bolted_ka * stage_abs / total_abs;
            let stage_fraction = arcing_of(stage_bolted)? / stage_bolted;
            for d in devices.iter_mut() {
                if d.static_time { continue; }
                if d.open_s.is_some_and(|s| s <= at + 1e-9) { continue; }
                let carried: Vec<&Stream> = streams.iter().filter(|s| s.device.is_some_and(|dev| dev.id == d.device.id)).collect();
                let base = carried.iter().fold(0.0, |sum, s| sum + cabs(s.base));
                let now = carried.iter().fold(0.0, |sum, s| sum + cabs(pick(s)));
                if base <= 1e-18 { continue; }
                let amps = d.initial_amps * (now / base) * (stage_fraction / fraction0);
                let exposure = device_exposure(d.device, amps, false).map_err(|detail| gap(detail))?;
                if !exposure.seconds.is_finite() || exposure.seconds <= 0.0 { continue; }
                // Trip time is measured from the event; add the interrupting
                // time that was already in the exposure (device_exposure
                // already includes breaker_interrupting_s for relays).
                let new_open = at + exposure.seconds;
                if d.open_s.is_none_or(|s| new_open + 1e-9 < s) {
                    d.open_s = Some(new_open);
                }
            }
        }
    }
    let motor_streams = streams.iter().any(|stream| stream.device.is_none());
    let staged = devices.len() > 1 || motor_streams;
    let continuous = !decrement_source_ids.is_empty()
        || machines.iter().any(|m| matches!(m.rule, MachineRule::Induction { tau_s: Some(_) } | MachineRule::Synchronous { tau_s: Some(_) }))
        || devices.iter().any(|d| d.relay.is_some() && d.open_s.is_none());
    let mut knots: Vec<f64> = Vec::new();
    if stages.no_induction.is_some() { knots.push(project.motor_contribution_s); }
    if stages.stepped.is_some() {
        if let Some(step) = &project.machine_step_down { knots.push(step.after_cycles / 60.0); }
    }
    let cap = project.arc_duration_cap_s;
    let limit_s = exposure_limit_s(project, decrement_source_ids);
    // Sort devices by clearing time, name as tiebreak.
    devices.sort_by(|a, b| {
        let ta = a.open_s.unwrap_or(f64::INFINITY);
        let tb = b.open_s.unwrap_or(f64::INFINITY);
        ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal).then(a.device.id.cmp(&b.device.id))
    });
    let remaining_at = |cleared: &[bool], time_s: f64| -> [f64; 2] {
        streams.iter().zip(cleared).filter(|(_, done)| !**done)
            .fold([0.0, 0.0], |sum, (stream, _)| { let p = stream.phasor_at(project, machines, time_s); [sum[0] + p[0], sum[1] + p[1]] })
    };
    let mut cleared = vec![false; streams.len()];
    let mut segments: Vec<EnergySegment> = Vec::new();
    let mut t = 0.0;
    let mut last_device: Option<String> = None;
    let mut governing: Option<String> = None;
    let mut crossing: Option<f64> = None;
    let mut stopped_at_cap = false;
    let mut ended_at_max = false;
    let mut trips: Vec<String> = Vec::new();
    let mut reset_matters = false;
    // Threshold: limit = (1 - fraction) * |bus total| using magnitudes.
    let remainder_limit = (1.0 - project.cleared_fault_fraction) * total_abs;
    // Process openings in trip order.
    let mut open_index = 0;
    loop {
        // Apply openings at t. The cleared-current threshold is checked only
        // after a device opens; natural decay alone does not extinguish the arc.
        let mut opened_here = false;
        while open_index < devices.len() {
            let open_s = devices[open_index].open_s.unwrap_or(f64::INFINITY);
            if open_s > t + 1e-9 { break; }
            let id = devices[open_index].device.id.clone();
            for (index, stream) in streams.iter().enumerate() {
                if !cleared[index] && stream.device.is_some_and(|d| d.id == id) { cleared[index] = true; }
            }
            last_device = Some(id);
            open_index += 1;
            opened_here = true;
        }
        let remaining = cabs(remaining_at(&cleared, t + 1e-9));
        if opened_here && remaining <= remainder_limit + 1e-9 * total_abs {
            if remaining > 1e-9 * total_abs {
                crossing = Some(1.0 - remaining / total_abs);
                governing = last_device.clone();
            }
            break;
        }
        if remaining <= 1e-9 * total_abs { break; }
        if project.escape_possible && t >= cap - 1e-12 { stopped_at_cap = true; break; }
        if t >= limit_s - 1e-12 && !continuous_decay_left(project, machines, t) {
            for d in devices.iter().filter(|d| d.relay.is_some() && d.open_s.is_none()) {
                if let Some(relay) = &d.relay {
                    if !relay.timing_at(d.prior_amps).unwrap_or(false) {
                        return Err(gap(format!("a trip of {} [{}] within the {limit_s:.3} s study cap. Under the {current_case} arcing current it is below every entered pickup at {:.0} A at the cap, so total clearing is unresolved", d.device.name, d.device.id, d.prior_amps)));
                    }
                }
            }
            ended_at_max = true;
            break;
        }
        if segments.len() >= 10_000 {
            return Err("staged arc exceeds 10,000 slices; reduce the arc duration or provide a shorter exposure limit".into());
        }
        let next_open = devices.iter().skip(open_index).filter_map(|d| d.open_s.filter(|s| *s > t + 1e-9)).fold(f64::INFINITY, f64::min);
        let next_knot = knots.iter().copied().filter(|k| *k > t + 1e-9).fold(f64::INFINITY, f64::min);
        let mut horizon = next_open.min(next_knot);
        if project.escape_possible { horizon = horizon.min(cap); }
        if !horizon.is_finite() {
            if continuous {
                horizon = next_decrement_end(project, decrement_source_ids, t, t + DECREMENT_STEP_S);
            } else {
                ended_at_max = true;
                break;
            }
        }
        let end = if continuous { next_decrement_end(project, decrement_source_ids, t, horizon) } else { horizon };
        if !end.is_finite() || end <= t {
            if t >= limit_s - 1e-12 {
                ended_at_max = true;
                break;
            }
            return Err(gap(format!("a device that clears the remaining contribution into this bus under the {current_case} arcing current. After {t:.3} s the remaining current does not decay and no device remains to open, so arc clearing is unresolved")));
        }
        let width = end - t;
        let midpoint = t + width / 2.0;
        let ibf_ka = bolted_ka * cabs(remaining_at(&cleared, midpoint)) / total_abs;
        let pending_relays = devices.iter().any(|d| d.relay.is_some() && d.open_s.is_none());
        if pending_relays {
            let fraction = if ibf_ka > 0.0 { arcing_of(ibf_ka)? / ibf_ka } else { 0.0 };
            for d in devices.iter_mut().filter(|d| d.relay.is_some() && d.open_s.is_none()) {
                let carried: Vec<&Stream> = streams.iter().filter(|s| s.device.is_some_and(|dev| dev.id == d.device.id)).collect();
                let base = carried.iter().fold(0.0, |sum, s| sum + cabs(s.base));
                let now = carried.iter().fold(0.0, |sum, s| sum + cabs(s.phasor_at(project, machines, midpoint)));
                let amps = if base > 0.0 { d.initial_amps * (now / base) * (fraction / fraction0) } else { 0.0 };
                let relay = d.relay.as_mut().unwrap();
                let step = advance_relay_over_ramp(relay, d.prior_amps, amps, width)
                    .map_err(|error| gap(format!("a trip time for {} [{}] on the reduced current. {error}", d.device.name, d.device.id)))?;
                d.prior_amps = amps;
                if let Some(within_s) = step {
                    let trip_s = t + within_s;
                    d.open_s = Some((trip_s + d.interrupting_s).max(end));
                    trips.push(format!("{} [{}] relay trip at {trip_s:.3} s plus entered breaker interruption of {:.3} s", d.device.name, d.device.id, d.interrupting_s));
                    reset_matters |= relay.reset_affects_result();
                } else if t >= limit_s - 1e-12 && !relay.timing_at(amps).unwrap_or(false) && !continuous_decay_left(project, machines, end) {
                    return Err(gap(format!("a trip of {} [{}] within the {limit_s:.3} s study cap. Under the {current_case} arcing current it is below every entered pickup at {amps:.0} A at the cap, so total clearing is unresolved", d.device.name, d.device.id)));
                }
            }
            devices.sort_by(|a, b| {
                let ta = a.open_s.unwrap_or(f64::INFINITY);
                let tb = b.open_s.unwrap_or(f64::INFINITY);
                ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal).then(a.device.id.cmp(&b.device.id))
            });
        }
        // Before the study cap, a pending relay that never picked up must fail.
        if project.escape_possible && end >= cap - 1e-12 {
            for d in devices.iter().filter(|d| d.relay.is_some() && d.open_s.is_none()) {
                if let Some(relay) = &d.relay {
                    if !relay.timing_at(d.prior_amps).unwrap_or(false) && !continuous_decay_left(project, machines, end) {
                        return Err(gap(format!("a trip of {} [{}] within the {cap:.3} s study cap. Under the {current_case} arcing current it is below every entered pickup at {:.0} A at the cap, so total clearing is unresolved", d.device.name, d.device.id, d.prior_amps)));
                    }
                }
            }
        }
        segments.push(EnergySegment { ibf_ka, duration_s: width });
        t = end;
    }
    if segments.is_empty() && t <= 1e-12 {
        // Instantaneous clearing: one zero-width is not useful; invent a tiny slice.
        segments.push(EnergySegment { ibf_ka: bolted_ka, duration_s: 1e-6 });
        t = 1e-6;
    }
    if segments.is_empty() {
        return Err(format!("staged arc at bus '{}' produced no current interval", case.bus));
    }
    let not_a_label = "This cap is not a field trip setting and is not for an arc-flash label.";
    let timing = devices.iter().any(|d| d.relay.is_some() && d.open_s.is_none());
    let mut clear = if stopped_at_cap {
        finish_clearing(f64::INFINITY, cap, true, if timing { "clearing, with a relay still timing on the reduced current," } else { "clearing still in progress" }, not_a_label)
    } else if ended_at_max {
        let end_t = t.min(limit_s);
        let mut c = finish_clearing(end_t, cap, project.escape_possible, &format!("clearing time of {end_t:.3} s from the one fault solution"), not_a_label);
        c.note.push_str(&format!(" The cleared-current threshold was not reached by {end_t:.3} s, so the arc ends at the study's maximum trip time."));
        c
    } else {
        finish_clearing(t, cap, project.escape_possible, &format!("clearing time of {t:.3} s from the one fault solution"), not_a_label)
    };
    if clear.capped { truncate_segments(&mut segments, clear.seconds); }
    clear.assumed |= assumed || staged;
    clear.basis = if !staged { basis_single.unwrap_or("trip_curve_plus_interrupting_time") } else if retime { "staged_decaying_current" } else { "staged_multi_device" };
    if staged { clear.reason(if retime { "staged_decaying_current" } else { "staged_clearing_assumed" }); }
    for code in reasons { clear.reason(code); }
    if !static_names.is_empty() { clear.reason("static_device_timing"); }
    let decaying: Vec<&str> = machines.iter().filter(|m| matches!(m.rule, MachineRule::Induction { tau_s: Some(_) } | MachineRule::Synchronous { tau_s: Some(_) })).map(|m| m.label.as_str()).collect();
    if !decaying.is_empty() { clear.reason("motor_decay_assumed"); }
    if !stages.stepped_labels.is_empty() { clear.reason("machine_step_down_assumed"); }
    let mut note = String::new();
    if staged {
        note.push_str(&format!(" The arc is staged from one fault solution with machine-stage variants (IEEE 1584-2018 Clause 6.9): each contribution into the bus is removed when its own device opens, and a device opening does not re-solve the network. {}", if retime {
            "Devices that are not fuses are re-timed on the reduced current (once at the motor window and once at the step-down; continuously under a source decrement curve)."
        } else {
            "Each device is timed once at its initial arcing current from the base solution."
        }));
    }
    if !trips.is_empty() { note.push_str(&format!(" {}.", trips.join("; "))); }
    if reset_matters {
        clear.reason("relay_reset_assumed");
        clear.assumed = true;
        note.push_str(" Relay progress was reset immediately when the current fell below pickup; verify the device reset setting.");
    }
    if !static_names.is_empty() {
        note.push_str(&format!(" {} keep one time read at the initial arcing current: a fuse, a total-clearing curve, or a current-limiting let-through time does not by itself establish clearing on a reduced current.", static_names.join(", ")));
    }
    let induction: Vec<&str> = machines.iter().filter(|m| matches!(m.rule, MachineRule::Induction { .. })).map(|m| m.label.as_str()).collect();
    if !induction.is_empty() {
        note.push_str(&format!(" Induction motor contribution ({}) is removed at {:.1} cycles by a separate fault solution with those motors taken out; the window is assumed.", induction.join(", "), project.motor_contribution_s * 60.0));
    }
    if !decaying.is_empty() {
        note.push_str(&format!(" {} decay as exp(-t / decay_time_constant_s); the time constant is entered and assumed.", decaying.join(", ")));
    }
    if let Some(step) = &project.machine_step_down {
        if !stages.stepped_labels.is_empty() {
            note.push_str(&format!(" {} step down to an entered percent of rated current after {:.1} cycles, using a separate fault solution.", stages.stepped_labels.join(", "), step.after_cycles));
        }
        if !stages.dropped_labels.is_empty() {
            note.push_str(&format!(" {} drop out at the step (zero rating or 0% of rated).", stages.dropped_labels.join(", ")));
        }
    }
    for warning in &stages.warnings {
        note.push(' ');
        note.push_str(warning);
    }
    if !decrement_source_ids.is_empty() {
        note.push_str(" Source contribution follows its entered decrement curve; source X/R is held constant. A source with an entered decrement curve is not stepped down.");
    }
    for extra in extras { note.push_str(&extra); }
    if let Some(share) = crossing {
        clear.reason("remaining_current_excluded");
        let left: Vec<&str> = streams.iter().zip(&cleared).filter(|(_, done)| !**done).map(|(s, _)| s.label.as_str()).collect();
        let gov = governing.clone().or_else(|| last_device.clone()).unwrap_or_default();
        note.push_str(&format!(" The arc is taken as extinguished at {t:.3} s, when {} [{}] brought the remaining bus fault current to or below the {:.0}% threshold (cleared share {:.0}%). The remaining contribution ({}) is excluded.",
            project.device(gov.as_str()).map(|d| d.name.as_str()).unwrap_or(""), gov, (1.0 - project.cleared_fault_fraction) * 100.0, share * 100.0, left.join(", ")));
        last_device = Some(gov);
    } else if governing.is_none() {
        // Governing device is the last in the trip list when the threshold
        // was never crossed (everything cleared, or max trip time).
        governing = last_device.clone();
    }
    clear.note.push_str(&note);
    Ok((clear, segments, governing.or(last_device)))
}

fn continuous_decay_left(project: &Project, machines: &[StagedMachine], time_s: f64) -> bool {
    machines.iter().any(|m| match m.rule {
        MachineRule::Induction { tau_s: Some(_) } => time_s < project.motor_contribution_s,
        MachineRule::Synchronous { tau_s: Some(_) } => true,
        MachineRule::Decrement(index) => project.sources[index].decrement_curve.last().is_some_and(|p| time_s < p.time_s),
        _ => false,
    })
}


fn truncate_segments(segments: &mut Vec<EnergySegment>, cap: f64) {
    let mut kept = Vec::new();
    let mut cursor = 0.0;
    for segment in segments.drain(..) {
        if cursor >= cap - 1e-12 { break; }
        let width = segment.duration_s.min(cap - cursor);
        if width > 1e-12 {
            kept.push(EnergySegment { ibf_ka: segment.ibf_ka, duration_s: width });
        }
        cursor += segment.duration_s;
    }
    *segments = kept;
}


fn located_devices(project: &Project) -> Vec<&Device> {
    project.devices.iter().filter(|device| {
        let Some(branch) = device.protected_branch.as_deref().and_then(|id| project.branches.iter().find(|branch| branch.id == id)) else {
            return false;
        };
        project.branch_closed(branch) && (device.bus == branch.from || device.bus == branch.to)
    }).collect()
}

/// True when opening `device` stops the path from `target_bus` feeding the
/// fault: either the target bus itself is cut off, or every in-service source
/// and motor bus beyond it is. The second case is a device further out on the
/// path, such as a primary fuse feeding a transformer whose secondary is faulted.
fn device_disconnects(project: &Project, device: &Device, fault_bus: &str, target_bus: &str) -> bool {
    let before = reachable_buses(project, fault_bus);
    if !before.contains(target_bus) {
        return false;
    }
    let after = buses_after_device_opens(project, device, fault_bus);
    if !after.contains(target_bus) {
        return true;
    }
    let beyond = component_beyond(project, fault_bus, target_bus);
    let feeding: Vec<&str> = project.sources.iter().filter(|s| s.in_service).map(|s| s.bus.as_str())
        .chain(project.motors.iter().map(|m| m.bus.as_str()))
        .filter(|bus| *bus != fault_bus && beyond.contains(*bus))
        .collect();
    !feeding.is_empty() && feeding.iter().all(|bus| !after.contains(*bus))
}

fn device_hops(project: &Project, fault_bus: &str, device: &Device) -> usize {
    let distance = bus_distance(project, fault_bus);
    let Some(branch) = device.protected_branch.as_deref().and_then(|id| project.branches.iter().find(|branch| branch.id == id)) else {
        return usize::MAX;
    };
    distance.get(&branch.from).copied().unwrap_or(usize::MAX).min(distance.get(&branch.to).copied().unwrap_or(usize::MAX))
}

fn reachable_buses(project: &Project, start: &str) -> std::collections::HashSet<String> {
    bus_distance(project, start).into_keys().collect()
}

fn bus_distance(project: &Project, start: &str) -> std::collections::HashMap<String, usize> {
    let mut distance = std::collections::HashMap::from([(start.to_string(), 0)]);
    let mut queue = std::collections::VecDeque::from([start.to_string()]);
    while let Some(bus) = queue.pop_front() {
        let next_distance = distance[&bus] + 1;
        for branch in project.branches.iter().filter(|branch| project.branch_closed(branch)) {
            let other = if branch.from == bus { Some(branch.to.clone()) } else if branch.to == bus { Some(branch.from.clone()) } else { None };
            if let Some(other) = other {
                if !distance.contains_key(&other) {
                    distance.insert(other.clone(), next_distance);
                    queue.push_back(other);
                }
            }
        }
    }
    distance
}





fn coordination(project: &Project, fault: &FaultResult) -> Vec<CoordRow> {
    project
        .devices
        .iter()
        .map(|device| {
            let kv = project.bus(&device.bus).map(|b| b.kv).unwrap_or(1.0);
            let (fault_bus, location) = coordination_fault_bus(project, device, fault);
            let bus_fault = fault.buses.iter().find(|b| b.id == fault_bus);
            let amps = bus_fault.and_then(|b| device_current(project, device, b, 1.0));
            let bolted = amps.map(|a| a / 1000.0);
            let trip = amps.and_then(|a| curves::trip_time_device(device, a));
            let total = amps.and_then(|a| curves::clearing_time(device, a));
            let two_bands = matches!(&device.curve, crate::model::CurveSpec::Points { max_points: Some(_), .. });
            let min_band = amps.filter(|_| two_bands).and_then(|a| curves::trip_time_fast_band(&device.curve, a));
            let point_note = amps.and_then(|a| curves::point_curve_note(&device.curve, a))
                .map(|(_, note)| format!(" {note}")).unwrap_or_default();
            let melt_note = amps.and_then(|a| curves::average_melt_clearing(device, a).map(|melt| format!(" {}", melt.detail(a)))).unwrap_or_default();
            let band_note = if two_bands { " Point curve: times use the slower (max) band; min_band_trip_s is the faster band." } else { "" };
            CoordRow {
                device_id: device.id.clone(),
                name: device.name.clone(),
                bus_id: device.bus.clone(),
                bus_kv: kv,
                curve: curve_label(&device.curve),
                bolted_ka: bolted,
                trip_at_bolted_s: trip,
                total_clearing_s: total,
                min_band_trip_s: min_band,
                fault_bus_id: fault_bus.clone(),
                note: format!("Current is the protected branch terminal contribution for a three-phase fault at bus '{fault_bus}' ({location}); operating and total-clearing times are separate. Unassigned terminals/times are unavailable.{band_note}{point_note}{melt_note}"),
            }
        })
        .collect()
}

/// Bus faulted for a device's coordination row: the terminal of its protected
/// branch on the source-free (protected) side. A device entered at its
/// line-side bus would otherwise see only backfeed for a fault at its own bus.
fn coordination_fault_bus(project: &Project, device: &crate::model::Device, fault: &FaultResult) -> (String, String) {
    let branch = device.protected_branch.as_deref()
        .and_then(|id| project.branches.iter().find(|b| b.id == id))
        .filter(|b| (b.from == device.bus || b.to == device.bus) && project.branch_closed(b));
    let Some(branch) = branch else {
        return (device.bus.clone(), "the device bus; protected branch terminal not available".into());
    };
    let far = if branch.from == device.bus { &branch.to } else { &branch.from };
    let sourced = |terminal: &str| {
        let seen = buses_after_device_opens(project, device, terminal);
        project.sources.iter().any(|s| s.in_service && seen.contains(&s.bus))
    };
    match (sourced(&device.bus), sourced(far)) {
        (true, false) => (far.clone(), format!("far terminal of protected branch '{}', away from the device bus, on the source-free side", branch.id)),
        (false, true) => (device.bus.clone(), format!("device bus, the source-free terminal of protected branch '{}'", branch.id)),
        (true, true) => {
            let current = |bus: &str| fault.buses.iter().find(|b| b.id == bus)
                .and_then(|b| device_current(project, device, b, 1.0));
            let (from, to) = (current(&branch.from), current(&branch.to));
            let (chosen, other, other_amps) = if to > from {
                (&branch.to, &branch.from, from)
            } else {
                (&branch.from, &branch.to, to)
            };
            let other_ka = other_amps.map(|a| format!("{:.6}", a / 1000.0)).unwrap_or_else(|| "unavailable".into());
            (chosen.clone(), format!("protected side undetermined in a meshed/parallel network; reported the terminal fault giving the larger device current (other terminal {other}: {other_ka} kA)"))
        }
        _ => (device.bus.clone(), format!("device bus; both terminals of protected branch '{}' have the same source connectivity, so the protected side is not determined", branch.id)),
    }
}

struct DutyPick {
    fault_name: &'static str,
    fault_bus_id: String,
    symmetrical_ka: f64,
    system_xr: Option<f64>,
    comparison: duty::DutyComparison,
}

struct RemoteDuty {
    contact_parting_cycles: Option<f64>,
    remote_multiplying_factor: Option<f64>,
    multiplied_symmetrical_ka: Option<f64>,
    nacd: Option<f64>,
    remote_status: Option<String>,
    local_ac_applied: bool,
}

impl RemoteDuty {
    fn none() -> Self {
        Self {
            contact_parting_cycles: None,
            remote_multiplying_factor: None,
            multiplied_symmetrical_ka: None,
            nacd: None,
            remote_status: None,
            local_ac_applied: false,
        }
    }
}

/// A load-flow prefault is used only when the load flow converged with no
/// error or warning message. Otherwise the whole study switches to the
/// no-load-with-tap prefault and says so. A flat 1.0 pu is never substituted.
fn prefault_fallback(project: &Project, loadflow: Option<&crate::loadflow::LoadflowResult>) -> Option<(Project, String)> {
    if project.prefault != Prefault::Loadflow {
        return None;
    }
    // The reference only switches load-flow prefault to no-load-with-tap under
    // the comprehensive fault method. Other methods keep the requested prefault.
    if project.fault_method != FaultMethod::Comprehensive {
        return None;
    }
    // Fall back only on load-flow errors (missing result or non-convergence).
    // Warnings alone do not switch the prefault. The switched setting is used
    // for this study run only; it is not written back to the project.
    let problem = match loadflow {
        None => "it could not be solved".to_string(),
        Some(result) if !result.converged => result.message.clone().unwrap_or_else(|| "it did not converge".into()),
        Some(_) => return None,
    };
    let mut changed = project.clone();
    changed.prefault = Prefault::NoLoadTap;
    Some((changed, format!("Prefault was requested from load-flow voltages, but the load flow reported an error ({problem}). The whole fault and arc-flash study uses the no-load-with-tap prefault instead.")))
}

/// Interrupting duty from a fault study of `project`.
///
/// A converged load-flow prefault is used only when the project asks for one.
/// An unconverged or missing load flow leaves the fault invalid, matching a
/// study. The error is the fault message, or "fault study is invalid".
pub fn interrupting_duty_rows(project: &Project) -> Result<Vec<InterruptingDuty>, String> {
    let loadflow = if project.prefault == Prefault::Loadflow {
        loadflow::solve(project).ok()
    } else {
        None
    };
    let switched;
    let project = match prefault_fallback(project, loadflow.as_ref()) {
        Some((changed, _)) => { switched = changed; &switched }
        None => project,
    };
    let fault = fault::solve(project, loadflow.as_ref())?;
    if !fault.valid {
        return Err(fault.error.unwrap_or_else(|| "fault study is invalid".into()));
    }
    interrupting_duties(project, loadflow.as_ref(), &fault)
}

/// Fault study for one breaker duty. Machine reactances follow IEEE C37.010
/// 5.4.1 for classified machines, and every bus X/R is replaced by the
/// separate-network value: reactance-only Thevenin X over resistance-only
/// Thevenin R. Symmetrical current stays from the complex network. The
/// half-cycle rms is recomputed from that X/R. Returns whether a machine was
/// scaled or omitted.
fn duty_fault(project: &Project, loadflow: Option<&crate::loadflow::LoadflowResult>, base: &FaultResult, duty: crate::machine::Duty) -> Result<(FaultResult, bool), String> {
    let scaled = crate::machine::duty_project(project, duty);
    let network_project = scaled.as_ref().unwrap_or(project);
    let mut fault = match &scaled {
        Some(scaled) => {
            let fault = fault::solve_uncorrected(scaled, loadflow)?;
            if !fault.valid {
                return Err(fault.error.unwrap_or_else(|| "duty fault study is invalid".into()));
            }
            fault
        }
        None => base.clone(),
    };
    let separate = fault::separate_xr(network_project)?;
    for bus in &mut fault.buses {
        let Some(row) = separate.iter().find(|row| row.bus_id == bus.id) else { continue };
        for (point, xr) in [(bus.three_phase.as_mut(), row.three_phase), (bus.line_to_ground.as_mut(), row.line_to_ground)] {
            let (Some(point), Some(xr)) = (point, xr) else { continue };
            // A zero-current ground fault (no zero-sequence path) keeps its row.
            if point.symmetrical_ka == 0.0 { continue; }
            point.x_over_r = xr;
            point.half_cycle_rms_ka = point.symmetrical_ka * fault::half_cycle_factor(xr)?;
        }
    }
    Ok((fault, scaled.is_some()))
}

fn interrupting_duties(project: &Project, loadflow: Option<&crate::loadflow::LoadflowResult>, fault: &FaultResult) -> Result<Vec<InterruptingDuty>, String> {
    let mut rows = Vec::new();
    let has_ratings = project.buses.iter().any(|b| b.bracing_ka.is_some()) || project.devices.iter().any(|d| d.interrupting.is_some());
    if !has_ratings { return Ok(rows); }
    let (first_cycle, first_scaled) = duty_fault(project, loadflow, fault, crate::machine::Duty::FirstCycle)?;
    let (interrupting, int_scaled) = duty_fault(project, loadflow, fault, crate::machine::Duty::Interrupting)?;
    let (first_cycle, interrupting) = (&first_cycle, &interrupting);
    let machines_scaled = first_scaled || int_scaled;
    for bus in &project.buses {
        if let Some(row) = bracing_duty(bus, first_cycle)? {
            rows.push(row);
        }
    }
    for device in &project.devices {
        if let Some(row) = device_duty(project, device, first_cycle, interrupting, machines_scaled)? {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn bracing_duty(bus: &crate::model::Bus, fault: &FaultResult) -> Result<Option<InterruptingDuty>, String> {
    let Some(rated) = bus.bracing_ka else { return Ok(None) };
    let Some(bus_fault) = fault.buses.iter().find(|row| row.id == bus.id) else { return Ok(None) };
    let mut chosen: Option<(&crate::fault::FaultPoint, &'static str)> = None;
    for (name, point) in [("three_phase", bus_fault.three_phase.as_ref()), ("line_to_ground", bus_fault.line_to_ground.as_ref())] {
        let Some(point) = point else { continue };
        if chosen.as_ref().map_or(true, |(prior, _)| point.half_cycle_rms_ka > prior.half_cycle_rms_ka) {
            chosen = Some((point, name));
        }
    }
    let Some((point, fault_name)) = chosen else { return Ok(None) };
    let comparison = duty::compare(&duty::DutyInput {
        symmetrical_ka: point.symmetrical_ka,
        system_xr: point.x_over_r,
        rated_symmetrical_ka: rated,
        test_xr: bus.bracing_test_xr,
    })?;
    Ok(Some(duty_row(
        &bus.id, &bus.name, "bus_bracing", fault_name, &bus.id,
        point.symmetrical_ka, point.x_over_r, rated, bus.bracing_test_xr, &comparison,
        &RemoteDuty::none(),
        &CloseLatch::none(),
        "Equipment bracing compared with the greater of the three-phase and line-to-ground bus faults. X/R is from separate resistance and reactance networks.".into(),
    )))
}

fn device_duty(project: &Project, device: &crate::model::Device, first_cycle: &FaultResult, interrupting: &FaultResult, machines_scaled: bool) -> Result<Option<InterruptingDuty>, String> {
    let Some(rating) = &device.interrupting else { return Ok(None) };
    let Some(best) = duty_pick(project, device, rating, first_cycle)? else { return Ok(None) };
    let mut note = if device.protected_branch.is_some() {
        "Device current is the bolted three-phase current through the protected branch. Line-to-ground current through the device is not stored, so it was not compared. Device current uses the faulted bus X/R from separate resistance and reactance networks.".to_string()
    } else {
        "No protected branch; bus fault current was used. The greater of the three-phase and line-to-ground bus faults was kept. Device current uses the faulted bus X/R from separate resistance and reactance networks.".to_string()
    };
    if machines_scaled {
        note.push_str(" The half-cycle comparison and closing and latching use the first-cycle machine reactances; the interrupting multiplier uses the interrupting machine reactances (IEEE C37.010 5.4.1, classified machines only).");
    }
    if rating.voltage_v > 0.0 {
        if let Some(bus) = project.bus(&device.bus) {
            let rated_kv = rating.voltage_v / 1000.0;
            if (bus.kv / rated_kv - 1.0).abs() > 0.10 {
                note.push_str(" Rating voltage does not match the bus nominal voltage.");
            }
        }
    }
    let remote = match duty_pick(project, device, rating, interrupting)? {
        Some(at_interrupting) => remote_duty(project, interrupting, device, rating, &at_interrupting, &mut note),
        None => RemoteDuty::none(),
    };
    let close_latch = close_latch_duty(project, device, rating, &best, &mut note);
    Ok(Some(duty_row(
        &device.id, &device.name, "device", best.fault_name, &device.bus,
        best.symmetrical_ka, best.system_xr, rating.symmetrical_ka, Some(rating.test_xr), &best.comparison,
        &remote, &close_latch, note,
    )))
}

fn duty_pick(project: &Project, device: &crate::model::Device, rating: &crate::model::InterruptingRating, fault: &FaultResult) -> Result<Option<DutyPick>, String> {
    let mut best: Option<DutyPick> = None;
    // Branch terminal current is stored only for the bolted three-phase fault.
    // Do not pair that magnitude with the line-to-ground X/R.
    if let Some(branch) = device.protected_branch.as_ref().and_then(|id| project.branches.iter().find(|b| b.id == *id)) {
        for terminal in [&branch.from, &branch.to] {
            let Some(bus_fault) = fault.buses.iter().find(|row| row.id == *terminal) else { continue };
            let Some(point) = bus_fault.three_phase.as_ref() else { continue };
            let Some(amps) = device_current(project, device, bus_fault, 1.0) else { continue };
            keep_larger(&mut best, "three_phase", terminal, amps / 1000.0, point.x_over_r, rating.symmetrical_ka, Some(rating.test_xr))?;
        }
    } else if let Some(bus_fault) = fault.buses.iter().find(|row| row.id == device.bus) {
        for (name, point) in [("three_phase", bus_fault.three_phase.as_ref()), ("line_to_ground", bus_fault.line_to_ground.as_ref())] {
            let Some(point) = point else { continue };
            keep_larger(&mut best, name, &device.bus, point.symmetrical_ka, point.x_over_r, rating.symmetrical_ka, Some(rating.test_xr))?;
        }
    }
    Ok(best)
}

fn remote_duty(
    project: &Project, fault: &FaultResult, device: &crate::model::Device,
    rating: &crate::model::InterruptingRating, best: &DutyPick, note: &mut String,
) -> RemoteDuty {
    let Some(cycles) = rating.contact_parting_cycles else { return RemoteDuty::none() };
    let device_kv = project.bus(&device.bus).map(|bus| bus.kv).unwrap_or(0.0);
    if device_kv <= 1.0 {
        note.push_str(" Contact parting time is not used at or below 1 kV. The remote multiplier is for breakers above 1 kV; this row stays on the half-cycle comparison.");
        return RemoteDuty::none();
    }
    let remote_factor = match duty::remote_multiplying_factor(best.system_xr, cycles) {
        Ok(factor) => factor,
        Err(err) => {
            note.push_str(&format!(" Remote multiplier was not applied: {err}."));
            return RemoteDuty::none();
        }
    };
    let bus = fault.buses.iter().find(|row| row.id == best.fault_bus_id);
    let applied = applied_multiplier(
        project,
        bus.map(|row| row.infeeds.as_slice()).unwrap_or(&[]),
        bus.and_then(|row| row.nacd),
        best.system_xr,
        cycles,
        remote_factor,
    );
    let multiplied = best.symmetrical_ka * applied.factor;
    let status = match duty::rated_symmetrical_status(multiplied, rating.symmetrical_ka) {
        Ok(status) => status,
        Err(err) => {
            note.push_str(&format!(" Remote multiplier was not applied: {err}."));
            return RemoteDuty::none();
        }
    };
    note.push_str(&applied.note);
    RemoteDuty {
        contact_parting_cycles: Some(cycles),
        remote_multiplying_factor: Some(applied.factor),
        multiplied_symmetrical_ka: Some(multiplied),
        nacd: applied.nacd,
        remote_status: Some(status.as_str().into()),
        local_ac_applied: applied.local_ac_applied,
    }
}

/// Contact parting is stored in cycles. Decrement curves are stored in seconds.
/// The half-cycle let-through table is 60 Hz, and this conversion uses the same frequency.
const PARTING_HZ: f64 = 60.0;

struct AppliedMultiplier {
    factor: f64,
    nacd: Option<f64>,
    local_ac_applied: bool,
    note: String,
}

fn remote_only_note(extra: &str) -> String {
    let mut note = " Remote multiplier is the DC-decrement factor at the entered contact parting time, divided by the breaker asymmetry capability, and not taken below 1. It does not replace the half-cycle comparison. Local AC-decrement curves are not applied. A source counts as remote unless it has a decrement curve. Motors are local and do not reduce the factor.".to_string();
    if !extra.is_empty() {
        note.push(' ');
        note.push_str(extra);
    }
    note
}

fn applied_multiplier(
    project: &Project,
    infeeds: &[crate::fault::ThreePhaseInfeed],
    nacd: Option<f64>,
    system_xr: Option<f64>,
    cycles: f64,
    remote_factor: f64,
) -> AppliedMultiplier {
    let Some(raw_nacd) = nacd else {
        return AppliedMultiplier {
            factor: remote_factor,
            nacd: None,
            local_ac_applied: false,
            note: remote_only_note("NACD was not reported because the infeed split did not match the fault current."),
        };
    };
    if !raw_nacd.is_finite() || raw_nacd < -1e-6 || raw_nacd > 1.0 + 1e-6 {
        return AppliedMultiplier {
            factor: remote_factor,
            nacd: None,
            local_ac_applied: false,
            note: remote_only_note("NACD was outside 0 to 1, so the remote multiplier was kept."),
        };
    }
    let nacd = raw_nacd.clamp(0.0, 1.0);
    if nacd >= 1.0 - 1e-9 {
        return AppliedMultiplier {
            factor: remote_factor,
            nacd: Some(nacd),
            local_ac_applied: false,
            note: remote_only_note(""),
        };
    }
    let time_s = cycles / PARTING_HZ;
    let mut weight = 0.0;
    let mut weighted = 0.0;
    let mut used_curve = false;
    for infeed in infeeds.iter().filter(|row| !row.remote) {
        if !infeed.current_pu.is_finite() || infeed.current_pu < 0.0 {
            return AppliedMultiplier {
                factor: remote_factor,
                nacd: Some(nacd),
                local_ac_applied: false,
                note: remote_only_note("A local infeed current was not finite, so the remote multiplier was kept."),
            };
        }
        if infeed.current_pu <= 1e-15 {
            continue;
        }
        let ac = if let Some(id) = &infeed.source_id {
            let Some(source) = project.sources.iter().find(|source| source.id == *id) else {
                return AppliedMultiplier {
                    factor: remote_factor,
                    nacd: Some(nacd),
                    local_ac_applied: false,
                    note: remote_only_note("A local source was missing from the project, so the remote multiplier was kept."),
                };
            };
            if source.decrement_curve.is_empty() {
                1.0
            } else {
                let ratio = source.decrement_ratio_at(time_s);
                if !ratio.is_finite() || ratio <= 0.0 {
                    return AppliedMultiplier {
                        factor: remote_factor,
                        nacd: Some(nacd),
                        local_ac_applied: false,
                        note: remote_only_note("An entered decrement was not a positive finite ratio at the contact parting time, so the remote multiplier was kept."),
                    };
                }
                used_curve = true;
                ratio
            }
        } else {
            1.0
        };
        weight += infeed.current_pu;
        weighted += infeed.current_pu * ac;
    }
    if !used_curve || weight <= 1e-15 {
        return AppliedMultiplier {
            factor: remote_factor,
            nacd: Some(nacd),
            local_ac_applied: false,
            note: remote_only_note("Generator AC decrement was not entered, so the local share does not reduce the factor."),
        };
    }
    let ac = weighted / weight;
    let local = match duty::local_multiplying_factor(system_xr, cycles, ac) {
        Ok(factor) => factor,
        Err(err) => {
            return AppliedMultiplier {
                factor: remote_factor,
                nacd: Some(nacd),
                local_ac_applied: false,
                note: remote_only_note(&format!("The local factor was not applied: {err}.")),
            };
        }
    };
    let factor = match duty::nacd_interpolated_factor(remote_factor, local, nacd) {
        Ok(factor) => factor,
        Err(err) => {
            return AppliedMultiplier {
                factor: remote_factor,
                nacd: Some(nacd),
                local_ac_applied: false,
                note: remote_only_note(&format!("The local factor was not applied: {err}.")),
            };
        }
    };
    AppliedMultiplier {
        factor,
        nacd: Some(nacd),
        local_ac_applied: true,
        note: format!(
            " Interrupting multiplier {factor:.3} blends the remote DC-decrement factor {remote_factor:.3} with the local factor {local:.3} by NACD {nacd:.3}. The local symmetrical-current ratio is {ac:.3}, weighted from entered generator decrement curves at {time_s:.4} s ({cycles:.2} cycles at 60 Hz). That ratio is combined with the DC term, divided by the breaker asymmetry capability, and not taken below 1. Motor contribution without a decrement curve stays at its initial symmetrical current and is not reduced. This is not a lookup of the published local application curves. It does not replace the half-cycle comparison. A source counts as remote unless it has a decrement curve."
        ),
    }
}

struct CloseLatch {
    close_latch_ka: Option<f64>,
    close_latch_capability_ka: Option<f64>,
    voltage_range_factor: Option<f64>,
    close_latch_status: Option<String>,
}

impl CloseLatch {
    fn none() -> Self {
        Self {
            close_latch_ka: None,
            close_latch_capability_ka: None,
            voltage_range_factor: None,
            close_latch_status: None,
        }
    }
}

fn close_latch_duty(
    project: &Project, device: &crate::model::Device, rating: &crate::model::InterruptingRating,
    best: &DutyPick, note: &mut String,
) -> CloseLatch {
    let device_kv = project.bus(&device.bus).map(|bus| bus.kv).unwrap_or(0.0);
    if device_kv <= 1.0 {
        note.push_str(" Closing and latching is not applied at or below 1 kV.");
        return CloseLatch::none();
    }
    let k = rating.voltage_range_factor.unwrap_or(1.0);
    let (duty_ka, capability) = match duty::close_latch_currents(best.symmetrical_ka, rating.symmetrical_ka, k) {
        Ok(currents) => currents,
        Err(err) => {
            note.push_str(&format!(" Closing and latching was not applied: {err}."));
            return CloseLatch::none();
        }
    };
    let status = match duty::rated_symmetrical_status(duty_ka, capability) {
        Ok(status) => status,
        Err(err) => {
            note.push_str(&format!(" Closing and latching was not applied: {err}."));
            return CloseLatch::none();
        }
    };
    note.push_str(&format!(
        " Closing and latching duty is 1.6 times the symmetrical current {sym:.3} kA, which is {duty_ka:.3} kA. Capability is 1.6 times K {k:.3} times the rated symmetrical current {rated:.3} kA, which is {capability:.3} kA. The entered symmetrical rating is the rated short-circuit current. Omitted K is 1. It does not replace the half-cycle comparison.",
        sym = best.symmetrical_ka,
        rated = rating.symmetrical_ka,
    ));
    CloseLatch {
        close_latch_ka: Some(duty_ka),
        close_latch_capability_ka: Some(capability),
        voltage_range_factor: Some(k),
        close_latch_status: Some(status.as_str().into()),
    }
}

fn keep_larger(best: &mut Option<DutyPick>, fault_name: &'static str, fault_bus_id: &str, symmetrical_ka: f64, system_xr: Option<f64>, rated_symmetrical_ka: f64, test_xr: Option<f64>) -> Result<(), String> {
    let comparison = duty::compare(&duty::DutyInput {
        symmetrical_ka,
        system_xr,
        rated_symmetrical_ka,
        test_xr,
    })?;
    if best.as_ref().map_or(true, |prior| comparison.half_cycle_rms_ka > prior.comparison.half_cycle_rms_ka) {
        *best = Some(DutyPick { fault_name, fault_bus_id: fault_bus_id.into(), symmetrical_ka, system_xr, comparison });
    }
    Ok(())
}

fn duty_row(
    id: &str, name: &str, kind: &str, fault_name: &str, bus_id: &str,
    symmetrical_ka: f64, system_xr: Option<f64>, rated: f64, test_xr: Option<f64>,
    comparison: &duty::DutyComparison, remote: &RemoteDuty, close_latch: &CloseLatch, note: String,
) -> InterruptingDuty {
    InterruptingDuty {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        fault: fault_name.into(),
        bus_id: bus_id.into(),
        symmetrical_ka,
        system_xr,
        half_cycle_rms_ka: comparison.half_cycle_rms_ka,
        rated_symmetrical_ka: rated,
        test_xr,
        rated_half_cycle_rms_ka: comparison.rated_half_cycle_rms_ka,
        adjusted_symmetrical_ka: comparison.adjusted_symmetrical_ka,
        status: comparison.status.as_str().into(),
        note,
        contact_parting_cycles: remote.contact_parting_cycles,
        remote_multiplying_factor: remote.remote_multiplying_factor,
        multiplied_symmetrical_ka: remote.multiplied_symmetrical_ka,
        nacd: remote.nacd,
        remote_status: remote.remote_status.clone(),
        local_ac_applied: remote.local_ac_applied,
        close_latch_ka: close_latch.close_latch_ka,
        close_latch_capability_ka: close_latch.close_latch_capability_ka,
        voltage_range_factor: close_latch.voltage_range_factor,
        close_latch_status: close_latch.close_latch_status.clone(),
    }
}

fn duty_warnings(rows: &[InterruptingDuty]) -> Vec<String> {
    let mut warnings: Vec<String> = rows.iter().filter_map(|row| match row.status.as_str() {
        "exceeds_rating" => Some(exceeds_warning(row)),
        "marginal" => Some(format!(
            "{}: duty is above 90% of the compared rating and still within it",
            row.id
        )),
        "not_adjusted" => Some(format!(
            "{}: rating was compared without a test X/R, so asymmetrical duty was not checked",
            row.id
        )),
        _ => None,
    }).collect();
    for row in rows {
        if let Some(warning) = remote_warning(row) {
            warnings.push(warning);
        }
        if let Some(warning) = close_latch_warning(row) {
            warnings.push(warning);
        }
    }
    warnings
}

fn close_latch_warning(row: &InterruptingDuty) -> Option<String> {
    let status = row.close_latch_status.as_deref()?;
    let duty_ka = row.close_latch_ka?;
    let capability = row.close_latch_capability_ka?;
    match status {
        "exceeds_rating" => Some(format!(
            "{}: close-and-latch duty {:.3} kA exceeds the closing and latching capability {:.3} kA",
            row.id, duty_ka, capability
        )),
        "marginal" => Some(format!(
            "{}: close-and-latch duty is above 90% of the closing and latching capability and still within it",
            row.id
        )),
        _ => None,
    }
}

fn remote_warning(row: &InterruptingDuty) -> Option<String> {
    let status = row.remote_status.as_deref()?;
    let factor = row.remote_multiplying_factor?;
    let cycles = row.contact_parting_cycles?;
    let multiplied = row.multiplied_symmetrical_ka?;
    let name = if row.local_ac_applied { "interrupting multiplier" } else { "remote multiplier" };
    let ac = if row.local_ac_applied {
        "Entered generator AC decrement is included. Motor contribution without a decrement curve is not reduced."
    } else {
        "Local AC decrement is not applied."
    };
    match status {
        "exceeds_rating" => Some(format!(
            "{}: {:.3} kA times {name} {:.3} is {:.3} kA, which exceeds the rated symmetrical current {:.3} kA at {:.2} cycles contact parting. {ac}",
            row.id, row.symmetrical_ka, factor, multiplied, row.rated_symmetrical_ka, cycles
        )),
        "marginal" => Some(format!(
            "{}: {name} duty is above 90% of the rated symmetrical current and still within it",
            row.id
        )),
        _ => None,
    }
}

fn exceeds_warning(row: &InterruptingDuty) -> String {
    let (ka, label) = match row.adjusted_symmetrical_ka {
        Some(ka) => (ka, "adjusted"),
        None => (row.rated_symmetrical_ka, "rated"),
    };
    match row.test_xr {
        Some(xr) => format!(
            "{}: {:.6} kA symmetrical exceeds the {label} rating {:.6} kA at test X/R {xr}",
            row.id, row.symmetrical_ka, ka
        ),
        None => format!(
            "{}: {:.6} kA symmetrical exceeds the {label} rating {:.6} kA",
            row.id, row.symmetrical_ka, ka
        ),
    }
}

/// Bus voltage outside ±5% of nominal, and line phasor drop above 3% of nominal.
/// A transformer is not screened as a line. An unenergized bus is not screened.
fn voltage_screens(project: &Project, lf: &crate::loadflow::LoadflowResult) -> Vec<String> {
    let mut out = Vec::new();
    for bus in &lf.buses {
        if bus.v_pu > 1e-6 && (bus.v_pu - 1.0).abs() > 0.05 + 1e-9 {
            out.push(format!(
                "{}: voltage {:.3} pu is outside the ±5% band around nominal. This is a study screen, not a code calculation.",
                bus.id, bus.v_pu
            ));
        }
    }
    for branch in &project.branches {
        let crate::model::BranchKind::Line { .. } = branch.kind else { continue };
        let Some(from) = lf.buses.iter().find(|row| row.id == branch.from) else { continue };
        let Some(to) = lf.buses.iter().find(|row| row.id == branch.to) else { continue };
        if from.v_pu <= 1e-6 || to.v_pu <= 1e-6 { continue; }
        let base = from.kv_nominal.max(to.kv_nominal).max(1e-6);
        if (from.kv_nominal - to.kv_nominal).abs() > 1e-6 * base { continue; }
        let drop = phasor_separation(from.v_pu, from.angle_deg, to.v_pu, to.angle_deg);
        if drop > 0.03 + 1e-9 {
            out.push(format!(
                "{}: line voltage drop {:.2}% of nominal is above 3%. This is a study screen, not a code calculation.",
                branch.id, drop * 100.0
            ));
        }
    }
    out
}

fn phasor_separation(v1: f64, angle1_deg: f64, v2: f64, angle2_deg: f64) -> f64 {
    let a1 = angle1_deg.to_radians();
    let a2 = angle2_deg.to_radians();
    let re = v1 * a1.cos() - v2 * a2.cos();
    let im = v1 * a1.sin() - v2 * a2.sin();
    (re * re + im * im).sqrt()
}

pub fn text_report(project: &Project, out: &StudyOutput) -> String {
    let mut lines = vec![
        format!("Flashmob  {}", project.name),
        format!("Arc flash method: {}", out.methods.arc_flash),
        "Calculated fault current is symmetrical rms from the stored impedances. Equipment AIC is bracing and is not the calculated available fault.".into(),
        String::new(),
        "Assumptions".into(),
    ];
    if project.assumptions.is_empty() {
        lines.push("  None recorded.".into());
    } else {
        for item in &project.assumptions {
            lines.push(format!("  [{}] {}", item.id, item.statement));
        }
    }
    lines.push(String::new());
    if let Some(lf) = &out.loadflow {
        lines.push(format!(
            "Load flow  {}  {} iterations  mismatch {:.2e} pu",
            if lf.converged { "converged" } else { "did not converge" },
            lf.iterations,
            lf.max_mismatch_pu
        ));
    }
    lines.push("Buses".into());
    for bus in &project.buses {
        lines.push(format!("  {}", bus.name));
        if let Some(protection) = out.protection.iter().find(|p| p.bus_id == bus.id) {
            lines.push(format!("    {}", protection.detail));
        }
        if let Some(lf) = out.loadflow.as_ref().filter(|lf| lf.converged).and_then(|lf| lf.buses.iter().find(|row| row.id == bus.id)) {
            let angle = if (lf.angle_deg * 100.0).round() == 0.0 { 0.0 } else { lf.angle_deg };
            lines.push(format!("    Load flow  {:.3} pu  {angle:.2} deg", lf.v_pu));
        }
        if let Some(fault) = out.fault.as_ref().and_then(|fault| fault.buses.iter().find(|row| row.id == bus.id)) {
            let three = fault.three_phase.as_ref().map(|p| format!("{:.2} kA", p.symmetrical_ka)).unwrap_or_else(|| "—".into());
            let lg = fault.line_to_ground.as_ref().map(|p| format!("{:.2} kA", p.symmetrical_ka)).unwrap_or_else(|| "—".into());
            lines.push(format!("    Short circuit  3P {three}  LG {lg}  (calculated, prefault {})", out.fault.as_ref().map(|f| f.prefault.as_str()).unwrap_or("")));
            if let Some(point) = fault.three_phase.as_ref().filter(|point| point.c_max > 0.0) {
                lines.push(format!(
                    "    Voltage factor  cmin {:.2}  cmax {:.2}  3P minimum {:.2} kA  3P maximum {:.2} kA. IEC 60909-0 Table 1 applied to this symmetrical current. {}",
                    point.c_min, point.c_max, point.symmetrical_min_ka, point.symmetrical_max_ka,
                    if point.iec_max_corrected { "The maximum uses the impedance correction factors." } else { "Impedance correction factors are not applied." },
                ));
            }
        }
        if let Some(bracing) = bus.bracing_ka {
            lines.push(format!(
                "    Equipment bracing  {bracing:.0} kA AIC, collected from the gear. This is not the calculated available fault."
            ));
        }
        if let Some(main) = bus.main_rating_a {
            lines.push(format!("    Main rating  {main:.0} A, collected. Trip-unit delay and instantaneous are not this rating."));
        }
        for row in out.interrupting.iter().filter(|row| row.bus_id == bus.id) {
            let xr = row.system_xr.map(|v| format!("{v:.2}")).unwrap_or_else(|| "infinite".into());
            let limit = match row.adjusted_symmetrical_ka {
                Some(ka) => format!("adjusted {ka:.3} kA"),
                None => format!("rated {:.3} kA", row.rated_symmetrical_ka),
            };
            lines.push(format!(
                "    Interrupting duty  {} ({}) {}  {:.3} kA symmetrical  X/R {}  half-cycle {:.3} kA  {}  {}  {}",
                row.id, row.kind, row.fault, row.symmetrical_ka, xr, row.half_cycle_rms_ka, limit, row.status, row.note
            ));
            if let (Some(factor), Some(cycles), Some(multiplied), Some(status)) = (
                row.remote_multiplying_factor,
                row.contact_parting_cycles,
                row.multiplied_symmetrical_ka,
                row.remote_status.as_deref(),
            ) {
                let nacd = row.nacd.map(|value| format!("{value:.3}")).unwrap_or_else(|| "not reported".into());
                let (label, tail) = if row.local_ac_applied {
                    (
                        "Interrupting multiplier",
                        "NACD blends the remote factor with the entered generator decrement at 60 Hz. Motor contribution without a decrement curve is not reduced.",
                    )
                } else {
                    ("Remote multiplier", "DC decrement only. Local AC-decrement curves are not applied.")
                };
                lines.push(format!(
                    "    {label}  {factor:.3} at {cycles:.2} cycles contact parting  {multiplied:.3} kA  {status}  NACD {nacd}. {tail}"
                ));
            }
            if let (Some(duty_ka), Some(capability), Some(k), Some(status)) = (
                row.close_latch_ka,
                row.close_latch_capability_ka,
                row.voltage_range_factor,
                row.close_latch_status.as_deref(),
            ) {
                lines.push(format!(
                    "    Closing and latching  1.6 times {sym:.3} kA is {duty_ka:.3} kA  capability {capability:.3} kA  K {k:.3}  {status}. The symmetrical current uses first-cycle machine reactances for classified machines.",
                    sym = row.symmetrical_ka
                ));
            }
        }
        if let Some(rows) = &out.arc_flash {
            let mine: Vec<_> = rows.iter().filter(|row| row.bus_id == bus.id).collect();
            if mine.is_empty() {
                let reasons: Vec<_> = out.arc_flash_failures.iter().filter(|failure| failure.bus_id == bus.id).collect();
                if reasons.is_empty() {
                    lines.push("    Arc flash  no IEEE 1584-2018 result for this bus.".into());
                } else {
                    for failure in reasons {
                        lines.push(format!("    Arc flash FAILED: {} Clearing basis unresolved. Not label-ready.", failure.error));
                    }
                }
            }
            for row in mine {
                lines.push(format!("    {}", row.duration_note));
                let basis = if row.clearing_basis == row.clearing_basis_reduced { row.clearing_basis.clone() } else { format!("{} (reduced: {})", row.clearing_basis, row.clearing_basis_reduced) };
                if row.label_ready {
                    lines.push(format!("    Clearing basis {basis}. Label-ready on its data basis (not an engineering review)."));
                } else {
                    lines.push(format!("    Clearing basis {basis}. Not label-ready: {}.", row.label_reasons.join(", ")));
                }
                if row.decrement_applied {
                    lines.push(format!("    Source-current decrement integrated for: {}", row.decrement_source_ids.join(", ")));
                }
                lines.push(format!(
                    "    IEEE 1584-2018  {:.2} cal/cm2  AFB {:.1} in  ({})",
                    row.governing_cal_cm2, row.afb_in, row.governing
                ));
                lines.push(format!("    AFB full {:.1} mm; reduced {:.1} mm; maximum {:.1} mm", row.afb_full_mm, row.afb_reduced_mm, row.afb_mm));
                if row.enclosure_assumed {
                    lines.push("    Enclosure gap, working distance, and electrode configuration are an assumption, not a field measurement.".into());
                }
            }
        }
    }
    for failure in &out.arc_flash_failures {
        lines.push(format!("Arc flash FAILED: {} ({}): {}", failure.name, failure.bus_id, failure.error));
    }
    if let Some(f) = &out.fault {
        lines.push(format!("Prefault requested: {}; used: {}; valid: {}", f.prefault_requested, f.prefault, f.valid));
        if let Some(error) = &f.error { lines.push(error.clone()); }
    }
    if !out.warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings".into());
        for w in &out.warnings {
            lines.push(format!("  {w}"));
        }
    }
    if let Some(envelope) = &out.worst_case {
        lines.push(String::new());
        lines.push("Operating cases".into());
        lines.push("  normal: entered base state; bus details above".into());
        for case in &out.operating_cases {
            if let Some(error) = &case.error {
                lines.push(format!("  {}: FAILED: {error}", case.id));
            } else if let Some(result) = &case.result {
                let fault_status = result.fault.as_ref().map(|fault| if fault.valid { "fault valid" } else { "fault invalid" });
                lines.push(format!("  {}: calculated{}; {} arc-flash failures", case.id,
                    fault_status.map(|status| format!("; {status}")).unwrap_or_default(),
                    result.arc_flash_failures.len()));
            }
        }
        lines.push("Worst across calculated cases (independent maximum for each quantity; short-circuit rows are symmetrical RMS unless labeled)".into());
        let display = |maximum: &Option<CaseMaximum>, unit: &str| maximum.as_ref()
            .map(|item| format!("{:.2} {unit} [{}]", item.value, item.case_id))
            .unwrap_or_else(|| "N/A".into());
        for bus in &envelope.buses {
            lines.push(format!("  {}: 3P {}; LG {}; LL {}; LLG {}{}",
                bus.bus_id, display(&bus.three_phase_ka, "kA"),
                display(&bus.line_to_ground_ka, "kA"), display(&bus.line_to_line_ka, "kA"),
                display(&bus.line_to_line_ground_ka, "kA"),
                if bus.complete { String::new() } else { format!("; INCOMPLETE: {}", bus.failed_case_ids.join(", ")) }));
            lines.push(format!("    3P IEC peak {}; half-cycle RMS {}",
                display(&bus.three_phase_iec_peak_ka, "kA"),
                display(&bus.three_phase_half_cycle_rms_ka, "kA")));
        }
        for item in &envelope.arc_equipment {
            lines.push(format!("  {}: incident energy {}; boundary {}{}",
                item.equipment_id, display(&item.incident_energy_cal_cm2, "cal/cm2"),
                display(&item.boundary_mm, "mm"),
                if item.complete { String::new() } else { format!("; INCOMPLETE: {}", item.failed_case_ids.join(", ")) }));
        }
    }
    lines.join("\n")
}


#[cfg(test)]
mod terminal_regression {
    use super::*;
    #[test]
    fn reduced_arc_scales_fault_increment_but_preserves_prefault_flow() {
        let p = Project::sample();
        let mut f = fault::solve(&p, None).unwrap().buses.remove(1);
        let t = f.terminal_currents.iter_mut().find(|t| t.branch_id == "t1").unwrap();
        t.to_prefault_a = [30.0, 40.0];
        t.to_increment_a = [-60.0, -80.0];
        // At half fault injection, terminal current is (60+j80) A, magnitude 100 A.
        let current = device_current(&p, &p.devices[0], &f, 0.5).unwrap();
        assert!((current-100.0).abs() < 1e-12);
    }
}

#[cfg(test)]
mod audit_regression {
    use super::*;
    use crate::arcflash::{self, IeeeInput};
    use crate::model::{classes_for_assumed_bus, CurrentLimiting, CurveSpec, DecrementPoint, Electrode};

    fn arc_only() -> Studies { Studies { loadflow: false, fault: true, arcflash: true, coordination: false } }

    fn row<'a>(out: &'a StudyOutput, id: &str) -> &'a ArcRow {
        out.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == id)
            .unwrap_or_else(|| panic!("no row {id}; failures {:?}", out.arc_flash_failures))
    }

    #[test]
    fn sample_rows_carry_a_clearing_basis_and_label_readiness() {
        let out = run(&Project::sample(), arc_only()).unwrap();
        let rows = out.arc_flash.as_ref().unwrap();
        assert!(!rows.is_empty());
        let known: Vec<&str> = LABEL_REASONS.iter().map(|(code, _)| *code).collect();
        for r in rows {
            assert!(!r.clearing_basis.is_empty() && !r.clearing_basis_reduced.is_empty(), "{}", r.equipment_id);
            assert_eq!(r.label_ready, r.label_reasons.is_empty());
            assert!(!(r.assumed && r.label_ready), "{}", r.equipment_id);
            assert!(r.label_reasons.iter().all(|code| known.contains(&code.as_str())), "{:?}", r.label_reasons);
        }
        let staged = rows.iter().find(|r| r.clearing_basis == "staged_multi_device").expect("staged sample row");
        assert!(staged.label_reasons.iter().any(|r| r == "staged_clearing_assumed"));
        assert!(rows.iter().any(|r| r.clearing_basis == "trip_curve_plus_interrupting_time" && r.label_ready));
        for f in &out.arc_flash_failures { assert_eq!(f.clearing_basis, "unresolved"); assert!(!f.label_ready); }
        let report = text_report(&Project::sample(), &out);
        assert!(report.contains("Not label-ready: staged_clearing_assumed") && report.contains("Label-ready on its data basis"), "{report}");
    }

    #[test]
    fn assumed_enclosure_reports_maximum_over_equipment_classes() {
        let mut p = Project::sample();
        p.equipment.retain(|e| e.id != "af-pnl");
        let out = run(&p, arc_only()).unwrap();
        let assumed = row(&out, "assumed:pnl").clone();
        assert!(assumed.assumed && assumed.enclosure_assumed);
        assert!(!assumed.label_ready && assumed.label_reasons.iter().any(|r| r == "assumed_equipment"), "{:?}", assumed.label_reasons);

        let classes = classes_for_assumed_bus(p.bus("pnl").unwrap());
        assert!(classes.len() >= 2, "{classes:?}");
        let mut by_class = Vec::new();
        for class in classes {
            let spec = class.spec(0.48);
            let mut q = p.clone();
            q.equipment.push(ArcEquipment {
                id: "explicit".into(), name: "explicit".into(), bus: "pnl".into(),
                electrode: spec.electrode, gap_mm: spec.gap_mm, distance_mm: spec.distance_mm,
                height_mm: spec.height_mm, width_mm: spec.width_mm, depth_mm: spec.depth_mm,
                upstream_device: None, clearing_s: None, fallback_duration_s: None, basis: "collected".into(),
            });
            by_class.push((spec.label, row(&run(&q, arc_only()).unwrap(), "explicit").clone()));
        }
        let max_energy = by_class.iter().map(|(_, row)| row.governing_cal_cm2).fold(0.0_f64, f64::max);
        let max_boundary = by_class.iter().map(|(_, row)| row.afb_mm).fold(0.0_f64, f64::max);
        let (energy_label, _) = by_class.iter().find(|(_, row)| row.governing_cal_cm2 == max_energy).unwrap();
        assert!((assumed.governing_cal_cm2 - max_energy).abs() < 1e-9, "{} vs {max_energy}", assumed.governing_cal_cm2);
        assert!((assumed.afb_mm - max_boundary).abs() < 1e-6, "{} vs {max_boundary}", assumed.afb_mm);
        assert!(assumed.warnings[0].contains(energy_label) && assumed.warnings[0].contains("governs incident energy"), "{:?}", assumed.warnings);
        assert!(assumed.warnings[0].contains("shallow low-voltage MCC or panelboard"), "{:?}", assumed.warnings);
        assert!(out.warnings.iter().any(|w| w.contains("enclosure dimensions were not collected")));
        assert_eq!(row(&run(&Project::sample(), arc_only()).unwrap(), "af-pnl").electrode, Electrode::VCB.as_str());
    }

    #[test]
    fn gap_below_the_model_minimum_is_still_calculated_and_marked_assumed() {
        let mut p = Project::sample();
        p.motors.clear();
        p.equipment.truncate(1);
        p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
        p.devices[0].breaker_interrupting_s = Some(0.04);
        p.equipment[0].gap_mm = 3.0;
        let out = run(&p, arc_only()).unwrap();
        let row = row(&out, "af-mcc");
        assert!(row.assumed && !row.enclosure_assumed);
        assert!(row.warnings.iter().any(|w| w.contains("gap") && w.contains("assumed")), "{:?}", row.warnings);
        assert!(row.governing_cal_cm2.is_finite() && row.governing_cal_cm2 > 0.0);
    }

    #[test]
    fn two_second_cap_is_assumed_only_when_escape_is_possible() {
        let mut p = Project::sample();
        p.motors.clear();
        p.equipment.truncate(1);
        p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 3.0 };
        p.devices[0].breaker_interrupting_s = Some(0.05);
        let capped_out = run(&p, arc_only()).unwrap();
        let capped = row(&capped_out, "af-mcc");
        assert!(capped.duration_capped && capped.assumed);
        assert!(!capped.label_ready && capped.label_reasons.iter().any(|r| r == "duration_capped"), "{:?}", capped.label_reasons);
        assert!((capped.time_s - 2.0).abs() < 1e-12, "{}", capped.time_s);
        assert!(capped.duration_note.contains("6.9.1") && capped.duration_note.contains("move away"), "{}", capped.duration_note);
        p.escape_possible = false;
        let open_out = run(&p, arc_only()).unwrap();
        let open = row(&open_out, "af-mcc");
        assert!(!open.duration_capped && !open.assumed);
        assert!(open.label_ready && open.label_reasons.is_empty(), "{:?}", open.label_reasons);
        assert!(!open.clearing_basis.is_empty());
        assert!((open.time_s - 3.05).abs() < 1e-12, "{}", open.time_s);
        assert!(open.duration_note.contains("not assumed possible"), "{}", open.duration_note);
    }

    #[test]
    fn sole_feed_current_limiting_replaces_the_calculated_arc_current() {
        let mut p = Project::sample();
        p.motors.clear();
        p.equipment.truncate(1);
        p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.5 };
        p.devices[0].breaker_interrupting_s = Some(0.05);
        p.devices[0].current_limiting = Some(CurrentLimiting { threshold_a: 1.0, clearing_s: 0.02, effective_arc_ka: Some(8.0), cycle_table: false });
        let limited_out = run(&p, arc_only()).unwrap();
        let limited = row(&limited_out, "af-mcc");
        assert!((limited.time_s - 0.02).abs() < 1e-12 && (limited.time_min_s - 0.02).abs() < 1e-12);
        assert!((limited.arcing_ka - 8.0).abs() < 1e-9 && (limited.arcing_min_ka - 8.0).abs() < 1e-9);
        assert!(limited.duration_note.contains("current-limiting") && limited.duration_note.contains("variation factor"), "{}", limited.duration_note);
        assert_eq!(limited.clearing_basis, "current_limiting_manufacturer_time");
        let expected = arcflash::calculate(&IeeeInput {
            voc_kv: 0.48, ibf_ka: limited.bolted_ka, gap_mm: p.equipment[0].gap_mm,
            distance_mm: p.equipment[0].distance_mm, height_mm: p.equipment[0].height_mm,
            width_mm: p.equipment[0].width_mm, depth_mm: p.equipment[0].depth_mm,
            electrode: p.equipment[0].electrode, time_s: 0.02, time_min_s: 0.02,
            arc_ka_override: Some(8.0), arc_min_ka_override: Some(8.0), lee: false,
        }).unwrap();
        assert!((limited.governing_cal_cm2 - expected.governing_cal_cm2).abs() < 1e-9);

        p.devices[0].current_limiting.as_mut().unwrap().threshold_a = 1.0e9;
        let curve_out = run(&p, arc_only()).unwrap();
        let curve = row(&curve_out, "af-mcc");
        assert!((curve.time_s - 0.55).abs() < 1e-12, "{}", curve.time_s);
        assert!((curve.arcing_ka - 8.0).abs() > 1.0);
    }

    fn limiting_device(threshold_a: f64, cycle_table: bool) -> crate::model::Device {
        let mut device = Project::sample().devices.into_iter().next().unwrap();
        device.curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.5 };
        device.breaker_interrupting_s = Some(0.05);
        device.curve_is_total_clearing = false;
        device.fuse_total_clearing = false;
        device.current_limiting = Some(CurrentLimiting {
            threshold_a, clearing_s: 0.2, effective_arc_ka: None, cycle_table,
        });
        device
    }

    #[test]
    fn cycle_table_splits_half_cycle_and_quarter_cycle_at_twice_the_threshold() {
        let device = limiting_device(10_000.0, true);
        let half = device_exposure(&device, 10_000.0, true).unwrap();
        assert!((half.seconds - 1.0 / 120.0).abs() < 1e-12, "{}", half.seconds);
        assert!(half.assumed);
        assert!(half.extra.contains("half-cycle") && half.extra.contains("0.008333"), "{}", half.extra);
        assert!(half.extra.contains("Clause 6.9.2 is not applied") && half.extra.contains("Annex H"), "{}", half.extra);
        assert!(half.extra.contains("manufacturer clearing time is not used"), "{}", half.extra);
        let quarter = device_exposure(&device, 20_000.0, true).unwrap();
        assert!((quarter.seconds - 1.0 / 240.0).abs() < 1e-12, "{}", quarter.seconds);
        assert!(quarter.extra.contains("quarter-cycle") && quarter.extra.contains("0.004167"), "{}", quarter.extra);
        let below = device_exposure(&device, 9_999.0, true).unwrap();
        assert!((below.seconds - 0.55).abs() < 1e-12, "{}", below.seconds);
        assert!(!below.extra.contains("half-cycle"), "{}", below.extra);
    }

    #[test]
    fn stored_total_clearing_faster_than_a_half_cycle_is_kept() {
        let mut device = limiting_device(10_000.0, true);
        device.breaker_interrupting_s = None;
        device.fuse_total_clearing = true;
        device.curve = CurveSpec::Points {
            points: vec![[100.0, 0.005], [100_000.0, 0.005]],
            max_points: None,
            beyond_last: crate::model::PointTail::Unavailable,
            band_interpretation: crate::model::BandInterpretation::Entered,
        };
        let kept = device_exposure(&device, 25_000.0, true).unwrap();
        assert!((kept.seconds - 0.005).abs() < 1e-12, "{}", kept.seconds);
        assert!(kept.extra.contains("stored total-clearing time") && kept.extra.contains("is kept"), "{}", kept.extra);
        assert!(!kept.extra.contains("quarter-cycle time"), "{}", kept.extra);

        device.fuse_total_clearing = false;
        device.breaker_interrupting_s = Some(0.05);
        let table = device_exposure(&device, 25_000.0, true).unwrap();
        assert!((table.seconds - 1.0 / 240.0).abs() < 1e-12, "{}", table.seconds);

        device.fuse_total_clearing = true;
        device.breaker_interrupting_s = None;
        device.curve = CurveSpec::Points {
            points: vec![[100.0, 1.0], [1_000.0, 0.01]],
            max_points: None,
            beyond_last: crate::model::PointTail::HoldLastTime,
            band_interpretation: crate::model::BandInterpretation::Entered,
        };
        let held = device_exposure(&device, 25_000.0, true).unwrap();
        assert!((held.seconds - 1.0 / 240.0).abs() < 1e-12, "{}", held.seconds);
        assert!(held.extra.contains("quarter-cycle"), "{}", held.extra);
    }

    #[test]
    fn cycle_table_sets_sample_energy_and_skips_source_decrement() {
        let mut p = Project::sample();
        p.motors.clear();
        p.equipment.truncate(1);
        p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.5 };
        p.devices[0].breaker_interrupting_s = Some(0.05);
        p.devices[0].current_limiting = Some(CurrentLimiting {
            threshold_a: 1.0, clearing_s: 0.2, effective_arc_ka: Some(8.0), cycle_table: true,
        });
        p.sources[0].decrement_curve = vec![
            DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
            DecrementPoint { time_s: 0.2, current_ratio: 0.5 },
        ];
        let out = run(&p, arc_only()).unwrap();
        let limited = row(&out, "af-mcc");
        assert!((limited.time_s - 1.0 / 240.0).abs() < 1e-12 && (limited.time_min_s - 1.0 / 240.0).abs() < 1e-12);
        assert!((limited.arcing_ka - 8.0).abs() < 1e-9 && (limited.arcing_min_ka - 8.0).abs() < 1e-9);
        assert!(limited.assumed && !limited.decrement_applied);
        assert!(limited.duration_note.contains("quarter-cycle") && limited.duration_note.contains("Annex H"), "{}", limited.duration_note);
        assert_eq!(limited.clearing_basis, "current_limiting_fixed_table");
        assert!(!limited.label_ready && limited.label_reasons.iter().any(|r| r == "current_limiting_fixed_table"), "{:?}", limited.label_reasons);
        assert!(limited.duration_note.contains("was not applied") && limited.duration_note.contains("variation factor"), "{}", limited.duration_note);
        let expected = arcflash::calculate(&IeeeInput {
            voc_kv: 0.48, ibf_ka: limited.bolted_ka, gap_mm: p.equipment[0].gap_mm,
            distance_mm: p.equipment[0].distance_mm, height_mm: p.equipment[0].height_mm,
            width_mm: p.equipment[0].width_mm, depth_mm: p.equipment[0].depth_mm,
            electrode: p.equipment[0].electrode, time_s: 1.0 / 240.0, time_min_s: 1.0 / 240.0,
            arc_ka_override: Some(8.0), arc_min_ka_override: Some(8.0), lee: false,
        }).unwrap();
        assert!((limited.governing_cal_cm2 - expected.governing_cal_cm2).abs() < 1e-9);
    }

    #[test]
    fn coordination_faults_the_protected_side_of_a_line_side_device() {
        let studies = Studies { loadflow: false, fault: true, arcflash: false, coordination: true };
        let coord = |p: &Project, id: &str| run(p, studies).unwrap().coordination.unwrap()
            .into_iter().find(|r| r.device_id == id).unwrap();
        let mut p = Project::sample();
        let load_side = coord(&p, "brk-feeder");
        assert_eq!(load_side.fault_bus_id, "pnl");
        p.devices.iter_mut().find(|d| d.id == "brk-feeder").unwrap().bus = "mcc".into();
        let line_side = coord(&p, "brk-feeder");
        assert_eq!(line_side.fault_bus_id, "pnl");
        assert!(line_side.note.contains("'pnl'") && line_side.note.contains("far terminal"), "{}", line_side.note);
        let pnl_ka = run(&p, studies).unwrap().fault.unwrap().buses.into_iter()
            .find(|b| b.id == "pnl").unwrap().three_phase.unwrap().symmetrical_ka;
        let line_ka = line_side.bolted_ka.unwrap();
        // Series feeder without shunt elements: same current at either terminal,
        // and it carries the whole panel fault (utility plus MCC motor).
        assert!((line_ka - load_side.bolted_ka.unwrap()).abs() < 1e-9 * line_ka);
        assert!((line_ka - pnl_ka).abs() < 1e-6 * pnl_ka, "{line_ka} vs {pnl_ka}");
        assert!(line_side.trip_at_bolted_s.is_some());
        // The main breaker (load-side terminal of T1) is still faulted at its own bus.
        let main = coord(&p, "brk-main");
        assert_eq!(main.fault_bus_id, "mcc");
        assert!(main.note.contains("source-free terminal"), "{}", main.note);
    }

    fn unplaced_main() -> Project {
        let mut p = Project::sample();
        p.motors.clear();
        p.equipment.truncate(1);
        p.equipment[0].upstream_device = Some("brk-main".into());
        p.devices[0].protected_branch = None;
        p
    }

    #[test]
    fn missing_device_terminal_current_is_an_error_in_both_paths() {
        let mut p = unplaced_main();
        let out = run(&p, arc_only()).unwrap();
        let failure = out.arc_flash_failures.iter().find(|f| f.equipment_id == "af-mcc").unwrap();
        assert!(failure.error.contains("Arc flash cannot be calculated for bus mcc"), "{}", failure.error);
        assert!(failure.error.contains("Missing: protected_branch"), "{}", failure.error);

        p.equipment[0].fallback_duration_s = Some(0.5);
        for decrement in [false, true] {
            if decrement {
                p.sources[0].decrement_curve = vec![
                    DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
                    DecrementPoint { time_s: 1.0, current_ratio: 1.0 },
                ];
            }
            let out = run(&p, arc_only()).unwrap();
            let failure = out.arc_flash_failures.iter().find(|f| f.equipment_id == "af-mcc").unwrap();
            assert!(failure.error.contains("Missing: protected_branch"), "decrement {decrement}: {}", failure.error);
            assert!(out.arc_flash.as_ref().unwrap().iter().all(|row| row.equipment_id != "af-mcc"));
        }
    }

    #[test]
    fn every_incomplete_bus_names_its_gap_and_a_complete_bus_still_calculates() {
        let mut p = Project::sample();
        p.motors.clear();
        p.loads.clear();
        for equipment in &mut p.equipment {
            equipment.fallback_duration_s = Some(2.0);
        }
        let (electrode, gap, distance, height, width, depth, _) = typical_equipment(12.47);
        let mut utility = p.equipment[0].clone();
        utility.id = "af-util".into();
        utility.name = "Utility gear".into();
        utility.bus = "util".into();
        utility.electrode = electrode;
        utility.gap_mm = gap;
        utility.distance_mm = distance;
        utility.height_mm = height;
        utility.width_mm = width;
        utility.depth_mm = depth;
        utility.upstream_device = None;
        utility.clearing_s = None;
        utility.fallback_duration_s = Some(2.0);
        p.equipment.push(utility);
        p.devices[1].curve = CurveSpec::SettingsNotCollected { note: "fuse size not on the sheet".into() };
        p.devices[1].breaker_interrupting_s = None;
        let out = run(&p, arc_only()).unwrap();
        let rows = out.arc_flash.as_ref().unwrap();
        assert!(rows.iter().any(|row| row.equipment_id == "af-mcc"), "a bus with a trip curve and breaker interrupting time is still calculated");
        assert!(rows.iter().all(|row| row.time_s != 2.0));
        assert!(rows.iter().all(|row| row.bus_id != "pnl" && row.bus_id != "util"));
        let pnl = out.arc_flash_failures.iter().find(|failure| failure.bus_id == "pnl").unwrap();
        assert!(pnl.error.contains("Arc flash cannot be calculated for bus pnl"), "{}", pnl.error);
        assert!(pnl.error.contains("Missing: a trip curve"), "{}", pnl.error);
        assert!(pnl.error.contains("Panel feeder"), "{}", pnl.error);
        assert!(pnl.error.contains("fuse size not on the sheet"), "{}", pnl.error);
        let util = out.arc_flash_failures.iter().find(|failure| failure.bus_id == "util").unwrap();
        assert!(util.error.contains("Arc flash cannot be calculated for bus util"), "{}", util.error);
        assert!(util.error.contains("Missing: an upstream device"), "{}", util.error);
        let report = text_report(&p, &out);
        assert!(report.contains(&pnl.error), "{report}");
        assert!(report.contains(&util.error), "{report}");
        assert!(!report.contains("2.000 s"), "stored fallback duration must not be reported as arc time:\n{report}");
    }

    #[test]
    fn decrement_rows_state_that_arcing_currents_are_initial_values() {
        let mut p = Project::sample();
        p.motors.clear();
        p.equipment.truncate(1);
        p.devices[0].curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.05 };
        p.devices[0].breaker_interrupting_s = Some(0.04);
        let constant = run(&p, arc_only()).unwrap();
        let constant = row(&constant, "af-mcc");
        assert!(constant.final_arcing_ka.is_none() && constant.final_bolted_ka.is_none());
        assert!(!constant.warnings.iter().any(|w| w.contains("initial (t=0)")));

        p.sources[0].decrement_curve = vec![
            DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
            DecrementPoint { time_s: 0.05, current_ratio: 0.5 },
        ];
        let out = run(&p, arc_only()).unwrap();
        let r = row(&out, "af-mcc");
        assert!(r.warnings.iter().any(|w| w.contains("arcing currents shown are initial (t=0) values")
            || w.contains("Arcing currents shown are initial (t=0) values")), "{:?}", r.warnings);
        assert_eq!(r.arcing_ka, constant.arcing_ka, "initial value is retained");
        let final_bolted = r.final_bolted_ka.unwrap();
        // Half the source strength behind T1 lowers the MCC current by about 13 %.
        assert!(final_bolted < 0.95 * r.bolted_ka, "final {final_bolted}, initial {}", r.bolted_ka);
        assert!(r.final_arcing_ka.unwrap() < r.arcing_ka);
        assert!(r.final_arcing_min_ka.unwrap() < r.arcing_min_ka);
    }
}

#[cfg(test)]
mod operating_case_regression {
    use super::*;
    use crate::model::{Branch, BranchKind, Bus, CurveSpec, EquipmentState, Load, OperatingCase, Prefault, SourceState, Switch, SwitchGroup, SwitchKind, SwitchState};

    fn bus(id: &str) -> Bus {
        Bus { id: id.into(), name: id.into(), kv: 0.48, shunt_kvar: 0.0,
            bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None, prefault_pu: None }
    }

    fn line(id: &str, from: &str, to: &str) -> Branch {
        Branch { id: id.into(), name: id.into(), from: from.into(), to: to.into(),
            kind: BranchKind::Line { r_ohm: 0.002, x_ohm: 0.004, b_siemens: 0.0,
                r0_ohm: 0.006, x0_ohm: 0.012, ampacity_a: None } }
    }

    fn switch(id: &str, branch_id: &str, kind: SwitchKind, closed: bool) -> Switch {
        Switch { id: id.into(), name: id.into(), branch_id: branch_id.into(), kind, closed }
    }

    fn states(id: &str, pairs: &[(&str, bool)]) -> OperatingCase {
        OperatingCase { id: id.into(), name: id.into(),
            switch_states: pairs.iter().map(|(switch_id, closed)| SwitchState {
                switch_id: (*switch_id).into(), closed: *closed,
            }).collect(), source_states: Vec::new(), equipment_states: Vec::new() }
    }

    fn main_tie_main() -> Project {
        let mut project = Project::sample();
        project.buses = ["src_l", "sec_l", "sec_r", "src_r"].map(bus).to_vec();
        project.branches = vec![line("main_l", "src_l", "sec_l"),
            line("tie", "sec_l", "sec_r"), line("main_r", "src_r", "sec_r")];
        let mut left = project.sources[0].clone();
        left.id = "source_l".into(); left.bus = "src_l".into(); left.mva_sc = 300.0;
        let mut right = left.clone();
        right.id = "source_r".into(); right.bus = "src_r".into(); right.mva_sc = 75.0;
        project.sources = vec![left, right];
        project.loads = ["sec_l", "sec_r"].iter().map(|id| Load {
            id: format!("load_{id}"), name: format!("load_{id}"), bus: (*id).into(),
            kw: 40.0, kvar: 10.0, basis: String::new(),
        }).collect();
        project.motors.clear(); project.devices.clear(); project.equipment.clear();
        project.switches = vec![switch("left_main", "main_l", SwitchKind::Breaker, true),
            switch("right_main", "main_r", SwitchKind::Breaker, true),
            switch("bus_tie", "tie", SwitchKind::Tie, false)];
        project.switch_groups = vec![SwitchGroup { id: "main_tie_main".into(),
            switch_ids: vec!["left_main".into(), "right_main".into(), "bus_tie".into()],
            max_closed: 2 }];
        project.operating_cases = vec![states("left_feed", &[("right_main", false), ("bus_tie", true)]),
            states("right_feed", &[("left_main", false), ("bus_tie", true)])];
        project
    }

    fn case<'a>(output: &'a StudyOutput, id: &str) -> &'a StudyOutput {
        output.operating_cases.iter().find(|case| case.id == id).unwrap().result.as_deref().unwrap()
    }

    fn fault_ka(output: &StudyOutput, bus: &str) -> f64 {
        output.fault.as_ref().unwrap().buses.iter().find(|row| row.id == bus).unwrap()
            .three_phase.as_ref().unwrap().symmetrical_ka
    }

    #[test]
    fn main_tie_main_cases_match_physically_removed_open_connections() {
        let project = main_tie_main();
        let requested = Studies { loadflow: true, fault: true, arcflash: false, coordination: false };
        let output = run(&project, requested).unwrap();
        assert_eq!(output.operating_cases.len(), 2);
        for id in ["normal", "left_feed", "right_feed"] {
            let case_project = if id == "normal" { project.clone() } else {
                project.apply_operating_case(project.operating_cases.iter().find(|c| c.id == id).unwrap()).unwrap()
            };
            let actual = if id == "normal" { &output } else { case(&output, id) };
            let mut reference = case_project.clone();
            reference.branches.retain(|branch| case_project.branch_closed(branch));
            reference.switches.clear(); reference.switch_groups.clear(); reference.operating_cases.clear();
            let expected = run(&reference, requested).unwrap();
            for bus_id in ["sec_l", "sec_r"] {
                let got = fault_ka(actual, bus_id);
                let want = fault_ka(&expected, bus_id);
                assert!((got - want).abs() < 1e-8, "{id}: {bus_id} got {got}, expected {want}");
                let got_v = actual.loadflow.as_ref().unwrap().buses.iter().find(|b| b.id == bus_id).unwrap().v_pu;
                let want_v = expected.loadflow.as_ref().unwrap().buses.iter().find(|b| b.id == bus_id).unwrap().v_pu;
                assert!((got_v - want_v).abs() < 1e-10, "{id}: {bus_id} voltage");
            }
        }
        assert!(case(&output, "left_feed").loadflow.as_ref().unwrap().converged);
        assert!(case(&output, "right_feed").loadflow.as_ref().unwrap().converged);
        let envelope = output.worst_case.as_ref().unwrap();
        for bus_id in ["sec_l", "sec_r"] {
            let max = envelope.buses.iter().find(|b| b.bus_id == bus_id).unwrap();
            assert!(max.complete);
            let highest = ["normal", "left_feed", "right_feed"].iter().map(|id| {
                fault_ka(if *id == "normal" { &output } else { case(&output, id) }, bus_id)
            }).fold(0.0_f64, f64::max);
            assert!((max.three_phase_ka.as_ref().unwrap().value - highest).abs() < 1e-8);
            let peak = ["normal", "left_feed", "right_feed"].iter().map(|id| {
                let result = if *id == "normal" { &output } else { case(&output, id) };
                result.fault.as_ref().unwrap().buses.iter().find(|row| row.id == bus_id).unwrap()
                    .three_phase.as_ref().unwrap().iec_peak_ka
            }).fold(0.0_f64, f64::max);
            assert!((max.three_phase_iec_peak_ka.as_ref().unwrap().value - peak).abs() < 1e-8);
            let half_cycle = ["normal", "left_feed", "right_feed"].iter().map(|id| {
                let result = if *id == "normal" { &output } else { case(&output, id) };
                result.fault.as_ref().unwrap().buses.iter().find(|row| row.id == bus_id).unwrap()
                    .three_phase.as_ref().unwrap().half_cycle_rms_ka
            }).fold(0.0_f64, f64::max);
            assert!((max.three_phase_half_cycle_rms_ka.as_ref().unwrap().value - half_cycle).abs() < 1e-8);
        }
    }

    #[test]
    fn ats_generator_transfer_reidentifies_protection_and_governing_arc_case() {
        let mut project = Project::sample();
        project.buses = ["grid_bus", "gen_bus", "load_bus"].map(bus).to_vec();
        project.branches = vec![line("grid_throw", "grid_bus", "load_bus"),
            line("gen_throw", "gen_bus", "load_bus")];
        let mut grid = project.sources[0].clone();
        grid.id = "grid".into(); grid.bus = "grid_bus".into(); grid.mva_sc = 500.0;
        let mut generator = grid.clone();
        generator.id = "generator".into(); generator.bus = "gen_bus".into();
        generator.mva_sc = 35.0; generator.in_service = false;
        project.sources = vec![grid, generator];
        project.loads.clear(); project.motors.clear();
        project.switches = vec![switch("grid_contact", "grid_throw", SwitchKind::Ats, true),
            switch("gen_contact", "gen_throw", SwitchKind::Ats, false)];
        project.switch_groups = vec![SwitchGroup { id: "ats_interlock".into(),
            switch_ids: vec!["grid_contact".into(), "gen_contact".into()], max_closed: 1 }];
        let mut emergency = states("generator_feed", &[("grid_contact", false), ("gen_contact", true)]);
        emergency.source_states = vec![SourceState { source_id: "grid".into(), in_service: false },
            SourceState { source_id: "generator".into(), in_service: true }];
        let mut off = states("off", &[("grid_contact", false)]);
        off.source_states = vec![SourceState { source_id: "grid".into(), in_service: false }];
        project.operating_cases = vec![emergency, off];
        let mut grid_device = project.devices[0].clone();
        grid_device.id = "grid_breaker".into(); grid_device.bus = "load_bus".into();
        grid_device.protected_branch = Some("grid_throw".into());
        grid_device.curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 0.02 };
        grid_device.breaker_interrupting_s = Some(0.03);
        let mut gen_device = grid_device.clone();
        gen_device.id = "gen_breaker".into(); gen_device.protected_branch = Some("gen_throw".into());
        gen_device.curve = CurveSpec::Definite { pickup_a: 1.0, time_s: 1.0 };
        project.devices = vec![grid_device, gen_device];
        let mut equipment = project.equipment[0].clone();
        equipment.id = "load_gear".into(); equipment.bus = "load_bus".into();
        equipment.upstream_device = None;
        project.equipment = vec![equipment];

        let output = run(&project, Studies::all()).unwrap();
        let generator = case(&output, "generator_feed");
        let off = case(&output, "off");
        assert!(off.fault.as_ref().unwrap().buses.iter().find(|b| b.id == "load_bus").unwrap().three_phase.is_none());
        let grid_ka = fault_ka(&output, "load_bus");
        let generator_ka = fault_ka(generator, "load_bus");
        assert!(grid_ka > generator_ka, "grid {grid_ka}, generator {generator_ka}");
        let grid_arc = output.arc_flash.as_ref().unwrap().iter().find(|a| a.equipment_id == "load_gear").unwrap();
        let gen_arc = generator.arc_flash.as_ref().unwrap().iter().find(|a| a.equipment_id == "load_gear").unwrap();
        assert_eq!(grid_arc.upstream_device.as_deref(), Some("grid_breaker"));
        assert_eq!(gen_arc.upstream_device.as_deref(), Some("gen_breaker"));
        assert!(gen_arc.governing_cal_cm2 > grid_arc.governing_cal_cm2,
            "generator energy {}, grid energy {}", gen_arc.governing_cal_cm2, grid_arc.governing_cal_cm2);
        let max = output.worst_case.as_ref().unwrap().arc_equipment.iter()
            .find(|row| row.equipment_id == "load_gear").unwrap();
        assert!(max.complete, "{:?}", max.failed_case_ids);
        assert_eq!(max.incident_energy_cal_cm2.as_ref().unwrap().case_id, "generator_feed");
        assert_eq!(max.boundary_mm.as_ref().unwrap().case_id, "generator_feed");

        // A manual duration and explicit upstream assignment belong to the
        // normal state. The transferred state must identify the generator
        // breaker unless a case-specific equipment override is supplied.
        let mut manual_normal = project.clone();
        manual_normal.equipment[0].clearing_s = Some(0.04);
        manual_normal.equipment[0].upstream_device = Some("grid_breaker".into());
        let manual_output = run(&manual_normal, Studies::all()).unwrap();
        let transferred = case(&manual_output, "generator_feed");
        let transferred_arc = transferred.arc_flash.as_ref().unwrap().iter()
            .find(|row| row.equipment_id == "load_gear").unwrap();
        assert_eq!(transferred_arc.upstream_device.as_deref(), Some("gen_breaker"));
        assert!(transferred_arc.time_s > 1.0, "{}", transferred_arc.time_s);
        assert!(manual_output.operating_cases[0].equipment_states[0].clearing_s.is_none());
        manual_normal.operating_cases[0].equipment_states.push(EquipmentState {
            equipment_id: "load_gear".into(), upstream_device: None,
            clearing_s: Some(0.2), fallback_duration_s: None,
        });
        let overridden = run(&manual_normal, Studies::all()).unwrap();
        let transferred_arc = case(&overridden, "generator_feed").arc_flash.as_ref().unwrap().iter()
            .find(|row| row.equipment_id == "load_gear").unwrap();
        assert!((transferred_arc.time_s - 0.2).abs() < 1e-12);

        // A source supplying an isolated load-flow island needs an explicit
        // slack designation. The unusable load flow switches that case to the
        // no-load-with-tap prefault, which is reported.
        let mut no_generator_slack = project.clone();
        no_generator_slack.prefault = Prefault::Loadflow;
        no_generator_slack.sources[1].is_slack = false;
        let switched = run(&no_generator_slack, Studies::all()).unwrap();
        let generator_case = case(&switched, "generator_feed");
        assert!(generator_case.fault.as_ref().unwrap().valid);
        assert_eq!(generator_case.fault.as_ref().unwrap().prefault, "no_load_tap");
        assert!(generator_case.warnings.iter().any(|w| w.contains("no-load-with-tap")), "{:?}", generator_case.warnings);

        // A break-before-make interlock rejects a case with both throws closed.
        project.operating_cases.push(states("both_closed", &[("gen_contact", true)]));
        assert!(run(&project, Studies::all()).is_err());
    }

    #[test]
    fn voltage_screens_flag_a_bus_outside_five_percent_and_a_line_above_three_percent() {
        use crate::loadflow::{BranchResult, BusResult, LoadflowResult};
        let project = Project {
            branches: vec![Branch {
                id: "feeder".into(),
                name: "Feeder".into(),
                from: "a".into(),
                to: "b".into(),
                kind: BranchKind::Line { r_ohm: 0.1, x_ohm: 0.1, b_siemens: 0.0, r0_ohm: 0.0, x0_ohm: 0.0, ampacity_a: None },
            }],
            ..Project::default()
        };
        let bus = |id: &str, v: f64| BusResult {
            id: id.into(), name: id.into(), kv_nominal: 0.48, v_pu: v, angle_deg: 0.0,
            p_mw: 0.0, q_mvar: 0.0, kind: "pq".into(),
        };
        let lf = LoadflowResult {
            converged: true,
            iterations: 1,
            max_mismatch_pu: 0.0,
            buses: vec![bus("a", 1.0), bus("b", 0.94)],
            branches: vec![BranchResult {
                id: "feeder".into(), name: "Feeder".into(), from: "a".into(), to: "b".into(),
                p_from_mw: 0.0, q_from_mvar: 0.0, p_to_mw: 0.0, q_to_mvar: 0.0,
                i_from_a: 0.0, loading_pct: None, p_loss_mw: 0.0,
            }],
            message: None,
        };
        let notes = voltage_screens(&project, &lf);
        assert!(notes.iter().any(|note| note.contains("b:") && note.contains("0.940 pu")), "{notes:?}");
        assert!(notes.iter().any(|note| note.contains("feeder:") && note.contains("6.00%")), "{notes:?}");
        assert!(notes.iter().all(|note| note.contains("not a code calculation")), "{notes:?}");
        let quiet = LoadflowResult { buses: vec![bus("a", 1.0), bus("b", 0.98)], ..lf };
        assert!(voltage_screens(&project, &quiet).is_empty(), "{:?}", voltage_screens(&project, &quiet));
    }
}
