use serde::{Deserialize, Serialize};

use crate::arcflash::{self, IeeeInput};
use crate::curves::{self, curve_label};
use crate::fault::{self, FaultResult};
use crate::loadflow::{self, LoadflowResult};
use crate::model::{typical_equipment, ArcEquipment, Project};

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
    pub methods: Methods,
    pub elapsed_ms: f64,
    pub warnings: Vec<String>,
    pub loadflow: Option<LoadflowResult>,
    pub fault: Option<FaultResult>,
    pub arc_flash: Option<Vec<ArcRow>>,
    pub coordination: Option<Vec<CoordRow>>,
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
    pub bolted_ka: f64,
    pub arcing_ka: f64,
    pub arcing_min_ka: f64,
    pub time_s: f64,
    pub time_min_s: f64,
    pub energy_cal_cm2: f64,
    pub energy_min_cal_cm2: f64,
    pub governing_cal_cm2: f64,
    pub afb_in: f64,
    pub afb_mm: f64,
    pub governing: String,
    pub duration_capped: bool,
    /// States whether the printed seconds are the upstream device time or the 2.0 s cap.
    pub duration_note: String,
    pub upstream_device: Option<String>,
    pub standard: String,
    pub warnings: Vec<String>,
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
}

pub fn run(project: &Project, studies: Studies) -> Result<StudyOutput, String> {
    let errors = project.validate();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let started = std::time::Instant::now();
    let mut warnings = Vec::new();
    if let Ok(model) = crate::network::build(project) {
        warnings.extend(model.warnings);
    }
    let need_lf = studies.loadflow || project.prefault == crate::model::Prefault::Loadflow;
    let need_fault = studies.fault || studies.arcflash || studies.coordination;

    let loadflow = if need_lf {
        match loadflow::solve(project) {
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
        Some(fault::solve(project, loadflow.as_ref())?)
    } else {
        None
    };

    let arc_flash = if studies.arcflash {
        Some(arc_flash(project, fault.as_ref().unwrap(), &mut warnings))
    } else {
        None
    };

    let coordination = if studies.coordination {
        Some(coordination(project, fault.as_ref().unwrap()))
    } else {
        None
    };
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));

    Ok(StudyOutput {
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
    })
}

fn arc_flash(project: &Project, fault: &FaultResult, warnings: &mut Vec<String>) -> Vec<ArcRow> {
    let mut rows = Vec::new();
    for case in cases(project) {
        let assumed = case.id.starts_with("assumed:") || case.basis == "assumed";
        let Some(bus) = project.bus(&case.bus) else { continue };
        let Some(bus_fault) = fault.buses.iter().find(|b| b.id == bus.id) else { continue };
        let Some(bolted) = bus_fault.three_phase.as_ref().map(|p| p.symmetrical_ka) else {
            warnings.push(format!("{} has no three-phase fault current", case.name));
            continue;
        };
        if !(0.208..=15.0).contains(&bus.kv) {
            warnings.push(format!(
                "{} is {:.2} kV, outside the IEEE 1584-2018 range of 208 V to 15 kV",
                case.name, bus.kv
            ));
            continue;
        }
        let preview = IeeeInput {
            voc_kv: bus.kv,
            ibf_ka: bolted,
            gap_mm: case.gap_mm,
            distance_mm: case.distance_mm,
            height_mm: case.height_mm,
            width_mm: case.width_mm,
            depth_mm: case.depth_mm,
            electrode: case.electrode,
            time_s: 0.1,
            time_min_s: 0.1,
        };
        let preview = match arcflash::calculate(&preview) {
            Ok(v) => v,
            Err(err) => {
                warnings.push(format!("{}: {err}", case.name));
                continue;
            }
        };
        let full_clear = duration(project, &case, bus.kv, preview.i_arc_ka);
        let min_clear = duration(project, &case, bus.kv, preview.i_arc_min_ka);
        let input = IeeeInput { time_s: full_clear.seconds, time_min_s: min_clear.seconds, ..preview_input(&case, bus.kv, bolted) };
        match arcflash::calculate(&input) {
            Ok(out) => {
                if assumed {
                    warnings.push(format!(
                        "{} enclosure dimensions were not collected; the values used are an assumption, not a field measurement.",
                        case.name
                    ));
                }
                for w in &out.warnings {
                    warnings.push(format!("{}: {w}", case.name));
                }
                let governing_clear = if out.governing == "reduced_arcing" { &min_clear } else { &full_clear };
                rows.push(ArcRow {
                    equipment_id: case.id,
                    name: case.name,
                    bus_id: bus.id.clone(),
                    bus_name: bus.name.clone(),
                    assumed,
                    electrode: case.electrode.as_str().into(),
                    bolted_ka: bolted,
                    arcing_ka: out.i_arc_ka,
                    arcing_min_ka: out.i_arc_min_ka,
                    time_s: out.time_s,
                    time_min_s: out.time_min_s,
                    energy_cal_cm2: out.energy_cal_cm2,
                    energy_min_cal_cm2: out.energy_min_cal_cm2,
                    governing_cal_cm2: out.governing_cal_cm2,
                    afb_in: out.afb_in,
                    afb_mm: out.afb_mm,
                    governing: out.governing.into(),
                    duration_capped: governing_clear.capped,
                    duration_note: governing_clear.note.clone(),
                    upstream_device: case.upstream_device.clone(),
                    standard: out.standard.into(),
                    warnings: out.warnings,
                });
            }
            Err(err) => warnings.push(format!("{}: {err}", case.name)),
        }
    }
    rows
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

fn cases(project: &Project) -> Vec<ArcEquipment> {
    let mut cases = project.equipment.clone();
    for bus in &project.buses {
        if cases.iter().any(|e| e.bus == bus.id) {
            continue;
        }
        let (electrode, gap, distance, height, width, depth, label) = typical_equipment(bus.kv);
        let on_bus: Vec<_> = project.devices.iter().filter(|d| d.bus == bus.id).collect();
        let upstream = if on_bus.len() == 1 { Some(on_bus[0].id.clone()) } else { None };
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
            basis: "assumed".into(),
        });
    }
    cases
}

