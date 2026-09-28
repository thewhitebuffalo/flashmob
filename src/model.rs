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
    /// Operable contacts on existing finite-impedance branches.
    #[serde(default)]
    pub switches: Vec<Switch>,
    /// Interlocks that limit the number of simultaneously closed contacts.
    #[serde(default)]
    pub switch_groups: Vec<SwitchGroup>,
    /// Named deviations from the normal switch and source state.
    #[serde(default)]
    pub operating_cases: Vec<OperatingCase>,
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
        /// HV winding turns change, independent of from/to orientation. Must exceed -100%.
        #[serde(default)]
        tap_percent: f64,
        #[serde(default = "default_x0")]
        x0_over_x1: f64,
        #[serde(default = "default_one")]
        r0_over_r1: f64,
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
    /// Whether this utility or generator is available in the current state.
    #[serde(default = "default_true")]
    pub in_service: bool,
    #[serde(default = "default_vpu")]
    pub v_pu: f64,
    #[serde(default)]
    pub angle_deg: f64,
    pub mva_sc: f64,
    /// Optional symmetrical three-phase short-circuit current profile at this
    /// source's terminals. Ratios are relative to the initial `mva_sc` value.
    /// An empty profile retains a constant source strength.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decrement_curve: Vec<DecrementPoint>,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecrementPoint {
    /// Seconds after fault inception; the first point must be at zero.
    pub time_s: f64,
    /// Symmetrical RMS current divided by the time-zero current.
    pub current_ratio: f64,
}

