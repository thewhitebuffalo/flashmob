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

Export the whole model for SKM Power*Tools Data Exchange. SKM imports XML, CSV, and tab-delimited files, and it can also take a Revit electrical schedule. The vendor does not publish the schema, so the files use the PTW component-editor field names. `IMPORT.txt` in the folder lists the mapping.

```bash
cargo run --release -- sample -o plant.json
cargo run --release -- export-skm plant.json -o skm-export
```

In PTW, use Project > Import or Data Exchange, and map `Name`, `FromBus`, `ToBus`, and `NominalSystemVoltage`. Voltages are in volts. Cable R and X are total ohms. Transformer percent impedance is on the transformer kVA base.

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
