//! Manufacturer protective-device library in Flashmob's own JSON format.
//!
//! A library is reference data, separate from a study project. It lists
//! devices (manufacturer, type, style, ratings) and the time-current
//! characteristic data that came with them. `apply_device` copies one point
//! curve into a project `Device` (a snapshot). Studies use that curve, not
//! the library file. `flashmob import-library` builds a library from a
//! binary manufacturer device library file (see `library_import`).
//!
//! Field meanings that could not be established from the source are kept
//! as unlabeled numbers (`unlabeled`, `arrays`, `fixed_hex`) rather than
//! guessed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "flashmob-device-library";
pub const VERSION: u32 = 1;

pub use crate::library_apply::{apply_device, ApplyDevice};

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

/// Directory for the converted manufacturer library on this machine.
/// macOS uses `~/Library/Application Support/flashmob`. Other systems use
/// `~/.local/share/flashmob`. The file is not part of a study project.
pub fn installed_library_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        Some(home.join("Library/Application Support/flashmob"))
    } else {
        Some(home.join(".local/share/flashmob"))
    }
}

/// `device-library.json` inside [`installed_library_dir`]. The file may be absent.
pub fn installed_library_file() -> Option<PathBuf> {
    installed_library_dir().map(|dir| dir.join("device-library.json"))
}

/// First existing library file. `FLASHMOB_LIBRARY` wins when it is set.
/// Then the installed file, then `./device-library.json`.
pub fn installed_library_path() -> Option<PathBuf> {
    // A set FLASHMOB_LIBRARY is the only candidate. A missing file does not
    // fall through to the installed path.
    if let Some(env) = std::env::var_os("FLASHMOB_LIBRARY") {
        if !env.is_empty() {
            let path = PathBuf::from(env);
            return path.is_file().then_some(path);
        }
    }
    let mut paths = Vec::new();
    if let Some(path) = installed_library_file() {
        paths.push(path);
    }
    paths.push(PathBuf::from("device-library.json"));
    paths.into_iter().find(|path| path.is_file())
}

/// The installed library, or an error that names where it was expected.
pub fn require_installed_library() -> Result<PathBuf, String> {
    if let Some(env) = std::env::var_os("FLASHMOB_LIBRARY") {
        if !env.is_empty() {
            let path = PathBuf::from(&env);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!("could not read {}: not a file", path.display()));
        }
    }
    if let Some(path) = installed_library_path() {
        return Ok(path);
    }
    match installed_library_file() {
        Some(path) => Err(format!(
            "no installed device library at {}. Pass a library JSON path, or set FLASHMOB_LIBRARY.",
            path.display()
        )),
        None => Err("no installed device library. Pass a library JSON path, or set FLASHMOB_LIBRARY.".into()),
    }
}

/// Name stored on a device when the curve came from the library built into the program.
pub const BUNDLED_LIBRARY_NAME: &str = "library/device-library.json";

/// Bytes of the device library compiled into this build.
pub fn bundled_library_bytes() -> Option<&'static [u8]> {
    #[cfg(feature = "bundle-library")]
    {
        Some(include_bytes!("../library/device-library.json"))
    }
    #[cfg(not(feature = "bundle-library"))]
    {
        None
    }
}

pub fn library_from_bytes(bytes: &[u8]) -> Result<Library, String> {
    let text = std::str::from_utf8(bytes).map_err(|err| format!("device library is not utf-8: {err}"))?;
    Library::from_json(text)
}

pub fn library_from_path(path: &Path) -> Result<Library, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("could not read {}: {err}", path.display()))?;
    Library::from_json(&text)
}

/// Display name of the library `open_default_library` would open.
pub fn default_library_name() -> String {
    if let Some(env) = std::env::var_os("FLASHMOB_LIBRARY") {
        if !env.is_empty() {
            return PathBuf::from(env).display().to_string();
        }
    }
    if bundled_library_bytes().is_some() {
        return BUNDLED_LIBRARY_NAME.to_string();
    }
    installed_library_path().map(|path| path.display().to_string()).unwrap_or_default()
}

/// `FLASHMOB_LIBRARY` when it is set, otherwise the library compiled into the
/// program, otherwise a device-library file already on disk.
pub fn open_default_library() -> Result<(Library, String), String> {
    if let Some(env) = std::env::var_os("FLASHMOB_LIBRARY") {
        if !env.is_empty() {
            let path = PathBuf::from(&env);
            if !path.is_file() {
                return Err(format!("could not read {}: not a file", path.display()));
            }
            let name = path.display().to_string();
            return Ok((library_from_path(&path)?, name));
        }
    }
    if let Some(bytes) = bundled_library_bytes() {
        return Ok((library_from_bytes(bytes)?, BUNDLED_LIBRARY_NAME.to_string()));
    }
    if let Some(path) = installed_library_path() {
        let name = path.display().to_string();
        return Ok((library_from_path(&path)?, name));
    }
    Err("no device library. Pass a library JSON path, or set FLASHMOB_LIBRARY.".into())
}

/// `explicit` when it is a file. Otherwise the installed library.
pub fn require_library_file(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    match explicit {
        Some(path) if path.is_file() => Ok(path),
        Some(path) => Err(format!("could not read {}: not a file", path.display())),
        None => require_installed_library(),
    }
}

/// Search arguments. An existing file in the first argument is the library.
/// A missing `.json` path or a path that contains a separator is an error.
/// Any other arguments are search terms against the installed library.
pub fn split_library_search(args: &[String], installed: Option<&Path>) -> Result<(PathBuf, Vec<String>), String> {
    if let Some(first) = args.first() {
        let path = PathBuf::from(first);
        let looks_like_path = first.ends_with(".json") || first.contains('/') || first.contains('\\');
        if path.is_file() {
            return Ok((path, args.iter().skip(1).cloned().collect()));
        }
        if looks_like_path {
            return Err(format!("could not read {first}: not a file"));
        }
    }
    match installed {
        Some(path) if path.is_file() => Ok((path.to_path_buf(), args.to_vec())),
        Some(path) => Err(format!(
            "no installed device library at {}. Pass a library JSON path, or set FLASHMOB_LIBRARY.",
            path.display()
        )),
        None => Err("no installed device library. Pass a library JSON path, or set FLASHMOB_LIBRARY.".into()),
    }
}

#[cfg(test)]
mod installed_path_tests {
    use super::*;

    #[test]
    fn search_uses_an_explicit_file_and_otherwise_the_installed_library() {
        let dir = std::env::temp_dir().join(format!("flashmob-lib-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let present = dir.join("device-library.json");
        std::fs::write(&present, b"{}").unwrap();
        let missing = dir.join("missing.json");

        let (path, terms) = split_library_search(&[present.display().to_string(), "lpj".into()], None).unwrap();
        assert_eq!(path, present);
        assert_eq!(terms, vec!["lpj".to_string()]);

        let (path, terms) = split_library_search(&["lpj".into()], Some(present.as_path())).unwrap();
        assert_eq!(path, present);
        assert_eq!(terms, vec!["lpj".to_string()]);

        let err = split_library_search(&[missing.display().to_string()], Some(present.as_path())).unwrap_err();
        assert!(err.contains("not a file"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
