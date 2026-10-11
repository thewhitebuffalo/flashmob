//! Point (time-current pair) curves through the studies: TCC, coordination,
//! arc flash with constant current and with a source decrement, SLD, package
//! export, and the exec `set_device` op.

use flashmob::curves;
use flashmob::exec;
use flashmob::model::*;
use flashmob::study::{self, ArcRow, StudyOutput, Studies};
use flashmob::{sld, tcc};

const MIN: [[f64; 2]; 3] = [[3000.0, 500.0], [10000.0, 0.5], [60000.0, 0.02]];
const MAX: [[f64; 2]; 3] = [[3500.0, 1000.0], [10000.0, 1.0], [60000.0, 0.04]];
const SHORT: [[f64; 2]; 3] = [[3000.0, 100.0], [10000.0, 0.5], [20000.0, 0.1]];

fn points(min: &[[f64; 2]], max: Option<&[[f64; 2]]>, tail: PointTail, interpretation: BandInterpretation) -> CurveSpec {
    CurveSpec::Points { points: min.to_vec(), max_points: max.map(<[_]>::to_vec), beyond_last: tail, band_interpretation: interpretation }
}

fn one_band(band: &[[f64; 2]], tail: PointTail) -> CurveSpec {
    points(band, None, tail, BandInterpretation::Entered)
}

fn two_band(tail: PointTail) -> CurveSpec {
    points(&MIN, Some(&MAX), tail, BandInterpretation::Entered)
}

/// The sample with its MCC main breaker carrying `curve`; one arc location (af-mcc).
fn project(curve: CurveSpec) -> Project {
    let mut p = Project::sample();
    p.motors.clear();
    p.equipment.truncate(1);
    p.devices[0].curve = curve;
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    p
}

fn run(p: &Project) -> StudyOutput {
    study::run(p, Studies { loadflow: true, fault: true, arcflash: true, coordination: true }).unwrap()
}

fn run_full(p: &Project) -> StudyOutput { run(p) }

fn arc(out: &StudyOutput) -> &ArcRow {
    out.arc_flash.as_ref().unwrap().iter().find(|row| row.equipment_id == "af-mcc").unwrap()
}

fn close(got: f64, expected: f64) {
    assert!((got - expected).abs() <= 1e-9 * expected.abs().max(1.0), "got {got}, expected {expected}");
}

#[test]
fn arc_flash_uses_the_slower_max_band() {
    let both = run(&project(two_band(PointTail::Unavailable)));
    let max_only = run(&project(one_band(&MAX, PointTail::Unavailable)));
    let min_only = run(&project(one_band(&MIN, PointTail::Unavailable)));
    let row = arc(&both);
    close(row.time_s, arc(&max_only).time_s);
    close(row.time_min_s, arc(&max_only).time_min_s);
    assert!(row.time_s > arc(&min_only).time_s + 1e-6);
    assert!(row.governing_cal_cm2 > arc(&min_only).governing_cal_cm2);
    assert!(!row.assumed);
    // The device carries the full arcing current here: max-band time plus breaker interruption.
    let amps = row.arcing_ka * 1000.0;
    close(row.time_s, curves::trip_time(&one_band(&MAX, PointTail::Unavailable), amps).unwrap() + 0.08);
}

#[test]
fn assumed_band_interpretation_marks_arc_flash_assumed() {
    let out = run(&project(points(&MIN, Some(&MAX), PointTail::Unavailable, BandInterpretation::Assumed)));
    let row = arc(&out);
    assert!(row.assumed);
    assert!(row.duration_note.contains("band interpretation is assumed"), "{}", row.duration_note);
}

#[test]
fn current_above_curve_holds_last_time_as_assumption() {
    let out = run(&project(one_band(&SHORT, PointTail::HoldLastTime)));
    let row = arc(&out);
    assert!(row.arcing_ka > 20.0);
    close(row.time_s, 0.1 + 0.08);
    assert!(row.assumed);
    assert!(row.duration_note.contains("above the point curve's last point (20000 A)") && row.duration_note.contains("held"),
        "{}", row.duration_note);
}

#[test]
fn current_above_curve_without_tail_leaves_clearing_unresolved() {
    let out = run(&project(one_band(&SHORT, PointTail::Unavailable)));
    assert!(!out.arc_flash.as_ref().unwrap().iter().any(|row| row.equipment_id == "af-mcc"));
    let failure = out.arc_flash_failures.iter().find(|f| f.equipment_id == "af-mcc").unwrap();
    assert!(failure.error.contains("no operating time is available"), "{}", failure.error);
}

