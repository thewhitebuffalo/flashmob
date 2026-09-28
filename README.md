# Flashmob

Flashmob is a macOS study tool for load flow, short circuit, protective-device coordination, and arc flash. Arc flash uses the IEEE 1584-2018 equations. The same engine runs in the window and on the command line.

```bash
cd "/Users/whitebuffalo/Coding Projects/flashmob"
cargo run --release -- gui
```

The one-line shows each bus with symmetrical three-phase and line-to-ground fault current, then the governing incident energy and arc-flash boundary. Export SLD writes that drawing as SVG. The TCC tab draws the device curves, and Export TCC writes an SVG plus a CSV of the points.

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

Export an engineering data package for mapping to SKM Power*Tools. No PTW version has been round-tripped, so this is not a verified PTW import. The XML is a Flashmob representation, not an SKM schema. `project.json` preserves the complete model; `IMPORT.txt` describes the mapping files.

```bash
cargo run --release -- sample -o plant.json
cargo run --release -- export-skm plant.json -o skm-export
```

Map and verify the package against the selected PTW version before using it. Voltages are in volts. Cable R and X are total ohms. Transformer percent impedance is on the transformer kVA base.

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

The response contains `project`, `results`, `sld_svg`, `tcc_svg`, and `tcc_csv`, plus `topology` when requested. `flashmob schema` includes a sample project and the command list.

`flashmob topology plant.json` (or `flashmob topology -` for stdin) validates the project and prints entered connectivity as JSON without running a study. Every bus lists its branch IDs, which terminal it is on (`from` or `to`), the neighboring bus, and attached sources, loads, motors, protection devices, and arc equipment. Global branch records show both endpoints; device records show the entered bus and protected branch terminal, with `null` when placement has not been entered. Equipment records identify the entered upstream device. Parallel branches remain separate. These are model relationships, not inferred protection paths or calculated results. An AI caller can use them with study output to produce an SLD when needed.

Switch records add their controlled branch, kind, and normal `closed` state to topology output. Branch records link back to their switch. The output also lists switch groups, named operating cases, and any source marked out of service. An omitted source service state means the default `in_service: true`.

### Operating cases

`switches` attach breakers, ties, and ATS contacts to branches. The normal state is each switch's `closed` value and each source's `in_service` value. A named `operating_case` overrides the listed states. A branch has at most one switch. A `switch_group` sets `max_closed` across its listed switches, rejecting a state that violates an entered interlock. The `ats` kind is a label, not an interlock: use `max_closed: 1` for two break-before-make contacts; use `max_closed: 2` only when explicitly modeling a closed transition. Cases must be entered explicitly; Flashmob does not invent switch combinations or a switching sequence.

In each alternate case, arc equipment starts without the normal state's `upstream_device`, `clearing_s`, or `fallback_duration_s`. Use `equipment_states`, for example `{"equipment_id":"arc_section","upstream_device":"tie_breaker"}`, to enter a case-specific device or duration. An item absent from `equipment_states` uses protection inferred from that case's active network; if clearing data remain unavailable, that case reports an arc-flash failure and the worst-case envelope is incomplete. A normal-state manual duration or assumed fallback never silently carries into a transfer case.

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

Save either object as a project JSON file and run `flashmob run project.json --study fault`. `flashmob exec` can build the same records with `add_switch`, `add_switch_group`, and `add_operating_case`; `add_source` and `set_source` accept `in_service`. A full arc-flash study also needs equipment geometry and a clearing duration or located protective device with sufficient settings.

`flashmob run` returns the normal result at the top level (`case_id: "normal"`) and each named result in `operating_cases`. `worst_case.buses` records independent maxima for symmetrical RMS, IEC peak, and half-cycle RMS fault current at each bus and fault type; `worst_case.arc_equipment` records the largest incident energy and boundary for each equipment item. Each maximum includes its governing `case_id`. The incident-energy case may differ from the boundary case, and the highest fault current may have lower incident energy when its protective device clears faster. Check `complete` and `failed_case_ids`: a failed case is retained and the reported maximum is incomplete for that bus or equipment.

Generator sources use fixed short-circuit impedance and current in this prototype. Generator fault-current decrement and time-varying arcing or clearing are not modeled. Long generator-fed arc results are screening estimates, not validated worst-case incident energy.

Current on a TCC is referred to the chosen voltage, so a device on another winding can sit on the same plot. Fault-current and arcing-current markers are drawn when a study has been run.

