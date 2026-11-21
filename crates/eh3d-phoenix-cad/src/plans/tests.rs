// SPDX-License-Identifier: GPL-3.0-or-later
use super::bom::{part_nodes, to_csv};
use super::geom::{view_segments, Seg};
use super::measure::{measure, resolve, Model};
use super::model::*;
use super::out;
use super::panel::{lines_of, PlanState, Tone};
use super::scene::evaluate;
use super::settings::{LineType, SheetSettings};
use crate::config::Settings;
use crate::enclosure::{Face, Id};
use crate::featuredag::{FeatureDag, HistorySettings, Op};
use crate::lang::Lang;
use crate::prism::{Limits, Prism, Solid};
use crate::project::Document;
use crate::sketch::{Sketch, SketchPlane};
use crate::Length;

fn id(n: u16) -> Id {
    Id::from_parts(1_700_000_000_000, n, u64::from(n) + 3).unwrap()
}

fn mm(v: i64) -> Length {
    Length::from_mm(v).unwrap()
}

fn cfg() -> Settings {
    Settings::defaults()
}

fn sheet() -> SheetSettings {
    SheetSettings::defaults()
}

/// A box of `w` by 20 by 10 millimetres with a closed cavity inside it.
fn hollow(w: &str) -> FeatureDag {
    let c = cfg();
    let mut d = FeatureDag::new(HistorySettings::defaults());
    d.add_parameter(id(1), "w", w, &c).unwrap();
    d.add_feature(
        id(2),
        "base",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "0 mm"),
            ("sx", "w"),
            ("sy", "20 mm"),
            ("sz", "10 mm"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(
        id(3),
        "cavity",
        Op::Box,
        &[],
        &[
            ("x", "10 mm"),
            ("y", "5 mm"),
            ("z", "2 mm"),
            ("sx", "10 mm"),
            ("sy", "10 mm"),
            ("sz", "6 mm"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(id(4), "body", Op::Difference, &[id(2), id(3)], &[], &c)
        .unwrap();
    d.rebuild(&c).unwrap();
    d
}

fn plain_box() -> FeatureDag {
    let c = cfg();
    let mut d = FeatureDag::new(HistorySettings::defaults());
    d.add_feature(
        id(2),
        "cube",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "0 mm"),
            ("sx", "10 mm"),
            ("sy", "10 mm"),
            ("sz", "10 mm"),
        ],
        &c,
    )
    .unwrap();
    d.rebuild(&c).unwrap();
    d
}

fn view(n: u16, kind: ViewKind, hidden: bool) -> View {
    View {
        id: id(100 + n),
        name: kind.name().to_string(),
        kind,
        source: Source::Tip,
        place: Place::Auto,
        scale: None,
        hidden,
        extra: Vec::new(),
    }
}

fn four_views(plan: &mut Plan) {
    let s = sheet();
    for (n, k) in [
        ViewKind::Front,
        ViewKind::Top,
        ViewKind::Right,
        ViewKind::Iso,
    ]
    .into_iter()
    .enumerate()
    {
        plan.add_view(view(n as u16, k, true), &s).unwrap();
    }
}

fn dim(n: u16, kind: DimKind, refs: Vec<Ref>, view: Id) -> Dim {
    Dim {
        id: id(200 + n),
        name: format!("d{n}"),
        kind,
        refs,
        view,
        offset_um: None,
        layer: "DIMENSION".to_string(),
        extra: Vec::new(),
    }
}

fn face(node: u16, axis: Axis, side: Side) -> Ref {
    Ref::Face {
        node: id(node),
        axis,
        side,
    }
}

fn plate() -> Sketch {
    let mut s = Sketch::new("plate", SketchPlane::on_face(Face::Top)).unwrap();
    let c = s.add_point(mm(10), mm(10)).unwrap();
    s.add_circle(c, mm(5)).unwrap();
    let a = s.add_point(mm(0), mm(0)).unwrap();
    let b = s.add_point(mm(20), mm(0)).unwrap();
    s.add_line(a, b).unwrap();
    s.add_line(a, c).unwrap();
    s
}

fn element_numbers(s: &Sketch) -> (u32, u32, u32) {
    use crate::sketch::Element;
    let mut circle = 0;
    let mut lines = Vec::new();
    for e in s.elements() {
        match e.element {
            Element::Circle { .. } => circle = e.id.0,
            Element::Line { .. } => lines.push(e.id.0),
            _ => {}
        }
    }
    (circle, lines[0], lines[1])
}

fn segs(dag: &FeatureDag, kind: ViewKind, hidden: bool) -> Vec<Seg> {
    let s = sheet();
    view_segments(
        dag.result().unwrap(),
        kind,
        hidden,
        s.max_cells,
        s.max_edges,
        s.max_iso_tests,
    )
    .unwrap()
}

// ----- settings -----

#[test]
fn settings() {
    let s = sheet();
    assert_eq!(s.sheet_size_um("A4", false), Some((297_000, 210_000)));
    assert_eq!(s.sheet_size_um("A0", true), Some((841_000, 1_189_000)));
    assert_eq!(s.formats.len(), 5);
    assert_eq!(s.layer("HIDDEN").unwrap().line, LineType::Dashed);
    assert_eq!(s.layer("OUTLINE").unwrap().width_um, 350);
    assert_eq!(s.layer("DIMENSION").unwrap().width_um, 175);
    // a user file adds a format and changes the style of the dimensions
    let u = SheetSettings::with_user_config(
        "[sheet]\nformat.B5 = 176 mm x 250 mm\ndefault_format = B5\ndim.decimals = 3\nlayer.OUTLINE.width = 0.5 mm\n",
    )
    .unwrap();
    assert_eq!(u.sheet_size_um("B5", true), Some((176_000, 250_000)));
    assert_eq!(u.default_format, "B5");
    assert_eq!(u.dim.decimals, 3);
    assert_eq!(u.layer("OUTLINE").unwrap().width_um, 500);
    // what is wrong is refused with the key
    for bad in [
        "[sheet]\ndefault_format = A9\n",
        "[sheet]\nformat.X = 10 mm x 10 mm\n",
        "[sheet]\nprojection = second\n",
        "[sheet]\nlayer.OUTLINE.color = 12345\n",
        "[sheet]\nlayer.names = OUTLINE\n",
        "[sheet]\nscales = 1:0\n",
        "[sheet]\ndim.decimals = 9\n",
        "[sheet]\nmax_views = 0\n",
    ] {
        let e = SheetSettings::with_user_config(bad).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-011", "{bad}");
    }
}

// ----- the file -----

fn full_plan() -> Plan {
    let s = sheet();
    let mut p = Plan::new(&s);
    four_views(&mut p);
    p.add_dim(
        dim(
            1,
            DimKind::Linear,
            vec![face(2, Axis::X, Side::Min), face(2, Axis::X, Side::Max)],
            id(100),
        ),
        &s,
    )
    .unwrap();
    p.add_balloon(
        Balloon {
            id: id(300),
            name: "b1".into(),
            part: id(2),
            at: (50_000, 50_000),
            to: (60_000, 60_000),
            extra: Vec::new(),
        },
        &s,
    )
    .unwrap();
    p.set_field("title", "Housing for a board", &s).unwrap();
    p.set_part(id(2), "aluminium", "Base plate", &s).unwrap();
    p.set_layer_visible("CENTER", false, &s).unwrap();
    p.set_layer_look("DIMENSION", "ff0000", 300, &s).unwrap();
    p.numbers.insert(id(2), 1);
    p.next_item = 2;
    p
}

#[test]
fn file_round() {
    let s = sheet();
    let p = full_plan();
    let bytes = p.to_bytes();
    let back = Plan::from_bytes(&bytes, &s).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.to_bytes(), bytes);
    assert!(bytes.starts_with(
        b"drawing 1\nformat A4\norientation landscape\nscale 1:1\nprojection third\nnextitem 2\n"
    ));
    // the domain is optional in the project file and a document without it has no sheet
    let mut doc = Document::new();
    assert_eq!(doc.plan(&s).unwrap(), None);
    doc.set_plan(&p).unwrap();
    assert!(!doc.get(DRAWING_DOMAIN).unwrap().critical);
    let again = Document::from_bytes(&doc.to_bytes(), 1 << 20).unwrap();
    assert_eq!(again.plan(&s).unwrap(), Some(p));
}

#[test]
fn file_keeps() {
    let s = sheet();
    let mut text = String::from_utf8(full_plan().to_bytes()).unwrap();
    // lines of a newer program stay where they were: in the head, in a part, a view, a dimension and a balloon
    text = text.replacen("\nitem ", "\nx-head one\nitem ", 1);
    let mut p = Plan::from_bytes(text.as_bytes(), &s).unwrap();
    assert_eq!(p.head_extra, vec!["x-head one".to_string()]);
    p.views[0].extra.push("x-view kept".to_string());
    p.dims[0].extra.push("x-dim kept".to_string());
    p.balloons[0].extra.push("x-balloon kept".to_string());
    p.parts
        .get_mut(&id(2))
        .unwrap()
        .extra
        .push("x-part kept".to_string());
    let bytes = p.to_bytes();
    let back = Plan::from_bytes(&bytes, &s).unwrap();
    assert_eq!(back.views[0].extra, vec!["x-view kept".to_string()]);
    assert_eq!(back.dims[0].extra, vec!["x-dim kept".to_string()]);
    assert_eq!(back.balloons[0].extra, vec!["x-balloon kept".to_string()]);
    assert_eq!(back.parts[&id(2)].extra, vec!["x-part kept".to_string()]);
    assert_eq!(back.to_bytes(), bytes);
    // the project file keeps the domain byte for byte when another program writes it
    let mut doc = Document::new();
    doc.set("drawing", false, bytes.clone()).unwrap();
    let again = Document::from_bytes(&doc.to_bytes(), 1 << 20).unwrap();
    assert_eq!(again.get("drawing").unwrap().data, bytes);
}

#[test]
fn file_refuses() {
    let s = sheet();
    let good = String::from_utf8(full_plan().to_bytes()).unwrap();
    let cases: Vec<(String, &str)> = vec![
        (good.replacen("drawing 1", "drawing 2", 1), "newer version"),
        (good.replacen("format A4", "format Z9", 1), "format"),
        (good.replacen("scale 1:1", "scale 1:0", 1), "scale"),
        (good.replacen("orientation landscape", "orientation tilted", 1), "orientation"),
        (good.replacen("hidden 1", "hidden 2", 1), "hidden"),
        (good.replacen("ref face", "ref plane", 1), "ref"),
        (good.replacen("field title Housing", "field title  Housing", 1), "double space"),
        (good.replace("nextitem 2\n", ""), "no nextitem"),
        (format!("{good}\n"), "empty line"),
        (good.trim_end().to_string(), "no end of line"),
        (good.replacen("layer CENTER visible 0", "layer NOPE visible 0", 1), "layer"),
        (good.replacen("\nitem ", "\nx-head one\nx-head two\nx-head three\nx-head four\nx-head five\nx-head six\nx-head seven\nx-head eight\nx-head nine\nitem ", 1), "too many lines to keep"),
        (good.replacen("view ", "vew ", 1), "word"),
    ];
    for (text, why) in cases {
        let e = Plan::from_bytes(text.as_bytes(), &s).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-054", "{why}: {e}");
    }
    // a size beyond the settings
    let tiny = SheetSettings::with_user_config("[sheet]\nmax_file_bytes = 1024\n").unwrap();
    let big = format!("{good}{}", "x-pad\n".repeat(400));
    assert!(Plan::from_bytes(big.as_bytes(), &tiny).is_err());
    // two records with one id
    let mut p = full_plan();
    p.dims[0].id = p.views[0].id;
    assert_eq!(p.validate(&s).unwrap_err().code(), "PHOENIX-CAD-054");
    // a dimension with the wrong number of references, or drawn in a view that is not there
    let mut p = full_plan();
    p.dims[0].refs.pop();
    assert_eq!(p.validate(&s).unwrap_err().code(), "PHOENIX-CAD-056");
    let mut p = full_plan();
    p.dims[0].view = id(999);
    assert_eq!(p.validate(&s).unwrap_err().code(), "PHOENIX-CAD-056");
}

#[test]
fn locked_layers() {
    let s = sheet();
    let mut p = full_plan();
    p.set_layer_locked("DIMENSION", true, &s).unwrap();
    let d = dim(
        2,
        DimKind::Linear,
        vec![face(2, Axis::Y, Side::Min), face(2, Axis::Y, Side::Max)],
        id(100),
    );
    assert_eq!(
        p.add_dim(d.clone(), &s).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
    assert_eq!(
        p.remove_dim(id(201), &s).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
    p.set_layer_locked("DIMENSION", false, &s).unwrap();
    p.add_dim(d, &s).unwrap();
    p.set_layer_locked("OUTLINE", true, &s).unwrap();
    assert!(p.add_view(view(9, ViewKind::Top, false), &s).is_err());
    p.set_layer_locked("BALLOON", true, &s).unwrap();
    assert!(p.move_balloon(id(300), (1, 1), (2, 2), &s).is_err());
    // a view that a dimension is drawn in cannot be taken away
    p.set_layer_locked("OUTLINE", false, &s).unwrap();
    assert_eq!(
        p.remove_view(id(100), &s).unwrap_err().code(),
        "PHOENIX-CAD-016"
    );
    // the look of a layer can be changed only while it is not locked
    p.set_layer_locked("TEXT", true, &s).unwrap();
    assert!(p.set_layer_look("TEXT", "00ff00", 200, &s).is_err());
    assert!(p.set_layer_look("NOPE", "00ff00", 200, &s).is_err());
    assert!(p.set_layer_look("CENTER", "zzzzzz", 200, &s).is_err());
}

// ----- the views -----

#[test]
fn ortho_views() {
    let d = hollow("40 mm");
    // the front: the outline of the box is visible, the cavity is hidden behind it
    let front = segs(&d, ViewKind::Front, true);
    let visible: Vec<&Seg> = front.iter().filter(|s| !s.hidden).collect();
    let hidden: Vec<&Seg> = front.iter().filter(|s| s.hidden).collect();
    assert_eq!(visible.len(), 4, "{front:?}");
    assert_eq!(hidden.len(), 4, "{front:?}");
    assert!(visible
        .iter()
        .any(|s| s.a == (0, 0) && s.b == (40_000_000, 0)));
    assert!(hidden
        .iter()
        .any(|s| s.a == (10_000_000, 2_000_000) && s.b == (20_000_000, 2_000_000)));
    // without the hidden edges only the outline is left
    assert_eq!(segs(&d, ViewKind::Front, false).len(), 4);
    // the top shows y up, the right shows y across
    let top = segs(&d, ViewKind::Top, true);
    assert!(top
        .iter()
        .any(|s| !s.hidden && s.b == (40_000_000, 20_000_000) || s.a == (40_000_000, 20_000_000)));
    assert_eq!(top.iter().filter(|s| s.hidden).count(), 4);
    let right = segs(&d, ViewKind::Right, true);
    assert!(
        right
            .iter()
            .any(|s| !s.hidden
                && (s.a == (20_000_000, 10_000_000) || s.b == (20_000_000, 10_000_000)))
    );
    assert_eq!(right.iter().filter(|s| s.hidden).count(), 4);
}

#[test]
fn step_edges() {
    // an L: a step shows an edge where the depth changes, and the edge behind it stays hidden
    let a = Prism::new([mm(0), mm(0), mm(0)], [mm(10), mm(10), mm(10)]).unwrap();
    let b = Prism::new([mm(10), mm(5), mm(0)], [mm(10), mm(5), mm(5)]).unwrap();
    let solid = Solid::from_prisms(&[a, b], &Limits::default()).unwrap();
    let s = sheet();
    let front = view_segments(
        &solid,
        ViewKind::Front,
        true,
        s.max_cells,
        s.max_edges,
        s.max_iso_tests,
    )
    .unwrap();
    // the front block hides the rear of the step, so the step is its right edge, in full, and the
    // two lines of the rear block that are not behind it
    let expect = [
        ((0, 0), (0, 10_000_000)),
        ((0, 0), (20_000_000, 0)),
        ((0, 10_000_000), (10_000_000, 10_000_000)),
        ((10_000_000, 0), (10_000_000, 10_000_000)),
        ((10_000_000, 5_000_000), (20_000_000, 5_000_000)),
        ((20_000_000, 0), (20_000_000, 5_000_000)),
    ];
    assert_eq!(front.len(), 6, "{front:?}");
    for (a, b) in expect {
        assert!(
            front.iter().any(|g| !g.hidden && g.a == a && g.b == b),
            "{a:?} {b:?} in {front:?}"
        );
    }
    // seen from above the rear block is behind nothing and the front block is the nearer one
    let top = view_segments(
        &solid,
        ViewKind::Top,
        true,
        s.max_cells,
        s.max_edges,
        s.max_iso_tests,
    )
    .unwrap();
    assert!(top.iter().all(|g| !g.hidden), "{top:?}");
    // nothing is drawn twice
    let mut sorted = front.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), front.len());
}

#[test]
fn iso_view() {
    let d = plain_box();
    let all = segs(&d, ViewKind::Iso, true);
    assert_eq!(all.iter().filter(|s| !s.hidden).count(), 9, "{all:?}");
    assert_eq!(all.iter().filter(|s| s.hidden).count(), 3);
    assert_eq!(segs(&d, ViewKind::Iso, false).len(), 9);
    // the cavity is hidden in the isometric view too: its twelve edges are all behind the walls
    let h = hollow("40 mm");
    let iso = segs(&h, ViewKind::Iso, true);
    assert_eq!(iso.iter().filter(|s| !s.hidden).count(), 9);
    assert_eq!(iso.iter().filter(|s| s.hidden).count(), 3 + 12);
    // the same solid gives the same lines
    assert_eq!(iso, segs(&h, ViewKind::Iso, true));
}

#[test]
fn view_limits() {
    let h = hollow("40 mm");
    let s = sheet();
    let e = view_segments(
        h.result().unwrap(),
        ViewKind::Front,
        true,
        4,
        10_000,
        1_000_000,
    )
    .unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-053");
    let e = view_segments(
        h.result().unwrap(),
        ViewKind::Front,
        true,
        s.max_cells,
        3,
        1_000_000,
    )
    .unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-053");
    let e = view_segments(
        h.result().unwrap(),
        ViewKind::Iso,
        true,
        s.max_cells,
        s.max_edges,
        1000,
    )
    .unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-053");
}

// ----- the dimensions -----

#[test]
fn linear_follows() {
    let c = cfg();
    let s = sheet();
    let mut d = hollow("40 mm");
    let mut p = Plan::new(&s);
    four_views(&mut p);
    p.add_dim(
        dim(
            1,
            DimKind::Linear,
            vec![face(2, Axis::X, Side::Min), face(2, Axis::X, Side::Max)],
            id(100),
        ),
        &s,
    )
    .unwrap();
    p.add_dim(
        dim(
            2,
            DimKind::Linear,
            vec![face(3, Axis::Z, Side::Min), face(2, Axis::Z, Side::Max)],
            id(102),
        ),
        &s,
    )
    .unwrap();
    let ev = |d: &FeatureDag| {
        let m = Model {
            dag: Some(d),
            sketches: &[],
        };
        evaluate(&p, &m, &c, &s, Lang::En).unwrap()
    };
    let before = ev(&d);
    assert!(before.is_clean(), "{:?}", before.issues);
    assert_eq!(before.dims[0].texts, vec!["40.00".to_string()]);
    assert_eq!(before.dims[1].texts, vec!["8.00".to_string()]);
    // an edit of the parameter changes the value and the drawing, and the dimension is still tied to the same node
    d.set_expression(id(1), None, "55 mm", &c).unwrap();
    d.rebuild(&c).unwrap();
    let after = ev(&d);
    assert!(after.is_clean());
    assert_eq!(after.dims[0].texts, vec!["55.00".to_string()]);
    assert_ne!(before.scene, after.scene);
}

#[test]
fn orphan_dims() {
    let c = cfg();
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = Plan::new(&s);
    four_views(&mut p);
    p.add_dim(
        dim(
            1,
            DimKind::Linear,
            vec![face(3, Axis::X, Side::Min), face(3, Axis::X, Side::Max)],
            id(100),
        ),
        &s,
    )
    .unwrap();
    let issues = |d: &FeatureDag| {
        let m = Model {
            dag: Some(d),
            sketches: &[],
        };
        evaluate(&p, &m, &c, &s, Lang::En).unwrap().issues
    };
    assert!(issues(&d).is_empty());
    // the node is built no more: the dimension is reported, not drawn with an old value
    let cavity_box_gone = {
        let mut d2 = hollow("40 mm");
        d2.set_tip(None, &c).unwrap();
        d2.remove(id(4), &c).unwrap();
        d2.remove(id(3), &c).unwrap();
        d2.rebuild(&c).unwrap();
        d2
    };
    let gone = issues(&cavity_box_gone);
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].code, "PHOENIX-CAD-055");
    assert!(gone[0].subject.contains("d1"), "{gone:?}");
    assert!(gone[0].detail.contains("not in the history"), "{gone:?}");
    // a node that failed has no solid
    let mut failing = hollow("40 mm");
    failing.set_expression(id(1), None, "0 mm", &c).unwrap();
    failing.rebuild(&c).unwrap();
    let m = Model {
        dag: Some(&failing),
        sketches: &[],
    };
    let (code, detail) = resolve(&face(2, Axis::X, Side::Min), &m).unwrap_err();
    assert_eq!(code, "PHOENIX-CAD-055");
    assert!(detail.contains("no solid"), "{detail}");
    // a parameter is not a feature, and a project without a history has nothing to tie to
    let (code, _) = resolve(
        &face(1, Axis::X, Side::Min),
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
    )
    .unwrap_err();
    assert_eq!(code, "PHOENIX-CAD-055");
    let (code, _) = resolve(
        &face(2, Axis::X, Side::Min),
        &Model {
            dag: None,
            sketches: &[],
        },
    )
    .unwrap_err();
    assert_eq!(code, "PHOENIX-CAD-055");
}

