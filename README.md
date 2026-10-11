# Flashmob

Flashmob is a macOS study tool for load flow, short circuit, protective-device coordination, and arc flash. Arc flash uses the IEEE 1584-2018 equations. The same engine runs in the window and on the command line.

```bash
cd "/Users/whitebuffalo/Coding Projects/flashmob"
cargo run --release -- gui
```

The one-line shows each bus with symmetrical three-phase and line-to-ground fault current, then the governing incident energy and arc-flash boundary. When the project has operating cases, each bus card also shows the worst-case incident energy and its governing case, marked incomplete if a case failed. Normally open branches are dashed with an open switch and an `N.O.` label, out-of-service sources are grey, and fuse-type devices (`fuse_total_clearing`) use a fuse symbol. Export SLD writes that drawing as SVG. The TCC tab draws the device curves, and Export TCC writes an SVG (light background) plus a CSV of the points.

`flashmob gui plant.json` opens that file. If it cannot be read, the window starts with no project and keeps the error visible until dismissed; it does not substitute the sample.

## Headless use

Stdout is JSON. There are no prompts.

```bash
flashmob schema
flashmob sample -o plant.json
flashmob topology plant.json
flashmob run plant.json
flashmob sld plant.json -o diagram.svg
flashmob tcc plant.json -o curves.svg --csv curves.csv --ref-kv 0.48
```

Export an engineering data package for mapping to a third-party study tool. No target tool version has been round-tripped, so this is not a verified import. The XML is a Flashmob representation, not a vendor schema. `project.json` preserves the complete model; `IMPORT.txt` describes the mapping files.

```bash
cargo run --release -- sample -o plant.json
cargo run --release -- export-package plant.json -o package-export
```

Map and verify the package against the selected target tool version before using it. Voltages are in volts. Cable R and X are total ohms. Transformer percent impedance is on the transformer kVA base. Transformer Primary is always the HV winding (bus and connection), whatever the branch drawing direction. Source and winding `GroundingMethod` is Solid, Impedance, or Ungrounded (N/A for delta); `NeutralR_ohm` and `NeutralX_ohm` are physical ohms, blank unless impedance-grounded. Transformer fields have Primary/Secondary prefixes. Wye connection text distinguishes Wye-Ground, Wye-Ungrounded, and Wye-Impedance. Numbers use the shortest text that round-trips (`2.5e-7` for very small values). Component names are unique across the package. Branch `NormalState` (Open/Closed, with the controlling switch) and source `ServiceStatus` record the normal operating state; operating cases and switch groups stay only in `project.json`.

The project must validate before export; otherwise nothing is written and the CLI exits 1. `-o` is a directory, or an `.xml` path where `project.xml` is written with the other files beside it; other file-like paths (`.json`, `.csv`, ...) are rejected. Existing files are never replaced unless `--force` is given (exec: `"overwrite": true`), and the input project file is never replaced. All existing targets must be regular files; preflight rejects other targets before changing any file. The complete package is written to a fresh staging directory before files are renamed into place, preserving the content of any hard-linked input. A rename failure can leave earlier files replaced; the staging directory is removed and the error reported.

`flashmob exec` reads a command list. This is the interface to drive from a model:

```bash
flashmob exec <<'EOF'
{"commands":[
  {"op":"sample"},
  {"op":"topology"},
  {"op":"run"},
  {"op":"sld"},
  {"op":"tcc","ref_kv":0.48}
]}
EOF
```

## MCP server

`flashmob-mcp` is a second program that speaks Model Context Protocol on stdin and stdout. It links this library with the GUI off and calls the same exec commands. `cargo build --release -p flashmob-mcp` produces it. The process keeps one project in memory. `open` loads a file. `save` writes it back only into the opened file's directory, `~/Documents`, or `/tmp`, and only replaces an existing file when asked. A path inside this repository is refused. `apply` takes a list of exec ops and returns one result per op, the validation errors, and the ids that were touched. It does not return the project. `flashmob://schema` serves the exec schema. `run`, `sld`, and `tcc` each say that a warning, `complete: false`, an unresolved clearing path, or an assumption means the number is not label-ready, and that feeder length and cable size must come from the model. `sld` and `tcc` write their files next to the open project. `library_search` and `apply_library` read the JSON file named by `FLASHMOB_LIBRARY`. `fuse_total_clearing` stays off unless that call sets it.

The response contains `project`, `results`, `sld_svg`, `tcc_svg`, and `tcc_csv`, plus `topology` when requested. Intermediate states may be incomplete, but the final project is validated: `validation_errors` lists any errors, and then `ok` is false and the CLI exits 1. Model edits clear results and every derived view; `run`, `topology`, `sld`, `tcc`, and `export_package` do not. `sld` and `tcc` reuse the last full study, even one whose load flow failed. `flashmob schema` includes a sample project and the command list.

The built-in sample has a motor directly on the MCC bus and no modeled motor feeder breaker. Its MCC arc-flash result therefore reports an explicit unresolved motor backfeed path instead of treating the main breaker opening as total arc clearing. The downstream panel's feeder breaker still separates its arc from that motor.

