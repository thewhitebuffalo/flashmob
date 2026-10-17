//! Zipped record-format project import. The fixture is synthetic and written here.

use flashmob::import_common::{build_project, match_library, ImportDevice, ImportDeviceKind, STYLE_ALIASES};
use flashmob::import_dez::{decode_number, model_text, network_from_text, parse_records};
use flashmob::library::{Library, LibraryDevice, TripUnit, FORMAT, VERSION};
use flashmob::model::{BranchKind, CurveSpec, XfmrConn};

fn hex(v: f32) -> String {
    format!("x{:08x}", v.to_bits())
}

/// Two-bus-level plant: source, MV bus, transformer, LV bus, cable, LV panel,
/// a main breaker, a feeder breaker, a fuse with no type, a motor, a switch.
fn fixture() -> String {
    let mut t = String::from("HEADER\nfBaseMva=");
    t += &hex(10.0);
    t += "\nnSysFrequency=60\n";
    let bus = |first: bool, addr: &str, name: &str, kv: f32| {
        if first {
            format!("RECORD=2  'Bus\n\tnADDR={addr}\n\tszName={name}\n\tr_base_kv={}\n\tr_bracing_ka=\n\tc_afh_encsize_user=1\n\tr_afh_encsize_user_h={}\n\tr_afh_encsize_user_w={}\n\tr_afh_encsize_user_d={}\nENDREC\n", hex(kv), hex(900.0), hex(500.0), hex(200.0))
        } else {
            format!("RECORD=2  'Bus\n\t{addr}\n\t{name}\n\t{}\n\t{}\n\t0\n\t\n\t\n\t\nENDREC\n", hex(kv), hex(42.0))
        }
    };
    t += &bus(true, "1", "MV BUS", 12.47);
    t += &bus(false, "2", "LV SWGR", 0.48);
    t += &bus(false, "3", "PANEL", 0.48);
    t += &bus(false, "4", "PANEL 208", 0.208);
    t += &format!("RECORD=3  'Utility\n\tnADDR=10\n\tszName=UTIL\n\ta_to_bus_rec=1\n\tr_kv={}\n\tr_3phase_mva={}\n\tr_3phase_x_r_ratio={}\n\tr_slg_mva={}\n\tr_ctrl_kv_pu={}\nENDREC\n", hex(0.208), hex(500.0), hex(15.0), hex(500.0), hex(1.0));
    t += &format!("RECORD=6  '2W Transformer\n\tnADDR=11\n\tszName=T1\n\ta_from_bus_rec=1\n\ta_to_bus_rec=2\n\tr_mva={}\n\tr_from_nom_kv={}\n\tr_to_nom_kv={}\n\tr_z_percent={}\n\tr_z0_percent=\n\tr_x_r_ratio={}\n\tc_from_conn_code=0\n\tc_to_conn_code=2\n\tr_zps_r1_pu=\n\tr_zps_x1_pu=\nENDREC\n", hex(1.0), hex(12.47), hex(0.48), hex(5.5), hex(6.0));
    // Second transformer record (positional) with a blank impedance.
    t += &format!("RECORD=6  '2W Transformer\n\t12\n\tT2\n\t3\n\t4\n\t{}\n\t{}\n\t{}\n\t\n\t\n\t{}\n\t0\n\t2\n\t\n\t\nENDREC\n", hex(0.03), hex(0.48), hex(0.208), hex(1.6));
    t += &format!("RECORD=16  'Cable\n\tnADDR=13\n\tszName=C-1\n\ta_from_bus_rec=2\n\ta_to_bus_rec=3\n\tr_r1_pu={}\n\tr_x1_pu={}\n\tr_r0_pu={}\n\tr_x0_pu={}\n\tl_cable_rating=380\nENDREC\n", hex(0.5), hex(0.4), hex(1.5), hex(1.2));
    t += "RECORD=13  'LV Breaker\n\tnADDR=20\n\tszName=FEEDER\n\ta_from_bus_rec=2\n\ta_to_bus_rec=13\n\ts_mfr_id=ACME\n\ts_type_id=B Frame\n\ts_style_id=ZZ 3P\n\ts_frame_size=125(100AT)\n\tr_ext_ltpu_amps=";
    t += &hex(100.0);
    t += "\n\tr_int_ka=\nENDREC\n";
    t += "RECORD=13  'LV Breaker\n\t21\n\tSPARE\n\t3\n\t0\n\tACME\n\t\n\tQQ\n\t\n\t\n\t\nENDREC\n";
    t += "RECORD=15  'Fuse\n\tnADDR=22\n\tszName=MV FUSE\n\ta_from_bus_rec=1\n\ta_to_bus_rec=11\n\ts_mfr_id=\n\ts_type_id=\nENDREC\n";
    t += &format!("RECORD=5  'Motor\n\tnADDR=30\n\tszName=M1\n\ta_to_bus_rec=3\n\tr_hp={}\n\tr_kv={}\n\tr_pfactor={}\n\tr_xppdv={}\n\tr_x_r_ratio={}\nENDREC\n", hex(50.0), hex(0.46), hex(0.82), hex(16.7), hex(8.0));
    t += "RECORD=14  'Switch\n\tnADDR=40\n\tszName=DISC\n\ta_from_bus_rec=3\n\ta_to_bus_rec=13\n\tc_normal_state=1\nENDREC\n";
    t += "RECORD=99  'Widget\n\tnADDR=50\nENDREC\nGRAPHICSU0=00\n";
    t
}

