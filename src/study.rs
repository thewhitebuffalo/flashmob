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
    pub arc_flash_failures: Vec<ArcFailure>,
    #[serde(default)]
    pub protection: Vec<crate::protection::Protection>,
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
    pub note: String,
}

pub fn run(project: &Project, studies: Studies) -> Result<StudyOutput, String> {
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
        Some(arc_flash(project, fault.as_ref().unwrap(), &protection, &mut warnings, &mut arc_flash_failures))
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
    })
}

fn arc_flash(project: &Project, fault: &FaultResult, protection: &[crate::protection::Protection], warnings: &mut Vec<String>, failures: &mut Vec<ArcFailure>) -> Vec<ArcRow> {
    let mut rows = Vec::new();
    for case in cases(project, protection) {
        let mut fail = |error: String| {
            warnings.push(format!("{}: {error}", case.name));
            failures.push(ArcFailure { equipment_id: case.id.clone(), name: case.name.clone(), bus_id: case.bus.clone(), error });
        };
        let assumed = case.id.starts_with("assumed:") || case.basis == "assumed";
        let Some(bus) = project.bus(&case.bus) else { fail("unknown bus".into()); continue };
        let Some(bus_fault) = fault.buses.iter().find(|b| b.id == bus.id) else { fail("missing fault result".into()); continue };
        let Some(bolted) = bus_fault.three_phase.as_ref().map(|p| p.symmetrical_ka) else {
            fail(bus_fault.note.clone().unwrap_or_else(|| "no three-phase fault current".into()));
            continue;
        };
        if !(0.208..=15.0).contains(&bus.kv) {
            fail(format!("{:.2} kV is outside the IEEE 1584-2018 range of 208 V to 15 kV", bus.kv));
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
                fail(err);
                continue;
            }
        };
        let protection = protection.iter().find(|p| p.bus_id == case.bus).expect("validated bus has protection status");
        let full_clear = match duration(project, &case, bus_fault, preview.i_arc_ka, protection) {
            Ok(v) => v, Err(e) => { fail(e); continue; }
        };
        let min_clear = match duration(project, &case, bus_fault, preview.i_arc_min_ka, protection) {
            Ok(v) => v, Err(e) => { fail(e); continue; }
        };
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
                    assumed: assumed || full_clear.assumed || min_clear.assumed,
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
                    warnings: {
                        let mut notes = out.warnings;
                        for clear in [&full_clear, &min_clear] {
                            if clear.assumed && !notes.contains(&clear.note) { notes.push(clear.note.clone()); }
                        }
                        notes
                    },
                });
            }
            Err(err) => fail(err),
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
    let bolted = fault.three_phase.as_ref().ok_or("fault current unavailable")?.symmetrical_ka;
    let amps = device_current(project, device, fault, i_ka / bolted).ok_or("protected branch terminal current unavailable; enter protected_branch and terminal bus")?;
    if !isolates_sources(project, device, &case.bus) {
        return Err("opening the selected device does not isolate all utility/generator paths to the fault; a multi-device clearing study is required".into());
    }
    let Some(raw) = curves::clearing_time(device, amps) else {
        return fallback(format!("Upstream device {} [{}] identified; total clearing unavailable at {amps:.0} A: enter operating settings and breaker interrupting time, or a fuse total-clearing curve; device may not operate at this current", device.name, device.id));
    };
    Ok(finish_clearing(raw, cap, &format!("total clearing time of {} at {amps:.0} A through its protected terminal (network terminal current; loadflow prefault includes initial branch flow)", device.name), not_a_label))
}

fn device_current(project: &Project, device: &crate::model::Device, fault: &crate::fault::BusFault, fraction: f64) -> Option<f64> {
    let id = device.protected_branch.as_ref()?;
    let branch = project.branches.iter().find(|b| b.id == *id)?;
    let current = fault.terminal_currents.iter().find(|c| c.branch_id == *id)?;
    let (pre, inc) = if device.bus == branch.from { (current.from_prefault_a, current.from_increment_a) }
        else if device.bus == branch.to { (current.to_prefault_a, current.to_increment_a) }
        else { return None; };
    Some((pre[0] - fraction*inc[0]).hypot(pre[1] - fraction*inc[1]))
}

fn isolates_sources(project: &Project, device: &crate::model::Device, fault_bus: &str) -> bool {
    let mut seen = std::collections::HashSet::from([fault_bus.to_string()]);
    loop {
        let old = seen.len();
        for b in &project.branches {
            if device.protected_branch.as_ref() == Some(&b.id) { continue; }
            if seen.contains(&b.from) { seen.insert(b.to.clone()); }
            if seen.contains(&b.to) { seen.insert(b.from.clone()); }
        }
        if old == seen.len() { break; }
    }
    !project.sources.iter().any(|s| seen.contains(&s.bus))
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
            let bus_fault = fault.buses.iter().find(|b| b.id == device.bus);
            let amps = bus_fault.and_then(|b| device_current(project, device, b, 1.0));
            let bolted = amps.map(|a| a / 1000.0);
            let trip = amps.and_then(|a| curves::trip_time_device(device, a));
            let total = amps.and_then(|a| curves::clearing_time(device, a));
            CoordRow {
                device_id: device.id.clone(),
                name: device.name.clone(),
                bus_id: device.bus.clone(),
                bus_kv: kv,
                curve: curve_label(&device.curve),
                bolted_ka: bolted,
                trip_at_bolted_s: trip,
                total_clearing_s: total,
                note: "Current is the protected branch terminal contribution for a fault at the device bus; operating and total-clearing times are separate. Unassigned terminals/times are unavailable.".into(),
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
