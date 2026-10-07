use flashmob::{model::*, skm};

fn row<'a>(text: &'a str, column: &str) -> &'a str {
    let mut lines = text.lines();
    let at = lines.next().unwrap().split(',').position(|h| h == column).unwrap();
    lines.next().unwrap().split(',').nth(at).unwrap()
}

#[test]
fn export_resolves_ungrounded_lv_and_impedance_source_in_physical_ohms() {
    let mut p = Project::sample();
    p.neutral_grounding.extend([
        NeutralGrounding { element_id: "t1".into(), winding: Some(Winding::Lv),
            method: GroundingMethod::Ungrounded, r_ohm: 0.0, x_ohm: 0.0 },
        NeutralGrounding { element_id: p.sources[0].id.clone(), winding: None,
            method: GroundingMethod::Impedance, r_ohm: 2.5, x_ohm: 0.75 },
    ]);
    assert!(p.validate().is_empty());
    for reversed in [false, true] {
        let mut p = p.clone();
        if reversed {
            let b = &mut p.branches[0];
            std::mem::swap(&mut b.from, &mut b.to);
            if let BranchKind::Transformer { connection, .. } = &mut b.kind { *connection = XfmrConn::Ynd; }
        }
        let export = skm::export(&p);
        let transformer = export.file("transformers.csv").unwrap();
        assert_eq!(row(transformer, "SecondaryConnection"), "Wye-Ungrounded");
        assert_eq!(row(transformer, "SecondaryGroundingMethod"), "Ungrounded");
        assert_eq!(row(transformer, "SecondaryNeutralR_ohm"), "");
        assert_eq!(row(transformer, "SecondaryNeutralX_ohm"), "");
        assert_eq!(row(transformer, "PrimaryGroundingMethod"), "N/A");
        assert_eq!(row(transformer, "PrimaryNeutralR_ohm"), "");
        assert_eq!(row(transformer, "PrimaryNeutralX_ohm"), "");
        let source = export.file("utilities.csv").unwrap();
        assert_eq!(row(source, "GroundingMethod"), "Impedance");
        assert_eq!(row(source, "NeutralR_ohm"), "2.5");
        assert_eq!(row(source, "NeutralX_ohm"), "0.75");
        for attr in ["GroundingMethod\" Unit=\"\">Impedance", "NeutralR_ohm\" Unit=\"ohm\">2.5",
            "NeutralX_ohm\" Unit=\"ohm\">0.75", "SecondaryGroundingMethod\" Unit=\"\">Ungrounded",
            "SecondaryConnection\" Unit=\"\">Wye-Ungrounded", "PrimaryGroundingMethod\" Unit=\"\">N/A"] {
            assert!(export.xml().contains(attr), "{attr}");
        }
        let tab = export.file("components.tab").unwrap();
        let headers: Vec<_> = tab.lines().next().unwrap().split('\t').collect();
        let value = |kind: &str, column: &str| {
            let at = headers.iter().position(|h| *h == column).unwrap();
            tab.lines().find(|l| l.starts_with(kind)).unwrap().split('\t').nth(at).unwrap()
        };
        assert_eq!(value("Source\t", "NeutralR_ohm"), "2.5");
        assert_eq!(value("Transformer\t", "SecondaryGroundingMethod"), "Ungrounded");
        let notes = export.file("IMPORT.txt").unwrap();
        assert!(notes.contains("neutral_grounding") && notes.contains("physical ohms"));
    }
    p.neutral_grounding[0].method = GroundingMethod::Impedance;
    p.neutral_grounding[0].r_ohm = 0.12;
    p.neutral_grounding[0].x_ohm = 0.03;
    let export = skm::export(&p);
    let transformer = export.file("transformers.csv").unwrap();
    assert_eq!(row(transformer, "SecondaryConnection"), "Wye-Impedance");
    assert_eq!(row(transformer, "SecondaryNeutralR_ohm"), "0.12");
    assert_eq!(row(transformer, "SecondaryNeutralX_ohm"), "0.03");
    p.neutral_grounding.clear();
    let export = skm::export(&p);
    assert_eq!(row(export.file("transformers.csv").unwrap(), "SecondaryGroundingMethod"), "Solid");
    assert_eq!(row(export.file("utilities.csv").unwrap(), "NeutralR_ohm"), "");
    assert_eq!(row(export.file("utilities.csv").unwrap(), "NeutralX_ohm"), "");
}
