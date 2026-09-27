use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_sbase")]
    pub s_base_mva: f64,
    #[serde(default)]
    pub buses: Vec<Bus>,
    #[serde(default)]
    pub branches: Vec<Branch>,
    #[serde(default)]
    pub loads: Vec<Load>,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub motors: Vec<Motor>,
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default)]
    pub equipment: Vec<ArcEquipment>,
    /// Maximum arc duration reported for incident energy. IEEE 1584 discusses 2 s when a person can move away.
    #[serde(default = "default_cap")]
    pub arc_duration_cap_s: f64,
    #[serde(default)]
    pub prefault: Prefault,
    /// Inputs that were not on the source document. The text report prints every entry.
    #[serde(default)]
    pub assumptions: Vec<Assumption>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub id: String,
    pub statement: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prefault {
    #[default]
    Flat,
    Loadflow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bus {
    pub id: String,
    pub name: String,
    pub kv: f64,
    #[serde(default)]
    pub shunt_kvar: f64,
    /// Equipment interrupting rating in kA. This is bracing, not a calculated fault.
    #[serde(default)]
    pub bracing_ka: Option<f64>,
    /// Collected main or bus ampere rating. Not a trip-unit setting.
    #[serde(default)]
    pub main_rating_a: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branch {
    pub id: String,
    pub name: String,
    pub from: String,
    pub to: String,
    pub kind: BranchKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BranchKind {
    Line {
        r_ohm: f64,
        x_ohm: f64,
        #[serde(default)]
        b_siemens: f64,
        #[serde(default)]
        r0_ohm: f64,
        #[serde(default)]
        x0_ohm: f64,
        #[serde(default)]
        ampacity_a: Option<f64>,
    },
    Transformer {
        kva: f64,
        z_percent: f64,
        xr: f64,
        hv_kv: f64,
        lv_kv: f64,
        connection: XfmrConn,
        #[serde(default)]
        tap_percent: f64,
        #[serde(default = "default_x0")]
        x0_over_x1: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XfmrConn {
    /// From-bus delta, to-bus grounded wye. Positive-sequence from-side leads by 30°.
    Dyn,
    /// From-bus grounded wye, to-bus delta.
    Ynd,
    /// Grounded wye on both sides.
    Ynyn,
    /// Delta on both sides. No zero-sequence path.
    Dd,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Load {
    pub id: String,
    pub name: String,
    pub bus: String,
    pub kw: f64,
    pub kvar: f64,
    /// "collected" or "assumed". Empty means unspecified.
    #[serde(default)]
    pub basis: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub bus: String,
    #[serde(default = "default_vpu")]
    pub v_pu: f64,
    #[serde(default)]
    pub angle_deg: f64,
    pub mva_sc: f64,
    pub xr: f64,
    #[serde(default = "default_one")]
    pub x0_over_x1: f64,
    #[serde(default = "default_one")]
    pub r0_over_r1: f64,
    #[serde(default)]
    pub is_slack: bool,
    /// Positive is generation. Ignored on the slack bus.
    #[serde(default)]
    pub p_mw: f64,
    #[serde(default)]
    pub qmin_mvar: Option<f64>,
    #[serde(default)]
    pub qmax_mvar: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motor {
    pub id: String,
    pub name: String,
    pub bus: String,
    pub hp: f64,
    pub kv: f64,
    #[serde(default = "default_pf")]
    pub pf: f64,
    #[serde(default = "default_eff")]
    pub efficiency: f64,
    #[serde(default = "default_xd")]
    pub x_subtransient_pu: f64,
    #[serde(default = "default_mxr")]
    pub xr: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub bus: String,
    pub curve: CurveSpec,
    /// "collected" or "assumed". Empty means unspecified.
    #[serde(default)]
    pub basis: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveSpec {
    Iec {
        kind: IecKind,
        pickup_a: f64,
        tms: f64,
        #[serde(default)]
        inst_a: Option<f64>,
        #[serde(default)]
        inst_s: Option<f64>,
    },
    Ieee {
        kind: IeeeKind,
        pickup_a: f64,
        td: f64,
        #[serde(default)]
        inst_a: Option<f64>,
        #[serde(default)]
        inst_s: Option<f64>,
    },
    Definite {
        pickup_a: f64,
        time_s: f64,
    },
    ThermalMagnetic {
        lt_pickup_a: f64,
        /// Trip time at 6× long-time pickup.
        lt_delay_s: f64,
        #[serde(default)]
        st_pickup_a: Option<f64>,
        #[serde(default)]
        st_delay_s: Option<f64>,
        #[serde(default)]
        inst_a: Option<f64>,
        #[serde(default)]
        inst_s: Option<f64>,
    },
    /// The device was on the collection sheet. No pickup, delay, fuse size, or instantaneous is stored.
    SettingsNotCollected {
        #[serde(default)]
        note: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IecKind {
    Si,
    Vi,
    Ei,
    Lti,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IeeeKind {
    ModeratelyInverse,
    VeryInverse,
    ExtremelyInverse,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcEquipment {
    pub id: String,
    pub name: String,
    pub bus: String,
    pub electrode: Electrode,
    pub gap_mm: f64,
    pub distance_mm: f64,
    pub height_mm: f64,
    pub width_mm: f64,
    pub depth_mm: f64,
    #[serde(default)]
    pub upstream_device: Option<String>,
    /// Manual arc duration. When omitted, the upstream device curve is used.
    #[serde(default)]
    pub clearing_s: Option<f64>,
    /// "collected" or "assumed". Empty means unspecified.
    #[serde(default)]
    pub basis: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Electrode {
    VCB,
    VCBB,
    HCB,
    VOA,
    HOA,
}

impl Electrode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VCB => "VCB",
            Self::VCBB => "VCBB",
            Self::HCB => "HCB",
            Self::VOA => "VOA",
            Self::HOA => "HOA",
        }
    }

    pub fn is_box(self) -> bool {
        matches!(self, Self::VCB | Self::VCBB | Self::HCB)
    }
}

fn default_name() -> String {
    "Untitled".to_string()
}
fn default_sbase() -> f64 {
    100.0
}
fn default_cap() -> f64 {
    2.0
}
fn default_vpu() -> f64 {
    1.0
}
fn default_one() -> f64 {
    1.0
}
fn default_x0() -> f64 {
    1.0
}
fn default_pf() -> f64 {
    0.85
}
fn default_eff() -> f64 {
    0.94
}
fn default_xd() -> f64 {
    0.17
}
fn default_mxr() -> f64 {
    6.0
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: default_name(),
            s_base_mva: default_sbase(),
            buses: Vec::new(),
            branches: Vec::new(),
            loads: Vec::new(),
            sources: Vec::new(),
            motors: Vec::new(),
            devices: Vec::new(),
            equipment: Vec::new(),
            arc_duration_cap_s: default_cap(),
            prefault: Prefault::Flat,
            assumptions: Vec::new(),
        }
    }
}

impl Project {
    pub fn sample() -> Self {
        Self {
            name: "Sample".to_string(),
            s_base_mva: 100.0,
            arc_duration_cap_s: 2.0,
            prefault: Prefault::Flat,
            assumptions: Vec::new(),
            buses: vec![
                Bus { id: "util".into(), name: "Utility".into(), kv: 12.47, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None },
                Bus { id: "mcc".into(), name: "MCC-1".into(), kv: 0.48, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None },
                Bus { id: "pnl".into(), name: "Panel A".into(), kv: 0.48, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None },
            ],
            branches: vec![
                Branch {
                    id: "t1".into(),
                    name: "T1".into(),
                    from: "util".into(),
                    to: "mcc".into(),
                    kind: BranchKind::Transformer {
                        kva: 2500.0,
                        z_percent: 5.75,
                        xr: 7.5,
                        hv_kv: 12.47,
                        lv_kv: 0.48,
                        connection: XfmrConn::Dyn,
                        tap_percent: 0.0,
                        x0_over_x1: 1.0,
                    },
                },
                Branch {
                    id: "feeder".into(),
                    name: "Feeder to Panel A".into(),
                    from: "mcc".into(),
                    to: "pnl".into(),
                    kind: BranchKind::Line {
                        // About 150 ft of 500 kcmil copper in steel conduit.
                        r_ohm: 0.0044,
                        x_ohm: 0.0072,
                        b_siemens: 0.0,
                        r0_ohm: 0.013,
                        x0_ohm: 0.015,
                        ampacity_a: Some(380.0),
                    },
                },
            ],
            loads: vec![
                Load { id: "load-mcc".into(), name: "MCC load".into(), bus: "mcc".into(), kw: 400.0, kvar: 200.0, basis: String::new() },
                Load { id: "load-pnl".into(), name: "Panel load".into(), bus: "pnl".into(), kw: 75.0, kvar: 40.0, basis: String::new() },
            ],
            sources: vec![Source {
                id: "grid".into(),
                name: "Utility".into(),
                bus: "util".into(),
                v_pu: 1.0,
                angle_deg: 0.0,
                mva_sc: 250.0,
                xr: 12.0,
                x0_over_x1: 1.5,
                r0_over_r1: 1.0,
                is_slack: true,
                p_mw: 0.0,
                qmin_mvar: None,
                qmax_mvar: None,
            }],
            motors: vec![Motor {
                id: "m1".into(),
                name: "M1".into(),
                bus: "mcc".into(),
                hp: 200.0,
                kv: 0.48,
                pf: 0.88,
                efficiency: 0.94,
                x_subtransient_pu: 0.17,
                xr: 6.0,
            }],
            devices: vec![
                Device {
                    id: "brk-main".into(),
                    name: "MCC main".into(),
                    bus: "mcc".into(),
                    curve: CurveSpec::ThermalMagnetic {
                        lt_pickup_a: 3200.0,
                        lt_delay_s: 8.0,
                        st_pickup_a: Some(16000.0),
                        st_delay_s: Some(0.30),
                        inst_a: Some(36000.0),
                        inst_s: Some(0.05),
                    },
                    basis: String::new(),
                },
                Device {
                    id: "brk-feeder".into(),
                    name: "Panel feeder".into(),
                    bus: "pnl".into(),
                    curve: CurveSpec::Iec {
                        kind: IecKind::Ei,
                        pickup_a: 600.0,
                        tms: 0.15,
                        inst_a: Some(6000.0),
                        inst_s: Some(0.03),
                    },
                    basis: String::new(),
                },
            ],
            equipment: vec![
                ArcEquipment {
                    id: "af-mcc".into(),
                    name: "MCC-1".into(),
                    bus: "mcc".into(),
                    electrode: Electrode::VCB,
                    gap_mm: 25.0,
                    distance_mm: 457.2,
                    height_mm: 508.0,
                    width_mm: 508.0,
                    depth_mm: 250.0,
                    upstream_device: Some("brk-main".into()),
                    clearing_s: None,
                    basis: String::new(),
                },
                ArcEquipment {
                    id: "af-pnl".into(),
                    name: "Panel A".into(),
                    bus: "pnl".into(),
                    electrode: Electrode::VCB,
                    gap_mm: 25.0,
                    distance_mm: 457.2,
                    height_mm: 355.6,
                    width_mm: 304.8,
                    depth_mm: 100.0,
                    upstream_device: Some("brk-feeder".into()),
                    clearing_s: None,
                    basis: String::new(),
                },
            ],
        }
    }

    pub fn bus(&self, id: &str) -> Option<&Bus> {
        self.buses.iter().find(|b| b.id == id)
    }

    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn ids(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for b in &self.buses {
            out.push(b.id.as_str());
        }
        for b in &self.branches {
            out.push(b.id.as_str());
        }
        for b in &self.loads {
            out.push(b.id.as_str());
        }
        for b in &self.sources {
            out.push(b.id.as_str());
        }
        for b in &self.motors {
            out.push(b.id.as_str());
        }
        for b in &self.devices {
            out.push(b.id.as_str());
        }
        for b in &self.equipment {
            out.push(b.id.as_str());
        }
        for b in &self.assumptions {
            out.push(b.id.as_str());
        }
        out
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.s_base_mva <= 0.0 {
            errors.push("s_base_mva must be positive".into());
        }
        if self.arc_duration_cap_s <= 0.0 {
            errors.push("arc_duration_cap_s must be positive".into());
        }
        let mut seen: HashMap<String, &'static str> = HashMap::new();
        let mut claim = |id: &str, kind: &'static str, errors: &mut Vec<String>| {
            if id.is_empty() {
                errors.push(format!("{kind} is missing an id"));
                return;
            }
            if let Some(prev) = seen.insert(id.to_string(), kind) {
                errors.push(format!("duplicate id '{id}' on {kind} and {prev}"));
            }
        };
        for b in &self.buses {
            claim(&b.id, "bus", &mut errors);
            if b.kv <= 0.0 {
                errors.push(format!("bus {} has a non-positive voltage", b.id));
            }
        }
        let bus_ids: HashMap<&str, ()> = self.buses.iter().map(|b| (b.id.as_str(), ())).collect();
        let need_bus = |id: &str, what: &str, errors: &mut Vec<String>| {
            if !bus_ids.contains_key(id) {
                errors.push(format!("{what} references unknown bus '{id}'"));
            }
        };
        for b in &self.branches {
            claim(&b.id, "branch", &mut errors);
            need_bus(&b.from, &format!("branch {}", b.id), &mut errors);
            need_bus(&b.to, &format!("branch {}", b.id), &mut errors);
            if b.from == b.to {
                errors.push(format!("branch {} starts and ends on the same bus", b.id));
            }
            match &b.kind {
                BranchKind::Line { r_ohm, x_ohm, .. } => {
                    if r_ohm.hypot(*x_ohm) < 1e-8 {
                        errors.push(format!("line {} impedance is zero", b.id));
                    }
                }
                BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, .. } => {
                    if *kva <= 0.0 || *z_percent <= 0.0 || *xr < 0.0 || *hv_kv <= 0.0 || *lv_kv <= 0.0 {
                        errors.push(format!("transformer {} has a non-positive rating or impedance", b.id));
                    }
                }
            }
        }
        for x in &self.loads {
            claim(&x.id, "load", &mut errors);
            need_bus(&x.bus, &format!("load {}", x.id), &mut errors);
        }
        for x in &self.sources {
            claim(&x.id, "source", &mut errors);
            need_bus(&x.bus, &format!("source {}", x.id), &mut errors);
            if x.mva_sc <= 0.0 || x.xr < 0.0 || x.v_pu <= 0.0 {
                errors.push(format!("source {} has a non-positive strength or voltage", x.id));
            }
        }
        for x in &self.motors {
            claim(&x.id, "motor", &mut errors);
            need_bus(&x.bus, &format!("motor {}", x.id), &mut errors);
            if x.hp <= 0.0 || x.pf <= 0.0 || x.pf > 1.0 || x.efficiency <= 0.0 || x.efficiency > 1.0 {
                errors.push(format!("motor {} ratings are out of range", x.id));
            }
        }
        for x in &self.devices {
            claim(&x.id, "device", &mut errors);
            need_bus(&x.bus, &format!("device {}", x.id), &mut errors);
        }
        for x in &self.assumptions {
            claim(&x.id, "assumption", &mut errors);
            if x.statement.trim().is_empty() {
                errors.push(format!("assumption {} has an empty statement", x.id));
            }
        }
        for x in &self.equipment {
            claim(&x.id, "equipment", &mut errors);
            need_bus(&x.bus, &format!("equipment {}", x.id), &mut errors);
            if let Some(dev) = &x.upstream_device {
                if self.device(dev).is_none() {
                    errors.push(format!("equipment {} references unknown device '{dev}'", x.id));
                }
            }
        }
        errors
    }
}

pub fn zbase_ohm(kv: f64, s_base_mva: f64) -> f64 {
    kv * kv / s_base_mva
}

pub fn i_base_ka(kv: f64, s_base_mva: f64) -> f64 {
    s_base_mva / (3.0_f64.sqrt() * kv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_project_roundtrips_through_json() {
        let text = serde_json::to_string_pretty(&Project::sample()).unwrap();
        let back: Project = serde_json::from_str(&text).unwrap();
        assert!(back.validate().is_empty(), "{:?}", back.validate());
        assert_eq!(back.branches.len(), 2);
    }
}

/// Typical IEEE 1584 enclosure, gap, and working distance for a bus voltage.
pub fn typical_equipment(kv: f64) -> (Electrode, f64, f64, f64, f64, f64, &'static str) {
    if kv <= 0.6 {
        (Electrode::VCB, 25.0, 457.2, 355.6, 304.8, 250.0, "low-voltage equipment")
    } else if kv <= 5.0 {
        (Electrode::VCBB, 104.0, 914.4, 1143.0, 762.0, 762.0, "5 kV switchgear")
    } else {
        (Electrode::VCBB, 152.0, 914.4, 1143.0, 762.0, 762.0, "15 kV switchgear")
    }
}
