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
    /// Maximum arc duration reported for incident energy. IEEE 1584-2018 Clause 6.9.1
    /// allows 2 s only when a person can move away from the arc.
    #[serde(default = "default_cap")]
    pub arc_duration_cap_s: f64,
    /// When true, a clearing time longer than `arc_duration_cap_s` is replaced
    /// by the cap and the result is marked assumed. When false, the arc is not
    /// capped. Existing studies default to true.
    #[serde(default = "default_true")]
    pub escape_possible: bool,
    /// How long a motor with no feeder device keeps feeding the arc, in seconds.
    /// IEEE 1584 requires the contribution and does not publish a decay time.
    /// The default is five 60 Hz cycles. A motor behind a feeder device clears
    /// when that device clears, and this window is not applied.
    #[serde(default = "default_motor_contribution")]
    pub motor_contribution_s: f64,
    #[serde(default)]
    pub prefault: Prefault,
    /// Fault-study network used for the main fault result and for arc flash.
    /// `comprehensive` (the default) uses each machine's entered reactance.
    /// `ansi` picks the first-cycle C37.010 duty network at buses through 1 kV
    /// and the interrupting network above 1 kV, and turns the generator /
    /// synchronous-motor step-down off. Arc-flash post-motor stages keep that
    /// duty with induction motors removed.
    #[serde(default, skip_serializing_if = "is_comprehensive")]
    pub fault_method: FaultMethod,
    /// Staged arc flash: incident energy accumulates in trip order until the
    /// devices that have operated have cleared this fraction of the total bus
    /// arcing current. The arc is then taken as extinguished. Default 0.80.
    #[serde(default = "default_cleared_fraction")]
    pub cleared_fault_fraction: f64,
    /// Optional step-down of generator and synchronous-motor contribution
    /// during an arc. None (the default) keeps their initial contribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_step_down: Option<MachineStepDown>,
    /// Optional upstream backup check for staged arc clearing. Off when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_backup: Option<UpstreamBackup>,
    /// Re-time each device on its reduced current after motors drop out,
    /// machines step down, or a source decrement falls. Default false: each
    /// device is timed once at its initial arcing current.
    #[serde(default)]
    pub retime_on_reduced_current: bool,
    /// The single per-unit prefault voltage for `prefault: flat`. Default 1.0.
    #[serde(default = "default_one")]
    pub prefault_pu: f64,
    /// Low-voltage IEC 60909-0 voltage factor. False uses cmax = 1.05
    /// (nominal-voltage tolerance of +6%). True uses cmax = 1.10 (+10%).
    /// cmin stays 0.95 at or below 1 kV. Above 1 kV the factor is 1.10 / 1.00
    /// either way.
    #[serde(default)]
    pub low_voltage_tolerance_plus_10: bool,
    /// Inputs that were not on the source document. The text report prints every entry.
    #[serde(default)]
    pub assumptions: Vec<Assumption>,
    /// Neutral grounding of sources and transformer wye windings. A source or
    /// wye winding without an entry is solidly grounded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub neutral_grounding: Vec<NeutralGrounding>,
}