fn stored_zip(name: &str, body: &[u8]) -> Vec<u8> {
    let mut z = Vec::new();
    let n = name.len() as u16;
    let len = body.len() as u32;
    z.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    z.extend_from_slice(&len.to_le_bytes());
    z.extend_from_slice(&len.to_le_bytes());
    z.extend_from_slice(&n.to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    z.extend_from_slice(name.as_bytes());
    z.extend_from_slice(body);
    let cd = z.len() as u32;
    z.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    z.extend_from_slice(&len.to_le_bytes());
    z.extend_from_slice(&len.to_le_bytes());
    z.extend_from_slice(&n.to_le_bytes());
    z.extend_from_slice(&[0; 12]);
    z.extend_from_slice(&0u32.to_le_bytes());
    z.extend_from_slice(name.as_bytes());
    let cd_len = z.len() as u32 - cd;
    z.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 1, 0, 1, 0]);
    z.extend_from_slice(&cd_len.to_le_bytes());
    z.extend_from_slice(&cd.to_le_bytes());
    z.extend_from_slice(&[0, 0]);
    z
}

#[test]
fn numbers_decode_from_single_double_and_text() {
    assert_eq!(decode_number("x41200000"), Some(10.0));
    assert_eq!(decode_number(&format!("L{}", 1.0f64.to_bits() as i64)), Some(1.0));
    assert_eq!(decode_number("15"), Some(15.0));
    assert_eq!(decode_number(""), None);
    assert_eq!(decode_number(&hex(3.1234568e38)), None);
}

#[test]
fn positional_records_use_the_first_records_field_names() {
    let (header, records) = parse_records(&fixture());
    assert_eq!(decode_number(&header["fBaseMva"]), Some(10.0));
    let buses: Vec<_> = records.iter().filter(|r| r.kind == "Bus").collect();
    assert_eq!(buses.len(), 4);
    assert_eq!(buses[1].text("szName"), "LV SWGR");
    assert_eq!(buses[2].num("r_bracing_ka"), Some(42.0));
}

#[test]
fn archive_member_with_records_is_read() {
    let text = fixture();
    let zip = stored_zip("M.U", text.replace('\n', "\r\n").as_bytes());
    assert_eq!(model_text(&zip).unwrap(), text);
}

