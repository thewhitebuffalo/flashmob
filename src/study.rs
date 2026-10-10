use serde::{Deserialize, Serialize};

use crate::arcflash::{self, EnergySegment, IeeeInput};
use crate::curves::{self, curve_label};
use crate::fault::{self, FaultResult};
use crate::loadflow::{self, LoadflowResult};
use crate::model::{typical_equipment, ArcEquipment, Project, ASSUMED_ENCLOSURE_ELECTRODES};

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

    let fault = if need_fault {
        Some(fault::solve_with_model(project, loadflow.as_ref(), model.as_ref().map_err(Clone::clone)?)?)
    } else {
        None
    };

    let protection = crate::protection::identify_all(project);
    let mut arc_flash_failures = Vec::new();
    let arc_flash = if studies.arcflash {
        Some(arc_flash(project, loadflow.as_ref(), fault.as_ref().unwrap(), &protection, &mut warnings, &mut arc_flash_failures))
    } else {
        None
    };

    let coordination = if studies.coordination {
        Some(coordination(project, fault.as_ref().unwrap()))
    } else {
        None
    };
    if let Some(error) = fault.as_ref().and_then(|f| f.error.as_ref()) { warnings.push(error.clone()); }
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));

    Ok(StudyOutput {
        case_id: "normal".into(),
        methods: Methods {
            load_flow: "Newton-Raphson positive-sequence power flow".into(),
            short_circuit: "Symmetrical components. Peak current uses the IEC 60909 factor; half-cycle rms is the first-half-cycle asymmetrical value.".into(),
            arc_flash: "IEEE 1584-2018".into(),
            coordination: "IEC 60255, IEEE C37.112, definite time, and a simplified thermal-magnetic breaker".into(),
        },
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        warnings,
        loadflow: if studies.loadflow { loadflow } else { None },
        fault: if studies.fault { fault } else { None },
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

fn arc_flash(project: &Project, loadflow: Option<&LoadflowResult>, fault: &FaultResult, protection: &[crate::protection::Protection], warnings: &mut Vec<String>, failures: &mut Vec<ArcFailure>) -> Vec<ArcRow> {
    let mut rows = Vec::new();
    let equipment_cases = cases(project, protection);
    let mut decrement_cache = DecrementFaultCache::for_cases(project, &equipment_cases);
    for case in equipment_cases {
        // A bus without collected equipment has an unknown electrode
        // configuration: evaluate each candidate with the default geometry
        // and report the maximum, rather than a single nonconservative choice.
        let assumed_enclosure = case.id.starts_with("assumed:");
        let electrodes = if assumed_enclosure { ASSUMED_ENCLOSURE_ELECTRODES.to_vec() } else { vec![case.electrode] };
        let mut evaluated = Vec::new();
        let mut error = None;
        for electrode in electrodes {
            let variant = ArcEquipment { electrode, ..case.clone() };
            match evaluate_equipment(project, loadflow, fault, protection, &variant, &mut decrement_cache) {
                Ok(result) => evaluated.push(result),
                Err(e) => {
                    error = Some(if assumed_enclosure {
                        format!("assumed {} electrode configuration: {e}; the maximum over assumed configurations is unavailable", electrode.as_str())
                    } else { e });
                    break;
                }
            }
        }
        if let Some(error) = error {
            warnings.push(format!("{}: {error}", case.name));
            failures.push(ArcFailure { equipment_id: case.id.clone(), name: case.name.clone(), bus_id: case.bus.clone(), error });
            continue;
        }
        let mut ieee_warnings: Vec<String> = Vec::new();
        for (_, items) in &evaluated {
            for w in items {
                if !ieee_warnings.contains(w) { ieee_warnings.push(w.clone()); }
            }
        }
        let row = if assumed_enclosure {
            let (row, note) = governing_assumed_enclosure(evaluated.into_iter().map(|(row, _)| row).collect());
            warnings.push(format!(
                "{} enclosure dimensions were not collected; the values used are an assumption, not a field measurement. {note}",
                case.name
            ));
            row
        } else {
            let row = evaluated.pop().expect("one electrode evaluated").0;
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

/// Select the configuration with the greatest governing incident energy and
/// take the greatest boundary over all configurations independently.
fn governing_assumed_enclosure(rows: Vec<ArcRow>) -> (ArcRow, String) {
    let energies = rows.iter().map(|row| format!("{} {:.2} cal/cm2", row.electrode, row.governing_cal_cm2))
        .collect::<Vec<_>>().join(", ");
    let boundary_row = rows.iter().max_by(|a, b| a.afb_mm.total_cmp(&b.afb_mm)).expect("assumed configurations");
    let boundary_electrode = boundary_row.electrode.clone();
    let afb_full_mm = rows.iter().map(|row| row.afb_full_mm).fold(f64::MIN, f64::max);
    let afb_reduced_mm = rows.iter().map(|row| row.afb_reduced_mm).fold(f64::MIN, f64::max);
    let mut governing = rows.into_iter()
        .reduce(|best, row| if row.governing_cal_cm2 > best.governing_cal_cm2 { row } else { best })
        .expect("assumed configurations");
    let configurations = ASSUMED_ENCLOSURE_ELECTRODES.map(|e| e.as_str()).join(", ");
    let note = format!(
        "Assumed enclosure: electrode configurations {configurations} were evaluated with the default geometry; {} governs incident energy ({energies}) and {boundary_electrode} governs the arc-flash boundary. The maximum of each is reported.",
        governing.electrode
    );
    governing.afb_full_mm = afb_full_mm;
    governing.afb_reduced_mm = afb_reduced_mm;
    governing.afb_mm = afb_full_mm.max(afb_reduced_mm);
    governing.afb_in = governing.afb_mm / 25.4;
    governing.assumed = true;
    governing.warnings.insert(0, note.clone());
    (governing, note)
}

/// One IEEE 1584 evaluation of an equipment item (or one assumed electrode
/// configuration). Returns the row and the IEEE model warnings for the study.
fn evaluate_equipment(project: &Project, loadflow: Option<&LoadflowResult>, fault: &FaultResult,
    protection: &[crate::protection::Protection], case: &ArcEquipment,
    decrement_cache: &mut DecrementFaultCache) -> Result<(ArcRow, Vec<String>), String> {
    let assumed = case.id.starts_with("assumed:") || case.basis == "assumed";
    let Some(bus) = project.bus(&case.bus) else { return Err("unknown bus".into()) };
    let decrement_source_ids = curved_sources_reaching(project, &case.bus);
    let Some(bus_fault) = fault.buses.iter().find(|b| b.id == bus.id) else { return Err("missing fault result".into()) };
    let Some(bolted) = bus_fault.three_phase.as_ref().map(|p| p.symmetrical_ka) else {
        return Err(bus_fault.note.clone().unwrap_or_else(|| "no three-phase fault current".into()));
    };
    if !(0.208..=15.0).contains(&bus.kv) {
        return Err(format!("{:.2} kV is outside the IEEE 1584-2018 range of 208 V to 15 kV", bus.kv));
    }
    let template = preview_input(case, bus.kv, bolted);
    let preview = arcflash::calculate(&template)?;
    let protection = protection.iter().find(|p| p.bus_id == case.bus).expect("validated bus has protection status");
    let full_clear = if decrement_source_ids.is_empty() {
        duration(project, case, bus_fault, preview.i_arc_ka, protection)
    } else {
        decrement_duration(project, loadflow, case, bus_fault, preview.i_arc_ka, protection,
            &template, false, &decrement_source_ids, decrement_cache)
    }?;
    let min_clear = if decrement_source_ids.is_empty() {
        duration(project, case, bus_fault, preview.i_arc_min_ka, protection)
    } else {
        decrement_duration(project, loadflow, case, bus_fault, preview.i_arc_min_ka, protection,
            &template, true, &decrement_source_ids, decrement_cache)
    }?;
    let input = IeeeInput { time_s: full_clear.seconds, time_min_s: min_clear.seconds, ..preview_input(case, bus.kv, bolted) };
    let (out, final_slice) = if decrement_source_ids.is_empty() {
        (arcflash::calculate(&input)?, None)
    } else {
        let (out, last) = calculate_decrement_arc(project, loadflow, &case.bus, &input, &decrement_source_ids, decrement_cache)?;
        (out, Some(last))
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
        assumed: assumed || full_clear.assumed || min_clear.assumed || out.bolted_out_of_range,
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
        standard: out.standard.into(),
        decrement_applied: !decrement_source_ids.is_empty(),
        decrement_source_ids: decrement_source_ids.clone(),
        final_bolted_ka: final_slice.as_ref().map(|last| last.bolted_ka),
        final_arcing_ka: final_slice.as_ref().map(|last| last.arcing_ka),
        final_arcing_min_ka: final_slice.as_ref().map(|last| last.arcing_min_ka),
        warnings: notes,
    };
    Ok((row, ieee_warnings))
}

/// Advance each entered relay element using the current at its protected
/// terminal, sampled from the full network as the source strengths change.
/// The breaker interruption time starts only after a relay element operates.
fn decrement_duration(project: &Project, loadflow: Option<&LoadflowResult>, equipment: &ArcEquipment,
    fault: &crate::fault::BusFault, i_ka: f64, protection: &crate::protection::Protection,
    template: &IeeeInput, reduced: bool, decrement_source_ids: &[String],
    cache: &mut DecrementFaultCache) -> Result<Clearing, String> {
    if equipment.clearing_s.is_some() {
        return duration(project, equipment, fault, i_ka, protection);
    }
    let fallback = |reason: String| -> Result<Clearing, String> {
        let Some(seconds) = equipment.fallback_duration_s else { return Err(reason) };
        let mut clear = finish_clearing(seconds, project.arc_duration_cap_s,
            &format!("authorized assumed exposure fallback of {seconds:.3} s with source-current decrement"),
            "This cap is not a field trip setting and is not for an arc-flash label.");
        clear.assumed = true;
        clear.note.push_str(&format!(" {reason}. {} No device clearing time is inferred from this assumption.", protection.detail));
        Ok(clear)
    };
    let Some(id) = equipment.upstream_device.as_ref() else { return fallback(protection.detail.clone()) };
    let device = project.device(id).ok_or("upstream device missing")?;
    if let Some(branch) = device.protected_branch.as_deref()
        .and_then(|id| project.branches.iter().find(|branch| branch.id == id)) {
        if !project.branch_closed(branch) {
            return fallback(format!("selected upstream device '{}' is on open branch '{}' in this operating case", device.id, branch.id));
        }
    }
    if device.protected_branch.is_none() {
        // Same as the constant-current path: no placement, no device current.
        return fallback(format!("protected branch terminal current unavailable for upstream device {} [{}]; enter protected_branch and terminal bus", device.name, device.id));
    }
    if !isolates_sources(project, device, &equipment.bus) {
        return Err("opening the selected device does not isolate all utility/generator paths to the fault; a multi-device clearing study is required".into());
    }
    reject_connected_motors(project, device, &equipment.bus)?;
    if device.fuse_total_clearing {
        return fallback(format!("upstream device {} [{}] has a fuse total-clearing curve; its melt and arcing history cannot be inferred from a time-varying relay model", device.name, device.id));
    }
    if device.curve_is_total_clearing {
        return fallback(format!("upstream device {} [{}] has a total-clearing curve (curve_is_total_clearing); the time-varying relay model integrates trip-element operating time, not total clearing, so enter a trip-only curve and breaker_interrupting_s", device.name, device.id));
    }
    let Some(interrupting_s) = device.breaker_interrupting_s else {
        return fallback(format!("upstream device {} [{}] lacks breaker interruption time", device.name, device.id));
    };
    let mut relay = match curves::DynamicRelay::new(&device.curve) {
        Ok(relay) => relay,
        Err(error) => return fallback(format!("upstream device {} [{}]: {error}", device.name, device.id)),
    };
    let bolted = fault.three_phase.as_ref().ok_or("fault current unavailable")?.symmetrical_ka;
    let Some(mut prior_amps) = device_current(project, device, fault, i_ka / bolted) else {
        return fallback(format!("protected branch terminal current unavailable for upstream device {} [{}]; enter protected_branch and terminal bus", device.name, device.id));
    };
    let current_case = if reduced { "reduced" } else { "full" };
    let not_a_label = "This cap is not a field trip setting and is not for an arc-flash label.";
    let reset_note = " Relay progress was reset immediately when sampled current fell below pickup, and an element holding its progress would have operated earlier; verify the device reset setting.";
    // Total clearing beyond the study cap is capped exactly as in the
    // constant-current path; the incident energy is integrated to the cap.
    let finish_trip = |trip_s: f64, reset_matters: bool, point_note: Option<(bool, String)>| -> Result<Clearing, String> {
        let raw = trip_s + interrupting_s;
        let mut clear = finish_clearing(raw, project.arc_duration_cap_s,
            &format!("{} [{}] {current_case}-arcing-current relay trip at {trip_s:.3} s plus entered breaker interruption of {interrupting_s:.3} s (total {raw:.3} s)", device.name, device.id),
            not_a_label);
        if reset_matters {
            clear.assumed = true;
            clear.note.push_str(reset_note);
        }
        add_point_note(&mut clear, point_note);
        Ok(clear)
    };
    let mut sampled_amps = |time_s: f64| -> Result<f64, String> {
        let sampled = cache.sample_at(project, loadflow, &equipment.bus, time_s)?;
        let input = IeeeInput { ibf_ka: sampled.bolted_ka, ..template.clone() };
        let arcing = arcflash::calculate(&input)
            .map_err(|error| format!("time-varying arcing current at {time_s:.3} s: {error}"))?;
        let i_arc_ka = if reduced { arcing.i_arc_min_ka } else { arcing.i_arc_ka };
        device_current_from_terminals(project, device, &sampled.terminal_currents,
            i_arc_ka / sampled.bolted_ka)
            .ok_or_else(|| format!("protected branch terminal current unavailable for upstream device {} [{}] at {time_s:.3} s", device.name, device.id))
    };
    let mut interval_start_s = 0.0;
    let mut prior_sample_s = 0.0;
    let mut slices = 0;
    while project.arc_duration_cap_s - interval_start_s > 1e-12 {
        if slices >= 10_000 {
            return Err("source-current decrement interval exceeds 10,000 slices; reduce the arc duration or provide a shorter exposure limit".into());
        }
        let end_s = next_decrement_end(project, decrement_source_ids, interval_start_s, project.arc_duration_cap_s);
        let midpoint_s = interval_start_s + (end_s - interval_start_s) / 2.0;
        let amps = sampled_amps(midpoint_s)?;
        match advance_relay_over_ramp(&mut relay, prior_amps, amps, midpoint_s - prior_sample_s) {
            Ok(Some(within_s)) => return finish_trip(prior_sample_s + within_s, relay.reset_affects_result(), relay.point_note()),
            Ok(None) => {},
            Err(error) => return fallback(format!("upstream device {} [{}]: {error}", device.name, device.id)),
        }
        prior_sample_s = midpoint_s;
        prior_amps = amps;
        if project.sources.iter().filter(|source| decrement_source_ids.contains(&source.id))
            .any(|source| source.decrement_curve.iter().any(|point| (point.time_s - end_s).abs() < 1e-12)) {
            // Preserve a profile's change of slope, including a brief peak
            // from generator excitation, instead of interpolating across it.
            let amps = sampled_amps(end_s)?;
            match advance_relay_over_ramp(&mut relay, prior_amps, amps, end_s - prior_sample_s) {
                Ok(Some(within_s)) => return finish_trip(prior_sample_s + within_s, relay.reset_affects_result(), relay.point_note()),
                Ok(None) => {},
                Err(error) => return fallback(format!("upstream device {} [{}]: {error}", device.name, device.id)),
            }
            prior_sample_s = end_s;
            prior_amps = amps;
        }
        interval_start_s = end_s;
        slices += 1;
    }
    // The final half-slice can contain a trip or a pickup dropout. Sample the
    // actual study cap so it is never extrapolated from the last midpoint.
    if project.arc_duration_cap_s - prior_sample_s > 1e-12 {
        let amps = sampled_amps(project.arc_duration_cap_s)?;
        match advance_relay_over_ramp(&mut relay, prior_amps, amps, project.arc_duration_cap_s - prior_sample_s) {
            Ok(Some(within_s)) => return finish_trip(prior_sample_s + within_s, relay.reset_affects_result(), relay.point_note()),
            Ok(None) => {},
            Err(error) => return fallback(format!("upstream device {} [{}]: {error}", device.name, device.id)),
        }
        prior_amps = amps;
    }
    // Still timing at the cap: the device operates later than the study limit,
    // which the constant-current path reports as a capped duration. A device
    // below every pickup at the cap (held final current) may never operate and
    // stays unresolved, as a static curve with no operating time does.
    match relay.timing_at(prior_amps) {
        Ok(true) => {
            let cap = project.arc_duration_cap_s;
            let mut clear = Clearing {
                seconds: cap,
                capped: true,
                assumed: false,
                note: format!("Arc duration is the {cap:.1} s cap. {} [{}] {current_case}-arcing-current relay operation plus entered breaker interruption of {interrupting_s:.3} s exceeds the cap: the relay is still timing at {prior_amps:.0} A through its protected terminal under the sampled decrement profile. {not_a_label}",
                    device.name, device.id),
            };
            if relay.reset_affects_result() {
                clear.assumed = true;
                clear.note.push_str(reset_note);
            }
            add_point_note(&mut clear, relay.point_note());
            Ok(clear)
        }
        Ok(false) => fallback(format!("upstream device {} [{}] did not trip within the {:.3} s study cap under the sampled {} arcing-current profile and is below every entered pickup at {prior_amps:.0} A at the cap; total clearing is unresolved",
            device.name, device.id, project.arc_duration_cap_s, current_case)),
        Err(error) => fallback(format!("upstream device {} [{}]: {error}", device.name, device.id)),
    }
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

fn calculate_decrement_arc(project: &Project, loadflow: Option<&LoadflowResult>, bus_id: &str, input: &IeeeInput, decrement_source_ids: &[String], cache: &mut DecrementFaultCache) -> Result<(arcflash::IeeeOutput, FinalSlice), String> {
    let mut out = arcflash::calculate(input)?;
    let full_segments = decrement_segments(project, loadflow, bus_id, input.time_s, decrement_source_ids, cache)?;
    let reduced_segments = if (input.time_s - input.time_min_s).abs() < 1e-12 {
        full_segments.clone()
    } else {
        decrement_segments(project, loadflow, bus_id, input.time_min_s, decrement_source_ids, cache)?
    };
    let full = arcflash::integrate_profile(input, &full_segments, false)?;
    let reduced = arcflash::integrate_profile(input, &reduced_segments, true)?;
    let last_current = |segments: &[EnergySegment]| segments.last().map(|s| s.ibf_ka)
        .ok_or_else(|| "decrement profile has no slices".to_string());
    let final_bolted_ka = last_current(&full_segments)?;
    let final_reduced_bolted_ka = last_current(&reduced_segments)?;
    let final_slice = FinalSlice {
        bolted_ka: final_bolted_ka,
        arcing_ka: arcflash::calculate(&IeeeInput { ibf_ka: final_bolted_ka, ..input.clone() })?.i_arc_ka,
        arcing_min_ka: arcflash::calculate(&IeeeInput { ibf_ka: final_reduced_bolted_ka, ..input.clone() })?.i_arc_min_ka,
    };
    out.bolted_out_of_range |= full.bolted_out_of_range || reduced.bolted_out_of_range;
    out.energy_cal_cm2 = full.energy_cal_cm2;
    out.energy_min_cal_cm2 = reduced.energy_cal_cm2;
    out.energy_j_cm2 = full.energy_cal_cm2 * 4.184;
    out.energy_min_j_cm2 = reduced.energy_cal_cm2 * 4.184;
    out.afb_full_mm = full.boundary_mm;
    out.afb_reduced_mm = reduced.boundary_mm;
    out.afb_mm = full.boundary_mm.max(reduced.boundary_mm);
    out.afb_in = out.afb_mm / 25.4;
    if reduced.energy_cal_cm2 >= full.energy_cal_cm2 {
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
    out.warnings.push("Source-current decrement was applied with repeated quasi-steady three-phase network solves and piecewise IEEE 1584 incident-energy integration; source X/R was held constant.".into());
    Ok((out, final_slice))
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

struct DecrementFaultCache {
    /// One network solve at each sampled time serves every arc-equipment item
    /// and both the full and reduced-current integrations in this case.
    faults_by_time: std::collections::HashMap<u64, std::collections::HashMap<String, SampledBusFault>>,
    /// Retain only terminal currents used by equipment's selected device.
    protected_branches_by_bus: std::collections::HashMap<String, std::collections::HashSet<String>>,
}

struct SampledBusFault {
    bolted_ka: f64,
    terminal_currents: Vec<crate::fault::TerminalCurrent>,
}

impl DecrementFaultCache {
    fn for_cases(project: &Project, cases: &[ArcEquipment]) -> Self {
        let mut protected_branches_by_bus: std::collections::HashMap<String, std::collections::HashSet<String>> = std::collections::HashMap::new();
        for equipment in cases {
            let branch = equipment.upstream_device.as_deref()
                .and_then(|id| project.device(id))
                .and_then(|device| device.protected_branch.as_ref());
            if let Some(branch) = branch {
                protected_branches_by_bus.entry(equipment.bus.clone()).or_default().insert(branch.clone());
            }
        }
        Self { faults_by_time: std::collections::HashMap::new(), protected_branches_by_bus }
    }

    fn sample_at(&mut self, project: &Project, loadflow: Option<&LoadflowResult>, bus_id: &str, midpoint_s: f64) -> Result<&SampledBusFault, String> {
        let key = midpoint_s.to_bits();
        if !self.faults_by_time.contains_key(&key) {
            let mut snapshot = project.clone();
            for source in snapshot.sources.iter_mut().filter(|source| source.in_service && !source.decrement_curve.is_empty()) {
                let ratio = source.decrement_ratio_at(midpoint_s);
                source.mva_sc *= ratio;
            }
            let sampled_fault = fault::solve(&snapshot, loadflow)?;
            if !sampled_fault.valid {
                return Err(sampled_fault.error.unwrap_or_else(|| "time-varying fault result is invalid".into()));
            }
            let faults_by_bus = sampled_fault.buses.into_iter().filter_map(|bus| {
                let terminals = bus.terminal_currents.into_iter()
                    .filter(|terminal| self.protected_branches_by_bus.get(&bus.id)
                        .is_some_and(|branches| branches.contains(&terminal.branch_id)))
                    .collect();
                bus.three_phase.map(|point| (bus.id, SampledBusFault {
                    bolted_ka: point.symmetrical_ka,
                    terminal_currents: terminals,
                }))
            }).collect();
            self.faults_by_time.insert(key, faults_by_bus);
        }
        self.faults_by_time[&key].get(bus_id)
            .ok_or_else(|| format!("no three-phase fault current at bus '{bus_id}' during decrement profile"))
    }

    fn bolted_at(&mut self, project: &Project, loadflow: Option<&LoadflowResult>, bus_id: &str, midpoint_s: f64) -> Result<f64, String> {
        Ok(self.sample_at(project, loadflow, bus_id, midpoint_s)?.bolted_ka)
    }
}

fn decrement_segments(project: &Project, loadflow: Option<&LoadflowResult>, bus_id: &str, duration_s: f64, decrement_source_ids: &[String], cache: &mut DecrementFaultCache) -> Result<Vec<EnergySegment>, String> {
    let mut segments = Vec::new();
    let mut time_s = 0.0;
    while duration_s - time_s > 1e-12 {
        if segments.len() >= 10_000 {
            return Err("source-current decrement interval exceeds 10,000 slices; reduce the arc duration or provide a shorter exposure limit".into());
        }
        let end_s = next_decrement_end(project, decrement_source_ids, time_s, duration_s);
        let width_s = end_s - time_s;
        let midpoint_s = time_s + width_s / 2.0;
        let bolted = cache.bolted_at(project, loadflow, bus_id, midpoint_s)?;
        segments.push(EnergySegment { ibf_ka: bolted, duration_s: width_s });
        time_s = end_s;
    }
    Ok(segments)
}

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

struct Clearing {
    seconds: f64,
    capped: bool,
    note: String,
    assumed: bool,
}

fn duration(project: &Project, case: &ArcEquipment, fault: &crate::fault::BusFault, i_ka: f64, protection: &crate::protection::Protection) -> Result<Clearing, String> {
    let cap = project.arc_duration_cap_s;
    let not_a_label = "This cap is not a field trip setting and is not for an arc-flash label.";
    if let Some(manual) = case.clearing_s {
        let basis = if case.basis == "assumed" { "assumed exposure duration" } else { "manual duration" };
        let mut clear = finish_clearing(manual, cap, &format!("{basis} of {manual:.3} s"), not_a_label);
        clear.assumed = case.basis == "assumed";
        clear.note.push_str(&format!(" {} Manual duration overrides device-based clearing.", protection.detail));
        return Ok(clear);
    }
    let fallback = |reason: String| -> Result<Clearing, String> {
        let Some(seconds) = case.fallback_duration_s else { return Err(reason) };
        let mut clear = finish_clearing(seconds, cap, &format!("authorized assumed exposure fallback of {seconds:.3} s"), not_a_label);
        clear.assumed = true;
        clear.note.push_str(&format!(" {reason}. {} No device clearing time is inferred from this assumption.", protection.detail));
        Ok(clear)
    };
    let Some(id) = case.upstream_device.as_ref() else { return fallback(protection.detail.clone()) };
    let device = project.device(id).ok_or("upstream device missing")?;
    if let Some(branch) = device.protected_branch.as_deref()
        .and_then(|id| project.branches.iter().find(|branch| branch.id == id)) {
        if !project.branch_closed(branch) {
            return fallback(format!("selected upstream device '{}' is on open branch '{}' in this operating case", device.id, branch.id));
        }
    }
    let bolted = fault.three_phase.as_ref().ok_or("fault current unavailable")?.symmetrical_ka;
    let Some(amps) = device_current(project, device, fault, i_ka / bolted) else {
        return fallback(format!("protected branch terminal current unavailable for upstream device {} [{}]; enter protected_branch and terminal bus", device.name, device.id));
    };
    if !isolates_sources(project, device, &case.bus) {
        return Err("opening the selected device does not isolate all utility/generator paths to the fault; a multi-device clearing study is required".into());
    }
    reject_connected_motors(project, device, &case.bus)?;
    let point_note = curves::point_curve_note(&device.curve, amps);
    let Some(raw) = curves::clearing_time(device, amps) else {
        let detail = point_note.map(|(_, note)| format!(" {note}")).unwrap_or_default();
        return fallback(format!("Upstream device {} [{}] identified; total clearing unavailable at {amps:.0} A: enter operating settings and breaker interrupting time, or a fuse total-clearing curve; device may not operate at this current.{detail}", device.name, device.id));
    };
    let mut clear = finish_clearing(raw, cap, &format!("total clearing time of {} at {amps:.0} A through its protected terminal (network terminal current; loadflow prefault includes initial branch flow)", device.name), not_a_label);
    add_point_note(&mut clear, point_note);
    Ok(clear)
}

/// Report point-curve assumptions (held tail, assumed band interpretation)
/// on a clearing result. Point curves always use their slower (max) band here.
fn add_point_note(clear: &mut Clearing, point_note: Option<(bool, String)>) {
    if let Some((assumed, note)) = point_note {
        clear.assumed |= assumed;
        clear.note.push(' ');
        clear.note.push_str(&note);
    }
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
fn reject_connected_motors(project: &Project, device: &crate::model::Device, fault_bus: &str) -> Result<(), String> {
    let seen = buses_after_device_opens(project, device, fault_bus);
    let mut motors: Vec<&str> = project.motors.iter()
        .filter(|motor| seen.contains(&motor.bus))
        .map(|motor| motor.id.as_str()).collect();
    if motors.is_empty() { return Ok(()); }
    motors.sort_unstable();
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

fn finish_clearing(raw: f64, cap: f64, what: &str, not_a_label: &str) -> Clearing {
    if raw > cap {
        Clearing {
            seconds: cap,
            capped: true,
            assumed: false,
            note: format!("Arc duration is the {cap:.1} s cap. The {what} exceeds the cap. {not_a_label}"),
        }
    } else {
        Clearing { seconds: raw, capped: false, assumed: false, note: format!("Arc duration {raw:.3} s is the {what}.") }
    }
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
                note: format!("Current is the protected branch terminal contribution for a three-phase fault at bus '{fault_bus}' ({location}); operating and total-clearing times are separate. Unassigned terminals/times are unavailable.{band_note}{point_note}"),
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
        }
        if let Some(bracing) = bus.bracing_ka {
            lines.push(format!(
                "    Equipment bracing  {bracing:.0} kA AIC, collected from the gear. This is not the calculated available fault."
            ));
        }
        if let Some(main) = bus.main_rating_a {
            lines.push(format!("    Main rating  {main:.0} A, collected. Trip-unit delay and instantaneous are not this rating."));
        }
        if let Some(rows) = &out.arc_flash {
            let mine: Vec<_> = rows.iter().filter(|row| row.bus_id == bus.id).collect();
            if mine.is_empty() {
                lines.push("    Arc flash  no IEEE 1584-2018 result for this bus.".into());
            }
            for row in mine {
                lines.push(format!("    {}", row.duration_note));
                if row.decrement_applied {
                    lines.push(format!("    Source-current decrement integrated for: {}", row.decrement_source_ids.join(", ")));
                }
                lines.push(format!(
                    "    IEEE 1584-2018  {:.2} cal/cm2  AFB {:.1} in  ({})",
                    row.governing_cal_cm2, row.afb_in, row.governing
                ));
                lines.push(format!("    AFB full {:.1} mm; reduced {:.1} mm; maximum {:.1} mm", row.afb_full_mm, row.afb_reduced_mm, row.afb_mm));
                if row.assumed {
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
    use crate::model::{CurveSpec, DecrementPoint, Electrode};

    fn arc_only() -> Studies { Studies { loadflow: false, fault: true, arcflash: true, coordination: false } }

    fn row<'a>(out: &'a StudyOutput, id: &str) -> &'a ArcRow {
        out.arc_flash.as_ref().unwrap().iter().find(|r| r.equipment_id == id)
            .unwrap_or_else(|| panic!("no row {id}; failures {:?}", out.arc_flash_failures))
    }

    #[test]
    fn assumed_enclosure_reports_maximum_over_box_electrode_configurations() {
        let mut p = Project::sample();
        p.equipment.retain(|e| e.id != "af-pnl");
        let out = run(&p, arc_only()).unwrap();
        let assumed = row(&out, "assumed:pnl").clone();
        assert!(assumed.assumed);

        // Evaluate each configuration explicitly with the default geometry.
        let (_, gap, distance, height, width, depth, _) = typical_equipment(0.48);
        let mut by_electrode = Vec::new();
        for electrode in ASSUMED_ENCLOSURE_ELECTRODES {
            let mut q = p.clone();
            q.equipment.push(ArcEquipment {
                id: "explicit".into(), name: "explicit".into(), bus: "pnl".into(), electrode,
                gap_mm: gap, distance_mm: distance, height_mm: height, width_mm: width, depth_mm: depth,
                upstream_device: None, clearing_s: None, fallback_duration_s: None, basis: "assumed".into(),
            });
            by_electrode.push(row(&run(&q, arc_only()).unwrap(), "explicit").clone());
        }
        let max_energy = by_electrode.iter().map(|r| r.governing_cal_cm2).fold(0.0, f64::max);
        let max_boundary = by_electrode.iter().map(|r| r.afb_mm).fold(0.0, f64::max);
        let governing = by_electrode.iter().find(|r| r.governing_cal_cm2 == max_energy).unwrap();
        assert_eq!(assumed.governing_cal_cm2, max_energy);
        assert_eq!(assumed.afb_mm, max_boundary);
        assert_eq!(assumed.electrode, governing.electrode);
        // VCB alone, the former assumption, is not the conservative choice here.
        let vcb = by_electrode.iter().find(|r| r.electrode == "VCB").unwrap();
        assert!(max_energy > vcb.governing_cal_cm2, "VCB {} max {max_energy}", vcb.governing_cal_cm2);
        assert!(assumed.warnings[0].contains(&format!("{} governs incident energy", governing.electrode)), "{:?}", assumed.warnings);
        assert!(out.warnings.iter().any(|w| w.contains("Assumed enclosure") && w.contains("VCB, VCBB, HCB")));
        // Explicit equipment keeps its entered configuration.
        assert_eq!(row(&run(&Project::sample(), arc_only()).unwrap(), "af-pnl").electrode, Electrode::VCB.as_str());
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
    fn missing_device_terminal_current_uses_authorized_fallback_in_both_paths() {
        let mut p = unplaced_main();
        let out = run(&p, arc_only()).unwrap();
        let failure = out.arc_flash_failures.iter().find(|f| f.equipment_id == "af-mcc").unwrap();
        assert!(failure.error.contains("terminal current unavailable"), "{}", failure.error);

        p.equipment[0].fallback_duration_s = Some(0.5);
        for decrement in [false, true] {
            if decrement {
                p.sources[0].decrement_curve = vec![
                    DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
                    DecrementPoint { time_s: 1.0, current_ratio: 1.0 },
                ];
            }
            let out = run(&p, arc_only()).unwrap();
            let r = row(&out, "af-mcc");
            assert_eq!(r.time_s, 0.5, "decrement {decrement}");
            assert_eq!(r.time_min_s, 0.5);
            assert!(r.assumed);
            assert!(r.duration_note.contains("assumed exposure fallback") && r.duration_note.contains("terminal current unavailable"), "{}", r.duration_note);
            assert_eq!(r.decrement_applied, decrement);
        }
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
            bracing_ka: None, main_rating_a: None }
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
        // slack designation. An invalid prefault must not enter the envelope.
        let mut no_generator_slack = project.clone();
        no_generator_slack.prefault = Prefault::Loadflow;
        no_generator_slack.sources[1].is_slack = false;
        let incomplete = run(&no_generator_slack, Studies::all()).unwrap();
        assert!(!case(&incomplete, "generator_feed").fault.as_ref().unwrap().valid);
        let generator_arc = incomplete.worst_case.as_ref().unwrap().arc_equipment.iter()
            .find(|row| row.equipment_id == "load_gear").unwrap();
        assert!(!generator_arc.complete);
        assert!(generator_arc.failed_case_ids.iter().any(|id| id == "generator_feed"));

        // A break-before-make interlock rejects a case with both throws closed.
        project.operating_cases.push(states("both_closed", &[("gen_contact", true)]));
        assert!(run(&project, Studies::all()).is_err());
    }
}