#[test]
fn bad_dims() {
    let c = cfg();
    let s = sheet();
    let d = hollow("40 mm");
    let st = s.dim.clone();
    let m = Model {
        dag: Some(&d),
        sketches: &[],
    };
    // faces across different axes
    let mixed = dim(
        1,
        DimKind::Linear,
        vec![face(2, Axis::X, Side::Min), face(2, Axis::Y, Side::Max)],
        id(100),
    );
    let (code, _) = measure(&mixed, &m, &st).unwrap_err();
    assert_eq!(code, "PHOENIX-CAD-056");
    // a length along z in the top view is not drawn
    let mut p = Plan::new(&s);
    p.add_view(view(1, ViewKind::Top, false), &s).unwrap();
    p.add_view(view(2, ViewKind::Iso, false), &s).unwrap();
    p.add_dim(
        dim(
            1,
            DimKind::Linear,
            vec![face(2, Axis::Z, Side::Min), face(2, Axis::Z, Side::Max)],
            id(101),
        ),
        &s,
    )
    .unwrap();
    p.add_dim(
        dim(
            2,
            DimKind::Linear,
            vec![face(2, Axis::X, Side::Min), face(2, Axis::X, Side::Max)],
            id(102),
        ),
        &s,
    )
    .unwrap();
    let ev = evaluate(&p, &m, &c, &s, Lang::En).unwrap();
    assert_eq!(ev.issues.len(), 2, "{:?}", ev.issues);
    assert!(ev.issues.iter().all(|i| i.code == "PHOENIX-CAD-056"));
    assert!(ev.issues[0].detail.contains("axis z"), "{:?}", ev.issues);
    assert!(
        ev.issues[1].detail.contains("front, top and right"),
        "{:?}",
        ev.issues
    );
}