/// Neutral connection of one source or one transformer wye winding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeutralGrounding {
    /// A source id or a transformer branch id.
    pub element_id: String,
    /// Transformer winding (`hv` or `lv`); omit for a source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winding: Option<Winding>,
    pub method: GroundingMethod,
    /// Neutral resistor/reactor in physical ohms; only for `impedance`.
    #[serde(default)]
    pub r_ohm: f64,
    #[serde(default)]
    pub x_ohm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Winding {
    Hv,
    Lv,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundingMethod {
    /// Neutral bonded directly to ground (the default when no entry exists).
    Solid,
    /// Neutral grounded through `r_ohm + j x_ohm`; 3Zn enters the zero sequence.
    Impedance,
    /// No neutral connection; the winding or source has no zero-sequence path.
    Ungrounded,
}

/// The rated winding voltage nearest a bus voltage on a logarithmic scale.
pub fn nearest_winding_kv(bus_kv: f64, hv: f64, lv: f64) -> f64 {
    if (bus_kv / hv).ln().abs() <= (bus_kv / lv).ln().abs() { hv } else { lv }
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
    /// One per-unit prefault voltage for every bus, `Project::prefault_pu`
    /// (default 1.0).
    #[default]
    Flat,
    /// Converged load-flow bus voltages. Under the comprehensive fault method,
    /// a load-flow error switches the study run to `NoLoadTap` (not persisted).
    Loadflow,
    /// No-load voltages from each source's entered voltage through
    /// transformer ratios and taps.
    NoLoadTap,
    /// An entered per-unit prefault voltage on every bus, `Bus::prefault_pu`.
    PerBus,
}

/// Which network the main fault study and arc flash use for bolted currents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultMethod {
    /// Entered machine reactances, no C37.010 duty multipliers. Default.
    #[default]
    Comprehensive,
    /// IEEE C37.010-1979 5.4.1 duty networks selected by bus voltage: first-cycle
    /// through 1 kV, interrupting above 1 kV. The generator and synchronous-motor
    /// step-down is not applied.
    Ansi,
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
    /// Test X/R of the equipment bracing rating. Absent means the bracing
    /// comparison is symmetrical only and is not an asymmetrical-duty pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bracing_test_xr: Option<f64>,
    /// IEEE 1584 equipment class used when this bus has no arc-equipment record.
    /// Absent means the class is inferred from the bus name, or every enclosed
    /// class at this voltage is evaluated and the governing result is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment_class: Option<EquipmentClass>,
    /// Entered per-unit prefault voltage, used when the study prefault is
    /// `per_bus`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefault_pu: Option<f64>,
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
    /// From-bus delta, to-bus grounded wye. The phase shift follows ANSI
    /// C57.12.00 (HV positive sequence leads LV by 30°) for either winding
    /// arrangement and either branch orientation.
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

/// JSON shape is unchanged from the plain derive; `defaulted` records which
/// engineering inputs were omitted so the assumption can be reported.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "SourceWire", into = "SourceWire")]
pub struct Source {
    pub id: String,
    pub name: String,
    pub bus: String,
    /// Whether this utility or generator is available in the current state.
    pub in_service: bool,
    pub v_pu: f64,
    pub angle_deg: f64,
    pub mva_sc: f64,
    /// Optional symmetrical three-phase short-circuit current profile at this
    /// source's terminals. Ratios are relative to the initial `mva_sc` value.
    /// An empty profile retains a constant source strength.
    pub decrement_curve: Vec<DecrementPoint>,
    pub xr: f64,
    /// Defaults to 1.0 when omitted (reported as an assumption).
    pub x0_over_x1: f64,
    /// Defaults to 1.0 when omitted (reported as an assumption).
    pub r0_over_r1: f64,
    pub is_slack: bool,
    /// Positive is generation. Ignored on the slack bus.
    pub p_mw: f64,
    pub qmin_mvar: Option<f64>,
    pub qmax_mvar: Option<f64>,
    /// Machine class for the first-cycle and interrupting reactance
    /// multipliers (IEEE C37.010-1979 5.4.1). None keeps `mva_sc` for both duties.
    pub machine_kind: Option<GeneratorKind>,
    /// Three-phase short-circuit MVA at the terminals from the transient
    /// reactance Xd'. Used only by `hydro_without_amortisseur`.
    pub transient_mva_sc: Option<f64>,
    /// Generator nameplate for the IEC 60909 generator or power station unit
    /// correction factor. None leaves the maximum current uncorrected for
    /// this source (reported when `machine_kind` is a rotating machine).
    pub generator: Option<GeneratorRating>,
    /// Names of fields omitted from the input and filled with defaults. Not a
    /// JSON field: a defaulted value that is still at its default is omitted
    /// again when saved, so the assumption stays visible.
    pub defaulted: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceWire {
    id: String,
    name: String,
    bus: String,
    #[serde(default = "default_true")]
    in_service: bool,
    #[serde(default = "default_vpu")]
    v_pu: f64,
    #[serde(default)]
    angle_deg: f64,
    mva_sc: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    decrement_curve: Vec<DecrementPoint>,
    xr: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x0_over_x1: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    r0_over_r1: Option<f64>,
    #[serde(default)]
    is_slack: bool,
    #[serde(default)]
    p_mw: f64,
    #[serde(default)]
    qmin_mvar: Option<f64>,
    #[serde(default)]
    qmax_mvar: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    machine_kind: Option<GeneratorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transient_mva_sc: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generator: Option<GeneratorRating>,
}

/// Take an entered value, or record the field as defaulted.
fn entered_or(value: Option<f64>, name: &str, default: f64, defaulted: &mut Vec<String>) -> f64 {
    value.unwrap_or_else(|| { defaulted.push(name.to_string()); default })
}

/// Omit a value on output only while it is still an unedited default.
fn unless_default(value: f64, name: &str, default: f64, defaulted: &[String]) -> Option<f64> {
    if value == default && defaulted.iter().any(|d| d == name) { None } else { Some(value) }
}

impl From<SourceWire> for Source {
    fn from(w: SourceWire) -> Self {
        let mut defaulted = Vec::new();
        let x0_over_x1 = entered_or(w.x0_over_x1, "x0_over_x1", default_one(), &mut defaulted);
        let r0_over_r1 = entered_or(w.r0_over_r1, "r0_over_r1", default_one(), &mut defaulted);
        Self {
            id: w.id, name: w.name, bus: w.bus, in_service: w.in_service, v_pu: w.v_pu, angle_deg: w.angle_deg,
            mva_sc: w.mva_sc, decrement_curve: w.decrement_curve, xr: w.xr, x0_over_x1, r0_over_r1,
            is_slack: w.is_slack, p_mw: w.p_mw, qmin_mvar: w.qmin_mvar, qmax_mvar: w.qmax_mvar,
            machine_kind: w.machine_kind, transient_mva_sc: w.transient_mva_sc, generator: w.generator, defaulted,
        }
    }
}

impl From<Source> for SourceWire {
    fn from(s: Source) -> Self {
        Self {
            x0_over_x1: unless_default(s.x0_over_x1, "x0_over_x1", default_one(), &s.defaulted),
            r0_over_r1: unless_default(s.r0_over_r1, "r0_over_r1", default_one(), &s.defaulted),
            id: s.id, name: s.name, bus: s.bus, in_service: s.in_service, v_pu: s.v_pu, angle_deg: s.angle_deg,
            mva_sc: s.mva_sc, decrement_curve: s.decrement_curve, xr: s.xr,
            is_slack: s.is_slack, p_mw: s.p_mw, qmin_mvar: s.qmin_mvar, qmax_mvar: s.qmax_mvar,
            machine_kind: s.machine_kind, transient_mva_sc: s.transient_mva_sc, generator: s.generator,
        }
    }
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
    /// Retained so older projects still load. Not used as an arc duration.
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

/// Generator nameplate used by the IEC 60909 correction factors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorRating {
    /// Rated apparent power, MVA.
    pub rated_mva: f64,
    /// Rated power factor, 0 to 1.
    pub rated_pf: f64,
    /// Rated voltage, kV. Omitted uses the bus nominal voltage and is reported.
    #[serde(default)]
    pub rated_kv: Option<f64>,
    /// Set when the generator and its step-up transformer form a power
    /// station unit; the unit factor then replaces K_G and K_T.
    #[serde(default)]
    pub unit: Option<PowerStationUnit>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerStationUnit {
    /// Id of the unit transformer branch, with one terminal on the source bus.
    pub transformer: String,
    /// True selects K_S, false K_SO.
    pub on_load_tap_changer: bool,
    /// K_SO generator voltage regulation range, per unit. Omitted is 0 and reported.
    #[serde(default)]
    pub p_g: Option<f64>,
    /// K_SO signed tap offset in use, per unit; the tap term is 1 + p_t. Omitted is 0 and reported.
    #[serde(default)]
    pub p_t: Option<f64>,
}

/// Motor class for the IEEE C37.010-1979 5.4.1 reactance multipliers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotorKind {
    Induction,
    Synchronous,
}

/// Rotating-machine class of a source for the IEEE C37.010-1979 5.4.1
/// reactance multipliers. `utility` is a remote equivalent and is not scaled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorKind {
    Utility,
    Turbogenerator,
    HydroWithAmortisseur,
    HydroWithoutAmortisseur,
    SynchronousCondenser,
}

/// JSON shape is unchanged from the plain derive; omitted ratings take the
/// typical values below and are reported as assumptions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "MotorWire", into = "MotorWire")]
pub struct Motor {
    pub id: String,
    pub name: String,
    pub bus: String,
    pub hp: f64,
    pub kv: f64,
    /// Defaults to 0.85 when omitted.
    pub pf: f64,
    /// Defaults to 0.94 when omitted.
    pub efficiency: f64,
    /// Defaults to 0.17 pu when omitted.
    pub x_subtransient_pu: f64,
    /// Defaults to 6 when omitted.
    pub xr: f64,
    /// Induction or synchronous, for the first-cycle and interrupting
    /// reactance multipliers (IEEE C37.010-1979 5.4.1). None keeps
    /// `x_subtransient_pu` unscaled for both duties and is reported.
    pub machine_kind: Option<MotorKind>,
    /// Synchronous speed in r/min. Needed to classify induction motors above
    /// 250 hp and up to 1000 hp.
    pub synchronous_rpm: Option<f64>,
    /// 1 or 3. None is treated as three-phase, the existing network model.
    pub phases: Option<u8>,
    /// Exponential decay time constant of this motor's arc-flash
    /// contribution, in seconds. None keeps the full contribution for the
    /// fixed motor contribution window.
    pub decay_time_constant_s: Option<f64>,
    /// Names of fields omitted from the input and filled with defaults; see `Source::defaulted`.
    pub defaulted: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotorWire {
    id: String,
    name: String,
    bus: String,
    hp: f64,
    kv: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pf: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    efficiency: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x_subtransient_pu: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    xr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    machine_kind: Option<MotorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    synchronous_rpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phases: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decay_time_constant_s: Option<f64>,
}

impl From<MotorWire> for Motor {
    fn from(w: MotorWire) -> Self {
        let mut defaulted = Vec::new();
        let pf = entered_or(w.pf, "pf", default_pf(), &mut defaulted);
        let efficiency = entered_or(w.efficiency, "efficiency", default_eff(), &mut defaulted);
        let x_subtransient_pu = entered_or(w.x_subtransient_pu, "x_subtransient_pu", default_xd(), &mut defaulted);
        let xr = entered_or(w.xr, "xr", default_mxr(), &mut defaulted);
        Self {
            id: w.id, name: w.name, bus: w.bus, hp: w.hp, kv: w.kv, pf, efficiency, x_subtransient_pu, xr,
            machine_kind: w.machine_kind, synchronous_rpm: w.synchronous_rpm, phases: w.phases, decay_time_constant_s: w.decay_time_constant_s, defaulted,
        }
    }
}

impl From<Motor> for MotorWire {
    fn from(m: Motor) -> Self {
        Self {
            pf: unless_default(m.pf, "pf", default_pf(), &m.defaulted),
            efficiency: unless_default(m.efficiency, "efficiency", default_eff(), &m.defaulted),
            x_subtransient_pu: unless_default(m.x_subtransient_pu, "x_subtransient_pu", default_xd(), &m.defaulted),
            xr: unless_default(m.xr, "xr", default_mxr(), &m.defaulted),
            machine_kind: m.machine_kind, synchronous_rpm: m.synchronous_rpm, phases: m.phases, decay_time_constant_s: m.decay_time_constant_s,
            id: m.id, name: m.name, bus: m.bus, hp: m.hp, kv: m.kv,
        }
    }
}

impl Source {
    /// Typical value used for an omitted source field, for assumption reports.
    pub fn default_value(field: &str) -> Option<f64> {
        matches!(field, "x0_over_x1" | "r0_over_r1").then(default_one)
    }
}

impl Motor {
    /// Typical value used for an omitted motor field, for assumption reports.
    pub fn default_value(field: &str) -> Option<f64> {
        match field {
            "pf" => Some(default_pf()),
            "efficiency" => Some(default_eff()),
            "x_subtransient_pu" => Some(default_xd()),
            "xr" => Some(default_mxr()),
            _ => None,
        }
    }
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
    /// True only when the device is a fuse and the entered curve is its
    /// total-clearing curve. Implies `curve_is_total_clearing`; it also marks
    /// the device as a fuse (SLD symbol, package export device type).
    #[serde(default)]
    pub fuse_total_clearing: bool,
    /// True when the curve's governing band (the max band of a point curve)
    /// already includes total clearing, so no breaker interrupting time is
    /// added. Use for breakers or other devices whose curve is a
    /// total-clearing band; fuses may keep using `fuse_total_clearing`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub curve_is_total_clearing: bool,
    /// "collected" or "assumed". Empty means unspecified.
    #[serde(default)]
    pub basis: String,
    /// Where the device data came from when it was applied from a device
    /// library. Identification only: studies use `curve` and the fields above,
    /// never the library file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryLink>,
    /// Symmetrical interrupting rating and the test X/R it was proved at.
    /// Snapshot. Studies do not reopen a library file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupting: Option<InterruptingRating>,
    /// Manufacturer current-limiting let-through. Used only when the device
    /// terminal current is at or above `threshold_a`. Annex H incident-energy
    /// equations are not applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_limiting: Option<CurrentLimiting>,
}

/// Peak let-through threshold and the clearing time above it.
/// IEEE 1584-2018 Clause 6.9.2, or the 60 Hz half-cycle / quarter-cycle table.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentLimiting {
    /// Device terminal current, in amperes, at which the fuse is current limiting.
    /// For the cycle table this is the current where the total-clearing curve
    /// crosses 0.01 s, or the manufacturer's stated threshold.
    pub threshold_a: f64,
    /// Manufacturer total clearing time in the current-limiting region.
    /// Used when `cycle_table` is false. Zero is accepted only with the cycle table.
    pub clearing_s: f64,
    /// Manufacturer effective arc current, in kA. When this fuse is the only
    /// source still feeding the arc, this current replaces the calculated
    /// arcing current and the variation factor is not applied again. Absent
    /// means the clearing time is used and the calculated arc current is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_arc_ka: Option<f64>,
    /// When true, duration at or above `threshold_a` follows the 60 Hz
    /// half-cycle / quarter-cycle table instead of `clearing_s`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cycle_table: bool,
}

/// Symmetrical interrupting rating proved at one test X/R and voltage.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterruptingRating {
    pub symmetrical_ka: f64,
    pub test_xr: f64,
    pub voltage_v: f64,
    /// Contact parting time in cycles at the study frequency. Set on a breaker
    /// above 1 kV to report the remote multiplying factor. Omitted ratings
    /// stay on the half-cycle comparison alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_parting_cycles: Option<f64>,
    /// Voltage range factor K. None means 1. Closing and latching capability
    /// above 1 kV is 1.6 times K times `symmetrical_ka`. Enter the rated
    /// short-circuit current in `symmetrical_ka`; do not also premultiply by K.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_range_factor: Option<f64>,
}

