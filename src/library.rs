//! Manufacturer protective-device library in Flashmob's own JSON format.
//!
//! A library is reference data, separate from a study project. It lists
//! devices (manufacturer, type, style, ratings) and the time-current
//! characteristic data that came with them. Nothing here is applied to a
//! project automatically: a project `Device` still needs its own entered
//! curve and settings. `flashmob import-library` builds a library from a
//! binary manufacturer device library file (see `library_import`).
//!
//! Field meanings that could not be established from the source are kept
//! as unlabeled numbers (`unlabeled`, `arrays`, `fixed_hex`) rather than
//! guessed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "flashmob-device-library";
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub format: String,
    pub version: u32,
    pub source: LibrarySource,
    pub summary: LibrarySummary,
    pub devices: Vec<LibraryDevice>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibrarySource {
    /// Generic description of the input, for example "binary manufacturer device library".
    pub description: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibrarySummary {
    pub devices: usize,
    pub devices_by_category: BTreeMap<String, usize>,
    pub functions_by_kind: BTreeMap<String, usize>,
    /// Records present in the source but not converted, by record class code.
    pub skipped_records_by_class: BTreeMap<String, usize>,
    /// Devices left out because an identifying field (manufacturer, category,
    /// type, style, catalog, reference, or rating label) contained a redacted term.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withheld_device_ids: Vec<u32>,
    /// Number of free-text values (notes, element names, labels) in which a
    /// redacted term was replaced by `[redacted]`.
    #[serde(default)]
    pub redacted_text_values: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryDevice {
    /// Source object number; unique within one converted library.
    pub id: u32,
    pub category: String,
    pub manufacturer: String,
    /// Device type text, typically the ampere range and pole/feature summary.
    #[serde(rename = "type")]
    pub device_type: String,
    pub style: String,
    /// Catalog or part-number pattern as entered in the source.
    pub catalog: String,
    /// Reference document (usually the manufacturer TCC or datasheet number).
    pub reference: String,
    pub notes: String,
    /// Creation and modification stamps exactly as stored (text).
    pub created: String,
    pub modified: String,
    /// Device-level voltage stored in the device header. In this source it
    /// equals the highest rating voltage for about 97% of devices.
    pub header_voltage_v: Option<f64>,
    pub ratings: Vec<Rating>,
    pub frames: Vec<Frame>,
    pub trip_units: Vec<TripUnit>,
    /// Child records attached to the device whose meaning is not established.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_records: Vec<RawRecord>,
}

/// One voltage / ampere rating line with its interrupting rating.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rating {
    pub label: String,
    pub voltage_v: f64,
    pub rating_a: f64,
    /// Symmetrical interrupting rating, kA.
    pub interrupting_ka: f64,
    /// Asymmetrical interrupting value stored with the rating, kA. It matches
    /// interrupting_ka × sqrt(1 + 2·exp(−2π/(X/R))) at test_xr in checked records.
    pub interrupting_asym_ka: f64,
    /// Test X/R ratio for the interrupting rating.
    pub test_xr: f64,
    /// Remaining stored numbers in source order (positions 3, 4, 6, 7, 8, 10, 11), meaning not established.
    pub unlabeled: Vec<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub rating_a: f64,
    /// Sensor / plug / trip ratings available in this frame, amperes.
    pub sensors_a: Vec<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TripUnit {
    /// The frame or size rating this characteristic set belongs to, amperes.
    pub rating_a: f64,
    pub functions: Vec<Function>,
    /// Numeric arrays stored with the trip unit, meaning not established.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrays: Vec<Vec<f64>>,
}

/// One element of the characteristic: a pickup list, delay band, curve, etc.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub name: String,
    /// Element kind code stored in the source (0–20).
    pub kind_code: u32,
    /// Descriptive family for the kind code, inferred from names and data in the source.
    pub kind: String,
    /// Second flag stored with the element (0 or 1). Meaning not established.
    pub flag: u32,
    pub data: FunctionData,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionData {
    pub class_code: u16,
    /// The two leading integer codes of the element record. The second one is
    /// 10 when point-curve currents are amperes; other values accompany
    /// multiples of an unspecified base.
    pub header_codes: [u32; 2],
    /// Selectable settings: label text and stored numeric value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
    /// Time-current point curves (one or more bands).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<PointCurve>,
    /// "A" for amperes, "multiple" for multiples of an unspecified base. Point curves only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_unit: Option<String>,
    /// Plus and minus current tolerance in percent stored with point curves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tolerance_pct: Option<[f64; 2]>,
    /// Curve families selected by a dial: current multiples shared by every dial curve.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dial_current_multiples: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dial_curves: Vec<DialCurve>,
    /// Text stored in the element other than setting labels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strings: Vec<String>,
    /// Numeric arrays not otherwise interpreted, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrays: Vec<Vec<f64>>,
    /// Variable parts whose layout is not decoded, as hex.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unparsed_hex: Vec<String>,
    /// Fixed-size part of the record as hex, kept so nothing is lost.
    pub fixed_hex: String,
    /// Set when the element record was missing or unreadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointCurve {
    pub current: Vec<f64>,
    pub time_s: Vec<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialCurve {
    pub dial: f64,
    pub time_s: Vec<f64>,
}

/// A record kept without interpretation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRecord {
    pub class_code: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrays: Vec<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unparsed_hex: Vec<String>,
    pub fixed_hex: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<RawRecord>,
}

impl Library {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let library: Library = serde_json::from_str(text).map_err(|err| err.to_string())?;
        let errors = library.validate();
        if errors.is_empty() { Ok(library) } else { Err(errors.join("; ")) }
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.format != FORMAT { errors.push(format!("format is {:?}, expected {FORMAT:?}", self.format)); }
        if self.version != VERSION { errors.push(format!("version {} is not supported (expected {VERSION})", self.version)); }
        let mut ids = std::collections::HashSet::new();
        for device in &self.devices {
            if !ids.insert(device.id) { errors.push(format!("duplicate device id {}", device.id)); }
            for unit in &device.trip_units {
                for function in &unit.functions {
                    for curve in &function.data.curves {
                        if curve.current.len() != curve.time_s.len() {
                            errors.push(format!("device {} function {:?}: curve has {} currents and {} times", device.id, function.name, curve.current.len(), curve.time_s.len()));
                        }
                    }
                }
            }
        }
        errors
    }

    /// Devices whose manufacturer, category, type, style, or catalog contains
    /// every given term (case-insensitive).
    pub fn search<'a>(&'a self, terms: &[String]) -> Vec<&'a LibraryDevice> {
        let terms: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
        self.devices.iter().filter(|d| {
            let hay = format!("{} {} {} {} {} {}", d.manufacturer, d.category, d.device_type, d.style, d.catalog, d.reference).to_lowercase();
            terms.iter().all(|t| hay.contains(t))
        }).collect()
    }
}