`flashmob topology plant.json` (or `flashmob topology -` for stdin) validates the project and prints entered connectivity as JSON without running a study. Every bus lists its branch IDs, which terminal it is on (`from` or `to`), the neighboring bus, and attached sources, loads, motors, protection devices, and arc equipment. Global branch records show both endpoints; device records show the entered bus and protected branch terminal, with `null` when placement has not been entered. Equipment records identify the entered upstream device. Parallel branches remain separate. These are model relationships, not inferred protection paths or calculated results. An AI caller can use them with study output to produce an SLD when needed.

Switch records add their controlled branch, kind, and normal `closed` state to topology output. Branch records link back to their switch. The output also lists switch groups, named operating cases, and any source marked out of service. An omitted source service state means the default `in_service: true`.

### Operating cases

`switches` attach breakers, ties, and ATS contacts to branches. The normal state is each switch's `closed` value and each source's `in_service` value. A named `operating_case` overrides the listed states. A branch has at most one switch. A `switch_group` sets `max_closed` across its listed switches, rejecting a state that violates an entered interlock. The `ats` kind is a label, not an interlock: use `max_closed: 1` for two break-before-make contacts; use `max_closed: 2` only when explicitly modeling a closed transition. Cases must be entered explicitly; Flashmob does not invent switch combinations or a switching sequence.

In each alternate case, arc equipment starts without the normal state's `upstream_device`, `clearing_s`, or `fallback_duration_s`. Use `equipment_states`, for example `{"equipment_id":"arc_section","upstream_device":"tie_breaker"}`, to enter a case-specific device or duration. An item absent from `equipment_states` uses protection inferred from that case's active network; if clearing data remain unavailable, that case reports an arc-flash failure and the worst-case envelope is incomplete. A normal-state manual duration never carries into a transfer case. A stored `fallback_duration_s` is not an arc duration in any case.

**An open switch removes its entire referenced branch from the electrical model**, including transformer grounding shunts or line charging on that branch. To represent an independent breaker or ATS contact, give it a separate short, finite-impedance branch and separate bus nodes; keep transformers and other equipment on their own branches. A breaker and ATS contact in series need separate branch segments and bus nodes. The `closed` field is required for every switch, especially normally open ties and ATS contacts.

This main–tie–main example has both mains closed and the tie open normally. Each transfer state has one main open and the tie closed. The electrical values are illustrative project inputs; replace them with the equipment and cable data being studied.

```json
{
  "name": "Main tie main example",
  "buses": [
    {"id":"source_left","name":"Left source","kv":0.48},
    {"id":"section_left","name":"Left section","kv":0.48},
    {"id":"section_right","name":"Right section","kv":0.48},
    {"id":"source_right","name":"Right source","kv":0.48}
  ],
  "branches": [
    {"id":"left_incoming","name":"Left incoming","from":"source_left","to":"section_left","kind":{"type":"line","r_ohm":0.002,"x_ohm":0.004}},
    {"id":"section_tie","name":"Section tie","from":"section_left","to":"section_right","kind":{"type":"line","r_ohm":0.004,"x_ohm":0.008}},
    {"id":"right_incoming","name":"Right incoming","from":"source_right","to":"section_right","kind":{"type":"line","r_ohm":0.002,"x_ohm":0.004}}
  ],
  "sources": [
    {"id":"utility_left","name":"Left utility","bus":"source_left","mva_sc":40,"xr":8,"is_slack":true},
    {"id":"utility_right","name":"Right utility","bus":"source_right","mva_sc":25,"xr":8,"is_slack":true}
  ],
  "switches": [
    {"id":"main_left","name":"Left main","branch_id":"left_incoming","kind":"breaker","closed":true},
    {"id":"tie","name":"Bus tie","branch_id":"section_tie","kind":"tie","closed":false},
    {"id":"main_right","name":"Right main","branch_id":"right_incoming","kind":"breaker","closed":true}
  ],
  "switch_groups": [
    {"id":"main_tie_interlock","switch_ids":["main_left","tie","main_right"],"max_closed":2}
  ],
  "operating_cases": [
    {"id":"left_supplies_both","name":"Left main and tie","switch_states":[{"switch_id":"main_right","closed":false},{"switch_id":"tie","closed":true}]},
    {"id":"right_supplies_both","name":"Right main and tie","switch_states":[{"switch_id":"main_left","closed":false},{"switch_id":"tie","closed":true}]}
  ]
}
```

An ATS with a generator uses two incoming branches and a group permitting at most one closed contact. The generator is out of service normally and enters service in the transfer case. For load-flow prefault, every energized island needs an in-service source with `is_slack: true`. Designate the standby generator as slack even while its normal `in_service` value is false, as shown below; its slack role takes effect when the generator enters service. Without a slack source in a transferred island, load-flow prefault is unavailable and that case's fault and arc-flash results are invalid or incomplete.