impl Device {
    /// The curve already includes total clearing: either a fuse total-clearing
    /// curve (`fuse_total_clearing`) or a general total-clearing curve
    /// (`curve_is_total_clearing`). No breaker interrupting time is added.
    pub fn curve_includes_clearing(&self) -> bool {
        self.fuse_total_clearing || self.curve_is_total_clearing
    }
}

/// Identity of the library entry a device was applied from. The curve itself
/// is copied into the project (a snapshot), so a later library change does not
/// silently change a study.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryLink {
    /// Device id within the library it came from.
    pub device_id: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub library: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub manufacturer: String,
    #[serde(default, rename = "type", skip_serializing_if = "String::is_empty")]
    pub device_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// Size or rating label chosen in the library.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_a: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_v: Option<f64>,
    /// Library modification stamp of the entry when it was applied.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modified: String,
    /// Library elements that were not applied because Flashmob cannot evaluate them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_elements: Vec<String>,
}

/// What happens above the highest current of a point curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointTail {
    /// Keep the last point's time. Results that use it are marked assumed.
    HoldLastTime,
    /// No operating time is available above the last point.
    Unavailable,
}

/// How the bands of a point curve were identified.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandInterpretation {
    /// Entered from a source that states which band is which.
    #[default]
    Entered,
    /// Which band is the faster band and which is the slower band was inferred.
    /// The slower band governs operating time. It is not total clearing unless
    /// the device is marked that way. Results that use the curve are marked assumed.
    Assumed,
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
    /// Time-current points as [amperes, seconds], currents strictly increasing
    /// and times non-increasing. Between points time is interpolated linearly
    /// in log(current)–log(time). Below the first current the device does not
    /// operate. With `max_points`, `points` is the faster (minimum) band and
    /// `max_points` the slower (maximum or total-clearing) band; operating and
    /// clearing times use the slower band.
    Points {
        points: Vec<[f64; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_points: Option<Vec<[f64; 2]>>,
        /// Behavior above the last point's current. Required so it is always explicit.
        beyond_last: PointTail,
        #[serde(default)]
        band_interpretation: BandInterpretation,
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
    /// Retained so older projects still load. The study does not turn this into
    /// incident energy. Missing clearing data is an arc-flash failure that names the gap.
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
fn default_motor_contribution() -> f64 {
    5.0 / 60.0
}
fn default_vpu() -> f64 {
    1.0
}
fn default_cleared_fraction() -> f64 {
    0.80
}

/// Generator and synchronous-motor contribution after `after_cycles` (60 Hz
/// cycles) becomes `percent_of_rated` percent of the machine's rated current.
/// `after_cycles` must exceed the induction-motor window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineStepDown {
    /// Generator step-down level, percent of rated current. Also used for
    /// synchronous motors unless `synchronous_motor_percent_of_rated` is set.
    pub percent_of_rated: f64,
    pub after_cycles: f64,
    /// Step generators down. Default true.
    #[serde(default = "default_true")]
    pub apply_to_generators: bool,
    /// Step synchronous motors down. Default true.
    #[serde(default = "default_true")]
    pub apply_to_synchronous_motors: bool,
    /// Separate synchronous-motor level; `percent_of_rated` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synchronous_motor_percent_of_rated: Option<f64>,
}

impl MachineStepDown {
    pub fn synchronous_percent(&self) -> f64 {
        self.synchronous_motor_percent_of_rated.unwrap_or(self.percent_of_rated)
    }
}

/// Optional upstream backup check for staged arc clearing. A device up to
/// `levels` device levels above a branch's own fastest device times that
/// branch instead when it clears sooner and carries at least `ratio` of the
/// near device's current. Fuses and current-limiting devices are not used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamBackup {
    #[serde(default = "default_backup_levels")]
    pub levels: usize,
    #[serde(default = "default_backup_ratio")]
    pub ratio: f64,
}