#[test]
fn coordination_reports_both_bands() {
    let out = run(&project(two_band(PointTail::Unavailable)));
    let rows = out.coordination.as_ref().unwrap();
    let row = rows.iter().find(|r| r.device_id == "brk-main").unwrap();
    let amps = row.bolted_ka.unwrap() * 1000.0;
    let curve = two_band(PointTail::Unavailable);
    close(row.trip_at_bolted_s.unwrap(), curves::trip_time(&curve, amps).unwrap());
    close(row.min_band_trip_s.unwrap(), curves::trip_time_fast_band(&curve, amps).unwrap());
    assert!(row.min_band_trip_s.unwrap() < row.trip_at_bolted_s.unwrap());
    close(row.total_clearing_s.unwrap(), row.trip_at_bolted_s.unwrap() + 0.08);
    assert!(row.curve.starts_with("Point curve"));
    assert!(row.note.contains("slower (max) band"), "{}", row.note);
    // Other devices do not gain the field.
    let other = rows.iter().find(|r| r.device_id == "brk-feeder").unwrap();
    assert_eq!(other.min_band_trip_s, None);
    assert!(!serde_json::to_string(other).unwrap().contains("min_band_trip_s"));
}

#[test]
fn tcc_draws_both_bands_from_stored_points_and_reports_tails() {
    let p = project(two_band(PointTail::HoldLastTime));
    let out = run(&p);
    let plot = tcc::plot(&p, Some(&out), Some(13.8), &[]).unwrap();
    let main = &p.devices[0].name;
    let kv = p.bus(&p.devices[0].bus).unwrap().kv;
    for (suffix, band) in [(" (min)", &MIN), (" (max)", &MAX)] {
        let curve = plot.curves.iter().find(|c| c.name == format!("{main}{suffix}")).unwrap();
        assert_eq!(curve.points.len(), band.len());
        for (got, [amps, time]) in curve.points.iter().zip(band.iter()) {
            close(got.0, amps * kv / 13.8);
            close(got.1, *time);
        }
    }
    assert!(plot.note.contains("last stored time is held (assumed)"), "{}", plot.note);
    assert!(tcc::to_csv(&plot).contains(&format!("{main} (max)")));
    assert!(tcc::to_svg(&plot).contains(&format!("{main} (min)")));
}

#[test]
fn decrement_path_integrates_max_band_and_reports_assumptions() {
    let mut p = project(points(&MIN, Some(&MAX), PointTail::HoldLastTime, BandInterpretation::Assumed));
    p.devices[0].breaker_interrupting_s = Some(0.04);
    p.sources[0].decrement_curve = vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.1, current_ratio: 0.8 },
        DecrementPoint { time_s: 0.5, current_ratio: 0.8 },
    ];
    let arc_only = Studies { loadflow: false, fault: false, arcflash: true, coordination: false };
    let out = study::run(&p, arc_only).unwrap();
    let row = arc(&out);
    assert!(row.decrement_applied);
    assert!(row.assumed);
    assert!(row.duration_note.contains("relay trip") && row.duration_note.contains("band interpretation is assumed"),
        "{}", row.duration_note);
    // Slower than the fast band would give even at the initial (highest) current.
    let fast = curves::trip_time_fast_band(&p.devices[0].curve, row.arcing_ka * 1000.0).unwrap();
    assert!(row.time_s > fast + 0.04);

    // An unavailable tail below the sampled current fails visibly instead of timing.
    p.devices[0].curve = one_band(&SHORT, PointTail::Unavailable);
    let out = study::run(&p, arc_only).unwrap();
    let failure = out.arc_flash_failures.iter().find(|f| f.equipment_id == "af-mcc").unwrap();
    assert!(failure.error.contains("unavailable"), "{}", failure.error);

    // A held tail is integrated at the last time and reported.
    p.devices[0].curve = one_band(&SHORT, PointTail::HoldLastTime);
    let out = study::run(&p, arc_only).unwrap();
    let row = arc(&out);
    assert!(row.assumed);
    assert!(row.duration_note.contains("held"), "{}", row.duration_note);
}

#[test]
fn sld_and_package_handle_point_curves() {
    let p = project(two_band(PointTail::Unavailable));
    let out = run(&p);
    let svg = sld::to_svg(&sld::diagram(&p, &out));
    assert!(svg.contains("<svg"));
    let dir = std::env::temp_dir().join(format!("flashmob-point-curves-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let request = serde_json::json!({
        "project": p,
        "commands": [{"op": "run"}, {"op": "export_package", "output": dir}],
    });
    let response = exec::exec_request(&request.to_string());
    assert!(response.ok, "{:?} {:?}", response.error, response.validation_errors);
    let mut found = false;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
        found |= text.contains("Point Curve") && text.contains("min 3 / max 3");
    }
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(found, "package export lacks the point-curve device");
}

