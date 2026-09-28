//! Entered model connectivity for callers that draw or inspect their own SLD.
//! No study results, protection inference, or electrical calculations appear here.

use std::collections::HashMap;

use serde::Serialize;

use crate::model::{BranchKind, OperatingCase, Project, SwitchGroup, SwitchKind};

#[derive(Clone, Debug, Serialize)]
pub struct Topology {
    pub project_name: String,
    pub buses: Vec<BusConnections>,
    pub branches: Vec<BranchConnection>,
    pub sources: Vec<SourceConnection>,
    pub loads: Vec<LocatedComponent>,
    pub motors: Vec<LocatedComponent>,
    pub devices: Vec<DeviceConnection>,
    pub equipment: Vec<EquipmentConnection>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub switches: Vec<SwitchConnection>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub switch_groups: Vec<SwitchGroup>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub operating_cases: Vec<OperatingCase>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BusConnections {
    pub id: String,
    pub name: String,
    pub kv: f64,
    pub branches: Vec<BusBranch>,
    pub sources: Vec<NamedRef>,
    pub loads: Vec<NamedRef>,
    pub motors: Vec<NamedRef>,
    pub devices: Vec<NamedRef>,
    pub equipment: Vec<NamedRef>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NamedRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BusBranch {
    pub branch_id: String,
    pub terminal: Terminal,
    pub neighbor_bus_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    From,
    To,
}

#[derive(Clone, Debug, Serialize)]
pub struct BranchConnection {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub from_bus_id: String,
    pub to_bus_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_closed: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocatedComponent {
    pub id: String,
    pub name: String,
    pub bus_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceConnection {
    pub id: String,
    pub name: String,
    pub bus_id: String,
    /// Omitted from JSON for the legacy/default true state.
    #[serde(skip_serializing_if = "is_true")]
    pub in_service: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SwitchConnection {
    pub id: String,
    pub name: String,
    pub branch_id: String,
    pub kind: SwitchKind,
    pub closed: bool,
}

fn is_true(value: &bool) -> bool { *value }

#[derive(Clone, Debug, Serialize)]
pub struct DeviceConnection {
    pub id: String,
    pub name: String,
    pub bus_id: String,
    pub protected_branch_id: Option<String>,
    pub protected_terminal: Option<Terminal>,
    pub protected_neighbor_bus_id: Option<String>,
    pub upstream_for_equipment_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EquipmentConnection {
    pub id: String,
    pub name: String,
    pub bus_id: String,
    pub upstream_device_id: Option<String>,
}

/// Return only relationships entered in a valid project. Device placement is
/// unknown when `protected_branch` was not entered, and remains null in output.
pub fn topology(project: &Project) -> Result<Topology, String> {
    let errors = project.validate();
    if !errors.is_empty() {
        return Err(format!("invalid project: {}", errors.join("; ")));
    }

    let mut buses: Vec<BusConnections> = project
        .buses
        .iter()
        .map(|bus| BusConnections {
            id: bus.id.clone(),
            name: bus.name.clone(),
            kv: bus.kv,
            branches: Vec::new(),
            sources: Vec::new(),
            loads: Vec::new(),
            motors: Vec::new(),
            devices: Vec::new(),
            equipment: Vec::new(),
        })
        .collect();
    let bus_index: HashMap<&str, usize> = project
        .buses
        .iter()
        .enumerate()
        .map(|(index, bus)| (bus.id.as_str(), index))
        .collect();

    let branches = project
        .branches
        .iter()
        .map(|branch| {
            let from = bus_index[branch.from.as_str()];
            let to = bus_index[branch.to.as_str()];
            buses[from].branches.push(BusBranch {
                branch_id: branch.id.clone(),
                terminal: Terminal::From,
                neighbor_bus_id: branch.to.clone(),
            });
            buses[to].branches.push(BusBranch {
                branch_id: branch.id.clone(),
                terminal: Terminal::To,
                neighbor_bus_id: branch.from.clone(),
            });
            BranchConnection {
                id: branch.id.clone(),
                name: branch.name.clone(),
                kind: match branch.kind {
                    BranchKind::Line { .. } => "line",
                    BranchKind::Transformer { .. } => "transformer",
                },
                from_bus_id: branch.from.clone(),
                to_bus_id: branch.to.clone(),
                switch_id: project.switches.iter().find(|item| item.branch_id == branch.id).map(|item| item.id.clone()),
                base_closed: project.switches.iter().find(|item| item.branch_id == branch.id).map(|item| item.closed),
            }
        })
        .collect();

    let sources = project
        .sources
        .iter()
        .map(|source| {
            buses[bus_index[source.bus.as_str()]]
                .sources
                .push(named(&source.id, &source.name));
            SourceConnection {
                id: source.id.clone(),
                name: source.name.clone(),
                bus_id: source.bus.clone(),
                in_service: source.in_service,
            }
        })
        .collect();
    let loads = project
        .loads
        .iter()
        .map(|load| {
            buses[bus_index[load.bus.as_str()]]
                .loads
                .push(named(&load.id, &load.name));
            located(&load.id, &load.name, &load.bus)
        })
        .collect();
    let motors = project
        .motors
        .iter()
        .map(|motor| {
            buses[bus_index[motor.bus.as_str()]]
                .motors
                .push(named(&motor.id, &motor.name));
            located(&motor.id, &motor.name, &motor.bus)
        })
        .collect();

    let branch_index: HashMap<&str, _> = project
        .branches
        .iter()
        .map(|branch| (branch.id.as_str(), branch))
        .collect();
    let devices = project
        .devices
        .iter()
        .map(|device| {
            buses[bus_index[device.bus.as_str()]]
                .devices
                .push(named(&device.id, &device.name));
            let protected = device
                .protected_branch
                .as_deref()
                .map(|id| branch_index[id]);
            let (protected_terminal, protected_neighbor_bus_id) = match protected {
                Some(branch) if branch.from == device.bus => {
                    (Some(Terminal::From), Some(branch.to.clone()))
                }
                Some(branch) => (Some(Terminal::To), Some(branch.from.clone())),
                None => (None, None),
            };
            DeviceConnection {
                id: device.id.clone(),
                name: device.name.clone(),
                bus_id: device.bus.clone(),
                protected_branch_id: device.protected_branch.clone(),
                protected_terminal,
                protected_neighbor_bus_id,
                upstream_for_equipment_ids: project
                    .equipment
                    .iter()
                    .filter(|equipment| {
                        equipment.upstream_device.as_deref() == Some(device.id.as_str())
                    })
                    .map(|equipment| equipment.id.clone())
                    .collect(),
            }
        })
        .collect();
    let equipment = project
        .equipment
        .iter()
        .map(|item| {
            buses[bus_index[item.bus.as_str()]]
                .equipment
                .push(named(&item.id, &item.name));
            EquipmentConnection {
                id: item.id.clone(),
                name: item.name.clone(),
                bus_id: item.bus.clone(),
                upstream_device_id: item.upstream_device.clone(),
            }
        })
        .collect();

    Ok(Topology {
        project_name: project.name.clone(),
        buses,
        branches,
        sources,
        loads,
        motors,
        devices,
        equipment,
        switches: project.switches.iter().map(|item| SwitchConnection {
            id: item.id.clone(),
            name: item.name.clone(),
            branch_id: item.branch_id.clone(),
            kind: item.kind.clone(),
            closed: item.closed,
        }).collect(),
        switch_groups: project.switch_groups.clone(),
        operating_cases: project.operating_cases.clone(),
    })
}

fn named(id: &str, name: &str) -> NamedRef {
    NamedRef {
        id: id.into(),
        name: name.into(),
    }
}

fn located(id: &str, name: &str, bus: &str) -> LocatedComponent {
    LocatedComponent {
        id: id.into(),
        name: name.into(),
        bus_id: bus.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Branch, BranchKind, Bus, CurveSpec, Device, Project, Switch, SwitchKind};

    fn line(id: &str, from: &str, to: &str) -> Branch {
        Branch {
            id: id.into(),
            name: id.into(),
            from: from.into(),
            to: to.into(),
            kind: BranchKind::Line {
                r_ohm: 0.01,
                x_ohm: 0.02,
                b_siemens: 0.0,
                r0_ohm: 0.0,
                x0_ohm: 0.0,
                ampacity_a: None,
            },
        }
    }

    #[test]
    fn parallel_branches_preserve_ids_and_terminal_orientation() {
        let mut project = Project::sample();
        project.branches.push(line("second-feeder", "pnl", "mcc"));
        let view = topology(&project).unwrap();
        let mcc = view.buses.iter().find(|bus| bus.id == "mcc").unwrap();
        let panel = view.buses.iter().find(|bus| bus.id == "pnl").unwrap();
        assert_eq!(
            mcc.branches
                .iter()
                .filter(|branch| branch.neighbor_bus_id == "pnl")
                .count(),
            2
        );
        assert_eq!(
            panel
                .branches
                .iter()
                .filter(|branch| branch.neighbor_bus_id == "mcc")
                .count(),
            2
        );
        assert_eq!(
            mcc.branches
                .iter()
                .find(|branch| branch.branch_id == "second-feeder")
                .unwrap()
                .terminal,
            Terminal::To
        );
        assert_eq!(
            panel
                .branches
                .iter()
                .find(|branch| branch.branch_id == "second-feeder")
                .unwrap()
                .terminal,
            Terminal::From
        );
        assert!(view
            .branches
            .iter()
            .any(|branch| branch.id == "second-feeder"
                && branch.from_bus_id == "pnl"
                && branch.to_bus_id == "mcc"));
    }

    #[test]
    fn devices_expose_exact_terminal_and_equipment_attachment() {
        let view = topology(&Project::sample()).unwrap();
        let main = view
            .devices
            .iter()
            .find(|device| device.id == "brk-main")
            .unwrap();
        assert_eq!(main.bus_id, "mcc");
        assert_eq!(main.protected_branch_id.as_deref(), Some("t1"));
        assert_eq!(main.protected_terminal, Some(Terminal::To));
        assert_eq!(main.protected_neighbor_bus_id.as_deref(), Some("util"));
        assert_eq!(main.upstream_for_equipment_ids, ["af-mcc"]);
        let panel = view.buses.iter().find(|bus| bus.id == "pnl").unwrap();
        assert_eq!(panel.devices[0].id, "brk-feeder");
        assert_eq!(panel.equipment[0].id, "af-pnl");
        assert_eq!(
            view.equipment
                .iter()
                .find(|item| item.id == "af-pnl")
                .unwrap()
                .upstream_device_id
                .as_deref(),
            Some("brk-feeder")
        );

        let mut reversed = Project::sample();
        reversed
            .devices
            .iter_mut()
            .find(|device| device.id == "brk-feeder")
            .unwrap()
            .bus = "mcc".into();
        let view = topology(&reversed).unwrap();
        let feeder = view
            .devices
            .iter()
            .find(|device| device.id == "brk-feeder")
            .unwrap();
        assert_eq!(feeder.protected_terminal, Some(Terminal::From));
        assert_eq!(feeder.protected_neighbor_bus_id.as_deref(), Some("pnl"));
    }

    #[test]
    fn empty_attachments_and_unknown_device_placement_remain_explicit() {
        let mut project = Project::sample();
        project.buses.push(Bus {
            id: "spare".into(),
            name: "Spare".into(),
            kv: 0.48,
            shunt_kvar: 0.0,
            bracing_ka: None,
            main_rating_a: None,
        });
        project.devices.push(Device {
            id: "unplaced".into(),
            name: "Unplaced".into(),
            bus: "spare".into(),
            protected_branch: None,
            curve: CurveSpec::SettingsNotCollected {
                note: String::new(),
            },
            breaker_interrupting_s: None,
            fuse_total_clearing: false,
            basis: String::new(),
        });
        let view = topology(&project).unwrap();
        let spare = view.buses.iter().find(|bus| bus.id == "spare").unwrap();
        assert!(
            spare.branches.is_empty()
                && spare.sources.is_empty()
                && spare.loads.is_empty()
                && spare.motors.is_empty()
                && spare.equipment.is_empty()
        );
        assert_eq!(spare.devices[0].id, "unplaced");
        let device = view
            .devices
            .iter()
            .find(|device| device.id == "unplaced")
            .unwrap();
        assert!(device.protected_terminal.is_none() && device.protected_neighbor_bus_id.is_none());
    }

    #[test]
    fn rejects_invalid_project_before_building_connections() {
        let mut project = Project::sample();
        project.branches[0].to = "missing".into();
        let err = topology(&project).unwrap_err();
        assert!(err.contains("unknown bus 'missing'"), "{err}");
    }

    #[test]
    fn topology_serializes_explicit_switch_and_source_states_without_changing_legacy_shape() {
        let mut project = Project::sample();
        let legacy = serde_json::to_value(topology(&project).unwrap()).unwrap();
        assert!(legacy.get("switches").is_none());
        assert!(legacy.get("switch_groups").is_none());
        assert!(legacy.get("operating_cases").is_none());
        assert!(legacy["branches"][0].get("switch_id").is_none());
        assert!(legacy["sources"][0].get("in_service").is_none());

        project.switches.push(Switch {
            id: "feeder-switch".into(), name: "Feeder switch".into(),
            branch_id: "feeder".into(), kind: SwitchKind::Breaker, closed: false,
        });
        project.sources[0].in_service = false;
        let active = serde_json::to_value(topology(&project).unwrap()).unwrap();
        assert_eq!(active["branches"][1]["switch_id"], "feeder-switch");
        assert_eq!(active["branches"][1]["base_closed"], false);
        assert_eq!(active["switches"][0]["branch_id"], "feeder");
        assert_eq!(active["sources"][0]["in_service"], false);
    }
}