fn default_backup_levels() -> usize {
    5
}
fn default_backup_ratio() -> f64 {
    0.80
}

fn default_one() -> f64 {
    1.0
}
fn is_comprehensive(method: &FaultMethod) -> bool {
    *method == FaultMethod::Comprehensive
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
            escape_possible: default_true(),
            motor_contribution_s: default_motor_contribution(),
            prefault: Prefault::Flat,
            fault_method: FaultMethod::Comprehensive,
            prefault_pu: 1.0,
            cleared_fault_fraction: 0.80,
            machine_step_down: None,
            upstream_backup: None,
            retime_on_reduced_current: false,
            low_voltage_tolerance_plus_10: false,
            assumptions: Vec::new(),
            neutral_grounding: Vec::new(),
        }
    }
}

impl Project {
    pub fn sample() -> Self {
        Self {
            name: "Sample".to_string(),
            s_base_mva: 100.0,
            arc_duration_cap_s: 2.0,
            escape_possible: true,
            motor_contribution_s: default_motor_contribution(),
            prefault: Prefault::Flat,
            fault_method: FaultMethod::Comprehensive,
            prefault_pu: 1.0,
            cleared_fault_fraction: 0.80,
            machine_step_down: None,
            upstream_backup: None,
            retime_on_reduced_current: false,
            low_voltage_tolerance_plus_10: false,
            assumptions: Vec::new(),
            switches: Vec::new(),
            switch_groups: Vec::new(),
            operating_cases: Vec::new(),
            neutral_grounding: Vec::new(),
            buses: vec![
                Bus { id: "util".into(), name: "Utility".into(), kv: 12.47, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None, prefault_pu: None },
                Bus { id: "mcc".into(), name: "MCC-1".into(), kv: 0.48, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None, prefault_pu: None },
                Bus { id: "pnl".into(), name: "Panel A".into(), kv: 0.48, shunt_kvar: 0.0, bracing_ka: None, main_rating_a: None, bracing_test_xr: None, equipment_class: None, prefault_pu: None },
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
            sources: vec![Source { generator: None, machine_kind: None, transient_mva_sc: None,
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
                defaulted: Vec::new(),
            }],
            motors: vec![Motor { machine_kind: None, synchronous_rpm: None, phases: None, decay_time_constant_s: None,
                id: "m1".into(),
                name: "M1".into(),
                bus: "mcc".into(),
                hp: 200.0,
                kv: 0.48,
                pf: 0.88,
                efficiency: 0.94,
                x_subtransient_pu: 0.17,
                xr: 6.0,
                defaulted: Vec::new(),
            }],
            devices: vec![
                Device {
                    protected_branch: Some("t1".into()),
                    // Explicit illustrative sample input; deserialization never supplies a time.
                    breaker_interrupting_s: Some(0.08),
                    fuse_total_clearing: false,
                    curve_is_total_clearing: false,
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
                    library: None, interrupting: None, current_limiting: None,
                },
                Device {
                    protected_branch: Some("feeder".into()),
                    // Explicit illustrative sample input; deserialization never supplies a time.
                    breaker_interrupting_s: Some(0.08),
                    fuse_total_clearing: false,
                    curve_is_total_clearing: false,
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
                    library: None, interrupting: None, current_limiting: None,
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

    /// Neutral grounding entered for a source (`winding: None`) or for one
    /// transformer winding. None means solidly grounded.
    pub fn grounding_of(&self, element_id: &str, winding: Option<Winding>) -> Option<&NeutralGrounding> {
        self.neutral_grounding.iter().find(|g| g.element_id == element_id && g.winding == winding)
    }

    /// Whether a transformer's HV winding is on its `from` bus, judged from
    /// the bus voltages as the network builder does.
    pub fn transformer_hv_is_from(&self, branch: &Branch) -> Option<bool> {
        let BranchKind::Transformer { hv_kv, lv_kv, .. } = branch.kind else { return None };
        let from = self.bus(&branch.from)?.kv;
        Some(nearest_winding_kv(from, hv_kv, lv_kv) == hv_kv)
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
        if !(self.cleared_fault_fraction.is_finite() && self.cleared_fault_fraction > 0.0 && self.cleared_fault_fraction <= 1.0) {
            errors.push("cleared_fault_fraction must be above 0 and at most 1".into());
        }
        if let Some(step) = &self.machine_step_down {
            if !(step.percent_of_rated.is_finite() && step.percent_of_rated >= 0.0) {
                errors.push("machine_step_down percent_of_rated must be zero or positive".into());
            }
            if !(step.synchronous_percent().is_finite() && step.synchronous_percent() >= 0.0) {
                errors.push("machine_step_down synchronous_motor_percent_of_rated must be zero or positive".into());
            }
            // Under the ANSI fault method the step-down is not applied, so the
            // cycle-order check is skipped (matching the reference options dialog).
            if self.fault_method != FaultMethod::Ansi
                && !(step.after_cycles.is_finite() && step.after_cycles / 60.0 > self.motor_contribution_s)
            {
                errors.push(format!("machine_step_down after_cycles must be greater than the induction-motor window of {:.2} cycles", self.motor_contribution_s * 60.0));
            }
        }
        if let Some(backup) = &self.upstream_backup {
            if !(1..=5).contains(&backup.levels) {
                errors.push("upstream_backup levels must be 1 to 5".into());
            }
            if !(backup.ratio.is_finite() && backup.ratio > 0.0 && backup.ratio <= 1.0) {
                errors.push("upstream_backup ratio must be above 0 and at most 1".into());
            }
        }
        if !(self.prefault_pu.is_finite() && self.prefault_pu > 0.0) {
            errors.push("prefault_pu must be positive".into());
        }
        if self.prefault == Prefault::PerBus {
            for bus in &self.buses {
                if !bus.prefault_pu.is_some_and(|v| v.is_finite() && v > 0.0) {
                    errors.push(format!("bus '{}' needs a positive prefault_pu because the study prefault is per_bus", bus.id));
                }
            }
        }
        if !(self.motor_contribution_s.is_finite() && self.motor_contribution_s > 0.0) {
            errors.push("motor_contribution_s must be positive".into());
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
        self.validate_grounding(&mut errors);
        self.validate_physical(&mut errors);
        errors
    }

    fn validate_grounding(&self, errors: &mut Vec<String>) {
        let mut entered = std::collections::HashSet::new();
        for g in &self.neutral_grounding {
            let label = match g.winding {
                Some(Winding::Hv) => format!("neutral grounding {} hv", g.element_id),
                Some(Winding::Lv) => format!("neutral grounding {} lv", g.element_id),
                None => format!("neutral grounding {}", g.element_id),
            };
            if !entered.insert((g.element_id.as_str(), g.winding)) {
                errors.push(format!("{label} is entered more than once"));
            }
            if !(g.r_ohm.is_finite() && g.r_ohm >= 0.0 && g.x_ohm.is_finite() && g.x_ohm >= 0.0) {
                errors.push(format!("{label}: r_ohm and x_ohm must be finite and nonnegative"));
            }
            let has_impedance = g.r_ohm > 0.0 || g.x_ohm > 0.0;
            match g.method {
                GroundingMethod::Impedance if !has_impedance => errors.push(format!("{label}: impedance grounding needs a positive r_ohm or x_ohm")),
                GroundingMethod::Solid | GroundingMethod::Ungrounded if has_impedance => errors.push(format!("{label}: r_ohm/x_ohm apply only to impedance grounding")),
                _ => {}
            }
            if self.sources.iter().any(|s| s.id == g.element_id) {
                if g.winding.is_some() { errors.push(format!("{label}: a source takes no winding")); }
            } else if let Some(branch) = self.branches.iter().find(|b| b.id == g.element_id && matches!(b.kind, BranchKind::Transformer { .. })) {
                let BranchKind::Transformer { connection, .. } = branch.kind else { unreachable!() };
                match (g.winding, self.transformer_hv_is_from(branch)) {
                    (None, _) => errors.push(format!("{label}: a transformer entry needs winding hv or lv")),
                    (Some(winding), Some(hv_is_from)) => {
                        let on_from = (winding == Winding::Hv) == hv_is_from;
                        let wye = match connection {
                            XfmrConn::Dyn => !on_from,
                            XfmrConn::Ynd => on_from,
                            XfmrConn::Ynyn => true,
                            XfmrConn::Dd => false,
                        };
                        if !wye { errors.push(format!("{label}: that winding is delta and has no neutral")); }
                    }
                    (Some(_), None) => {} // unknown terminal bus is reported for the branch
                }
            } else {
                errors.push(format!("{label} references unknown source or transformer '{}'", g.element_id));
            }
        }
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
        check(positive(self.s_base_mva) && positive(self.arc_duration_cap_s) && positive(self.motor_contribution_s), "project", "base, duration cap, or motor contribution");
        for b in &self.buses {
            check(positive(b.kv) && b.shunt_kvar.is_finite(), &b.id, "bus voltage or shunt");
            check(b.bracing_ka.map_or(true, positive) && b.main_rating_a.map_or(true, positive), &b.id, "equipment rating");
            check(b.bracing_test_xr.map_or(true, positive), &b.id, "bracing test X/R");
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
            // An ungrounded or impedance-grounded neutral has no zero (or a
            // nonzero) zero-sequence shunt even when the source Z0 is zero.
            let zero_sequence_defined = matches!(self.grounding_of(&s.id, None).map(|g| g.method),
                Some(GroundingMethod::Ungrounded | GroundingMethod::Impedance))
                || s.r0_over_r1 > 0.0 || s.xr * s.x0_over_x1 > 0.0;
            check(positive(s.mva_sc) && positive(s.v_pu) && nonnegative(s.xr)
                && s.angle_deg.is_finite() && s.p_mw.is_finite()
                && nonnegative(s.r0_over_r1) && nonnegative(s.x0_over_x1)
                && zero_sequence_defined, &s.id, "source impedance or voltage");
            check(s.qmin_mvar.map_or(true, f64::is_finite) && s.qmax_mvar.map_or(true, f64::is_finite)
                && s.qmin_mvar.unwrap_or(f64::NEG_INFINITY) <= s.qmax_mvar.unwrap_or(f64::INFINITY), &s.id, "reactive limits");
            check(s.transient_mva_sc.map_or(true, positive), &s.id, "transient_mva_sc");
            if let Some(g) = &s.generator {
                let unit_ok = g.unit.as_ref().map_or(true, |u| {
                    self.branches.iter().any(|b| b.id == u.transformer && matches!(b.kind, BranchKind::Transformer { .. }) && (b.from == s.bus || b.to == s.bus))
                        && u.p_g.map_or(true, |p| p.is_finite() && p > -1.0) && u.p_t.map_or(true, |p| p.is_finite() && p > -1.0)
                });
                check(positive(g.rated_mva) && g.rated_pf.is_finite() && (0.0..=1.0).contains(&g.rated_pf) && g.rated_kv.map_or(true, positive)
                    && s.machine_kind != Some(GeneratorKind::Utility) && unit_ok,
                    &s.id, "generator rating (rated_mva, rated_pf 0-1, rated_kv, not a utility, unit transformer on the source bus)");
            }
        }
        for l in &self.loads { check(l.kw.is_finite() && l.kvar.is_finite(), &l.id, "load"); }
        for m in &self.motors {
            check([m.hp, m.kv, m.pf, m.efficiency, m.x_subtransient_pu, m.xr].into_iter().all(positive)
                && m.pf <= 1.0 && m.efficiency <= 1.0, &m.id, "motor ratings, reactance, or X/R");
            check(m.synchronous_rpm.map_or(true, positive) && m.phases.map_or(true, |n| n == 1 || n == 3), &m.id, "motor synchronous_rpm or phases (1 or 3)");
            check(m.decay_time_constant_s.map_or(true, positive), &m.id, "motor decay_time_constant_s");
            // Allow customary nameplate/nominal differences (e.g. 460 V on 480 V).
            check(self.bus(&m.bus).map_or(false, |b| (m.kv / b.kv - 1.0).abs() <= 0.10), &m.id, "motor voltage (must be within 10% of bus voltage)");
        }
        for d in &self.devices {
            check(d.breaker_interrupting_s.map_or(true, positive) && !(d.curve_includes_clearing() && d.breaker_interrupting_s.is_some()), &d.id, "clearing-time definition");
            if let Some(rating) = &d.interrupting {
                check(positive(rating.symmetrical_ka) && positive(rating.test_xr)
                    && rating.voltage_v.is_finite() && rating.voltage_v >= 0.0, &d.id, "interrupting rating");
                check(rating.voltage_range_factor.map_or(true, |k| k.is_finite() && k >= 1.0), &d.id, "voltage range factor");
            }
            if let Some(limit) = &d.current_limiting {
                let clearing_ok = if limit.cycle_table {
                    limit.clearing_s == 0.0 || positive(limit.clearing_s)
                } else {
                    positive(limit.clearing_s)
                };
                check(positive(limit.threshold_a) && clearing_ok
                    && limit.effective_arc_ka.map_or(true, positive), &d.id, "current-limiting let-through");
            }
            if let Some(id) = &d.protected_branch {
                check(self.branches.iter().any(|b| b.id == *id && (b.from == d.bus || b.to == d.bus)), &d.id, "protected branch terminal");
            }
            let pair = |p: Option<f64>, t: Option<f64>| p.map_or(true, positive) && t.map_or(true, positive) && (t.is_none() || p.is_some());
            let ok = match d.curve {
                CurveSpec::Iec { pickup_a, tms, inst_a, inst_s, .. } => positive(pickup_a) && positive(tms) && pair(inst_a, inst_s),
                CurveSpec::Ieee { pickup_a, td, inst_a, inst_s, .. } => positive(pickup_a) && positive(td) && pair(inst_a, inst_s),
                CurveSpec::Definite { pickup_a, time_s } => positive(pickup_a) && positive(time_s),
                CurveSpec::ThermalMagnetic { lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s } => positive(lt_pickup_a) && positive(lt_delay_s) && pair(st_pickup_a, st_delay_s) && pair(inst_a, inst_s),
                CurveSpec::Points { .. } => true,
                CurveSpec::SettingsNotCollected { .. } => true,
            };
            check(ok, &d.id, "pickup or delay");
            if let CurveSpec::Points { points, max_points, .. } = &d.curve {
                if let Err(problem) = crate::curves::check_point_curve(points, max_points.as_deref()) {
                    check(false, &d.id, &format!("point curve ({problem})"));
                }
            }
            // Pickups of successive elements must not decrease: an instantaneous
            // below the timed pickup, or a short-time below long-time, inverts
            // the curve. Equal pickups are real settings (e.g. instantaneous at
            // the short-time pickup) and are accepted.
            let ordered = match d.curve {
                CurveSpec::Iec { pickup_a, inst_a, .. } | CurveSpec::Ieee { pickup_a, inst_a, .. } =>
                    inst_a.map_or(true, |inst| inst >= pickup_a),
                CurveSpec::ThermalMagnetic { lt_pickup_a, st_pickup_a, inst_a, .. } =>
                    [Some(lt_pickup_a), st_pickup_a, inst_a].into_iter().flatten()
                        .collect::<Vec<_>>().windows(2).all(|pair| pair[0] <= pair[1]),
                _ => true,
            };
            check(ordered, &d.id, "pickup order (requires long-time <= short-time <= instantaneous, and instantaneous at or above the timed pickup)");
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

    #[test]
    fn cycle_table_defaults_off_and_allows_a_zero_manufacturer_time() {
        let parsed: CurrentLimiting = serde_json::from_str(r#"{"threshold_a":1000,"clearing_s":0.02}"#).unwrap();
        assert!(!parsed.cycle_table);
        let mut project = Project::sample();
        project.devices[0].current_limiting = Some(CurrentLimiting {
            threshold_a: 1000.0, clearing_s: 0.0, effective_arc_ka: None, cycle_table: false,
        });
        assert!(project.validate().iter().any(|error| error.contains("current-limiting let-through")));
        project.devices[0].current_limiting = Some(CurrentLimiting {
            threshold_a: 1000.0, clearing_s: 0.0, effective_arc_ka: None, cycle_table: true,
        });
        assert!(project.validate().is_empty(), "{:?}", project.validate());
        let text = serde_json::to_string(&project.devices[0].current_limiting).unwrap();
        assert!(text.contains("cycle_table"), "{text}");
        project.devices[0].current_limiting.as_mut().unwrap().clearing_s = -0.01;
        assert!(project.validate().iter().any(|error| error.contains("current-limiting let-through")));
    }
}

/// IEEE 1584-2018 equipment class (gap, enclosure, working distance, electrode).
/// Tables 9 and 10. Each class uses one electrode. Open air is used only when
/// the class is selected or the bus name says open air.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentClass {
    LvSwitchgear,
    LvMccPanelShallow,
    LvMccPanelDeep,
    LvCableJunctionShallow,
    LvCableJunctionDeep,
    MvMcc5kv,
    /// 914.4 mm cube. The smaller published 5 kV switchgear size.
    MvSwitchgear5kvSmall,
    /// 1143 × 762 × 762 mm.
    MvSwitchgear5kvLarge,
    MvMcc15kv,
    MvSwitchgear15kv,
    OpenAirVertical,
    OpenAirHorizontal,
}

/// Geometry and electrode for one equipment class at a bus voltage.
#[derive(Clone, Copy, Debug)]
pub struct EquipmentSpec {
    pub class: EquipmentClass,
    pub label: &'static str,
    pub electrode: Electrode,
    pub gap_mm: f64,
    pub distance_mm: f64,
    pub height_mm: f64,
    pub width_mm: f64,
    pub depth_mm: f64,
    /// Open air has no typical gap in the enclosed-equipment table.
    pub gap_not_collected: bool,
}

impl EquipmentClass {
    pub fn spec(self, kv: f64) -> EquipmentSpec {
        let (label, electrode, gap, distance, height, width, depth, gap_not_collected) = match self {
            Self::LvSwitchgear => ("low-voltage switchgear", Electrode::VCB, 32.0, 609.6, 508.0, 508.0, 508.0, false),
            // Depth is the shallow limit so the enclosure-size correction uses the shallow factor.
            Self::LvMccPanelShallow => ("shallow low-voltage MCC or panelboard", Electrode::VCB, 25.0, 457.2, 355.6, 304.8, 203.2, false),
            Self::LvMccPanelDeep => ("deep low-voltage MCC or panelboard", Electrode::VCB, 25.0, 457.2, 355.6, 304.8, 250.0, false),
            Self::LvCableJunctionShallow => ("shallow low-voltage cable junction", Electrode::VCB, 13.0, 457.2, 355.6, 304.8, 203.2, false),
            Self::LvCableJunctionDeep => ("deep low-voltage cable junction", Electrode::VCB, 13.0, 457.2, 355.6, 304.8, 250.0, false),
            Self::MvMcc5kv => ("5 kV MCC", Electrode::VCB, 104.0, 914.4, 660.4, 660.4, 660.4, false),
            Self::MvSwitchgear5kvSmall => ("5 kV switchgear, 914 mm enclosure", Electrode::VCB, 104.0, 914.4, 914.4, 914.4, 914.4, false),
            Self::MvSwitchgear5kvLarge => ("5 kV switchgear, 1143 mm enclosure", Electrode::VCB, 104.0, 914.4, 1143.0, 762.0, 762.0, false),
            Self::MvMcc15kv => ("15 kV MCC", Electrode::VCB, 152.0, 914.4, 914.4, 914.4, 914.4, false),
            Self::MvSwitchgear15kv => ("15 kV switchgear", Electrode::VCB, 152.0, 914.4, 1143.0, 762.0, 762.0, false),
            Self::OpenAirVertical => ("open air, vertical electrodes", Electrode::VOA, open_air_gap_mm(kv), 914.4, 1000.0, 1000.0, 1000.0, true),
            Self::OpenAirHorizontal => ("open air, horizontal electrodes", Electrode::HOA, open_air_gap_mm(kv), 914.4, 1000.0, 1000.0, 1000.0, true),
        };
        EquipmentSpec { class: self, label, electrode, gap_mm: gap, distance_mm: distance, height_mm: height, width_mm: width, depth_mm: depth, gap_not_collected }
    }
}

fn open_air_gap_mm(kv: f64) -> f64 {
    if kv <= 0.6 { 25.0 } else if kv <= 5.0 { 104.0 } else { 152.0 }
}

/// Enclosed classes at a bus voltage. Open air is not included.
pub fn enclosed_classes(kv: f64) -> Vec<EquipmentClass> {
    if kv <= 0.6 {
        vec![
            EquipmentClass::LvSwitchgear,
            EquipmentClass::LvMccPanelShallow,
            EquipmentClass::LvMccPanelDeep,
            EquipmentClass::LvCableJunctionShallow,
            EquipmentClass::LvCableJunctionDeep,
        ]
    } else if kv <= 5.0 {
        vec![EquipmentClass::MvMcc5kv, EquipmentClass::MvSwitchgear5kvSmall, EquipmentClass::MvSwitchgear5kvLarge]
    } else {
        vec![EquipmentClass::MvMcc15kv, EquipmentClass::MvSwitchgear15kv]
    }
}

/// Classes to evaluate for a bus that has no arc-equipment record.
/// A selected class is used as entered. Otherwise the bus name selects the
/// class. An unrecognized name evaluates every enclosed class at that voltage.
pub fn classes_for_assumed_bus(bus: &Bus) -> Vec<EquipmentClass> {
    if let Some(class) = bus.equipment_class {
        return vec![class];
    }
    inferred_classes(&bus.name, bus.kv).unwrap_or_else(|| enclosed_classes(bus.kv))
}

fn inferred_classes(name: &str, kv: f64) -> Option<Vec<EquipmentClass>> {
    let name = name.to_ascii_lowercase();
    if name.contains("open air") || name.contains("open-air") || name.contains("outdoor") {
        return Some(vec![EquipmentClass::OpenAirVertical, EquipmentClass::OpenAirHorizontal]);
    }
    let lv = kv <= 0.6;
    let mid = kv <= 5.0;
    if name.contains("cable") || name.contains("junction") {
        return lv.then(|| vec![EquipmentClass::LvCableJunctionShallow, EquipmentClass::LvCableJunctionDeep]);
    }
    if name.contains("mcc") {
        return Some(if lv {
            vec![EquipmentClass::LvMccPanelShallow, EquipmentClass::LvMccPanelDeep]
        } else if mid {
            vec![EquipmentClass::MvMcc5kv]
        } else {
            vec![EquipmentClass::MvMcc15kv]
        });
    }
    if name.contains("panel") || name.contains("pnl") {
        return lv.then(|| vec![EquipmentClass::LvMccPanelShallow, EquipmentClass::LvMccPanelDeep]);
    }
    if name.contains("switchgear") || name.contains("switchboard") || name.contains("swg") || name.contains("mdp") {
        return Some(if lv {
            vec![EquipmentClass::LvSwitchgear]
        } else if mid {
            vec![EquipmentClass::MvSwitchgear5kvSmall, EquipmentClass::MvSwitchgear5kvLarge]
        } else {
            vec![EquipmentClass::MvSwitchgear15kv]
        });
    }
    None
}

/// One representative geometry for exports. A study of a bus with no equipment
/// record evaluates `classes_for_assumed_bus` and keeps the governing result.
/// This single row is the smaller enclosed size when the name does not pick one class.
pub fn typical_equipment(kv: f64) -> (Electrode, f64, f64, f64, f64, f64, &'static str) {
    let class = if kv <= 0.6 {
        EquipmentClass::LvMccPanelShallow
    } else if kv <= 5.0 {
        EquipmentClass::MvSwitchgear5kvSmall
    } else {
        EquipmentClass::MvSwitchgear15kv
    };
    let spec = class.spec(kv);
    (spec.electrode, spec.gap_mm, spec.distance_mm, spec.height_mm, spec.width_mm, spec.depth_mm, spec.label)
}