```json
{
  "name": "Utility generator ATS example",
  "buses": [
    {"id":"utility_bus","name":"Utility bus","kv":0.48},
    {"id":"generator_bus","name":"Generator bus","kv":0.48},
    {"id":"load_bus","name":"ATS load bus","kv":0.48}
  ],
  "branches": [
    {"id":"utility_throw","name":"Utility ATS connection","from":"utility_bus","to":"load_bus","kind":{"type":"line","r_ohm":0.003,"x_ohm":0.004}},
    {"id":"generator_throw","name":"Generator ATS connection","from":"generator_bus","to":"load_bus","kind":{"type":"line","r_ohm":0.003,"x_ohm":0.004}}
  ],
  "sources": [
    {"id":"utility","name":"Utility","bus":"utility_bus","mva_sc":40,"xr":8,"is_slack":true},
    {"id":"generator","name":"Generator","bus":"generator_bus","mva_sc":8,"xr":6,"is_slack":true,"in_service":false}
  ],
  "switches": [
    {"id":"ats_utility","name":"ATS utility contact","branch_id":"utility_throw","kind":"ats","closed":true},
    {"id":"ats_generator","name":"ATS generator contact","branch_id":"generator_throw","kind":"ats","closed":false}
  ],
  "switch_groups": [
    {"id":"ats_interlock","switch_ids":["ats_utility","ats_generator"],"max_closed":1}
  ],
  "operating_cases": [
    {"id":"on_generator","name":"Generator supplies load","switch_states":[{"switch_id":"ats_utility","closed":false},{"switch_id":"ats_generator","closed":true}],"source_states":[{"source_id":"utility","in_service":false},{"source_id":"generator","in_service":true}]}
  ]
}
```

Save either object as a project JSON file and run `flashmob run project.json --study fault`. `flashmob exec` can build the same records with `add_switch`, `add_switch_group`, and `add_operating_case`; `add_source` and `set_source` accept `in_service` and optional `decrement_curve`. A full arc-flash study also needs equipment geometry and a clearing duration or located protective device with sufficient settings.

`flashmob run` returns the normal result at the top level (`case_id: "normal"`) and each named result in `operating_cases`. `worst_case.buses` records independent maxima for symmetrical RMS, IEC peak, and half-cycle RMS fault current at each bus and fault type; `worst_case.arc_equipment` records the largest incident energy and boundary for each equipment item. Each maximum includes its governing `case_id`. The incident-energy case may differ from the boundary case, and the highest fault current may have lower incident energy when its protective device clears faster. Check `complete` and `failed_case_ids`: a failed case is retained and the reported maximum is incomplete for that bus or equipment.

For a generator with a supplied symmetrical three-phase short-circuit decrement curve, add `decrement_curve` to its source. `mva_sc` is its **initial terminal** three-phase fault strength. Curve points are elapsed seconds and current divided by that initial symmetrical RMS current:

```json
{"id":"generator","name":"Standby generator","bus":"gen_bus","mva_sc":35,"xr":8,"is_slack":true,"decrement_curve":[{"time_s":0,"current_ratio":1},{"time_s":0.1,"current_ratio":0.55},{"time_s":0.5,"current_ratio":0.34},{"time_s":2,"current_ratio":0.4}]}
```

These numbers illustrate the input shape, **not** a generator to use in a study. Use manufacturer data for the actual set, excitation system, and fault condition. The curve must begin at `(0 s, 1)`; times must increase strictly and ratios must stay positive. Flashmob interpolates linearly in elapsed time and holds the final ratio after the last point. A later rise is allowed because excitation can support or raise current. `set_source` can replace the curve; `"decrement_curve": []` removes it. An absent curve preserves the constant-current behavior.

For arc flash with decrement, Flashmob changes the curved source's equivalent short-circuit strength and resolves the network in time slices of at most 10 ms, split at entered curve points. It uses the midpoint bolted and arcing currents for each slice and sums the IEEE 1584 incident-energy contributions; the boundary comes from accumulated energy. For an isolating upstream breaker with an entered trip-only curve, protected branch terminal, and `breaker_interrupting_s`, it advances each relay trip element using the sampled **protected-terminal arcing current**, separately for full and reduced arcing-current cases. Between samples it interpolates current, splitting at each entered pickup threshold so a short pickup interval cannot count as an entire time slice. Breaker interruption time starts after relay operation. An entered total `clearing_s` overrides this calculation. When automatic clearing cannot be established, that bus is an arc-flash failure naming the missing data. `fallback_duration_s` is not used as an arc duration. A manual duration is not required when the entered device data permit automatic clearing.

This is a quasi-steady approximation with fixed source X/R, not a full generator transient or arc simulation. A timed relay element is assumed to reset immediately when sampled current falls below its pickup; if an element that reset would have operated earlier had it held its progress, the result is marked assumed because the actual device reset characteristic may differ. A reset that cannot change the operating time does not mark the result. A total-clearing curve alone does not describe dynamic melting, arcing, or breaker interruption, so a decrement case with `fuse_total_clearing: true` or `curve_is_total_clearing: true` still needs a manual total duration. Otherwise that bus is an arc-flash failure naming the missing trip-only curve and breaker interrupting time. `arc_duration_cap_s` limits the study only when `escape_possible` is true (the default), which is IEEE 1584-2018 Clause 6.9.1: the cap is allowed when a person can move away. That capped duration is marked assumed. It is never a field clearing setting. When `escape_possible` is false the cap is not applied and the device time is kept. As with constant current, a relay still timing at the cap, or a trip plus breaker interruption beyond it, gives a capped duration (`duration_capped`) with energy integrated to the cap when escape is possible; a relay below every pickup at the cap (where the held final current may never operate it) leaves clearing unresolved, so the arc result fails and its case envelope is incomplete. A multi-source or motor-fed arc integrates source decrement into the energy between openings. Each device is still timed at the initial arcing current through its own terminal, and that staged result stays assumed. Source X/R is held constant. A slice outside IEEE 1584's bolted-current model range is calculated with a model-range warning and the row is marked assumed, as for a constant out-of-range current. Decrement rows report initial (t = 0) bolted and arcing currents, say so in their warnings, and add `final_bolted_ka`, `final_arcing_ka`, and `final_arcing_min_ka` for the last integration slice. Initial short-circuit duty remains based on `mva_sc`. See the [Cummins generator protection guidance](https://www.cummins.com/sites/default/files/2021-02/PowerHour%20-%20Generator%20Set%20Overcurrent%20Protection%202020-11-12.pdf) for why excitation changes a decrement curve, and the [IEEE C37.112 scope](https://standards.ieee.org/ieee/C37.112/7036) for varying-current relay integration.

