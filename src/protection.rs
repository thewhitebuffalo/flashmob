//! Identify protection from explicitly located devices and network connectivity.
//! Curve availability never changes which device is nearest to a fault.
use std::collections::{HashMap, VecDeque};
use serde::{Deserialize, Serialize};
use crate::model::{CurveSpec, Project};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Protection {
    pub bus_id: String,
    pub status: String,
    pub device_id: Option<String>,
    pub candidates: Vec<String>,
    pub clearing_data: String,
    pub detail: String,
}

type Graph = HashMap<String, Vec<(String, String)>>;

fn distances(graph: &Graph, start: &str, omitted: Option<&str>) -> HashMap<String, usize> {
    let mut distance = HashMap::from([(start.to_string(), 0)]);
    let mut queue = VecDeque::from([start.to_string()]);
    while let Some(bus) = queue.pop_front() {
        let next_distance = distance[&bus] + 1;
        for (next, edge) in graph.get(&bus).into_iter().flatten() {
            if omitted == Some(edge.as_str()) { continue; }
            if !distance.contains_key(next) {
                distance.insert(next.clone(), next_distance);
                queue.push_back(next.clone());
            }
        }
    }
    distance
}

/// Cache each device's branch cut once for the complete study, rather than
/// traversing the network again for every bus/current/duration combination.
struct Index {
    graph: Graph,
    cuts: HashMap<String, (HashMap<String, usize>, usize)>,
}
impl Index {
    fn new(project: &Project) -> Self {
        let mut graph = Graph::new();
        for b in &project.branches {
            graph.entry(b.from.clone()).or_default().push((b.to.clone(), b.id.clone()));
            graph.entry(b.to.clone()).or_default().push((b.from.clone(), b.id.clone()));
        }
        let mut cuts = HashMap::new();
        for d in &project.devices {
            let Some(b) = d.protected_branch.as_ref().and_then(|id| project.branches.iter().find(|b| b.id == *id)) else { continue };
            cuts.entry(b.id.clone()).or_insert_with(|| {
                let side = distances(&graph, &b.from, Some(&b.id));
                let sources = project.sources.iter().filter(|s| side.contains_key(&s.bus)).count();
                (side, sources)
            });
        }
        Self { graph, cuts }
    }
}

pub fn identify_all(project: &Project) -> Vec<Protection> {
    let index = Index::new(project);
    project.buses.iter().map(|b| identify_with_index(project, &b.id, &index)).collect()
}

/// Find the nearest located device whose opening disconnects every utility /
/// generator source from this bus. Use connectivity, not branch drawing direction,
/// equipment names, curve completeness, or project insertion order.
pub fn identify(project: &Project, bus: &str) -> Protection {
    identify_with_index(project, bus, &Index::new(project))
}

