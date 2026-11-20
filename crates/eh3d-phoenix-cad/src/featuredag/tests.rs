use super::*;

/// One cubic millimetre in cubic nanometres.
const MM3: i128 = 1_000_000_000_000_000_000;

fn id(n: u16) -> Id {
    Id::from_parts(1_700_000_000_000, n, u64::from(n) * 7919 + 1).unwrap()
}

fn cfg() -> Settings {
    Settings::defaults()
}

fn dag() -> FeatureDag {
    FeatureDag::new(HistorySettings::defaults())
}

/// A plate with a square hole through it, from three parameters: ids 1 to 3 are the parameters,
/// 10 the plate, 11 the hole, 12 the plate with the hole.
fn plate() -> FeatureDag {
    let c = cfg();
    let mut d = dag();
    d.add_parameter(id(1), "width", "40 mm", &c).unwrap();
    d.add_parameter(id(2), "thick", "5 mm", &c).unwrap();
    d.add_parameter(id(3), "drill", "8 mm", &c).unwrap();
    d.add_feature(
        id(10),
        "plate",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "0 mm"),
            ("sx", "width"),
            ("sy", "width"),
            ("sz", "thick"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(
        id(11),
        "hole",
        Op::Box,
        &[],
        &[
            ("x", "width / 2 - drill / 2"),
            ("y", "width / 2 - drill / 2"),
            ("z", "0 mm"),
            ("sx", "drill"),
            ("sy", "drill"),
            ("sz", "thick"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(
        id(12),
        "drilled",
        Op::Difference,
        &[id(10), id(11)],
        &[],
        &c,
    )
    .unwrap();
    d.set_tip(Some(id(12)), &c).unwrap();
    d
}

#[test]
fn a_plate_with_a_hole_is_built_from_its_parameters() {
    let c = cfg();
    let mut d = plate();
    let r = d.rebuild(&c).unwrap();
    assert!(r.is_clean(), "{r:?}");
    assert_eq!((r.computed(), r.reused()), (6, 0));
    let drilled = d.result().unwrap();
    assert_eq!(drilled.volume_nm3(), (40 * 40 * 5 - 8 * 8 * 5) * MM3);
    assert!(!drilled.contains([20_000_000, 20_000_000, 1_000_000]));
    assert!(drilled.contains([2_000_000, 2_000_000, 1_000_000]));
    assert_eq!(
        d.value("width").unwrap().to_length(0).unwrap().get(),
        40_000_000
    );
    assert!(d.solid(id(10)).is_some() && d.value("plate").is_none());
}

#[test]
fn a_second_rebuild_changes_nothing_and_an_edit_builds_only_what_it_touches() {
    let c = cfg();
    let mut d = plate();
    d.rebuild(&c).unwrap();
    let again = d.rebuild(&c).unwrap();
    assert_eq!((again.computed(), again.reused()), (0, 6));
    // the drill: the parameter, the hole and the plate with the hole; the plate is not touched
    d.set_expression(id(3), None, "10 mm", &c).unwrap();
    let r = d.rebuild(&c).unwrap();
    assert_eq!((r.computed(), r.reused()), (3, 3), "{r:?}");
    assert_eq!(r.report(id(10)).unwrap().state, BuildState::Reused);
    assert_eq!(r.report(id(11)).unwrap().state, BuildState::Computed);
    assert_eq!(
        d.result().unwrap().volume_nm3(),
        (40 * 40 * 5 - 10 * 10 * 5) * MM3
    );
    // the width reaches the plate, the hole and the result, but not the other parameters
    d.set_expression(id(1), None, "50 mm", &c).unwrap();
    let r = d.rebuild(&c).unwrap();
    assert_eq!((r.computed(), r.reused()), (4, 2), "{r:?}");
    assert_eq!(r.report(id(2)).unwrap().state, BuildState::Reused);
    // a parameter of a feature
    d.set_expression(id(10), Some("sz"), "6 mm", &c).unwrap();
    let r = d.rebuild(&c).unwrap();
    assert_eq!((r.computed(), r.reused()), (2, 4), "{r:?}");
}

#[test]
fn what_depends_on_a_node_is_found() {
    let c = cfg();
    let d = plate();
    assert_eq!(d.dependents_of(id(3), &c).unwrap(), vec![id(11)]);
    assert_eq!(d.dependents_of(id(10), &c).unwrap(), vec![id(12)]);
    assert_eq!(
        d.affected_by(id(3), &c).unwrap(),
        vec![id(3), id(11), id(12)]
    );
    assert_eq!(d.affected_by(id(12), &c).unwrap(), vec![id(12)]);
    assert!(d.dependents_of(id(99), &c).is_err());
    assert!(d.affected_by(id(99), &c).is_err());
}

#[test]
fn a_parameter_that_depends_on_itself_is_a_cycle_and_the_history_stays_as_it_was() {
    let c = cfg();
    let mut d = plate();
    d.add_parameter(id(4), "second", "1 mm", &c).unwrap();
    d.add_parameter(id(5), "first", "second + 1 mm", &c)
        .unwrap();
    let before = d.to_bytes();
    let e = d
        .set_expression(id(4), None, "first + 1 mm", &c)
        .unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-013", "{e}");
    assert_eq!(d.to_bytes(), before);
    let e = d.set_expression(id(4), None, "second", &c).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-013", "{e}");
    assert_eq!(d.to_bytes(), before);
    assert!(d.rebuild(&c).unwrap().is_clean());
}

#[test]
fn features_that_would_depend_on_themselves_are_refused_too() {
    let c = cfg();
    let mut d = plate();
    // the plate would take the plate with the hole, which takes the plate
    let e = d.set_inputs(id(12), &[id(12), id(11)], &c).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-013");
    d.add_feature(id(13), "top", Op::Union, &[id(12), id(10)], &[], &c)
        .unwrap();
    let before = d.to_bytes();
    let e = d.set_inputs(id(12), &[id(13), id(11)], &c).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-013");
    assert_eq!(d.to_bytes(), before);
    // an input of the wrong number, one that is not there and one that is a parameter
    for inputs in [vec![id(10)], vec![id(10), id(77)], vec![id(10), id(1)]] {
        let e = d.set_inputs(id(12), &inputs, &c).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-051", "{inputs:?}");
        assert_eq!(d.to_bytes(), before);
    }
}

#[test]
fn the_order_is_the_same_whatever_the_order_the_nodes_were_added_in() {
    let c = cfg();
    let a = plate();
    // the same nodes, features first where that can be done
    let mut b = dag();
    for n in a.nodes().values() {
        if matches!(n.kind, Kind::Parameter { .. }) {
            if let Kind::Parameter { expr } = &n.kind {
                b.add_parameter(n.id, &n.name, expr, &c).unwrap();
            }
        }
    }
    let mut features: Vec<&Node> = a
        .nodes()
        .values()
        .filter(|n| matches!(n.kind, Kind::Feature { .. }))
        .collect();
    features.reverse();
    // the plate with the hole needs its inputs first
    features.sort_by_key(|n| match &n.kind {
        Kind::Feature { inputs, .. } => inputs.len(),
        Kind::Parameter { .. } => 0,
    });
    for n in features {
        if let Kind::Feature {
            op, inputs, params, ..
        } = &n.kind
        {
            let p: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            b.add_feature(n.id, &n.name, *op, inputs, &p, &c).unwrap();
        }
    }
    b.set_tip(a.tip(), &c).unwrap();
    assert_eq!(a.order(&c).unwrap(), b.order(&c).unwrap());
    assert_eq!(a.to_bytes(), b.to_bytes());
    // a node comes after everything it depends on
    let order = a.order(&c).unwrap();
    let at = |i: Id| order.iter().position(|x| *x == i).unwrap();
    assert!(at(id(1)) < at(id(10)) && at(id(10)) < at(id(12)) && at(id(3)) < at(id(11)));
}

#[test]
fn a_node_that_fails_is_reported_with_its_code_and_the_rest_is_built() {
    let c = cfg();
    let mut d = plate();
    d.add_parameter(id(6), "gap", "0 mm", &c).unwrap();
    d.add_feature(
        id(14),
        "bad",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "0 mm"),
            ("sx", "gap"),
            ("sy", "1 mm"),
            ("sz", "1 mm"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(id(15), "after", Op::Union, &[id(12), id(14)], &[], &c)
        .unwrap();
    d.set_tip(Some(id(15)), &c).unwrap();
    let r = d.rebuild(&c).unwrap();
    assert!(!r.is_clean());
    assert_eq!((r.failed(), r.inherited()), (1, 1));
    let bad = r.report(id(14)).unwrap();
    assert_eq!(
        (bad.state, bad.code),
        (BuildState::Failed, Some("PHOENIX-CAD-052"))
    );
    let after = r.report(id(15)).unwrap();
    assert_eq!(
        (after.state, after.code),
        (BuildState::Inherited, Some("PHOENIX-CAD-052"))
    );
    assert!(after.detail.contains("bad"));
    // the rest of the model is built
    assert_eq!(r.report(id(12)).unwrap().state, BuildState::Computed);
    assert!(d.solid(id(12)).is_some() && d.solid(id(15)).is_none() && d.result().is_none());
    // the text is in the eight languages
    for (lang, word) in [
        ("en", "could not be built"),
        ("es", "No se pudo construir"),
        ("de", "nicht aufgebaut"),
        ("fr", "pas pu être construite"),
        ("it", "Impossibile costruire"),
        ("ja", "構築できませんでした"),
        ("zh", "无法构建"),
        ("pt", "Não foi possível"),
    ] {
        let t = bad.text(lang);
        assert!(
            t.contains(word) && t.contains("PHOENIX-CAD-052") && t.starts_with("bad"),
            "{lang}: {t}"
        );
    }
    // fixing the parameter builds the nodes that were left, and only those and the ones they feed
    d.set_expression(id(6), None, "3 mm", &c).unwrap();
    let r = d.rebuild(&c).unwrap();
    assert!(r.is_clean(), "{r:?}");
    assert_eq!(r.report(id(12)).unwrap().state, BuildState::Reused);
    assert!(d.result().is_some());
}

#[test]
fn an_expression_that_cannot_be_calculated_keeps_its_own_code() {
    let c = cfg();
    let mut d = plate();
    d.set_expression(id(2), None, "5 mm / (width - width)", &c)
        .unwrap();
    let r = d.rebuild(&c).unwrap();
    let t = r.report(id(2)).unwrap();
    assert_eq!(
        (t.state, t.code),
        (BuildState::Failed, Some("PHOENIX-CAD-024"))
    );
    // the plate and the hole read it, so they inherit it, with the same code
    for n in [10, 11, 12] {
        let x = r.report(id(n)).unwrap();
        assert_eq!(
            (x.state, x.code),
            (BuildState::Inherited, Some("PHOENIX-CAD-024")),
            "{n}"
        );
    }
    // a name that nothing defines
    let mut d = plate();
    d.set_expression(id(3), None, "nothing_defines_this", &c)
        .unwrap();
    let r = d.rebuild(&c).unwrap();
    assert_eq!(r.report(id(3)).unwrap().state, BuildState::Failed);
    // an expression that is not written in the language is refused when it is set
    let e = d.set_expression(id(3), None, "3 +", &c).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-023");
}

#[test]
fn a_suppressed_feature_is_skipped_and_comes_back() {
    let c = cfg();
    let mut d = plate();
    d.rebuild(&c).unwrap();
    d.set_suppressed(id(11), true, &c).unwrap();
    let r = d.rebuild(&c).unwrap();
    assert_eq!(r.report(id(11)).unwrap().state, BuildState::Suppressed);
    assert_eq!(d.result().unwrap().volume_nm3(), 40 * 40 * 5 * MM3);
    // the one that makes the hole is skipped: it gives what its first input gave
    d.set_suppressed(id(11), false, &c).unwrap();
    d.set_suppressed(id(12), true, &c).unwrap();
    d.rebuild(&c).unwrap();
    assert_eq!(d.result().unwrap().volume_nm3(), 40 * 40 * 5 * MM3);
    d.set_suppressed(id(12), false, &c).unwrap();
    d.rebuild(&c).unwrap();
    assert_eq!(
        d.result().unwrap().volume_nm3(),
        (40 * 40 * 5 - 8 * 8 * 5) * MM3
    );
    // a parameter is not suppressed
    assert!(d.set_suppressed(id(1), true, &c).is_err());
}

#[test]
fn an_array_repeats_a_feature_along_a_step() {
    let c = cfg();
    let mut d = plate();
    d.add_feature(
        id(20),
        "row",
        Op::Array,
        &[id(10)],
        &[
            ("count", "3"),
            ("dx", "width + 2 mm"),
            ("dy", "0 mm"),
            ("dz", "0 mm"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(
        id(21),
        "raised",
        Op::Move,
        &[id(20)],
        &[("dx", "0 mm"), ("dy", "0 mm"), ("dz", "thick")],
        &c,
    )
    .unwrap();
    d.set_tip(Some(id(21)), &c).unwrap();
    let r = d.rebuild(&c).unwrap();
    assert!(r.is_clean(), "{r:?}");
    let s = d.result().unwrap();
    assert_eq!(s.volume_nm3(), 3 * 40 * 40 * 5 * MM3);
    assert_eq!(s.bounds().unwrap().0[2], 5_000_000);
    assert_eq!(s.bounds().unwrap().1[0], 124_000_000);
    // a count that is not allowed is a failure of the node
    for count in ["0", "1001", "2 mm", "1 / 2"] {
        d.set_expression(id(20), Some("count"), count, &c).unwrap();
        let r = d.rebuild(&c).unwrap();
        assert_eq!(
            r.report(id(20)).unwrap().code,
            Some("PHOENIX-CAD-052"),
            "{count}"
        );
    }
}

#[test]
fn a_solid_beyond_the_limits_is_a_failure_with_its_own_code() {
    let c = cfg();
    let mut d =
        FeatureDag::new(HistorySettings::with_user_config("[history]\nmax_prisms = 1\n").unwrap());
    for (i, x) in [(1u16, "0 mm"), (2, "10 mm")] {
        d.add_feature(
            id(i),
            &format!("b{i}"),
            Op::Box,
            &[],
            &[
                ("x", x),
                ("y", "0 mm"),
                ("z", "0 mm"),
                ("sx", "5 mm"),
                ("sy", "5 mm"),
                ("sz", "5 mm"),
            ],
            &c,
        )
        .unwrap();
    }
    d.add_feature(id(3), "both", Op::Union, &[id(1), id(2)], &[], &c)
        .unwrap();
    let r = d.rebuild(&c).unwrap();
    assert_eq!(r.report(id(3)).unwrap().code, Some("PHOENIX-CAD-053"));
}

#[test]
fn a_node_that_is_used_cannot_be_taken_away_and_one_that_is_not_can() {
    let c = cfg();
    let mut d = plate();
    let before = d.to_bytes();
    for used in [1, 3, 10, 11] {
        let e = d.remove(id(used), &c).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-016", "{used}");
        assert_eq!(d.to_bytes(), before);
    }
    d.remove(id(12), &c).unwrap();
    assert_eq!(d.tip(), None);
    d.remove(id(11), &c).unwrap();
    d.remove(id(3), &c).unwrap();
    assert_eq!(d.len(), 3);
    assert!(d.remove(id(3), &c).is_err());
    assert!(d.rebuild(&c).unwrap().is_clean());
}

#[test]
fn names_and_shapes_that_do_not_hold_are_refused_and_change_nothing() {
    let c = cfg();
    let mut d = plate();
    let before = d.to_bytes();
    let bad: Vec<Result<(), CadError>> = vec![
        d.add_parameter(id(1), "other", "1 mm", &c),
        d.add_parameter(id(50), "width", "1 mm", &c),
        d.add_parameter(id(51), "9lives", "1 mm", &c),
        d.add_parameter(id(52), "and", "1 mm", &c),
        d.add_feature(id(53), "f", Op::Box, &[], &[("x", "0 mm")], &c),
        d.add_feature(id(54), "g", Op::Union, &[id(10)], &[], &c),
        d.add_feature(
            id(55),
            "h",
            Op::Move,
            &[id(10)],
            &[("dx", "1 mm"), ("dy", "1 mm"), ("dz", "1 mm"), ("dw", "1")],
            &c,
        ),
        d.add_feature(
            id(56),
            "i",
            Op::Move,
            &[id(10)],
            &[("dx", "1 mm"), ("dy", "1 mm"), ("dz", "1 mm +")],
            &c,
        ),
        d.set_tip(Some(id(1)), &c),
        d.set_tip(Some(id(404)), &c),
        d.set_expression(id(1), Some("x"), "1 mm", &c),
        d.set_expression(id(10), None, "1 mm", &c),
        d.set_expression(id(10), Some("nope"), "1 mm", &c),
        d.add_extra(id(1), "not kept", &c),
        d.add_extra(id(1), "x-a\u{7}", &c),
    ];
    let codes: Vec<&str> = bad.iter().map(|r| r.as_ref().unwrap_err().code()).collect();
    assert_eq!(
        codes,
        [
            "PHOENIX-CAD-015",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-017",
            "PHOENIX-CAD-017",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-023",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051",
            "PHOENIX-CAD-051"
        ]
    );
    assert_eq!(d.to_bytes(), before);
    // more nodes than the settings allow
    let mut small =
        FeatureDag::new(HistorySettings::with_user_config("[history]\nmax_nodes = 2\n").unwrap());
    small.add_parameter(id(1), "a", "1 mm", &c).unwrap();
    small.add_parameter(id(2), "b", "1 mm", &c).unwrap();
    let e = small.add_parameter(id(3), "c", "1 mm", &c).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-051");
    assert_eq!(small.len(), 2);
}

#[test]
fn the_history_is_written_in_one_form_and_read_back_the_same() {
    let c = cfg();
    let mut d = plate();
    d.set_suppressed(id(11), true, &c).unwrap();
    let bytes = d.to_bytes();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("history 1\ntip "), "{text}");
    assert!(
        text.contains("parameter ") && text.contains("feature ") && text.contains("suppressed 1\n")
    );
    let back = FeatureDag::from_bytes(&bytes, HistorySettings::defaults(), &c).unwrap();
    assert_eq!(back.to_bytes(), bytes);
    assert_eq!(back.nodes(), d.nodes());
    assert_eq!(back.tip(), d.tip());
    // the same history built from the file gives the same solid
    let (mut a, mut b) = (d.clone(), back);
    a.rebuild(&c).unwrap();
    b.rebuild(&c).unwrap();
    assert_eq!(a.result(), b.result());
    // the file lives in a domain of the project, critical, and a project without it has none
    let mut doc = Document::new();
    assert!(doc
        .history(HistorySettings::defaults(), &c)
        .unwrap()
        .is_none());
    doc.set_history(&d).unwrap();
    assert!(doc.get(HISTORY_DOMAIN).unwrap().critical);
    let again = doc
        .history(HistorySettings::defaults(), &c)
        .unwrap()
        .unwrap();
    assert_eq!(again.to_bytes(), bytes);
    let opened = Document::open(&doc.to_bytes(), 1 << 20, &[HISTORY_DOMAIN]).unwrap();
    assert!(!opened.read_only);
    let unaware = Document::open(&doc.to_bytes(), 1 << 20, &[]).unwrap();
    assert!(unaware.read_only);
}

#[test]
fn lines_of_a_newer_program_are_kept_where_they_were() {
    let c = cfg();
    let mut d = plate();
    d.add_extra(id(10), "x-colour 3", &c).unwrap();
    d.add_extra(id(1), "x-note width of the plate", &c).unwrap();
    let bytes = d.to_bytes();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("x-colour 3\n") && text.contains("x-note width of the plate\n"));
    let back = FeatureDag::from_bytes(&bytes, HistorySettings::defaults(), &c).unwrap();
    assert_eq!(
        back.node(id(10)).unwrap().extra,
        vec!["x-colour 3".to_string()]
    );
    assert_eq!(back.to_bytes(), bytes);
    // more of them than the settings allow
    let few = HistorySettings::with_user_config("[history]\nmax_extra_lines = 0\n").unwrap();
    assert_eq!(
        FeatureDag::from_bytes(&bytes, few, &c).unwrap_err().code(),
        "PHOENIX-CAD-051"
    );
}

#[test]
fn a_file_that_is_not_a_history_is_refused_with_its_line() {
    let c = cfg();
    let good = String::from_utf8(plate().to_bytes()).unwrap();
    let a = id(1).to_string();
    let cases: Vec<(String, &str)> = vec![
        (String::new(), "first line"),
        ("history 2\n".to_string(), "first line"),
        (good.replace("history 1", "history 1\n"), "empty"),
        (good.trim_end().to_string(), "line end"),
        (good.replace("parameter", "parametre"), "not understood"),
        (
            good.replace("feature", "feature  ").replacen("  ", " ", 1),
            "",
        ),
        (format!("history 1\ninput {a}\n"), "outside a feature"),
        ("history 1\nx-keep 1\n".to_string(), "before any node"),
        (
            format!("history 1\nparameter {a} one 1 mm\nparameter {a} two 2 mm\n"),
            "used twice",
        ),
        (
            "history 1\nparameter notanid one 1 mm\n".to_string(),
            "not an id",
        ),
        (format!("history 1\nparameter {a} one\n"), "three fields"),
        (
            format!("history 1\nfeature {a} sphere ball\n"),
            "no operation",
        ),
        (format!("history 1\nparameter {a} one 1  mm\n"), "canonical"),
        (
            format!("history 1\nparameter {a} one 1 mm\ntip {a}\n"),
            "tip comes first",
        ),
        (
            format!("history 1\nfeature {a} move m\nsuppressed 2\n"),
            "suppressed",
        ),
        (
            format!("history 1\nfeature {a} move m\nparam dx 1\nparam dx 2\n"),
            "twice",
        ),
        (
            format!("history 1\nfeature {a} move m\nparam dx\n"),
            "a key and an expression",
        ),
        (
            format!("history 1\nparameter {a} one 1 mm\u{1}\n"),
            "control",
        ),
    ];
    for (text, part) in cases {
        let r = FeatureDag::from_bytes(text.as_bytes(), HistorySettings::defaults(), &c);
        if part.is_empty() {
            continue;
        }
        let e = r.unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-051", "{text:?}: {e}");
        assert!(e.to_string().contains(part), "{text:?}: {e}");
    }
    // not text, and larger than the settings allow
    let e = FeatureDag::from_bytes(&[0xff, 0xfe], HistorySettings::defaults(), &c).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-051");
    let tiny = HistorySettings::with_user_config("[history]\nmax_file_bytes = 1024\n").unwrap();
    let big = format!("history 1\n{}", "x-fill\n".repeat(400));
    assert_eq!(
        FeatureDag::from_bytes(big.as_bytes(), tiny, &c)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-051"
    );
    // a cycle in a file is the code of the cycle
    let (p, q) = (id(1), id(2));
    let cyc = format!("history 1\nparameter {p} one two + 1 mm\nparameter {q} two one + 1 mm\n");
    let e = FeatureDag::from_bytes(cyc.as_bytes(), HistorySettings::defaults(), &c).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-013");
}

#[test]
fn the_graph_can_be_written_for_graphviz() {
    let c = cfg();
    let dot = plate().to_dot(&c).unwrap();
    assert!(dot.starts_with("digraph"));
    for name in ["width", "plate", "hole", "drilled"] {
        assert!(dot.contains(name), "{name}");
    }
}

/// A generator of numbers with a fixed seed, the same on every system.
struct Seed(u64);

impl Seed {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A history of random boxes joined by random booleans, from four parameters.
fn random_history(seed: &mut Seed) -> FeatureDag {
    let c = cfg();
    let mut d = dag();
    for (i, name) in ["pa", "pb", "pc", "pd"].iter().enumerate() {
        d.add_parameter(
            id(i as u16 + 1),
            name,
            &format!("{} mm", 1 + seed.below(6)),
            &c,
        )
        .unwrap();
    }
    let boxes = 3 + seed.below(4) as u16;
    let mut next = 100u16;
    let mut tips: Vec<Id> = Vec::new();
    for k in 0..boxes {
        let pick = |s: &mut Seed| ["pa", "pb", "pc", "pd"][s.below(4) as usize];
        let (px, py, pz) = (pick(seed), pick(seed), pick(seed));
        let (sx, sy, sz) = (pick(seed), pick(seed), pick(seed));
        let params: Vec<(&str, String)> = vec![
            ("x", format!("{px} + {} mm", seed.below(5))),
            ("y", format!("{py} * 2 - {py}")),
            ("z", pz.to_string()),
            ("sx", format!("{sx} + 1 mm")),
            ("sy", format!("{sy} + 1 mm")),
            ("sz", format!("{sz} + 1 mm")),
        ];
        let refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        d.add_feature(id(next), &format!("box{k}"), Op::Box, &[], &refs, &c)
            .unwrap_or_else(|e| panic!("{e}"));
        tips.push(id(next));
        next += 1;
    }
    // join them, one after the other, with random operations
    let mut acc = tips[0];
    for (k, t) in tips.iter().enumerate().skip(1) {
        let op = [Op::Union, Op::Difference, Op::Intersection, Op::Union][seed.below(4) as usize];
        let (a, b) = if seed.below(2) == 0 {
            (acc, *t)
        } else {
            (*t, acc)
        };
        d.add_feature(id(next), &format!("join{k}"), op, &[a, b], &[], &c)
            .unwrap();
        acc = id(next);
        next += 1;
    }
    d.set_tip(Some(acc), &c).unwrap();
    d
}

#[test]
fn editing_a_parameter_gives_the_same_solids_as_building_everything_again() {
    let c = cfg();
    let mut seed = Seed(0x5eed_00d1);
    for round in 0..40 {
        let mut d = random_history(&mut seed);
        d.rebuild(&c).unwrap();
        for _ in 0..4 {
            let p = 1 + seed.below(4) as u16;
            d.set_expression(id(p), None, &format!("{} mm", 1 + seed.below(6)), &c)
                .unwrap();
            let r = d.rebuild(&c).unwrap();
            assert!(r.is_clean(), "round {round}: {r:?}");
            // a fresh history from the same file, built from nothing
            let mut fresh =
                FeatureDag::from_bytes(&d.to_bytes(), HistorySettings::defaults(), &c).unwrap();
            let all = fresh.rebuild(&c).unwrap();
            assert_eq!(all.reused(), 0);
            assert_eq!(fresh.result(), d.result(), "round {round}");
            for n in d.nodes().keys() {
                assert_eq!(fresh.solid(*n), d.solid(*n), "round {round}");
            }
            // only what depends on the parameter was calculated
            let touched = d.affected_by(id(p), &c).unwrap();
            let computed: Vec<Id> = r
                .reports
                .iter()
                .filter(|x| x.state == BuildState::Computed)
                .map(|x| x.id)
                .collect();
            for x in &computed {
                assert!(
                    touched.contains(x),
                    "round {round}: {x} is not a user of the parameter"
                );
            }
        }
    }
}

#[test]
fn the_result_of_a_history_does_not_depend_on_the_order_it_was_built_in() {
    let c = cfg();
    let mut seed = Seed(0x5eed_00d2);
    for round in 0..30 {
        let mut d = random_history(&mut seed);
        d.rebuild(&c).unwrap();
        // the same text, read back, in the same order
        let order = d.order(&c).unwrap();
        let again = FeatureDag::from_bytes(&d.to_bytes(), HistorySettings::defaults(), &c).unwrap();
        assert_eq!(again.order(&c).unwrap(), order, "round {round}");
        // an order is a topological one
        for (k, n) in order.iter().enumerate() {
            for dep in d
                .nodes()
                .values()
                .find(|x| x.id == *n)
                .map(|x| match &x.kind {
                    Kind::Feature { inputs, .. } => inputs.clone(),
                    Kind::Parameter { .. } => Vec::new(),
                })
                .unwrap()
            {
                assert!(
                    order.iter().position(|x| *x == dep).unwrap() < k,
                    "round {round}"
                );
            }
        }
    }
}

#[test]
fn damaged_files_never_panic_and_what_is_read_is_what_is_written_with_a_fixed_seed() {
    let c = cfg();
    let mut seed = Seed(0x5eed_00d3);
    let mut sources: Vec<Vec<u8>> = vec![plate().to_bytes()];
    for _ in 0..3 {
        sources.push(random_history(&mut seed).to_bytes());
    }
    let (mut read, mut refused) = (0u32, 0u32);
    for round in 0..4000 {
        let mut bytes = sources[round % sources.len()].clone();
        for _ in 0..1 + seed.below(3) {
            if bytes.is_empty() {
                break;
            }
            let at = seed.below(bytes.len() as u64) as usize;
            match seed.below(5) {
                0 => bytes[at] ^= 1 << seed.below(8),
                1 => {
                    bytes.truncate(at);
                }
                2 => {
                    bytes.remove(at);
                }
                3 => bytes.insert(at, b" \n0xz-"[seed.below(6) as usize]),
                _ => {
                    let from = seed.below(bytes.len() as u64) as usize;
                    let piece: Vec<u8> = bytes[from..bytes.len().min(from + 12)].to_vec();
                    bytes.splice(at..at, piece);
                }
            }
        }
        match FeatureDag::from_bytes(&bytes, HistorySettings::defaults(), &c) {
            Ok(d) => {
                read += 1;
                assert_eq!(d.to_bytes(), bytes, "round {round}");
            }
            Err(e) => {
                refused += 1;
                assert!(
                    [
                        "PHOENIX-CAD-051",
                        "PHOENIX-CAD-013",
                        "PHOENIX-CAD-017",
                        "PHOENIX-CAD-023",
                        "PHOENIX-CAD-024",
                        "PHOENIX-CAD-025",
                        "PHOENIX-CAD-026",
                        "PHOENIX-CAD-014"
                    ]
                    .contains(&e.code()),
                    "round {round}: {e}"
                );
            }
        }
    }
    assert!(read > 0 && refused > 1000, "{read} read, {refused} refused");
}

#[test]
fn the_settings_of_the_history_are_read_from_the_file_and_can_share_it() {
    let s = HistorySettings::defaults();
    assert_eq!(
        (s.max_nodes, s.max_array, s.max_extra_lines),
        (2000, 1000, 8)
    );
    assert_eq!(
        HistorySettings::with_user_config(default_file()).unwrap(),
        s
    );
    let both = format!(
        "{}\n{}\n{}",
        crate::config::DEFAULT_CONFIG,
        crate::sketch::settings::default_file(),
        default_file()
    );
    assert_eq!(HistorySettings::with_user_config(&both).unwrap(), s);
    assert!(Settings::with_user_config(&both).is_ok());
    for text in [
        "[history]\nmax_nodes = 0\n",
        "[history]\nmax_array = 0\n",
        "[history]\nmax_file_bytes = 10\n",
        "[history]\nmax_cells = 4\n",
        "[history]\nmax_prisms = 0\n",
        "[history]\nmax_extra_bytes = 2\n",
        "[history]\nmax_nodes = many\n",
    ] {
        let e = HistorySettings::with_user_config(text).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-011", "{text}");
    }
    assert_eq!(s.limits().max_cells, 2_000_000);
}

#[test]
fn the_operations_have_names_inputs_and_keys() {
    for op in Op::ALL {
        assert_eq!(Op::from_name(op.name()), Some(op));
    }
    assert_eq!(Op::from_name("sphere"), None);
    assert_eq!(Op::ALL.map(Op::inputs), [0, 1, 1, 2, 2, 2]);
    assert_eq!(Op::Box.keys().len(), 6);
    assert!(Op::Union.keys().is_empty());
}