Current on a TCC is referred to the chosen voltage, so a device on another winding can sit on the same plot. Fault-current and arcing-current markers are drawn when a study has been run; arcing markers from assumed inputs are dotted and labeled `(assumed)`. The time axis starts at 0.01 s and extends down (to at most 0.1 ms) to show faster elements. `--device` names that match nothing are reported as such.

## Studies

- Load flow is Newton-Raphson on the positive-sequence network. The roundoff bound uses only energized Newton equations, and the mismatch tolerance is capped at 1e-5 pu on the system base (1 kW at 100 MVA). If the roundoff estimate exceeds this limit and the iteration cannot converge, the result reports non-convergence and names the roundoff limit.
- Short circuit uses symmetrical components. The peak value uses IEC 60909-0 method B: κ from the Thevenin R/X at the fault, times 1.15 for meshed networks, limited to 1.8 at or below 1 kV and 2.0 above (never below the uncorrected κ). The 1.15 factor is always applied because Flashmob does not check that every branch has R/X < 0.3, so radial results are conservative. No IEC voltage factor c is applied; currents use the prefault voltage, so this is not a complete IEC 60909 I″k/ip calculation. Half-cycle RMS uses the Thevenin X/R without a meshed-network correction. A failure confined to one bus (for example a capacitive Thevenin impedance) is reported in that bus's `error` and `note`; other buses are still calculated. A delta winding blocks zero sequence, so a line-to-ground fault on the wye side does not see the utility zero-sequence impedance.
- Coordination curves are IEC 60255, IEEE C37.112, definite time, and a simplified thermal-magnetic breaker. Arc duration comes from the upstream device at the arcing current, and again at the reduced arcing current. Each coordination row's current is the device's protected-terminal current for a bolted fault at the source-free (protected) end of its protected branch, reported as `fault_bus_id`; a device entered at its line-side bus is faulted at the branch's far end rather than seeing only backfeed. When both terminals remain source-connected after that branch opens in a mesh or parallel network, both terminal faults are compared and the larger device current governs the row. Its note identifies the other terminal and current.
- Arc flash is IEEE 1584-2018, checked against Annex D.1 and Annex D.2. A bus with no equipment record is evaluated once for each applicable equipment class (switchgear, MCC or panel, cable junction, and the 5 kV or 15 kV sizes the voltage and bus name select). Each class uses one electrode, its own gap, enclosure, and working distance. Open air is used only when the bus name says open air or outdoor, or when that class is selected. The maximum incident energy and the maximum boundary are reported independently, and the row is marked assumed. A gap outside the model range, a working distance under 305 mm, or an enclosure width under four times the gap is still calculated and marked assumed. Enclosure height or width above 1244.6 mm is clamped per the standard and warned, and that clamp alone does not mark the row assumed.

Buses are entered in physical units: kV, ohms, kVA, and percent impedance. The system base defaults to 100 MVA.