#[test]
fn sketch_dims() {
    let c = cfg();
    let s = sheet();
    let sk = plate();
    let (circle, l1, l2) = element_numbers(&sk);
    let sketches = [sk];
    let mut p = Plan::new(&s);
    let mut v = view(1, ViewKind::Sketch, false);
    v.source = Source::Sketch("plate".into());
    p.add_view(v, &s).unwrap();
    let el = |n: u32| Ref::Element {
        sketch: "plate".into(),
        element: n,
    };
    p.add_dim(dim(1, DimKind::Radius, vec![el(circle)], id(101)), &s)
        .unwrap();
    p.add_dim(dim(2, DimKind::Diameter, vec![el(circle)], id(101)), &s)
        .unwrap();
    p.add_dim(dim(3, DimKind::Angle, vec![el(l1), el(l2)], id(101)), &s)
        .unwrap();
    let m = Model {
        dag: None,
        sketches: &sketches,
    };
    let ev = evaluate(&p, &m, &c, &s, Lang::En).unwrap();
    assert!(ev.is_clean(), "{:?}", ev.issues);
    assert_eq!(ev.dims[0].texts, vec!["R5.00".to_string()]);
    assert_eq!(ev.dims[1].texts, vec!["\u{d8}10.00".to_string()]);
    assert_eq!(ev.dims[2].texts, vec!["45.00\u{b0}".to_string()]);
    // the radius follows the sketch
    let mut changed = sketches[0].clone();
    changed
        .set_circle_radius(crate::sketch::ElementId(circle), mm(8))
        .unwrap();
    let after = [changed];
    let ev2 = evaluate(
        &p,
        &Model {
            dag: None,
            sketches: &after,
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert_eq!(ev2.dims[0].texts, vec!["R8.00".to_string()]);
    assert_eq!(ev2.dims[1].texts, vec!["\u{d8}16.00".to_string()]);
    // an element that is gone, and a sketch that is gone, are orphans
    p.add_dim(dim(4, DimKind::Radius, vec![el(9999)], id(101)), &s)
        .unwrap();
    let ev3 = evaluate(&p, &m, &c, &s, Lang::En).unwrap();
    assert_eq!(ev3.issues.len(), 1);
    assert_eq!(ev3.issues[0].code, "PHOENIX-CAD-055");
    let ev4 = evaluate(
        &p,
        &Model {
            dag: None,
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    // the view of the sketch cannot be made, and the dimensions tied to it are orphans
    assert!(ev4.issues.iter().any(|i| i.code == "PHOENIX-CAD-054"));
    assert_eq!(
        ev4.issues
            .iter()
            .filter(|i| i.code == "PHOENIX-CAD-055")
            .count(),
        4
    );
    // a radius of a line, an angle of a circle, parallel lines
    let (code, _) =
        measure(&dim(5, DimKind::Radius, vec![el(l1)], id(101)), &m, &s.dim).unwrap_err();
    assert_eq!(code, "PHOENIX-CAD-056");
    let (code, _) = measure(
        &dim(6, DimKind::Angle, vec![el(circle), el(l1)], id(101)),
        &m,
        &s.dim,
    )
    .unwrap_err();
    assert_eq!(code, "PHOENIX-CAD-056");
    let mut par = plate();
    let a = par.add_point(mm(0), mm(5)).unwrap();
    let b = par.add_point(mm(20), mm(5)).unwrap();
    let l3 = par.add_line(a, b).unwrap().0;
    let pk = [par];
    let pm = Model {
        dag: None,
        sketches: &pk,
    };
    let parallel = dim(7, DimKind::Angle, vec![el(l1), el(l3)], id(101));
    let mut p2 = Plan::new(&s);
    let mut v2 = view(1, ViewKind::Sketch, false);
    v2.source = Source::Sketch("plate".into());
    p2.add_view(v2, &s).unwrap();
    p2.add_dim(parallel, &s).unwrap();
    let ev5 = evaluate(&p2, &pm, &c, &s, Lang::En).unwrap();
    assert!(
        ev5.issues[0].detail.contains("parallel"),
        "{:?}",
        ev5.issues
    );
}

#[test]
fn cumulative() {
    let c = cfg();
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = Plan::new(&s);
    p.add_view(view(1, ViewKind::Front, false), &s).unwrap();
    p.add_dim(
        dim(
            1,
            DimKind::Cumulative,
            vec![
                face(2, Axis::X, Side::Min),
                face(3, Axis::X, Side::Min),
                face(3, Axis::X, Side::Max),
                face(2, Axis::X, Side::Max),
            ],
            id(101),
        ),
        &s,
    )
    .unwrap();
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert!(ev.is_clean(), "{:?}", ev.issues);
    assert_eq!(
        ev.dims[0].texts,
        vec![
            "10.00".to_string(),
            "20.00".to_string(),
            "40.00".to_string()
        ]
    );
    // fewer than three references are refused when the dimension is added
    let two = dim(
        2,
        DimKind::Cumulative,
        vec![face(2, Axis::X, Side::Min), face(2, Axis::X, Side::Max)],
        id(101),
    );
    assert_eq!(p.add_dim(two, &s).unwrap_err().code(), "PHOENIX-CAD-056");
}

// ----- the bill and the balloons -----

fn parts_history() -> FeatureDag {
    let c = cfg();
    let mut d = FeatureDag::new(HistorySettings::defaults());
    d.add_parameter(id(1), "n", "3", &c).unwrap();
    let bx = |d: &mut FeatureDag, n: u16, name: &str, x: &str| {
        d.add_feature(
            id(n),
            name,
            Op::Box,
            &[],
            &[
                ("x", x),
                ("y", "0 mm"),
                ("z", "0 mm"),
                ("sx", "10 mm"),
                ("sy", "10 mm"),
                ("sz", "10 mm"),
            ],
            &c,
        )
        .unwrap();
    };
    bx(&mut d, 2, "pin", "0 mm");
    d.add_feature(
        id(3),
        "row",
        Op::Array,
        &[id(2)],
        &[
            ("count", "n"),
            ("dx", "20 mm"),
            ("dy", "0 mm"),
            ("dz", "0 mm"),
        ],
        &c,
    )
    .unwrap();
    bx(&mut d, 4, "plate", "0 mm");
    d.add_feature(
        id(5),
        "plate_up",
        Op::Move,
        &[id(4)],
        &[("dx", "0 mm"), ("dy", "20 mm"), ("dz", "0 mm")],
        &c,
    )
    .unwrap();
    bx(&mut d, 6, "tool", "2 mm");
    d.add_feature(id(7), "cut", Op::Difference, &[id(5), id(6)], &[], &c)
        .unwrap();
    d.add_feature(id(8), "all", Op::Union, &[id(3), id(7)], &[], &c)
        .unwrap();
    d.rebuild(&c).unwrap();
    d
}

#[test]
fn bom_numbers() {
    let c = cfg();
    let s = sheet();
    let mut d = parts_history();
    let parts = part_nodes(&d, &c, 100).unwrap();
    // the pins three times, the plate once; the tool that is cut away is not a part
    assert_eq!(parts, vec![(id(2), 3), (id(4), 1)]);
    let mut p = Plan::new(&s);
    p.set_part(id(4), "steel", "Plate", &s).unwrap();
    assert!(p.assign_numbers(&parts));
    assert!(!p.assign_numbers(&parts));
    let bom = p.bom(&d, &c, &s).unwrap();
    assert_eq!(bom.len(), 2);
    assert_eq!(
        (
            bom[0].number,
            bom[0].quantity,
            bom[0].description.as_str(),
            bom[0].material.as_str()
        ),
        (1, 3, "pin", "unspecified")
    );
    assert_eq!(
        (
            bom[1].number,
            bom[1].quantity,
            bom[1].description.as_str(),
            bom[1].material.as_str()
        ),
        (2, 1, "Plate", "steel")
    );
    // the count follows the parameter, the numbers stay
    d.set_expression(id(1), None, "5", &c).unwrap();
    d.rebuild(&c).unwrap();
    let bom = p.bom(&d, &c, &s).unwrap();
    assert_eq!((bom[0].number, bom[0].quantity), (1, 5));
    // a new part gets the next number; one that goes away keeps its number, never given again
    d.add_feature(
        id(9),
        "lid",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "20 mm"),
            ("sx", "10 mm"),
            ("sy", "10 mm"),
            ("sz", "2 mm"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(id(10), "top", Op::Union, &[id(8), id(9)], &[], &c)
        .unwrap();
    d.rebuild(&c).unwrap();
    let parts = part_nodes(&d, &c, 100).unwrap();
    p.assign_numbers(&parts);
    assert_eq!(p.numbers[&id(9)], 3);
    d.set_tip(Some(id(3)), &c).unwrap();
    d.rebuild(&c).unwrap();
    let only_pins = part_nodes(&d, &c, 100).unwrap();
    assert_eq!(only_pins, vec![(id(2), 5)]);
    p.assign_numbers(&only_pins);
    d.set_tip(None, &c).unwrap();
    d.add_feature(
        id(11),
        "extra",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "0 mm"),
            ("sx", "1 mm"),
            ("sy", "1 mm"),
            ("sz", "1 mm"),
        ],
        &c,
    )
    .unwrap();
    let all = part_nodes(&d, &c, 100).unwrap();
    p.assign_numbers(&all);
    assert_eq!(p.numbers[&id(11)], 4, "{:?}", p.numbers);
    assert_eq!(p.numbers[&id(4)], 2);
    // the limit of the settings
    assert!(part_nodes(&d, &c, 1).is_err());
    // the file keeps the numbers
    let back = Plan::from_bytes(&p.to_bytes(), &s).unwrap();
    assert_eq!(back.numbers, p.numbers);
    assert_eq!(back.next_item, 5);
}

#[test]
fn bom_csv() {
    let c = cfg();
    let s = sheet();
    let d = parts_history();
    let mut p = Plan::new(&s);
    p.set_part(id(2), "=cmd|' /C calc'!A0", "Pin, \"M3\"", &s)
        .unwrap();
    let csv = to_csv(&p.bom(&d, &c, &s).unwrap());
    assert!(
        csv.starts_with(
            "item,quantity,description,material\n1,3,\"Pin, \"\"M3\"\"\",'=cmd|' /C calc'!A0\n"
        ),
        "{csv}"
    );
}

#[test]
fn balloons() {
    let c = cfg();
    let s = sheet();
    let d = parts_history();
    let mut p = Plan::new(&s);
    four_views(&mut p);
    for (n, part) in [(1u16, id(2)), (2, id(4)), (3, id(6))] {
        p.add_balloon(
            Balloon {
                id: id(300 + n),
                name: format!("b{n}"),
                part,
                at: (30_000 + i64::from(n) * 12_000, 30_000),
                to: (60_000, 60_000),
                extra: Vec::new(),
            },
            &s,
        )
        .unwrap();
    }
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    // the tool is not a part, so its balloon is an orphan; the other two carry their numbers
    assert_eq!(ev.issues.len(), 1, "{:?}", ev.issues);
    assert_eq!(ev.issues[0].code, "PHOENIX-CAD-057");
    assert_eq!(
        ev.balloons.iter().map(|b| b.number).collect::<Vec<_>>(),
        vec![Some(1), Some(2), None]
    );
    let texts: Vec<String> = ev
        .scene
        .prims
        .iter()
        .filter(|p| p.layer == "BALLOON")
        .filter_map(|p| match &p.shape {
            super::prim::Shape::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["1".to_string(), "2".to_string()]);
}

// ----- the sheet and its files -----

fn sheet_with_dims(d: &FeatureDag, s: &SheetSettings) -> Plan {
    let mut p = Plan::new(s);
    four_views(&mut p);
    p.add_dim(
        dim(
            1,
            DimKind::Linear,
            vec![face(2, Axis::X, Side::Min), face(2, Axis::X, Side::Max)],
            id(100),
        ),
        s,
    )
    .unwrap();
    p.add_dim(
        dim(
            2,
            DimKind::Linear,
            vec![face(2, Axis::Z, Side::Min), face(2, Axis::Z, Side::Max)],
            id(100),
        ),
        s,
    )
    .unwrap();
    p.set_field("title", "Housing & <lid>", s).unwrap();
    p.set_field("author", "A. Author", s).unwrap();
    let _ = d;
    p
}

#[test]
fn views_align() {
    let c = cfg();
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = sheet_with_dims(&d, &s);
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert!(ev.is_clean(), "{:?}", ev.issues);
    let by = |name: &str| ev.views.iter().find(|v| v.name == name).unwrap().clone();
    let (f, t, r, i) = (by("front"), by("top"), by("right"), by("iso"));
    // third angle: the top over the front with the same left edge, the right beside it with the same bottom
    assert_eq!(t.origin.0, f.origin.0);
    assert_eq!(t.origin.1, f.origin.1 + f.size.1 + s.view_gap_um);
    assert_eq!(r.origin.1, f.origin.1);
    assert_eq!(r.origin.0, f.origin.0 + f.size.0 + s.view_gap_um);
    assert_eq!((i.origin.0, i.origin.1), (r.origin.0, t.origin.1));
    // first angle: the top under the front and the right to the left of it
    p.third_angle = false;
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    let by = |name: &str| ev.views.iter().find(|v| v.name == name).unwrap().clone();
    let (f, t, r) = (by("front"), by("top"), by("right"));
    assert_eq!(t.origin.0, f.origin.0);
    assert_eq!(t.origin.1 + t.size.1 + s.view_gap_um, f.origin.1);
    assert_eq!(r.origin.0 + r.size.0 + s.view_gap_um, f.origin.0);
    // a view put by hand stays where it was put
    p.views[3].place = Place::At(20_000, 20_000);
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    let iso = ev.views.iter().find(|v| v.name == "iso").unwrap();
    assert_eq!(iso.origin, (20_000, 20_000));
}

#[test]
fn scales() {
    let c = cfg();
    let s = sheet();
    // a plate of 150 mm fits at 1:1 on an A4 sheet with its views; one of 400 mm needs 1:2 or less
    let small = hollow("150 mm");
    let mut p = Plan::new(&s);
    p.scale = SheetScale::Auto;
    p.add_view(view(0, ViewKind::Front, false), &s).unwrap();
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&small),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert_eq!(ev.scale, (1, 1));
    let big = hollow("400 mm");
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&big),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert_eq!(ev.scale, (1, 2));
    assert!(ev.is_clean(), "{:?}", ev.issues);
    // a fixed scale that does not fit says so
    p.scale = SheetScale::Ratio(1, 1);
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&big),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert_eq!(ev.issues.len(), 1);
    assert_eq!(ev.issues[0].code, "PHOENIX-CAD-054");
    assert!(
        ev.issues[0].detail.contains("not inside the frame"),
        "{:?}",
        ev.issues
    );
    // a view with a scale of its own
    p.scale = SheetScale::Ratio(1, 2);
    p.views[0].scale = Some((1, 4));
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&big),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert_eq!(ev.views[0].scale, (1, 4));
    // views of a node that is gone, or of a history that is not there
    p.views[0].source = Source::Node(id(77));
    let ev = evaluate(
        &p,
        &Model {
            dag: Some(&big),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert_eq!(ev.issues[0].code, "PHOENIX-CAD-054");
    let ev = evaluate(
        &p,
        &Model {
            dag: None,
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    assert!(ev.issues[0].detail.contains("no history"));
}

#[test]
fn formats() {
    let c = cfg();
    let d = hollow("40 mm");
    let s = SheetSettings::with_user_config("[sheet]\nformat.B5 = 176 mm x 250 mm\n").unwrap();
    for (name, portrait, w, h) in [
        ("A4", false, 297_000, 210_000),
        ("A3", true, 297_000, 420_000),
        ("B5", false, 250_000, 176_000),
    ] {
        let mut p = Plan::new(&s);
        p.format = name.to_string();
        p.portrait = portrait;
        p.add_view(view(0, ViewKind::Front, false), &s).unwrap();
        let ev = evaluate(
            &p,
            &Model {
                dag: Some(&d),
                sketches: &[],
            },
            &c,
            &s,
            Lang::En,
        )
        .unwrap();
        assert_eq!((ev.scene.width_um, ev.scene.height_um), (w, h), "{name}");
        assert!(ev.is_clean());
    }
    let mut p = Plan::new(&s);
    p.format = "A9".into();
    assert_eq!(
        evaluate(
            &p,
            &Model {
                dag: Some(&d),
                sketches: &[]
            },
            &c,
            &s,
            Lang::En
        )
        .unwrap_err()
        .code(),
        "PHOENIX-CAD-054"
    );
}

#[test]
fn svg_file() {
    let c = cfg();
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = sheet_with_dims(&d, &s);
    p.set_part(id(2), "aluminium", "Base", &s).unwrap();
    let ev = |p: &Plan, lang| {
        evaluate(
            p,
            &Model {
                dag: Some(&d),
                sketches: &[],
            },
            &c,
            &s,
            lang,
        )
        .unwrap()
    };
    let e1 = ev(&p, Lang::En);
    let svg = out::to_svg(&e1.scene, "p.phoenixproj");
    assert_eq!(
        svg,
        out::to_svg(&ev(&p, Lang::En).scene, "p.phoenixproj"),
        "the same sheet is the same text"
    );
    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"297mm\" height=\"210mm\" viewBox=\"0 0 297 210\">"));
    assert!(svg.contains("<g id=\"layer-OUTLINE\"") && svg.contains("<g id=\"layer-HIDDEN\""));
    assert!(svg.contains("stroke-dasharray"));
    assert!(
        svg.contains("Housing &amp; &lt;lid&gt;"),
        "the text is escaped"
    );
    assert!(svg.contains(">40.00<") && svg.contains(">10.00<"));
    assert!(svg.contains(">Scale<") && svg.contains(">Drawing number<"));
    assert!(svg.contains(">aluminium<") && svg.contains(">Base<"));
    assert!(!svg.contains("NaN") && !svg.contains("inf"));
    // the title block speaks the language of the window
    let de = out::to_svg(&ev(&p, Lang::De).scene, "p");
    assert!(de.contains(">Maßstab<") && de.contains(">Zeichnungsnummer<"));
    let ja = out::to_svg(&ev(&p, Lang::Ja).scene, "p");
    assert!(ja.contains(">縮尺<"));
    // a layer that is hidden is not in the file, and the colour and the width follow the drawing
    p.set_layer_visible("HIDDEN", false, &s).unwrap();
    p.set_layer_look("OUTLINE", "0000ff", 500, &s).unwrap();
    let svg2 = out::to_svg(&ev(&p, Lang::En).scene, "p");
    assert!(!svg2.contains("layer-HIDDEN"));
    assert!(svg2
        .contains("<g id=\"layer-OUTLINE\" fill=\"none\" stroke=\"#0000ff\" stroke-width=\"0.5\""));
}

#[test]
fn pdf_dxf() {
    let c = cfg();
    let s = sheet();
    let d = hollow("40 mm");
    let p = sheet_with_dims(&d, &s);
    let e = evaluate(
        &p,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    let pdf = out::to_pdf(&e.scene, "p.phoenixproj");
    let (w, h, _) = crate::pdf::check(&pdf).expect("a well formed page");
    assert!(
        (w - 297.0 * crate::pdf::POINTS_PER_MM).abs() < 0.01
            && (h - 210.0 * crate::pdf::POINTS_PER_MM).abs() < 0.01
    );
    assert_eq!(pdf, out::to_pdf(&e.scene, "p.phoenixproj"));
    let dxf = out::to_dxf(&e.scene);
    assert!(dxf.starts_with("0\nSECTION\n2\nHEADER\n"));
    assert!(dxf.ends_with("0\nEOF\n"));
    for word in [
        "LTYPE",
        "DASHED",
        "LAYER",
        "OUTLINE",
        "HIDDEN",
        "DIMENSION",
        "FRAME",
        "LINE",
        "TEXT",
        "ENTITIES",
    ] {
        assert!(dxf.contains(&format!("\n{word}\n")), "{word}");
    }
    assert_eq!(dxf, out::to_dxf(&e.scene));
    // the arcs and circles of a sketch are in it as such
    let sk = [plate()];
    let mut p = Plan::new(&s);
    let mut v = view(1, ViewKind::Sketch, false);
    v.source = Source::Sketch("plate".into());
    p.add_view(v, &s).unwrap();
    let e = evaluate(
        &p,
        &Model {
            dag: None,
            sketches: &sk,
        },
        &c,
        &s,
        Lang::En,
    )
    .unwrap();
    let dxf = out::to_dxf(&e.scene);
    assert!(dxf.contains("\nCIRCLE\n") && dxf.contains("\nCENTER\n"));
    let svg = out::to_svg(&e.scene, "p");
    assert!(svg.contains("<circle ") && svg.contains("layer-CENTER"));
}

// ----- the panel -----

fn project_bytes(d: &FeatureDag, p: Option<&Plan>) -> Vec<u8> {
    let mut doc = Document::new();
    doc.set_history(d).unwrap();
    if let Some(p) = p {
        doc.set_plan(p).unwrap();
    }
    doc.to_bytes()
}

#[test]
fn panel_flow() {
    let d = hollow("40 mm");
    let mut st = PlanState::with_defaults();
    // no project, then a project with no sheet
    assert!(st
        .lines(Lang::En)
        .head
        .unwrap()
        .text
        .contains("Open a project"));
    st.load_bytes(
        &project_bytes(&d, None),
        Some(std::path::PathBuf::from("p.phoenixproj")),
        Lang::En,
    )
    .unwrap();
    assert!(st
        .lines(Lang::En)
        .head
        .unwrap()
        .text
        .contains("no drawing sheet"));
    assert!(!st.has_sheet());
    // the sheet is made from the history
    st.create_sheet(Lang::En).unwrap();
    assert!(st.dirty && st.has_sheet());
    let l = st.lines(Lang::En);
    assert_eq!(
        l.head.as_ref().unwrap().text,
        "Sheet A4 landscape, scale 1:1: 4 views, 0 dimensions, 0 balloons"
    );
    assert_eq!(l.views.len(), 4);
    assert!(
        l.views[0]
            .text
            .starts_with("View front (front): 4 visible lines, 4 hidden lines"),
        "{:?}",
        l.views
    );
    assert_eq!(l.layers.len(), 7);
    assert!(l.issues.is_empty());
    assert_eq!(
        st.lines(Lang::Es).head.unwrap().text,
        "Hoja A4 horizontal, escala 1:1: 4 vistas, 0 cotas, 0 globos"
    );
    assert_eq!(st.lines(Lang::Pt).views[0].tone, Tone::Plain);
    // the layers are shown, hidden and locked from the panel
    st.toggle_visible("HIDDEN", Lang::En).unwrap();
    st.toggle_locked("DIMENSION", Lang::En).unwrap();
    let l = st.lines(Lang::En);
    let hidden = l.layers.iter().find(|x| x.name == "HIDDEN").unwrap();
    assert!(!hidden.visible && hidden.text.contains("hidden"));
    let dims = l.layers.iter().find(|x| x.name == "DIMENSION").unwrap();
    assert!(dims.locked && dims.text.contains("locked"));
    assert!(!st
        .eval
        .as_ref()
        .unwrap()
        .scene
        .prims
        .iter()
        .any(|p| p.layer == "HIDDEN"));
    // a project with no history has nothing to make a sheet of
    let mut empty = PlanState::with_defaults();
    empty
        .load_bytes(&Document::new().to_bytes(), None, Lang::En)
        .unwrap();
    assert_eq!(
        empty.create_sheet(Lang::En).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
}

static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cad21_plans_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn panel_save() {
    let d = hollow("40 mm");
    let dir = temp_dir();
    let path = dir.join("p.phoenixproj");
    let original = project_bytes(&d, None);
    std::fs::write(&path, &original).unwrap();
    let mut st = PlanState::with_defaults();
    st.load(&path, Lang::En).unwrap();
    st.create_sheet(Lang::En).unwrap();
    st.toggle_visible("CENTER", Lang::En).unwrap();
    st.save(Lang::En).unwrap();
    assert!(!st.dirty);
    assert!(st
        .note
        .as_ref()
        .unwrap()
        .text
        .starts_with("Drawing kept in"));
    // the file has the sheet and the history is as it was
    let doc = Document::from_bytes(&std::fs::read(&path).unwrap(), 1 << 24).unwrap();
    assert_eq!(
        doc.get("history").unwrap().data,
        Document::from_bytes(&original, 1 << 24)
            .unwrap()
            .get("history")
            .unwrap()
            .data
    );
    let plan = doc.plan(&sheet()).unwrap().unwrap();
    assert_eq!(plan.views.len(), 4);
    assert!(!plan.layer_visible("CENTER", &sheet()));
    // the items of the bill have their numbers from now on
    assert_eq!(plan.numbers.len(), 1);
    // the same file opened again gives the same sheet, and an SVG is written once and not over a file
    let mut again = PlanState::with_defaults();
    again.load(&path, Lang::En).unwrap();
    assert_eq!(again.plan, Some(plan));
    let svg = dir.join("p.svg");
    again.export_svg(&svg, false, Lang::En).unwrap();
    assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));
    assert_eq!(
        again.export_svg(&svg, false, Lang::En).unwrap_err().code(),
        "PHOENIX-CAD-040"
    );
    again.export_svg(&svg, true, Lang::En).unwrap();
    // the sheet is worked out again when the model changes in the file
    let mut grown = hollow("40 mm");
    grown.set_expression(id(1), None, "70 mm", &cfg()).unwrap();
    let mut doc = Document::from_bytes(&std::fs::read(&path).unwrap(), 1 << 24).unwrap();
    doc.set_history(&grown).unwrap();
    std::fs::write(&path, doc.to_bytes()).unwrap();
    let before = again.eval.clone();
    again.refresh(Lang::En).unwrap();
    assert_ne!(before, again.eval);
    // no file, nothing to keep
    let mut loose = PlanState::with_defaults();
    loose
        .load_bytes(&project_bytes(&d, None), None, Lang::En)
        .unwrap();
    loose.create_sheet(Lang::En).unwrap();
    assert!(loose.save(Lang::En).is_err());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn panel_lines() {
    let c = cfg();
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = sheet_with_dims(&d, &s);
    p.add_dim(
        dim(
            9,
            DimKind::Linear,
            vec![face(77, Axis::X, Side::Min), face(77, Axis::X, Side::Max)],
            id(100),
        ),
        &s,
    )
    .unwrap();
    let e = evaluate(
        &p,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &c,
        &s,
        Lang::De,
    )
    .unwrap();
    let l = lines_of(&p, &e, &s, Lang::De);
    assert_eq!(l.dims.len(), 2);
    assert!(l.dims[0].text.starts_with("Maß d1: 40.00"), "{:?}", l.dims);
    assert_eq!(l.issues.len(), 1);
    assert_eq!(l.issues[0].tone, Tone::Bad);
    assert!(
        l.issues[0]
            .text
            .contains("PHOENIX-CAD-055 Maß ohne das Element, das es misst"),
        "{}",
        l.issues[0].text
    );
    for lang in Lang::ALL {
        assert!(!lines_of(&p, &e, &s, lang).issues[0].text.is_empty());
    }
}

// ----- fuzz with a fixed seed -----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..1 + rng.below(4) {
        if v.is_empty() {
            break;
        }
        let at = rng.below(v.len());
        match rng.below(5) {
            0 => v[at] = (rng.next() & 0xff) as u8,
            1 => {
                v.remove(at);
            }
            2 => v.insert(at, b" \n-x0123456789abcdef"[rng.below(20)]),
            3 => v.truncate(at),
            _ => {
                let end = (at + rng.below(32)).min(v.len());
                let piece: Vec<u8> = v[at..end].to_vec();
                v.splice(at..at, piece);
            }
        }
    }
    v
}

#[test]
fn fuzz_reader() {
    let s = sheet();
    let base = full_plan().to_bytes();
    let mut rng = Rng(0x00c0_ffee);
    let mut accepted = 0;
    for _ in 0..4000 {
        let input = mutate(&mut rng, &base);
        // the reader never panics; what it accepts is read back to the same bytes
        if let Ok(p) = Plan::from_bytes(&input, &s) {
            assert_eq!(p.to_bytes(), input);
            accepted += 1;
        }
    }
    assert!(accepted < 4000);
    // any bytes at all
    for _ in 0..500 {
        let n = rng.below(200);
        let junk: Vec<u8> = (0..n).map(|_| (rng.next() & 0xff) as u8).collect();
        assert!(Plan::from_bytes(&junk, &s).is_err());
        let mut text = b"drawing 1\n".to_vec();
        text.extend(&junk);
        let _ = Plan::from_bytes(&text, &s);
    }
}

#[test]
fn fuzz_config() {
    let base = crate::plans::settings::default_file().as_bytes().to_vec();
    let mut rng = Rng(0x0bad_cafe);
    let mut ok = 0;
    for _ in 0..1500 {
        let input = mutate(&mut rng, &base);
        let text = String::from_utf8_lossy(&input).into_owned();
        if let Ok(s) = SheetSettings::with_user_config(&text) {
            // what is accepted is consistent
            assert!(s.formats.contains_key(&s.default_format));
            assert!(s.layer("OUTLINE").is_some() && !s.scales.is_empty());
            assert!(s.dim.decimals <= 6);
            ok += 1;
        }
    }
    assert!(ok > 0);
}