fn identify_with_index(project: &Project, bus: &str, index: &Index) -> Protection {
    let distance = distances(&index.graph, bus, None);
    let mut result = Protection { bus_id: bus.into(), status: "unprotected".into(),
        device_id: None, candidates: Vec::new(), clearing_data: "unavailable".into(), detail: String::new() };
    let live_sources: Vec<_> = project.sources.iter().filter(|s| distance.contains_key(&s.bus)).collect();
    if live_sources.is_empty() {
        result.status = "unenergized".into();
        result.detail = "No connected utility/generator source; no upstream protection inferred.".into();
        return result;
    }
    let mut candidates = Vec::new();
    let mut partial = Vec::new();
    for device in &project.devices {
        let Some(edge) = device.protected_branch.as_ref().and_then(|id| project.branches.iter().find(|b| b.id == *id)) else { continue };
        if device.bus != edge.from && device.bus != edge.to { continue; }
        let Some(&hops) = distance.get(&device.bus) else { continue };
        let (side, source_count) = &index.cuts[&edge.id];
        let remaining = if side.contains_key(bus) { *source_count }
            else { live_sources.len() - source_count };
        if remaining == 0 { candidates.push((hops, device)); }
        else if remaining < live_sources.len() { partial.push(device.id.clone()); }
    }
    candidates.sort_by(|(a, x), (b, y)| a.cmp(b).then(x.id.cmp(&y.id)));
    let Some((nearest, _)) = candidates.first() else {
        if !partial.is_empty() {
            partial.sort(); result.candidates = partial;
            result.status = "multiple_infeeds".into();
            result.detail = "No upstream single device isolates all source paths; multiple-device clearing analysis is required.".into();
        } else {
            result.detail = "No upstream located device isolates the connected source paths (check line-side location, bypass paths, and device terminal assignments).".into();
        }
        return result;
    };
    result.candidates = candidates.iter().filter(|(hops, _)| hops == nearest).map(|(_, d)| d.id.clone()).collect();
    if result.candidates.len() != 1 {
        result.status = "ambiguous".into();
        result.detail = format!("Equally near upstream devices: {}; resolve device locations or explicitly select the governing device.", result.candidates.join(", "));
        return result;
    }
    let device = candidates[0].1;
    result.status = "identified".into();
    result.device_id = Some(device.id.clone());
    result.clearing_data = if matches!(device.curve, CurveSpec::SettingsNotCollected { .. }) {
        "missing_curve"
    } else if !device.fuse_total_clearing && device.breaker_interrupting_s.is_none() {
        "missing_interrupting_time"
    } else { "entered" }.into();
    result.detail = format!("Upstream protection: {} [{}]. Opening branch {} isolates all connected utility/generator paths. Clearing data: {} (operation at arcing current is checked separately).",
        device.name, device.id, device.protected_branch.as_deref().unwrap(), result.clearing_data);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::*, study::{self, Studies}};

    fn radial() -> Project {
        let mut p = Project::sample();
        p.loads.clear(); p.motors.clear(); p.equipment.clear();
        for bus in &mut p.buses { bus.kv = 0.48; }
        p.branches[0].kind = p.branches[1].kind.clone();
        p.sources[0].mva_sc = 40.0;
        p.devices[0].curve = CurveSpec::Definite { pickup_a: 100.0, time_s: 0.1 };
        p.devices[0].fuse_total_clearing = true;
        p.devices[0].breaker_interrupting_s = None;
        p
    }

    #[test]
    fn remote_bus_inherits_main_fuse_and_uses_its_curve_without_assignment() {
        let mut p = radial(); p.devices.truncate(1);
        assert_eq!(identify(&p, "pnl").device_id.as_deref(), Some("brk-main"));
        let out = study::run(&p, Studies::all()).unwrap();
        let arc = out.arc_flash.unwrap().into_iter().find(|a| a.bus_id == "pnl").unwrap();
        assert_eq!(arc.upstream_device.as_deref(), Some("brk-main"));
        assert!((arc.time_s - 0.1).abs() < 1e-12);
        assert!((arc.time_min_s - 0.1).abs() < 1e-12);
    }

    #[test]
    fn missing_curve_is_not_missing_protection_and_does_not_choose_backup() {
        let mut p = radial();
        p.devices[1].curve = CurveSpec::SettingsNotCollected { note: "unknown fuse".into() };
        let protection = identify(&p, "pnl");
        assert_eq!(protection.device_id.as_deref(), Some("brk-feeder"));
        assert_eq!(protection.clearing_data, "missing_curve");
        let out = study::run(&p, Studies::all()).unwrap();
        let failure = out.arc_flash_failures.iter().find(|f| f.bus_id == "pnl").unwrap();
        assert!(failure.error.contains("brk-feeder") && failure.error.contains("identified; total clearing unavailable"));
    }

    #[test]
    fn downstream_devices_do_not_protect_upstream_buses_or_source_side() {
        let mut p = radial(); p.devices.remove(0);
        assert_eq!(identify(&p, "pnl").device_id.as_deref(), Some("brk-feeder"));
        assert!(identify(&p, "mcc").device_id.is_none());
        assert!(identify(&p, "util").device_id.is_none());
    }

    #[test]
    fn drawing_direction_and_input_order_do_not_control_protection() {
        let mut p = radial();
        for b in &mut p.branches { std::mem::swap(&mut b.from, &mut b.to); }
        p.buses.reverse(); p.branches.reverse(); p.devices.reverse();
        assert_eq!(identify(&p, "pnl").device_id.as_deref(), Some("brk-feeder"));
        assert_eq!(identify(&p, "mcc").device_id.as_deref(), Some("brk-main"));
    }

    #[test]
    fn equal_locations_are_ambiguous_and_unlocated_devices_are_ignored() {
        let mut p = radial();
        let mut copy = p.devices[1].clone(); copy.id = "duplicate-position".into(); p.devices.push(copy);
        assert_eq!(identify(&p, "pnl").status, "ambiguous");
        assert!(identify(&p, "pnl").device_id.is_none());
        p.devices[2].protected_branch = None;
        assert_eq!(identify(&p, "pnl").device_id.as_deref(), Some("brk-feeder"));
    }

    #[test]
    fn bypass_and_parallel_sources_do_not_get_false_single_device_credit() {
        let mut p = radial();
        let mut bypass = p.branches[1].clone(); bypass.id = "bypass".into(); bypass.from = "util".into(); p.branches.push(bypass);
        assert!(identify(&p, "pnl").device_id.is_none());
        p.branches.pop();
        let mut source = p.sources[0].clone(); source.id = "second".into(); source.bus = "pnl".into(); p.sources.push(source);
        assert_eq!(identify(&p, "mcc").status, "multiple_infeeds");
        assert!(identify(&p, "mcc").device_id.is_none());
    }

    #[test]
    fn assumed_duration_keeps_detected_protection_and_reports_override() {
        let mut p = radial(); p.devices.truncate(1);
        let mut case = Project::sample().equipment.remove(1);
        case.upstream_device = None; case.clearing_s = Some(2.0); case.basis = "assumed".into(); p.equipment.push(case);
        let out = study::run(&p, Studies::all()).unwrap();
        let arc = out.arc_flash.unwrap().into_iter().find(|a| a.bus_id == "pnl").unwrap();
        assert_eq!(arc.upstream_device.as_deref(), Some("brk-main"));
        assert_eq!(arc.time_s, 2.0);
        assert!(arc.duration_note.contains("assumed exposure duration"));
        assert!(arc.duration_note.contains("Manual duration overrides device-based clearing"));
    }

    #[test]
    fn unenergized_network_does_not_infer_a_random_device() {
        let mut p = radial(); p.sources.clear();
        assert_eq!(identify(&p, "pnl").status, "unenergized");
        assert!(identify(&p, "pnl").device_id.is_none());
    }
}