This is a study tool for building and checking a model. Manufacturer curves come from the device library compiled into the program (see [Device libraries](#device-libraries)). The built-in sample is an example plant, not a device catalog. Tests use small written libraries so each rule can be checked on its own. Flashmob does not ship ANSI C37.010 NACD multipliers, three-winding transformers, or DC systems.

## Device libraries

`flashmob import-library` converts a binary manufacturer device library into Flashmob's library JSON (`format: "flashmob-device-library"`, `version: 1`). `flashmob library` searches a library file.

The device library ships in `library/device-library.json` and is compiled into the program. Each device keeps its manufacturer, type, style, and catalog number. The Library button loads this library. `flashmob library` and `flashmob apply-library` use it when you do not pass a path. `FLASHMOB_LIBRARY` names a different file. A study still stores only the curve copied onto a device.

```bash
flashmob import-library devices.lib -o device-library.json --compact
flashmob library bussmann lpj --limit 5
flashmob library device-library.json bussmann lpj --limit 5
```

Each device keeps its manufacturer, type, style, catalog pattern, reference document, notes, and category; its voltage / ampere / interrupting ratings with test X/R; frames and their sensor ratings; and, per frame, the characteristic elements (pickup and delay setting lists, point curves, dial curve families, and so on). Values whose meaning is not established are kept as unlabeled numbers, arrays, or hex rather than guessed. Point-curve currents are amperes when `current_unit` is `"A"` and multiples of an unspecified base when it is `"multiple"`. The summary counts devices by category and elements by kind and lists records that were not converted.

`--redact TERM` (repeatable, case-insensitive) removes a term from the output: free text has it replaced by `[redacted]`, and a device whose identifying text contains it is withheld and listed in `summary.withheld_device_ids`. Existing output is not replaced without `--force`.

A library is reference data. Nothing in it is applied until `flashmob apply-library` copies one point curve into a project device (see below). Studies still use only that device, not the library file.

### Point curves and library links

A device curve can be entered as time-current points (`"type": "points"`), for example a fuse or breaker band read from manufacturer data:

```json
{"type": "points",
 "points":     [[3000, 500], [10000, 0.5], [60000, 0.02]],
 "max_points": [[3500, 1000], [10000, 1.0], [60000, 0.04]],
 "beyond_last": "hold_last_time",
 "band_interpretation": "entered"}
```

- Each pair is `[amperes, seconds]`. Currents increase strictly and times never increase. `points` is the faster (minimum, e.g. minimum melt) band; optional `max_points` is the slower (maximum or total-clearing) band and must not be faster than `points` where they overlap.
- Time between points is interpolated linearly in log(current)–log(time). Below the first point the device does not operate. Current from the faster band's first point up to, and not including, the slower band's first point is unresolved. The failure names that ampere range. The faster band's time is not used, and the slower band's first time is not held.
- `beyond_last` is required. `hold_last_time` keeps the last point's time at higher currents and marks the result assumed with a note; `unavailable` gives no time there, so clearing that depends on it is unresolved and reported. A library fuse melting curve is the exception described with library point curves: current above a last point of 0.01 s uses 0.01 s.
- Operating time, total clearing, coordination, and arc flash (constant current and source decrement) use the slower `max_points` band when present. Coordination rows add `min_band_trip_s` for the faster band. The TCC draws both bands as `name (min)` and `name (max)`.
- `band_interpretation: "assumed"` (default `"entered"`) records that the band meaning was inferred rather than stated by the data, for example which library band is total clearing. Results that use such a curve are marked assumed.
- Total clearing adds `breaker_interrupting_s` to the governing band, as for other curves. When the governing band already includes total clearing (for example a breaker's total-clearing max band), set `curve_is_total_clearing: true` instead and leave `breaker_interrupting_s` out.

A device may carry an optional `library` link recording where its data came from: `device_id` (required), and optionally `library`, `manufacturer`, `type`, `style`, `category`, `size`, `rating_a`, `voltage_v`, `modified`, and `unsupported_elements`. Studies use the device's curve and fields. The category is also read so a library fuse can use the IEEE 1584-2018 Clause 6.9.2 melting-time estimate described below. Projects without the link load unchanged. `flashmob exec` accepts `library` on `add_device`, and `set_device` changes an existing device (`name`, `bus`, `curve`, `basis`, `protected_branch`, `breaker_interrupting_s`, `fuse_total_clearing`, `curve_is_total_clearing`, `library`; `"clear": ["library", "breaker_interrupting_s", "protected_branch"]` removes optional fields).

### Applying a library device

`flashmob apply-library` copies one library characteristic onto a project device. The copy is a snapshot: the project stores the curve, and studies use that curve only, never the library file. The device's `library` link records the source. The path is chosen by the request, not by the device category. If any long-time, short-time, or instantaneous setting is named, trip settings are applied. Otherwise a selected `time_dial_curve_family` is a relay dial curve. Otherwise one point curve is copied. `basis` is `assumed`. `curve_is_total_clearing` and `breaker_interrupting_s` stay unset. A symmetrical interrupting rating is copied when the usable library rows agree: interrupting kA and test X/R must be positive, `--voltage-v` keeps rows within 0.05% of that voltage, and the sensor or trip-unit amperes prefer a matching row. Several different duties are left unset and named on the link. A zero interrupting rating is not copied. `fuse_total_clearing` stays false unless you set it after confirming that the governing band is total clearing.

```bash
flashmob apply-library --device-id 12 --id fuse-1 --bus mcc --project project.json -o project.json
flashmob apply-library device-library.json --device-id 12 --id fuse-1 --bus mcc --project project.json -o project.json
```

Without `--project` the command writes the device JSON only. `--name` overrides the name built from the manufacturer, style, catalog, and rating. `--rating-a` selects the trip unit when more than one is present (within 0.05% of the requested rating). One trip unit is used even when its stored rating is 0; a requested rating of 0 matches only a unit whose rating is 0. `--function-name` selects the curve when more than one usable function is present. A usable function is a point curve (`point_curve` or `point_curve_bands` with a non-empty curve) or a dial family (`time_dial_curve_family` with a non-empty dial list). Names match case-insensitively. `--current-base-a` sets the ampere scale of a point curve stored as multiples. `--voltage-v` is recorded on the link. `--library-name` is the label stored on the link; omit it and the library path is stored. `--protected-branch` is copied onto the device. `--fuse-total-clearing` marks the governing band as total clearing and draws the device as a fuse; leave it off when the slower band has not been confirmed.

`flashmob exec` does the same with `apply_library_device` (`library_path`, `device_id`, `id`, `bus`, and the same optional fields). Each trip setting is two strings, for example `lt_pickup_function` and `lt_pickup_label` (the same pairing for `lt_delay`, `st_pickup`, `st_delay`, and `inst`). Omit both and the setting is absent. An empty `library_name` stores `library_path`. A missing file, a library that does not load, or an id already used in the project is an error and does not change the project.

Point curves:

- `current_unit` `"A"` means the stored currents are already amperes. `current_base_a` with an ampere curve is an error. `current_unit` `"multiple"` is scaled to amperes by `current_base_a` when given, otherwise by the selected trip unit's rating. The scale is noted on the link (`current scale 100 A`). Any other or missing current unit is an error. A non-finite or non-positive current or time is an error. Points are not reordered. A current that decreases, or a time that increases, is an error. An exact duplicate point is dropped. A repeated current with a shorter time is a vertical segment: the later point is moved up by the smallest step that keeps the current increasing, the original current keeps the longer time, and the link notes the split.
- No point curve, or more than two bands, is an error. The band count is reported; three or more are not guessed.
- One band is stored as `points` with no `max_points`.
- Two bands are ordered by time. The faster band is `points` and the slower is `max_points`. Faster means the smaller time at the geometric-mean current of the overlapping range, using the same log(current)–log(time) interpolation as an entered point curve. Ranges that do not overlap, or bands that cross, are an error. Unless `--fuse-total-clearing` is set, the link notes that the slower band is not marked total clearing. A two-band library fuse stays a melt-tolerance pair.
- `beyond_last` is `unavailable`. `band_interpretation` is `assumed`.
- Without the fuse flag, the slower band is not stored as the published total-clearing curve. A check of five 100 A library curves against the published sheets found that a slower band is not, in general, total clearing: LPS-RK, AJT, and TRS pairs sit at a current ratio of 1.10/0.90, which is a tolerance band around one curve, while the published LPJ, LPS-RK, and FRS-R sheets each show one average-melt plot. For a library fuse (the link category contains "fuse") with neither total-clearing flag nor `breaker_interrupting_s`, arc flash estimates clearing from that melting curve by IEEE 1584-2018 Clause 6.9.2. Add 10% of the melting time plus 0.004 s. If that total is under 0.01 s, use 0.01 s. The same adjustment applies to one average-melt band and to the slower band of a tolerance pair. It is not applied when the governing band is the published total-clearing curve. When the arcing current is above a last point whose time is 0.01 s, the clearing time is 0.01 s and the arc current is not reduced unless `current_limiting` gives a manufacturer let-through. Current below the first point still does not operate. Current from the faster band's first point up to the slower band's first point is unresolved, and the failure names that ampere range. The faster time is not used and the slower band's first time is not held. Current above a last point that is not the 0.01 s bottom stays unresolved. The estimate is marked assumed. It is not the published total-clearing curve and it is not for an arc-flash label. That five-curve check is the product rule. The study does not promote a slower library band to total clearing. `fuse_total_clearing` stays false unless the caller sets it after confirming the governing band is the published total-clearing curve. A point curve whose category is not a fuse still needs `breaker_interrupting_s` or a total-clearing flag. With a source decrement reaching the bus, this single-current estimate is not used; that bus fails until `clearing_s` is entered or the decrement curve is removed. Above an entered peak let-through threshold, a sole-feed fuse uses the manufacturer's clearing time and, when given, its effective arc current in place of the calculated arcing current. The variation factor is not applied to that current. Set `current_limiting.cycle_table` to use the 60 Hz let-through table instead of that single time: from the threshold up to twice the threshold the duration is one half-cycle (1/120 s), and at or above twice the threshold it is one quarter-cycle (1/240 s). A stored total-clearing time already faster than one half-cycle and drawn below 0.01 s is kept. A melting time is not kept. Below the threshold the curve is read as before, including the Clause 6.9.2 melt estimate. Annex H incident-energy equations are not applied. Those equations are for one manufacturer's Class L and Class RK1 fuses at the tested voltage and working distance. Energy stays on the IEEE 1584-2018 model at the let-through duration.
- Trip units that were not selected, and every other function on the selected trip unit, are named in `library.unsupported_elements`.

Relay dial curves (`--dial`, `--ct-ratio`, `--tap-a`):

- Stored currents are multiples of a tap. Primary amperes are multiple × `tap_a` × `ct_ratio`. `--ct-ratio` is required, finite, and greater than 0. `--dial` selects one dial within 0.05% relative or 1e-9 absolute. Zero matches, or more than one, is an error that lists the dials. One band is stored, with no `max_points`. Points are not sorted. A non-positive or non-finite current or time is an error.
- `--tap-a` is secondary amperes. If it is omitted and the setting whose value matched the dial has a label ending in `A` or `a`, the dial value is used as the tap and the link notes `tap {n} A taken from the dial label, assumed secondary amperes`. Otherwise the copy errors, because `ct_ratio` alone does not set the pickup.
- The link notes `dial {dial} scaled by tap {tap} A and CT ratio {ct}`, and names every function that was not applied.

Trip settings (`--sensor-a`, a function/label pair for long-time pickup and delay, and optionally short-time pickup, short-time delay, and instantaneous, plus `--lt-delay-multiple`, `--inst-s`, and `--pickup-base`):

- `--sensor-a` is required and must match a frame sensor within 0.05%. The same sensor on several frames is accepted. No match lists the sensors. The link notes `sensor {n} A`. Long-time pickup and long-time delay are both required. The function name and the setting label match case-insensitively; zero or more than one match is an error.
- The long-time time is the setting value, or the larger of that value and every same-length numeric array at that setting's index. A slower stored time is noted. `--lt-delay-multiple` is required: the library does not state the current multiple (pass 6 when the time is at 6 times long-time pickup). The thermal-magnetic model stores seconds at 6× pickup, so `lt_delay_s = t_used × (lt_delay_multiple / 6)²`, and that conversion is noted.
- A pickup whose second header code is 10 is already in amperes and is not multiplied. A name containing `x ltpu`, `x ir`, or `× ltpu` is a multiple of the long-time pickup already computed; the long-time pickup itself cannot use that base. A name containing `x p`, `x in`, `x sensor`, or `x plug` as its own base phrase is a multiple of the sensor. A stray letter is not a base. Otherwise pass `--pickup-base sensor` or `--pickup-base long_time`.
- A short-time delay whose name contains `i2t`, `i^2t`, or `i²t` is not copied. The link notes that the flat short-time model was not used, and long-time is still applied. Short-time pickup without a flat short-time delay is an error. A flat short-time delay uses the slower stored time in seconds, with no 6× conversion, and is noted as assumed.
- Instantaneous pickup uses the same pickup rules. `--inst-s`, when finite and greater than 0, is the instantaneous time. If the pickup is set and the time is not, the time stays empty and operating time above that pickup stays unavailable until `inst_s` is entered.
- The curve is thermal-magnetic. No point curve is copied on this path. Functions that were not applied are named on the link.

## Interrupting duty

After a fault study, each bus with `bracing_ka` and each device with `interrupting` (`symmetrical_ka`, `test_xr`, `voltage_v`) is checked. The half-cycle factor is the same one the fault study uses: `sqrt(1 + 2·exp(−2π/(X/R)))`. No resistance (infinite X/R) uses `sqrt(3)`. An X/R of 0 uses 1. With a test X/R, the adjusted symmetrical rating is `rated × factor(test) / factor(system)`, and the duty is within that rating only when the half-cycle rms is within the rated half-cycle rms. A higher system X/R derates the nameplate.

Bus bracing uses the three-phase or line-to-ground bus fault with the greater half-cycle rms. `bracing_test_xr` is the test X/R of that bracing rating. Without it, symmetrical current above the bracing exceeds the rating, and symmetrical current at or below it is `not_adjusted`: that is not an asymmetrical pass. A device with `protected_branch` uses the bolted three-phase current through that branch and the faulted bus Thevenin X/R. Line-to-ground current through the branch is not stored, so it is not compared. Without `protected_branch`, the device bus fault is used. This is not a separate-network breaker duty. A rating voltage more than 10% from the device bus nominal voltage is noted and does not by itself fail the current comparison. `exceeds_rating` and `not_adjusted` each add one warning. The engineering data package writes those rows to `interrupting-duty.csv`. `protective-devices.csv` and the protective-device XML add `InterruptingSymmetricalKA`, `InterruptingTestXR`, and `InterruptingVoltageV` when a rating is entered.

## Engineering input and result conventions

- A transformer tap changes **HV winding turns** by `1 + tap_percent/100`, independent of branch orientation. `Dyn`/`Ynd` describe from/to connections; reverse those connections when reversing a branch. For either delta-wye arrangement and either drawing direction, the positive-sequence HV voltage leads the LV by 30° (ANSI C57.12.00); negative sequence shifts the opposite way. A bus voltage more than 10% from its matched winding voltage is reported as a warning. Leakage impedance is based on the untapped LV winding rating. `r0_over_r1` defaults to 1 independently of `x0_over_x1`.
- Sources and wye windings are solidly grounded unless listed in the optional top-level `neutral_grounding` array. Each entry names a source (`element_id` only) or a transformer winding (`element_id` plus `winding: "hv"|"lv"`, which must be a wye winding) and a `method`: `solid`, `impedance` (with `r_ohm`/`x_ohm` in physical ohms at that winding's bus), or `ungrounded`. Impedance grounding adds 3Zn to the zero-sequence path (a 277 V/5 A, 55.4 Ω resistor on a 480 V wye limits a ground fault to about 5 A). An ungrounded source or winding has no zero-sequence path, so a ground fault there reports zero current with a note; an ungrounded or impedance-grounded source does not need nonzero `r0_over_r1`/`x0_over_x1`. Omitting the array keeps the legacy solidly grounded model.
- Missing cable R0/X0 uses 3R1/3X1, reported as an assumption. If only one of R0 or X0 is entered, the other is used as zero and a warning is reported. An omitted source `x0_over_x1`/`r0_over_r1` (1.0) or motor `pf` (0.85), `efficiency` (0.94), `x_subtransient_pu` (0.17), or `xr` (6) uses the typical value shown and is reported in the study warnings; saving keeps such a field omitted until it is edited. Zero-sequence cable charging is omitted and reported when positive-sequence charging is entered.
- A line's impedance is entered in physical ohms. When its terminal buses use different nominal kV bases, the solver converts both ends to the same physical voltage and current relationship; a greater than 2% base mismatch still produces a warning for model review.
- Load flow excludes source-free islands from the Newton equations and reports their buses as `unenergized` at 0 pu. Each energized island needs its own slack source; otherwise load flow reports the island and bus that lacks a reference.
- Motor impedance uses its nameplate kV and kVA. Nameplate voltage must be within 10% of bus voltage, allowing customary 460/480 V differences. Motors and capacitors do not energize an otherwise source-free island.
- A device's `protected_branch` and `bus` identify its branch terminal. Missing placement yields no device current or automatic clearing result. Fault results include terminal-current phasors. `*_increment_a` is the current driven by the fault's voltage drop and is subtracted: total current = `*_prefault_a` − `*_increment_a`. Flat prefault neglects initial load flow; loadflow prefault includes it. Arcing currents use the network transfer response at each terminal for full and reduced fault injections, with initial flow retained under loadflow prefault.
- Curve times are relay/element operating times. Enter `breaker_interrupting_s` to obtain total clearing, or mark a curve that already includes total clearing: `curve_is_total_clearing: true` for any device (for example a breaker's total-clearing band), or `fuse_total_clearing: true` for a fuse's total-clearing curve. `fuse_total_clearing` implies `curve_is_total_clearing` and additionally marks the device as a fuse (SLD fuse symbol, package device type `Fuse`); a breaker with `curve_is_total_clearing` stays a breaker. Existing projects are not migrated: both flags are read as entered, `curve_is_total_clearing` is omitted from saved JSON when false, and a project using `fuse_total_clearing` loads, saves, and calculates exactly as before. Either flag together with `breaker_interrupting_s` is a validation error. The package export adds a `CurveIsTotalClearing` column/attribute. No instantaneous time is supplied by default. `clearing_s` on equipment means an explicitly entered total duration. An active stage with an unknown delay makes the operating time unavailable. With source decrement, automatic clearing requires a trip-only device curve and entered breaker interruption time; fuse total-clearing under changing current remains unresolved.
- When equipment has no explicit `upstream_device`, Flashmob identifies the nearest located device whose branch opening isolates every connected utility/generator path. Candidates are ranked by the fault-side end of their protected branch, so a feeder breaker entered at either its line-side or load-side bus is nearer than the main ahead of it; a device at that branch's fault-side terminal precedes one at its source-side terminal. It does not infer protection from device names, branch drawing direction, or curve availability. Equally placed candidates (the same branch terminal, or equally positioned parallel paths) are reported as ambiguous, and partial infeed protection is reported separately. The JSON `protection` results and GUI bus inspector show the identified device and missing clearing data independently.
- `clearing_s` is an explicit manual duration override. When `escape_possible` is true (the default), a duration longer than `arc_duration_cap_s` is replaced by the cap, marked assumed, and noted under IEEE 1584-2018 Clause 6.9.1. When escape is not possible the cap is not applied. `fallback_duration_s` is kept so older files load and is not used as an arc duration. A bus without a manual duration and without device total-clearing data is an arc-flash failure. A library fuse melting curve supplies an assumed clearing time by IEEE 1584-2018 Clause 6.9.2, described with the library point curves above. That time is not label-ready. The failure names the bus and the missing data: the isolating device, `protected_branch`, the trip curve, total clearing, or `breaker_interrupting_s`. A device that never trips stays unresolved; it is not replaced by 2 s. Buses that have the required data are still calculated. Failures are listed for every such bus.
- The arc lasts until the last connected utility, generator, or motor clears. One device that isolates every in-service source and leaves no motor keeps that device's time, and that result is not marked assumed merely because other buses need more than one device. Otherwise each located device is timed at the initial arcing current through its own terminal. Branches open at those times and the bolted current is recalculated between openings. Those rows are marked assumed. A utility or generator path with no isolating device still fails. `motor_contribution_s` (default five 60 Hz cycles) is the assumed window for a motor that has no feeder device; a motor behind a feeder device clears when that device clears. When a source decrement reaches the bus, the energy between those openings follows the entered decrement curve and source X/R is held constant. Trip times stay at the initial arcing current.
- An already open motor feeder branch disconnects its motor, so the upstream device can clear on its own. Put a motor that has its own breaker on a separate bus behind a finite-impedance feeder branch and switch. An explicit `clearing_s` must cover all contributing paths if used as an override. Do not invent feeder length or cable size.
- Arc-flash calculation failures remain in `arc_flash_failures` with equipment, bus, and reason, and appear in the report. Successful rows include both case boundaries; the reported boundary is their maximum independently of governing incident energy.
- Requested loadflow prefault is never replaced by flat prefault. Fault output states requested/used methods and validity; missing or unconverged loadflow invalidates dependent cases.
- Model edits invalidate results and derived exports. TCCs with no usable settings return a status. Validation errors cause a nonzero CLI exit. Device pickups must increase from long-time to short-time to instantaneous, and an IEC/IEEE instantaneous must exceed its timed pickup. Negative load kW (modeled as generation), negative line `b_siemens` (an inductive shunt), and a source `v_pu` outside 0.8–1.2 are accepted with a warning.

## Performance checks

Fault studies reuse one LU factorization per used sequence network and reuse the
positive-sequence unit-injection solution to calculate terminal currents. Each
study validates and builds its network once. Factors are local to a study, so
project edits cannot reuse stale factors. Full terminal-current output still
scales with the number of faulted buses times the number of eligible branches.

To compare versions, save the previous release binary before rebuilding, then run:

```bash
cargo build --release --no-default-features
python3 scripts/benchmark_studies.py /path/to/previous-flashmob target/release/flashmob
```

The script generates the sample once (or reads `--sample plant.json`) and feeds
the same project to both binaries. It checks every result field both binaries
return except elapsed time, allowing numerical roundoff (`rel_tol=1e-8`,
`abs_tol=1e-7`); keys present in only one result are listed on stderr as added
or removed instead of failing. It reports median engine times from five
runs after one warm-up per binary. Engine timing excludes initial validation,
process startup, and JSON serialization. Cases cover the sample's full studies
with flat and loadflow prefault, plus synthetic radial and meshed fault studies.
Use `--sizes 100 300 1000 --repeats 7` to change the comparison workload.