struct Clearing {
    seconds: f64,
    capped: bool,
    note: String,
}

fn duration(project: &Project, case: &ArcEquipment, bus_kv: f64, i_ka: f64) -> Clearing {
    let cap = project.arc_duration_cap_s;
    let not_a_label = "This cap is not a field trip setting and is not for an arc-flash label.";
    if let Some(manual) = case.clearing_s {
        return finish_clearing(manual, cap, &format!("manual clearing time of {manual:.3} s"), not_a_label);
    }
    let Some(id) = &case.upstream_device else {
        return Clearing {
            seconds: cap,
            capped: true,
            note: format!("Arc duration is the {cap:.1} s cap. No upstream device was collected. {not_a_label}"),
        };
    };
    let Some(device) = project.device(id) else {
        return Clearing {
            seconds: cap,
            capped: true,
            note: format!("Arc duration is the {cap:.1} s cap. Upstream device {id} is missing. {not_a_label}"),
        };
    };
    if let crate::model::CurveSpec::SettingsNotCollected { note } = &device.curve {
        let detail = if note.trim().is_empty() {
            "The device was collected but its trip settings were not.".to_string()
        } else {
            note.trim().to_string()
        };
        return Clearing {
            seconds: cap,
            capped: true,
            note: format!("Arc duration is the {cap:.1} s cap. {detail} {not_a_label}"),
        };
    }
    let dev_kv = project.bus(&device.bus).map(|b| b.kv).unwrap_or(bus_kv);
    let amps = i_ka * 1000.0 * bus_kv / dev_kv;
    let assumed = if device.basis == "assumed" {
        " The trip curve is an assumption, not a collected trip unit."
    } else {
        ""
    };
    match curves::trip_time(&device.curve, amps) {
        Some(raw) if raw > cap => Clearing {
            seconds: cap,
            capped: true,
            note: format!(
                "Arc duration is the {cap:.1} s cap. Upstream device {} calculates {raw:.3} s at {amps:.0} A, which exceeds the cap. {not_a_label}{assumed}",
                device.name
            ),
        },
        Some(raw) => Clearing {
            seconds: raw,
            capped: false,
            note: format!(
                "Arc duration {raw:.3} s is the upstream device {} time at {amps:.0} A arcing current.{assumed}",
                device.name
            ),
        },
        None => Clearing {
            seconds: cap,
            capped: true,
            note: format!(
                "Arc duration is the {cap:.1} s cap. Upstream device {} does not clear {amps:.0} A. {not_a_label}{assumed}",
                device.name
            ),
        },
    }
}

fn finish_clearing(raw: f64, cap: f64, what: &str, not_a_label: &str) -> Clearing {
    if raw > cap {
        Clearing {
            seconds: cap,
            capped: true,
            note: format!("Arc duration is the {cap:.1} s cap. The {what} exceeds the cap. {not_a_label}"),
        }
    } else {
        Clearing { seconds: raw, capped: false, note: format!("Arc duration {raw:.3} s is the {what}.") }
    }
}

fn coordination(project: &Project, fault: &FaultResult) -> Vec<CoordRow> {
    project
        .devices
        .iter()
        .map(|device| {
            let kv = project.bus(&device.bus).map(|b| b.kv).unwrap_or(1.0);
            let bolted = fault
                .buses
                .iter()
                .find(|b| b.id == device.bus)
                .and_then(|b| b.three_phase.as_ref().map(|p| p.symmetrical_ka));
            let trip = bolted.and_then(|ka| curves::trip_time_device(device, ka * 1000.0));
            CoordRow {
                device_id: device.id.clone(),
                name: device.name.clone(),
                bus_id: device.bus.clone(),
                bus_kv: kv,
                curve: curve_label(&device.curve),
                bolted_ka: bolted,
                trip_at_bolted_s: trip,
            }
        })
        .collect()
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
        if let Some(lf) = out.loadflow.as_ref().and_then(|lf| lf.buses.iter().find(|row| row.id == bus.id)) {
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
                lines.push(format!(
                    "    IEEE 1584-2018  {:.2} cal/cm2  AFB {:.1} in  ({})",
                    row.governing_cal_cm2, row.afb_in, row.governing
                ));
                if row.assumed {
                    lines.push("    Enclosure gap, working distance, and electrode configuration are an assumption, not a field measurement.".into());
                }
            }
        }
    }
    if !out.warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings".into());
        for w in &out.warnings {
            lines.push(format!("  {w}"));
        }
    }
    lines.join("\n")
}

