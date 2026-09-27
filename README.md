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
  {"op":"run"},
  {"op":"sld"},
  {"op":"tcc","ref_kv":0.48}
]}
EOF
```

The response contains `project`, `results`, `sld_svg`, `tcc_svg`, and `tcc_csv`. `flashmob schema` includes a sample project and the command list.

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
- Motor impedance uses its nameplate kV and kVA. Nameplate voltage must be within 10% of bus voltage, allowing customary 460/480 V differences. Motors and capacitors do not energize an otherwise source-free island.
- A device's `protected_branch` and `bus` identify its branch terminal. Missing placement yields no device current or automatic clearing result. Fault results include terminal-current phasors. Flat prefault neglects initial load flow; loadflow prefault includes it. Arcing currents use the network transfer response at each terminal for full and reduced fault injections, with initial flow retained under loadflow prefault.
- Curve times are relay/element operating times. Enter `breaker_interrupting_s` to obtain total clearing, or set `fuse_total_clearing: true` only for an entered fuse total-clearing curve. No instantaneous time is supplied by default. `clearing_s` on equipment means an explicitly entered total duration. An active stage with an unknown delay makes the operating time unavailable.
- Automatic arc clearing requires the selected device to isolate all utility/generator paths. Cases requiring multiple-device switching are returned as failures. The present fault model uses fixed subtransient motor impedances; it does not simulate motor-current decay or a switching sequence.
- Arc-flash calculation failures remain in `arc_flash_failures` with equipment, bus, and reason, and appear in the report. Successful rows include both case boundaries; the reported boundary is their maximum independently of governing incident energy.
- Requested loadflow prefault is never replaced by flat prefault. Fault output states requested/used methods and validity; missing or unconverged loadflow invalidates dependent cases.
- Model edits invalidate results and derived exports. TCCs with no usable settings return a status. Validation errors cause a nonzero CLI exit.