#[test]
fn network_maps_to_a_project_with_flags() {
    let net = network_from_text(&fixture(), "Synthetic");
    let result = build_project(&net, None);
    let p = &result.project;
    let codes: Vec<&str> = result.warnings.iter().map(|w| w.code.as_str()).collect();
    assert_eq!(p.buses.len(), 4);
    assert_eq!(p.sources[0].mva_sc, 500.0);
    assert!((p.sources[0].x0_over_x1 - 1.0).abs() < 1e-9);
    assert!(codes.contains(&"source_kv_mismatch"));
    let t1 = p.branches.iter().find(|b| b.name == "T1").unwrap();
    match &t1.kind {
        BranchKind::Transformer { z_percent, connection, kva, .. } => {
            assert!((z_percent - 5.5).abs() < 1e-6);
            assert_eq!(*connection, XfmrConn::Dyn);
            assert!((kva - 1000.0).abs() < 1e-3);
        }
        _ => panic!("T1 is a transformer"),
    }
    assert!(result.warnings.iter().any(|w| w.code == "transformer_z_missing" && w.element == "T2"));
    assert_eq!(p.assumptions.len(), 1);
    let c1 = p.branches.iter().find(|b| b.name == "C-1").unwrap();
    match &c1.kind {
        BranchKind::Line { r_ohm, x_ohm, ampacity_a, .. } => {
            let zb = 0.48f64 * 0.48 / 10.0;
            assert!((r_ohm - 0.5 * zb).abs() < 1e-6 && (x_ohm - 0.4 * zb).abs() < 1e-6);
            assert_eq!(*ampacity_a, Some(380.0));
        }
        _ => panic!("C-1 is a line"),
    }
    assert!((p.motors[0].x_subtransient_pu - 0.167).abs() < 1e-6);
    assert!(result.warnings.iter().any(|w| w.code == "device_unresolved" && w.element == "MV FUSE"));
    assert!(result.warnings.iter().any(|w| w.code == "device_unmatched" && w.element == "FEEDER"));
    assert!(result.warnings.iter().any(|w| w.code == "device_unconnected" && w.element == "SPARE"));
    assert!(codes.contains(&"switch_closed_not_modeled"));
    assert!(codes.contains(&"record_skipped"));
    assert_eq!(p.devices.len(), 2);
    assert!(p.devices.iter().all(|d| matches!(d.curve, CurveSpec::SettingsNotCollected { .. })));
    let panel = p.equipment.iter().find(|e| e.name == "PANEL").unwrap();
    assert_eq!(panel.upstream_device.as_deref(), Some("feeder"));
    let p208 = p.equipment.iter().find(|e| e.name == "PANEL 208").unwrap();
    assert_eq!(p208.upstream_device.as_deref(), Some("feeder"));
    let mv = p.equipment.iter().find(|e| e.name == "MV BUS").unwrap();
    assert_eq!(mv.upstream_device, None);
    let lv = p.equipment.iter().find(|e| e.name == "LV SWGR").unwrap();
    assert_eq!(lv.upstream_device.as_deref(), Some("mv-fuse"));
    assert_eq!((mv.height_mm, mv.width_mm, mv.depth_mm), (900.0, 500.0, 200.0));
    assert!(lv.height_mm > 0.0 && lv.height_mm != 900.0);
    assert_eq!(p.buses[1].kv, 0.48);
    let errors = p.validate();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn library_match_needs_style_and_trip_rating() {
    let unit = |a: f64| TripUnit { rating_a: a, ..Default::default() };
    let dev = |id: u32, style: &str, units: Vec<TripUnit>| LibraryDevice {
        id, manufacturer: "Acme Electric".into(), style: style.into(), trip_units: units, ..Default::default()
    };
    let lib = Library { format: FORMAT.into(), version: VERSION, devices: vec![dev(1, "ZZ", vec![unit(50.0), unit(100.0)]), dev(2, "GZZ", vec![unit(100.0)])], ..Default::default() };
    let entered = |style: &str, a: f64| ImportDevice {
        key: "1".into(), name: "B".into(), kind: ImportDeviceKind::Breaker, bus: None, branch: None,
        manufacturer: "Acme Electric".into(), device_type: String::new(), style: style.into(), frame: String::new(),
        rating_a: Some(a), interrupting_ka: None,
    };
    assert_eq!(match_library(&lib, &entered("ZZ 3P", 100.0)).map(|m| m.device_id), Ok(1));
    assert_eq!(match_library(&lib, &entered("ZZ", 100.0)).unwrap().alias, None);
    assert!(match_library(&lib, &entered("ZZ", 70.0)).is_err());
    assert!(match_library(&lib, &entered("Z", 100.0)).is_err());
}

#[test]
fn style_alias_links_to_the_aliased_family_and_is_marked_assumed() {
    let unit = |a: f64| TripUnit { rating_a: a, ..Default::default() };
    let dev = |id: u32, style: &str, units: Vec<TripUnit>| LibraryDevice {
        id, manufacturer: "SQUARE D".into(), style: style.into(), trip_units: units, ..Default::default()
    };
    let lib = Library { format: FORMAT.into(), version: VERSION, devices: vec![dev(5, "EG", vec![unit(20.0), unit(50.0)]), dev(6, "GJL", vec![unit(100.0)])], ..Default::default() };
    let entered = |style: &str, a: f64| ImportDevice {
        key: "1".into(), name: "B".into(), kind: ImportDeviceKind::Breaker, bus: None, branch: None,
        manufacturer: "SQD".into(), device_type: String::new(), style: style.into(), frame: String::new(),
        rating_a: Some(a), interrupting_ka: None,
    };
    let eg = match_library(&lib, &entered("EGB", 50.0)).unwrap();
    assert_eq!(eg.device_id, 5);
    assert_eq!(eg.alias.map(|a| (a.0, a.1)), Some(("EGB", "EG")));
    let jl = match_library(&lib, &entered("JL", 100.0)).unwrap();
    assert_eq!((jl.device_id, jl.alias.map(|a| a.1)), (6, Some("GJL")));
    // The alias does not invent a rating the aliased family lacks.
    let err = match_library(&lib, &entered("JL", 175.0)).unwrap_err();
    assert!(err.contains("GJL") && err.contains("assumed alias") && err.contains("175"), "{err}");
    // Styles without an alias stay unmatched.
    assert!(match_library(&lib, &entered("QBL", 100.0)).is_err());
    assert!(STYLE_ALIASES.iter().all(|(_, e, l, why)| e != l && !why.is_empty()));
}