#[test]
fn set_device_edits_curve_and_library_link() {
    let response = exec::exec_request(&serde_json::json!({"commands": [
        {"op": "sample"},
        {"op": "set_device", "id": "brk-main",
         "curve": {"type": "points", "points": MIN, "max_points": MAX, "beyond_last": "unavailable", "band_interpretation": "assumed"},
         "breaker_interrupting_s": 0.05,
         "library": {"device_id": 4242, "manufacturer": "Example Co", "type": "breaker", "rating_a": 3200.0,
                     "unsupported_elements": ["ground fault"]}},
        {"op": "run"},
        {"op": "tcc"}
    ]}).to_string());
    assert!(response.ok, "{:?} {:?}", response.error, response.validation_errors);
    let device = &response.project.devices[0];
    assert!(matches!(device.curve, CurveSpec::Points { band_interpretation: BandInterpretation::Assumed, .. }));
    assert_eq!(device.breaker_interrupting_s, Some(0.05));
    let link = device.library.as_ref().unwrap();
    assert_eq!((link.device_id, link.manufacturer.as_str(), link.device_type.as_str()), (4242, "Example Co", "breaker"));
    assert!(response.tcc_csv.as_deref().unwrap().contains("MCC main (max)"));
    // The link uses the documented field names and omits empty fields.
    let json = serde_json::to_value(device).unwrap();
    assert_eq!(json["library"]["type"], "breaker");
    assert!(json["library"].get("style").is_none());

    let cleared = exec::exec_request(&serde_json::json!({"project": response.project, "commands": [
        {"op": "set_device", "id": "brk-main", "clear": ["library", "breaker_interrupting_s"], "fuse_total_clearing": true}
    ]}).to_string());
    assert!(cleared.ok, "{:?} {:?}", cleared.error, cleared.validation_errors);
    assert!(cleared.project.devices[0].library.is_none());
    assert!(cleared.project.devices[0].breaker_interrupting_s.is_none());
    assert!(cleared.project.devices[0].fuse_total_clearing);

    for (command, message) in [
        (serde_json::json!({"op": "set_device", "id": "nope", "name": "x"}), "no device 'nope'"),
        (serde_json::json!({"op": "set_device", "id": "brk-main", "clear": ["curve"]}), "cannot clear 'curve'"),
        (serde_json::json!({"op": "set_device", "id": "brk-main", "breaker_interrupting_s": 0.1, "clear": ["breaker_interrupting_s"]}), "both sets and clears"),
    ] {
        let r = exec::exec_request(&serde_json::json!({"commands": [{"op": "sample"}, command]}).to_string());
        assert!(!r.ok);
        assert!(r.error.as_deref().unwrap_or_default().contains(message), "{:?}", r.error);
    }
    // Invalid point curves are validation errors.
    let r = exec::exec_request(&serde_json::json!({"commands": [{"op": "sample"},
        {"op": "set_device", "id": "brk-main", "curve": {"type": "points", "points": MAX, "max_points": MIN, "beyond_last": "unavailable"}}]}).to_string());
    assert!(!r.ok);
    assert!(r.validation_errors.iter().any(|e| e.contains("brk-main: invalid point curve (max_points is faster")), "{:?}", r.validation_errors);
}

#[test]
fn projects_without_library_links_still_load_and_omit_the_field() {
    let text = serde_json::to_string(&Project::sample()).unwrap();
    assert!(!text.contains("\"library\""));
    let project = exec::load_project(&text).unwrap();
    assert!(project.devices.iter().all(|d| d.library.is_none()));
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["devices"][0]["library"] = serde_json::json!({"device_id": 7, "unknown": 1});
    assert!(exec::load_project(&value.to_string()).is_err());
}

/// A breaker whose max band is its total-clearing band.
fn total_clearing_breaker() -> Project {
    let mut p = Project::sample();
    p.motors.clear();
    p.equipment.truncate(1);
    p.devices[0].curve = two_band(PointTail::Unavailable);
    p.devices[0].breaker_interrupting_s = None;
    p.devices[0].curve_is_total_clearing = true;
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    p
}

#[test]
fn curve_is_total_clearing_adds_no_breaker_time_and_keeps_breaker_identity() {
    let p = total_clearing_breaker();
    let out = run(&p);
    let row = arc(&out);
    close(row.time_s, curves::trip_time(&p.devices[0].curve, row.arcing_ka * 1000.0).unwrap());
    let coord = out.coordination.as_ref().unwrap().iter().find(|r| r.device_id == "brk-main").unwrap();
    close(coord.total_clearing_s.unwrap(), coord.trip_at_bolted_s.unwrap());
    let protection = out.protection.iter().find(|r| r.device_id.as_deref() == Some("brk-main")).unwrap();
    assert_eq!(protection.clearing_data, "entered");
    // Still drawn and exported as a breaker, not a fuse.
    let diagram = sld::diagram(&p, &out);
    let t1 = diagram.branches.iter().find(|b| b.id == "t1").unwrap();
    assert_eq!(t1.protector, "breaker");

    // Same clearing as the fuse flag on the same curve; only the fuse identity differs.
    let mut fuse = p.clone();
    fuse.devices[0].curve_is_total_clearing = false;
    fuse.devices[0].fuse_total_clearing = true;
    let fuse_out = run(&fuse);
    close(arc(&fuse_out).time_s, row.time_s);
    assert_eq!(sld::diagram(&fuse, &fuse_out).branches.iter().find(|b| b.id == "t1").unwrap().protector, "fuse");
}