#[cfg(test)]
mod fallback_tests {
    use crate::{model::*, study::{self, Studies}};

    #[test]
    fn device_curve_supersedes_fallback_and_unknown_curve_uses_it() {
        let mut p = Project::sample(); p.motors.clear(); p.loads.clear();
        p.equipment[0].upstream_device = None;
        p.equipment[0].basis = "collected".into();
        p.equipment[0].fallback_duration_s = Some(2.0);
        p.devices[0].curve = CurveSpec::Definite { pickup_a: 100.0, time_s: 0.1 };
        p.devices[0].breaker_interrupting_s = None;
        p.devices[0].fuse_total_clearing = true;
        let run = |p: &Project| study::run(p, Studies::all()).unwrap().arc_flash.unwrap()
            .into_iter().find(|r| r.equipment_id == "af-mcc").unwrap();
        let known = run(&p);
        assert_eq!(known.time_s, 0.1);
        assert!(!known.assumed);
        assert!(!known.duration_note.contains("fallback"));
        p.devices[0].curve = CurveSpec::SettingsNotCollected { note: "curve missing".into() };
        let unknown = run(&p);
        assert_eq!(unknown.time_s, 2.0);
        assert!(unknown.assumed);
        assert!(unknown.duration_note.contains("assumed exposure fallback"));
        assert_eq!(unknown.upstream_device.as_deref(), Some("brk-main"));
        // Fallback does not override an explicit manual duration or suppress bad placement.
        p.equipment[0].clearing_s = Some(0.3);
        assert_eq!(run(&p).time_s, 0.3);
        p.equipment[0].clearing_s = None;
        p.equipment[0].upstream_device = Some("brk-feeder".into());
        let result = study::run(&p, Studies::all()).unwrap();
        assert!(result.arc_flash_failures.iter().any(|f| f.equipment_id == "af-mcc" && f.error.contains("does not isolate")));
        p.equipment[0].fallback_duration_s = Some(-1.0);
        assert!(!p.validate().is_empty());
    }
}