## Studies

- Load flow is Newton-Raphson on the positive-sequence network.
- Short circuit uses symmetrical components. The peak value uses the IEC 60909 factor. A delta winding blocks zero sequence, so a line-to-ground fault on the wye side does not see the utility zero-sequence impedance.
- Coordination curves are IEC 60255, IEEE C37.112, definite time, and a simplified thermal-magnetic breaker. Arc duration comes from the upstream device at the arcing current, and again at the reduced arcing current.
- Arc flash is IEEE 1584-2018, checked against Annex D.1 and Annex D.2. A bus with no equipment record gets an assumed enclosure from its voltage, marked as assumed.

Buses are entered in physical units: kV, ohms, kVA, and percent impedance. The system base defaults to 100 MVA.

This is a study tool for building and checking a model. It does not include a manufacturer device library, ANSI C37.010 NACD multipliers, three-winding transformers, or DC systems.

## Engineering input and result conventions

- A transformer tap changes **HV winding turns** by `1 + tap_percent/100`, independent of branch orientation. `Dyn`/`Ynd` describe from/to connections; reverse those connections when reversing a branch. Leakage impedance is based on the untapped LV winding rating. `r0_over_r1` defaults to 1 independently of `x0_over_x1`.
- Missing cable R0/X0 uses 3R1/3X1, reported as an assumption. Zero-sequence cable charging is omitted and reported when positive-sequence charging is entered.
- A line's impedance is entered in physical ohms. When its terminal buses use different nominal kV bases, the solver converts both ends to the same physical voltage and current relationship; a greater than 2% base mismatch still produces a warning for model review.
- Load flow excludes source-free islands from the Newton equations and reports their buses as `unenergized` at 0 pu. Each energized island needs its own slack source; otherwise load flow reports the island and bus that lacks a reference.
- Motor impedance uses its nameplate kV and kVA. Nameplate voltage must be within 10% of bus voltage, allowing customary 460/480 V differences. Motors and capacitors do not energize an otherwise source-free island.
- A device's `protected_branch` and `bus` identify its branch terminal. Missing placement yields no device current or automatic clearing result. Fault results include terminal-current phasors. Flat prefault neglects initial load flow; loadflow prefault includes it. Arcing currents use the network transfer response at each terminal for full and reduced fault injections, with initial flow retained under loadflow prefault.
- Curve times are relay/element operating times. Enter `breaker_interrupting_s` to obtain total clearing, or set `fuse_total_clearing: true` only for an entered fuse total-clearing curve. No instantaneous time is supplied by default. `clearing_s` on equipment means an explicitly entered total duration. An active stage with an unknown delay makes the operating time unavailable.
- When equipment has no explicit `upstream_device`, Flashmob identifies the nearest located device whose branch opening isolates every connected utility/generator path. It does not infer protection from device names, branch drawing direction, or curve availability. Equal-distance candidates and partial infeed protection are reported separately. The JSON `protection` results and GUI bus inspector show the identified device and missing clearing data independently.
- `clearing_s` is an explicit manual duration override. Optional `fallback_duration_s` is an authorized assumed exposure duration used only if automatic clearing is unavailable; an available device curve takes precedence. Both are subject to `arc_duration_cap_s`. No fallback is enabled by default. Fallback use is stated in duration notes and successful rows are marked assumed.
- Automatic arc clearing requires the selected device to isolate all utility/generator paths. Cases requiring multiple-device switching are returned as failures. The present fault model uses fixed subtransient motor impedances; it does not simulate motor-current decay or a switching sequence.
- Arc-flash calculation failures remain in `arc_flash_failures` with equipment, bus, and reason, and appear in the report. Successful rows include both case boundaries; the reported boundary is their maximum independently of governing incident energy.
- Requested loadflow prefault is never replaced by flat prefault. Fault output states requested/used methods and validity; missing or unconverged loadflow invalidates dependent cases.
- Model edits invalidate results and derived exports. TCCs with no usable settings return a status. Validation errors cause a nonzero CLI exit.

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

The script checks all result fields except elapsed time, allowing numerical
roundoff (`rel_tol=1e-8`, `abs_tol=1e-7`). It reports median engine times from five
runs after one warm-up per binary. Engine timing excludes initial validation,
process startup, and JSON serialization. Cases cover the sample's full studies
with flat and loadflow prefault, plus synthetic radial and meshed fault studies.
Use `--sizes 100 300 1000 --repeats 7` to change the comparison workload.