impl Source {
    /// Interpolate a supplied generator decrement profile in elapsed time and
    /// hold the endpoint values outside its recorded interval. Validation
    /// ensures the first point is (0 s, 1.0) and all ratios stay positive.
    pub fn decrement_ratio_at(&self, time_s: f64) -> f64 {
        if !time_s.is_finite() { return f64::NAN; }
        let Some(first) = self.decrement_curve.first() else { return 1.0 };
        if time_s <= first.time_s { return first.current_ratio; }
        for pair in self.decrement_curve.windows(2) {
            let [start, end] = pair else { unreachable!() };
            if time_s <= end.time_s {
                let fraction = (time_s - start.time_s) / (end.time_s - start.time_s);
                return start.current_ratio + fraction * (end.current_ratio - start.current_ratio);
            }
        }
        self.decrement_curve.last().unwrap().current_ratio
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchKind {
    Breaker,
    Tie,
    Ats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Switch {
    pub id: String,
    pub name: String,
    /// The existing branch whose series path this contact opens or closes.
    pub branch_id: String,
    pub kind: SwitchKind,
    /// Explicit normal state. Requiring this avoids silently closing a tie or
    /// both throws of an ATS when an input omits the field.
    pub closed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchGroup {
    pub id: String,
    pub switch_ids: Vec<String>,
    pub max_closed: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchState {
    pub switch_id: String,
    pub closed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceState {
    pub source_id: String,
    pub in_service: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentState {
    pub equipment_id: String,
    /// Case-specific protective device. None permits automatic selection.
    #[serde(default)]
    pub upstream_device: Option<String>,
    /// Case-specific explicit total arc duration.
    #[serde(default)]
    pub clearing_s: Option<f64>,
    /// Case-specific authorized fallback if device clearing is unavailable.
    #[serde(default)]
    pub fallback_duration_s: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingCase {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub switch_states: Vec<SwitchState>,
    #[serde(default)]
    pub source_states: Vec<SourceState>,
    #[serde(default)]
    pub equipment_states: Vec<EquipmentState>,
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
    /// Branch protected at the terminal identified by `bus`. No location is inferred.
    #[serde(default)]
    pub protected_branch: Option<String>,
    /// Relay/element operation must be followed by an entered breaker interruption time.
    #[serde(default)]
    pub breaker_interrupting_s: Option<f64>,
    /// True only when the entered curve is a fuse total-clearing curve.
    #[serde(default)]
    pub fuse_total_clearing: bool,
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
    /// Explicit total arc duration. Automatic clearing requires a located upstream device and total-clearing data.
    #[serde(default)]
    pub clearing_s: Option<f64>,
    /// Explicitly authorized assumed exposure duration, used only when automatic
    /// total clearing is unavailable. A usable device curve takes precedence.
    #[serde(default)]
    pub fallback_duration_s: Option<f64>,
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
fn default_true() -> bool { true }
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
            switches: Vec::new(),
            switch_groups: Vec::new(),
            operating_cases: Vec::new(),
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
            switches: Vec::new(),
            switch_groups: Vec::new(),
            operating_cases: Vec::new(),
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
                        r0_over_r1: 1.0,
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
                in_service: true,
                v_pu: 1.0,
                angle_deg: 0.0,
                mva_sc: 250.0,
                decrement_curve: Vec::new(),
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
                    protected_branch: Some("t1".into()),
                    // Explicit illustrative sample input; deserialization never supplies a time.
                    breaker_interrupting_s: Some(0.08),
                    fuse_total_clearing: false,
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
                    protected_branch: Some("feeder".into()),
                    // Explicit illustrative sample input; deserialization never supplies a time.
                    breaker_interrupting_s: Some(0.08),
                    fuse_total_clearing: false,
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
                    fallback_duration_s: None,
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
                    fallback_duration_s: None,
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

    /// Branches without an explicit switch are always connected.
    pub fn branch_closed(&self, branch: &Branch) -> bool {
        self.switches.iter().find(|s| s.branch_id == branch.id).map_or(true, |s| s.closed)
    }

    /// Apply a named state to a project snapshot, retaining interlocks while
    /// dropping case definitions so this snapshot has one unambiguous state.
    pub fn apply_operating_case(&self, case: &OperatingCase) -> Result<Self, String> {
        let mut errors = self.validate();
        if !errors.is_empty() { return Err(errors.join("; ")); }
        let mut result = self.clone();
        let mut switch_targets = std::collections::HashSet::new();
        for state in &case.switch_states {
            if !switch_targets.insert(&state.switch_id) {
                errors.push(format!("operating case {} repeats switch '{}'", case.id, state.switch_id));
            } else if let Some(switch) = result.switches.iter_mut().find(|s| s.id == state.switch_id) {
                switch.closed = state.closed;
            } else {
                errors.push(format!("operating case {} references unknown switch '{}'", case.id, state.switch_id));
            }
        }
        let mut source_targets = std::collections::HashSet::new();
        for state in &case.source_states {
            if !source_targets.insert(&state.source_id) {
                errors.push(format!("operating case {} repeats source '{}'", case.id, state.source_id));
            } else if let Some(source) = result.sources.iter_mut().find(|s| s.id == state.source_id) {
                source.in_service = state.in_service;
            } else {
                errors.push(format!("operating case {} references unknown source '{}'", case.id, state.source_id));
            }
        }
        // A normal-state device or time may not clear a fault after transfer.
        // Start each alternate case with no inherited arc-clearing assignment.
        for equipment in &mut result.equipment {
            equipment.upstream_device = None;
            equipment.clearing_s = None;
            equipment.fallback_duration_s = None;
        }
        let mut equipment_targets = std::collections::HashSet::new();
        for state in &case.equipment_states {
            if !equipment_targets.insert(&state.equipment_id) {
                errors.push(format!("operating case {} repeats equipment '{}'", case.id, state.equipment_id));
            } else if let Some(equipment) = result.equipment.iter_mut().find(|e| e.id == state.equipment_id) {
                equipment.upstream_device = state.upstream_device.clone();
                equipment.clearing_s = state.clearing_s;
                equipment.fallback_duration_s = state.fallback_duration_s;
            } else {
                errors.push(format!("operating case {} references unknown equipment '{}'", case.id, state.equipment_id));
            }
        }
        result.operating_cases.clear();
        errors.extend(result.validate());
        if errors.is_empty() { Ok(result) } else { Err(errors.join("; ")) }
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
        for s in &self.switches {
            out.push(s.id.as_str());
        }
        for g in &self.switch_groups {
            out.push(g.id.as_str());
        }
        for c in &self.operating_cases {
            out.push(c.id.as_str());
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
            if let Some(first) = x.decrement_curve.first() {
                if first.time_s != 0.0 || first.current_ratio != 1.0 {
                    errors.push(format!("source {} decrement_curve must start at time_s 0 with current_ratio 1", x.id));
                }
                let mut previous_time = f64::NEG_INFINITY;
                for (index, point) in x.decrement_curve.iter().enumerate() {
                    if !point.time_s.is_finite() || point.time_s < 0.0 || point.time_s <= previous_time
                        || !point.current_ratio.is_finite() || point.current_ratio <= 0.0 {
                        errors.push(format!("source {} decrement_curve point {} must have finite, strictly increasing nonnegative time_s and finite positive current_ratio", x.id, index));
                    }
                    previous_time = point.time_s;
                }
            }
        }
        let mut controlled_branches = std::collections::HashSet::new();
        for x in &self.switches {
            claim(&x.id, "switch", &mut errors);
            if !self.branches.iter().any(|b| b.id == x.branch_id) {
                errors.push(format!("switch {} references unknown branch '{}'", x.id, x.branch_id));
            }
            if !controlled_branches.insert(&x.branch_id) {
                errors.push(format!("multiple switches control branch '{}'", x.branch_id));
            }
        }
        for x in &self.switch_groups {
            claim(&x.id, "switch group", &mut errors);
            if x.switch_ids.is_empty() {
                errors.push(format!("switch group {} has no switches", x.id));
            }
            if x.max_closed > x.switch_ids.len() {
                errors.push(format!("switch group {} max_closed exceeds its switch count", x.id));
            }
            let mut members = std::collections::HashSet::new();
            for id in &x.switch_ids {
                if !members.insert(id) {
                    errors.push(format!("switch group {} repeats switch '{}'", x.id, id));
                }
                if !self.switches.iter().any(|s| s.id == *id) {
                    errors.push(format!("switch group {} references unknown switch '{}'", x.id, id));
                }
            }
        }
        errors.extend(self.group_state_errors("normal", &self.switches));
        errors.extend(Self::source_setpoint_errors("normal", &self.sources));
        for case in &self.operating_cases {
            claim(&case.id, "operating case", &mut errors);
            if case.id == "normal" {
                errors.push("operating case id 'normal' is reserved for the base state".into());
            }
            let mut switches = self.switches.clone();
            let mut switch_targets = std::collections::HashSet::new();
            for state in &case.switch_states {
                if !switch_targets.insert(&state.switch_id) {
                    errors.push(format!("operating case {} repeats switch '{}'", case.id, state.switch_id));
                }
                if let Some(switch) = switches.iter_mut().find(|s| s.id == state.switch_id) {
                    switch.closed = state.closed;
                } else {
                    errors.push(format!("operating case {} references unknown switch '{}'", case.id, state.switch_id));
                }
            }
            let mut source_targets = std::collections::HashSet::new();
            let mut sources = self.sources.clone();
            for state in &case.source_states {
                if !source_targets.insert(&state.source_id) {
                    errors.push(format!("operating case {} repeats source '{}'", case.id, state.source_id));
                }
                if let Some(source) = sources.iter_mut().find(|s| s.id == state.source_id) {
                    source.in_service = state.in_service;
                } else {
                    errors.push(format!("operating case {} references unknown source '{}'", case.id, state.source_id));
                }
            }
            errors.extend(self.group_state_errors(&case.id, &switches));
            errors.extend(Self::source_setpoint_errors(&case.id, &sources));
            let mut equipment_targets = std::collections::HashSet::new();
            for state in &case.equipment_states {
                if !equipment_targets.insert(&state.equipment_id) {
                    errors.push(format!("operating case {} repeats equipment '{}'", case.id, state.equipment_id));
                }
                if !self.equipment.iter().any(|equipment| equipment.id == state.equipment_id) {
                    errors.push(format!("operating case {} references unknown equipment '{}'", case.id, state.equipment_id));
                }
                if let Some(device) = &state.upstream_device {
                    if self.device(device).is_none() {
                        errors.push(format!("operating case {} equipment {} references unknown device '{}'", case.id, state.equipment_id, device));
                    }
                }
                for (what, seconds) in [("clearing_s", state.clearing_s), ("fallback_duration_s", state.fallback_duration_s)] {
                    if seconds.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                        errors.push(format!("operating case {} equipment {} has invalid {}", case.id, state.equipment_id, what));
                    }
                }
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
        self.validate_physical(&mut errors);
        errors
    }

    fn group_state_errors(&self, label: &str, switches: &[Switch]) -> Vec<String> {
        let mut errors = Vec::new();
        for group in &self.switch_groups {
            let closed = group.switch_ids.iter().filter(|id| switches.iter().any(|s| s.id == **id && s.closed)).count();
            if closed > group.max_closed {
                errors.push(format!("state {label} closes {closed} switches in group {} (max {})", group.id, group.max_closed));
            }
        }
        errors
    }

    fn source_setpoint_errors(label: &str, sources: &[Source]) -> Vec<String> {
        let mut errors = Vec::new();
        for (i, source) in sources.iter().enumerate().filter(|(_, source)| source.in_service) {
            for other in sources.iter().skip(i + 1).filter(|other| other.in_service && other.bus == source.bus) {
                if (other.v_pu - source.v_pu).abs() >= 1e-8 {
                    errors.push(format!("state {label}: sources '{}' and '{}' have conflicting voltage setpoints on bus '{}'", source.id, other.id, source.bus));
                }
            }
        }
        errors
    }

    fn validate_physical(&self, errors: &mut Vec<String>) {
        let positive = |x: f64| x.is_finite() && x > 0.0;
        let nonnegative = |x: f64| x.is_finite() && x >= 0.0;
        let mut check = |ok: bool, id: &str, what: &str| {
            if !ok { errors.push(format!("{id}: invalid {what}")); }
        };
        check(positive(self.s_base_mva) && positive(self.arc_duration_cap_s), "project", "base or duration cap");
        for b in &self.buses {
            check(positive(b.kv) && b.shunt_kvar.is_finite(), &b.id, "bus voltage or shunt");
            check(b.bracing_ka.map_or(true, positive) && b.main_rating_a.map_or(true, positive), &b.id, "equipment rating");
        }
        for b in &self.branches {
            match b.kind {
                BranchKind::Line { r_ohm, x_ohm, b_siemens, r0_ohm, x0_ohm, ampacity_a } => {
                    check(nonnegative(r_ohm) && nonnegative(x_ohm) && positive(r_ohm.hypot(x_ohm))
                        && b_siemens.is_finite() && nonnegative(r0_ohm) && nonnegative(x0_ohm)
                        && ampacity_a.map_or(true, positive), &b.id, "line impedance or rating");
                }
                BranchKind::Transformer { kva, z_percent, xr, hv_kv, lv_kv, tap_percent, r0_over_r1, x0_over_x1, .. } => {
                    check([kva, z_percent, hv_kv, lv_kv].into_iter().all(positive) && hv_kv > lv_kv
                        && nonnegative(xr) && tap_percent.is_finite() && tap_percent > -100.0
                        && nonnegative(r0_over_r1) && nonnegative(x0_over_x1)
                        && (r0_over_r1 > 0.0 || xr * x0_over_x1 > 0.0), &b.id, "transformer impedance, winding voltage, or tap");
                }
            }
        }
        for s in &self.sources {
            check(positive(s.mva_sc) && positive(s.v_pu) && nonnegative(s.xr)
                && s.angle_deg.is_finite() && s.p_mw.is_finite()
                && nonnegative(s.r0_over_r1) && nonnegative(s.x0_over_x1)
                && (s.r0_over_r1 > 0.0 || s.xr * s.x0_over_x1 > 0.0), &s.id, "source impedance or voltage");
            check(s.qmin_mvar.map_or(true, f64::is_finite) && s.qmax_mvar.map_or(true, f64::is_finite)
                && s.qmin_mvar.unwrap_or(f64::NEG_INFINITY) <= s.qmax_mvar.unwrap_or(f64::INFINITY), &s.id, "reactive limits");
        }
        for l in &self.loads { check(l.kw.is_finite() && l.kvar.is_finite(), &l.id, "load"); }
        for m in &self.motors {
            check([m.hp, m.kv, m.pf, m.efficiency, m.x_subtransient_pu, m.xr].into_iter().all(positive)
                && m.pf <= 1.0 && m.efficiency <= 1.0, &m.id, "motor ratings, reactance, or X/R");
            // Allow customary nameplate/nominal differences (e.g. 460 V on 480 V).
            check(self.bus(&m.bus).map_or(false, |b| (m.kv / b.kv - 1.0).abs() <= 0.10), &m.id, "motor voltage (must be within 10% of bus voltage)");
        }
        for d in &self.devices {
            check(d.breaker_interrupting_s.map_or(true, positive) && !(d.fuse_total_clearing && d.breaker_interrupting_s.is_some()), &d.id, "clearing-time definition");
            if let Some(id) = &d.protected_branch {
                check(self.branches.iter().any(|b| b.id == *id && (b.from == d.bus || b.to == d.bus)), &d.id, "protected branch terminal");
            }
            let pair = |p: Option<f64>, t: Option<f64>| p.map_or(true, positive) && t.map_or(true, positive) && (t.is_none() || p.is_some());
            let ok = match d.curve {
                CurveSpec::Iec { pickup_a, tms, inst_a, inst_s, .. } => positive(pickup_a) && positive(tms) && pair(inst_a, inst_s),
                CurveSpec::Ieee { pickup_a, td, inst_a, inst_s, .. } => positive(pickup_a) && positive(td) && pair(inst_a, inst_s),
                CurveSpec::Definite { pickup_a, time_s } => positive(pickup_a) && positive(time_s),
                CurveSpec::ThermalMagnetic { lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s } => positive(lt_pickup_a) && positive(lt_delay_s) && pair(st_pickup_a, st_delay_s) && pair(inst_a, inst_s),
                CurveSpec::SettingsNotCollected { .. } => true,
            };
            check(ok, &d.id, "pickup or delay");
        }
        for e in &self.equipment {
            check([e.gap_mm, e.distance_mm, e.height_mm, e.width_mm, e.depth_mm].into_iter().all(positive)
                && e.clearing_s.map_or(true, positive) && e.fallback_duration_s.map_or(true, positive), &e.id, "enclosure, gap, distance, or clearing time");
        }
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

    #[test]
    fn operating_cases_apply_overrides_and_enforce_interlocks() {
        let mut project = Project::sample();
        project.switches = vec![
            Switch { id: "main".into(), name: "Main".into(), branch_id: "t1".into(), kind: SwitchKind::Breaker, closed: true },
            Switch { id: "tie".into(), name: "Tie".into(), branch_id: "feeder".into(), kind: SwitchKind::Tie, closed: false },
        ];
        project.switch_groups.push(SwitchGroup { id: "interlock".into(), switch_ids: vec!["main".into(), "tie".into()], max_closed: 1 });
        let transfer = OperatingCase {
            id: "transfer".into(), name: "Transfer".into(),
            switch_states: vec![SwitchState { switch_id: "main".into(), closed: false }, SwitchState { switch_id: "tie".into(), closed: true }],
            source_states: vec![SourceState { source_id: "grid".into(), in_service: false }],
            equipment_states: Vec::new(),
        };
        project.operating_cases.push(transfer.clone());
        assert!(project.validate().is_empty(), "{:?}", project.validate());
        let active = project.apply_operating_case(&transfer).unwrap();
        assert!(!active.branch_closed(&active.branches[0]));
        assert!(active.branch_closed(&active.branches[1]));
        assert!(!active.sources[0].in_service);
        assert!(active.operating_cases.is_empty());
        assert!(project.branch_closed(&project.branches[0]));
        assert!(!project.branch_closed(&project.branches[1]));

        project.operating_cases[0].switch_states.remove(0);
        assert!(project.validate().iter().any(|error| error.contains("max 1")));
        project.operating_cases[0].id = "normal".into();
        assert!(project.validate().iter().any(|error| error.contains("reserved")));
    }

    #[test]
    fn legacy_json_defaults_sources_on_and_has_no_switches() {
        let mut value = serde_json::to_value(Project::sample()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("switches");
        object.remove("switch_groups");
        object.remove("operating_cases");
        object.get_mut("sources").unwrap().as_array_mut().unwrap()[0].as_object_mut().unwrap().remove("in_service");
        assert!(object["sources"][0].get("decrement_curve").is_none());
        let project: Project = serde_json::from_value(value).unwrap();
        assert!(project.sources[0].in_service);
        assert!(project.sources[0].decrement_curve.is_empty());
        assert!(project.switches.is_empty() && project.switch_groups.is_empty() && project.operating_cases.is_empty());
    }

    #[test]
    fn source_decrement_curve_roundtrips_and_interpolates_with_endpoint_hold() {
        let mut project = Project::sample();
        let source = &mut project.sources[0];
        assert_eq!(source.decrement_ratio_at(0.7), 1.0);
        source.decrement_curve = vec![
            DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
            DecrementPoint { time_s: 0.2, current_ratio: 0.4 },
            DecrementPoint { time_s: 1.0, current_ratio: 1.2 },
        ];
        assert!(project.validate().is_empty(), "{:?}", project.validate());
        let source = &project.sources[0];
        assert_eq!(source.decrement_ratio_at(-1.0), 1.0);
        assert!((source.decrement_ratio_at(0.1) - 0.7).abs() < 1e-12);
        assert!((source.decrement_ratio_at(0.6) - 0.8).abs() < 1e-12);
        assert_eq!(source.decrement_ratio_at(3.0), 1.2);

        let value = serde_json::to_value(&project).unwrap();
        assert_eq!(value["sources"][0]["decrement_curve"][2]["current_ratio"], 1.2);
        let parsed: Project = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.sources[0].decrement_curve, source.decrement_curve);
    }

    #[test]
    fn source_decrement_curve_rejects_invalid_anchor_times_and_ratios() {
        let mut project = Project::sample();
        let source = &mut project.sources[0];
        source.decrement_curve = vec![DecrementPoint { time_s: 0.1, current_ratio: 1.0 }];
        assert!(project.validate().iter().any(|error| error.contains("must start at time_s 0")));
        project.sources[0].decrement_curve[0].time_s = 0.0;
        project.sources[0].decrement_curve[0].current_ratio = 0.9;
        assert!(project.validate().iter().any(|error| error.contains("current_ratio 1")));

        project.sources[0].decrement_curve = vec![
            DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
            DecrementPoint { time_s: 0.1, current_ratio: 0.5 },
        ];
        for (time_s, current_ratio) in [(0.0, 0.4), (-0.1, 0.4), (f64::NAN, 0.4), (0.2, 0.0), (0.2, f64::INFINITY)] {
            project.sources[0].decrement_curve[1] = DecrementPoint { time_s, current_ratio };
            assert!(project.validate().iter().any(|error| error.contains("decrement_curve point 1")), "{time_s:?} {current_ratio:?}");
        }
        project.sources[0].decrement_curve[1] = DecrementPoint { time_s: 0.1, current_ratio: 0.5 };
        project.sources[0].decrement_curve.push(DecrementPoint { time_s: 0.05, current_ratio: 0.4 });
        assert!(project.validate().iter().any(|error| error.contains("decrement_curve point 2")));
    }

    #[test]
    fn switch_normal_state_must_be_explicit() {
        let mut value = serde_json::to_value(Project::sample()).unwrap();
        value["switches"] = serde_json::json!([{
            "id": "tie", "name": "Tie", "branch_id": "feeder", "kind": "tie"
        }]);
        let error = serde_json::from_value::<Project>(value).unwrap_err().to_string();
        assert!(error.contains("closed"), "{error}");
    }

    #[test]
    fn alternate_case_clearing_inputs_require_explicit_reentry() {
        let mut project = Project::sample();
        project.equipment[0].clearing_s = Some(0.2);
        project.equipment[0].fallback_duration_s = Some(0.8);
        let case = OperatingCase {
            id: "transfer".into(), name: "Transfer".into(),
            switch_states: Vec::new(), source_states: Vec::new(), equipment_states: Vec::new(),
        };
        project.operating_cases.push(case.clone());
        let active = project.apply_operating_case(&case).unwrap();
        assert_eq!(project.equipment[0].upstream_device.as_deref(), Some("brk-main"));
        assert_eq!(project.equipment[0].clearing_s, Some(0.2));
        assert_eq!(project.equipment[0].fallback_duration_s, Some(0.8));
        assert!(active.equipment.iter().all(|e| e.upstream_device.is_none() && e.clearing_s.is_none() && e.fallback_duration_s.is_none()));

        project.operating_cases[0].equipment_states.push(EquipmentState {
            equipment_id: "af-mcc".into(), upstream_device: Some("brk-main".into()),
            clearing_s: Some(0.4), fallback_duration_s: Some(1.0),
        });
        let active = project.apply_operating_case(&project.operating_cases[0]).unwrap();
        assert_eq!(active.equipment[0].upstream_device.as_deref(), Some("brk-main"));
        assert_eq!(active.equipment[0].clearing_s, Some(0.4));
        assert_eq!(active.equipment[0].fallback_duration_s, Some(1.0));
        assert!(active.equipment[1].upstream_device.is_none());

        project.operating_cases[0].equipment_states[0].clearing_s = Some(-0.1);
        assert!(project.validate().iter().any(|error| error.contains("invalid clearing_s")));
        project.operating_cases[0].equipment_states[0].clearing_s = Some(0.4);
        project.operating_cases[0].equipment_states[0].upstream_device = Some("missing".into());
        assert!(project.validate().iter().any(|error| error.contains("unknown device 'missing'")));
    }

    #[test]
    fn standby_sources_on_same_bus_may_have_distinct_voltage_setpoints() {
        let mut project = Project::sample();
        let mut generator = project.sources[0].clone();
        generator.id = "standby".into();
        generator.name = "Standby".into();
        generator.v_pu = 1.05;
        generator.in_service = false;
        project.sources.push(generator);
        let transfer = OperatingCase {
            id: "generator".into(), name: "Generator".into(), switch_states: Vec::new(),
            source_states: vec![SourceState { source_id: "grid".into(), in_service: false }, SourceState { source_id: "standby".into(), in_service: true }],
            equipment_states: Vec::new(),
        };
        project.operating_cases.push(transfer.clone());
        assert!(project.validate().is_empty(), "{:?}", project.validate());
        let active = project.apply_operating_case(&transfer).unwrap();
        assert!(!active.sources[0].in_service && active.sources[1].in_service);
        project.operating_cases[0].source_states.remove(0);
        assert!(project.validate().iter().any(|error| error.contains("conflicting voltage setpoints")));
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