#[test]
fn curve_is_total_clearing_conflicts_with_breaker_time_and_decrement() {
    let mut p = total_clearing_breaker();
    p.devices[0].breaker_interrupting_s = Some(0.05);
    assert!(p.validate().iter().any(|e| e == "brk-main: invalid clearing-time definition"), "{:?}", p.validate());

    let mut p = total_clearing_breaker();
    p.sources[0].decrement_curve = vec![
        DecrementPoint { time_s: 0.0, current_ratio: 1.0 },
        DecrementPoint { time_s: 0.5, current_ratio: 0.8 },
    ];
    let out = study::run(&p, Studies { loadflow: false, fault: false, arcflash: true, coordination: false }).unwrap();
    let failure = out.arc_flash_failures.iter().find(|f| f.equipment_id == "af-mcc").unwrap();
    assert!(failure.error.contains("Arc flash cannot be calculated for bus mcc"), "{}", failure.error);
    assert!(failure.error.contains("Missing: a trip-only curve and breaker_interrupting_s"), "{}", failure.error);
    assert!(failure.error.contains("curve_is_total_clearing"), "{}", failure.error);
}

#[test]
fn fuse_total_clearing_projects_load_and_save_unchanged() {
    let mut p = Project::sample();
    p.devices[1].fuse_total_clearing = true;
    p.devices[1].breaker_interrupting_s = None;
    let text = serde_json::to_string_pretty(&p).unwrap();
    assert!(!text.contains("curve_is_total_clearing"));
    let loaded = exec::load_project(&text).unwrap();
    assert!(loaded.devices[1].fuse_total_clearing && !loaded.devices[1].curve_is_total_clearing);
    assert!(loaded.devices[1].curve_includes_clearing());
    assert_eq!(serde_json::to_string_pretty(&loaded).unwrap(), text);
    // Identical results to the general flag's clearing path for the same device.
    let out = run_full(&loaded);
    let fuse_row = out.coordination.as_ref().unwrap().iter().find(|r| r.device_id == loaded.devices[1].id).unwrap();
    close(fuse_row.total_clearing_s.unwrap(), fuse_row.trip_at_bolted_s.unwrap());
}

#[test]
fn exec_and_package_carry_curve_is_total_clearing() {
    let dir = std::env::temp_dir().join(format!("flashmob-total-clearing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let response = exec::exec_request(&serde_json::json!({"commands": [
        {"op": "sample"},
        {"op": "set_device", "id": "brk-main", "curve": {"type": "points", "points": MIN, "max_points": MAX, "beyond_last": "unavailable"},
         "curve_is_total_clearing": true, "clear": ["breaker_interrupting_s"]},
        {"op": "add_device", "id": "extra", "bus": "pnl", "curve": {"type": "definite", "pickup_a": 100.0, "time_s": 0.2},
         "curve_is_total_clearing": true},
        {"op": "run"},
        {"op": "export_package", "output": dir}
    ]}).to_string());
    assert!(response.ok, "{:?} {:?}", response.error, response.validation_errors);
    assert!(response.project.devices[0].curve_is_total_clearing && !response.project.devices[0].fuse_total_clearing);
    assert!(response.project.devices.iter().find(|d| d.id == "extra").unwrap().curve_is_total_clearing);
    let csv = std::fs::read_to_string(dir.join("protective-devices.csv")).unwrap();
    assert!(csv.lines().next().unwrap().ends_with("FuseTotalClearingCurve,CurveIsTotalClearing"), "{csv}");
    std::fs::remove_dir_all(&dir).unwrap();
    let main_line = csv.lines().find(|l| l.contains("Point Curve")).unwrap();
    assert!(main_line.ends_with("false,true"), "{main_line}");

    let unset = exec::exec_request(&serde_json::json!({"project": response.project, "commands": [
        {"op": "set_device", "id": "brk-main", "curve_is_total_clearing": false, "breaker_interrupting_s": 0.08}
    ]}).to_string());
    assert!(unset.ok, "{:?} {:?}", unset.error, unset.validation_errors);
    assert!(!unset.project.devices[0].curve_is_total_clearing);
}
