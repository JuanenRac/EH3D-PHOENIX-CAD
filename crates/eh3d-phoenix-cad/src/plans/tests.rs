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
        section: None,
        detail: None,
        hatch: None,
        brk: None,
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
        step_um: None,
        tol: None,
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
    assert_eq!(l.layers.len(), 12);
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

// ----- the second set of drawings: sections, details, tolerances, extended dimensions, revisions -----

use super::extended::{place as place_dims, DimLine, Slot};
use super::prim::Shape;
use super::scene::Evaluation;
use super::sectioning::{
    clip_solid, clip_to_circle, clip_to_rect, cut_face, hatch_lines, keeps_high,
};
use super::settings::{HatchStyle, RevisionNumbering, TolDisplay};
use super::tol::{annotate, apply, check, pair_text};

fn run(p: &Plan, d: &FeatureDag) -> Evaluation {
    evaluate(
        p,
        &Model {
            dag: Some(d),
            sketches: &[],
        },
        &cfg(),
        &sheet(),
        Lang::En,
    )
    .unwrap()
}

fn count_layer(ev: &Evaluation, layer: &str) -> usize {
    ev.scene.prims.iter().filter(|p| p.layer == layer).count()
}

fn has_text(ev: &Evaluation, wanted: &str) -> bool {
    ev.scene
        .prims
        .iter()
        .any(|p| matches!(&p.shape, Shape::Text { text, .. } if text == wanted))
}

fn section_of(n: u16, kind: ViewKind, parent: u16, at_mm: i64, label: &str) -> View {
    let mut v = View::plain(id(110 + n), label, kind, Source::Tip, false);
    v.section = Some(SectionCut {
        parent: id(parent),
        at_nm: at_mm * 1_000_000,
        label: label.to_string(),
    });
    v
}

fn code(e: crate::CadError) -> &'static str {
    e.code()
}

#[test]
fn cut_clips() {
    let d = hollow("40 mm");
    let solid = d.result().unwrap();
    let lim = Limits::default();
    // the right view keeps what lies below the cut in x, the front what lies above it in y
    let right = clip_solid(solid, ViewKind::Right, 15_000_000, &lim).unwrap();
    let (lo, hi) = right.bounds().unwrap();
    assert_eq!((lo[0], hi[0]), (0, 15_000_000));
    let front = clip_solid(solid, ViewKind::Front, 5_000_000, &lim).unwrap();
    let (lo, hi) = front.bounds().unwrap();
    assert_eq!((lo[1], hi[1]), (5_000_000, 20_000_000));
    assert!(keeps_high(ViewKind::Front) && !keeps_high(ViewKind::Top));
    // the cut face is the ring round the cavity: its area is the section of the box less the cavity
    let faces = cut_face(&right, ViewKind::Right, 15_000_000);
    let area: i128 = faces
        .iter()
        .map(|((a, b), (c, d))| i128::from(c - a) * i128::from(d - b))
        .sum();
    assert_eq!(area, (20 * 10 - 10 * 6) * 1_000_000_000_000);
    // a plane beyond the solid cuts nothing, and a view along the iso axis has no section
    let beyond = clip_solid(solid, ViewKind::Right, 90_000_000, &lim).unwrap();
    assert!(cut_face(&beyond, ViewKind::Right, 90_000_000).is_empty());
    assert!(clip_solid(solid, ViewKind::Iso, 0, &lim).is_err());
}

#[test]
fn hatch_joins() {
    let two = [((0, 0), (5000, 10_000)), ((5000, 0), (10_000, 10_000))];
    let style = |a| HatchStyle {
        angle_deg: a,
        spacing_um: 1000,
    };
    // horizontal lines run across both rectangles as one line each
    let h = hatch_lines(&two, style(0), 1000).unwrap();
    assert_eq!(h.len(), 11);
    assert!(h
        .iter()
        .all(|(a, b)| a.0 == 0 && b.0 == 10_000 && a.1 == b.1));
    // vertical lines: the shared edge is one line
    let v = hatch_lines(&two, style(90), 1000).unwrap();
    assert_eq!(v.len(), 11);
    // slanted lines stay inside the box and are the same twice
    let a = hatch_lines(&two, style(45), 1000).unwrap();
    assert!(a.len() >= 15 && a.len() <= 25, "{}", a.len());
    for (p, q) in &a {
        for r in [p, q] {
            assert!(
                (0..=10_000).contains(&r.0) && (0..=10_000).contains(&r.1),
                "{r:?}"
            );
        }
    }
    assert_eq!(a, hatch_lines(&two, style(45), 1000).unwrap());
    // a hole in the middle breaks the lines it crosses
    let ring = [
        ((0, 0), (10_000, 4000)),
        ((0, 6000), (10_000, 10_000)),
        ((0, 4000), (4000, 6000)),
        ((6000, 4000), (10_000, 6000)),
    ];
    let r = hatch_lines(&ring, style(90), 1000).unwrap();
    assert!(r.len() > 11, "{}", r.len());
    // too many lines is refused with the count
    assert!(hatch_lines(&two, style(45), 5).is_err());
}

#[test]
fn clip_shapes() {
    let c = (0, 0);
    // through the centre: the diameter; outside: nothing; inside: whole
    assert_eq!(
        clip_to_circle((-10, 0), (10, 0), c, 5),
        Some(((-5, 0), (5, 0)))
    );
    assert_eq!(clip_to_circle((-10, 9), (10, 9), c, 5), None);
    assert_eq!(
        clip_to_circle((-1, 0), (1, 0), c, 5),
        Some(((-1, 0), (1, 0)))
    );
    assert_eq!(clip_to_circle((3, 3), (3, 3), c, 5), None);
    let rect = ((0, 0), (10, 10));
    assert_eq!(
        clip_to_rect((-5, 5), (15, 5), rect),
        Some(((0, 5), (10, 5)))
    );
    assert_eq!(clip_to_rect((-5, 11), (15, 11), rect), None);
    assert_eq!(clip_to_rect((2, 2), (4, 4), rect), Some(((2, 2), (4, 4))));
    assert_eq!(
        clip_to_rect((-5, -5), (15, 15), rect),
        Some(((0, 0), (10, 10)))
    );
}

#[test]
fn section_view() {
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = Plan::new(&s);
    four_views(&mut p);
    p.add_view(section_of(0, ViewKind::Right, 100, 15, "A"), &s)
        .unwrap();
    let ev = run(&p, &d);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    // the hatching, the line of the cut with its letters at both ends, and the caption
    assert!(count_layer(&ev, "HATCH") > 5);
    assert!(count_layer(&ev, "SECTION") >= 1);
    assert!(has_text(&ev, "A-A"));
    let letters = ev
        .scene
        .prims
        .iter()
        .filter(|p| matches!(&p.shape, Shape::Text { text, .. } if text == "A"))
        .count();
    // the two ends of the cut line and the value of the field revision of the title block
    assert_eq!(letters, 3);
    // the section is not the right view of the projection: the right view is still beside the front one
    let right = ev.views.iter().find(|v| v.name == "right").unwrap();
    let front = ev.views.iter().find(|v| v.name == "front").unwrap();
    assert_eq!(
        right.origin.0,
        front.origin.0 + front.size.0 + s.view_gap_um
    );
    // no hatching line enters the cavity (y 5 to 15 mm, z 2 to 8 mm of the section view)
    let sec = ev.views.iter().find(|v| v.name == "A").unwrap();
    let (n, dd) = (i64::from(sec.scale.0), i64::from(sec.scale.1));
    let at = |y_mm: i64, z_mm: i64| {
        (
            sec.origin.0 + y_mm * 1000 * n / dd,
            sec.origin.1 + z_mm * 1000 * n / dd,
        )
    };
    let (lo, hi) = (at(5, 2), at(15, 8));
    for prim in ev.scene.prims.iter().filter(|p| p.layer == "HATCH") {
        if let Shape::Line { a, b } = prim.shape {
            let mid = ((a.0 + b.0) / 2, (a.1 + b.1) / 2);
            let inside =
                mid.0 > lo.0 + 2 && mid.0 < hi.0 - 2 && mid.1 > lo.1 + 2 && mid.1 < hi.1 - 2;
            assert!(!inside, "{a:?} {b:?}");
        }
    }
    // the hatching follows the material: a part of aluminium goes the other way
    let mut q = p.clone();
    q.views.last_mut().unwrap().hatch = Some("aluminium".to_string());
    let ev2 = run(&q, &d);
    let slope = |ev: &Evaluation| {
        ev.scene
            .prims
            .iter()
            .find_map(|p| match (&p.layer[..], &p.shape) {
                ("HATCH", Shape::Line { a, b }) => {
                    Some((b.0 - a.0).signum() * (b.1 - a.1).signum())
                }
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(slope(&ev), 1);
    assert_eq!(slope(&ev2), -1);
    // the same sheet gives the same list
    assert_eq!(ev.scene, run(&p, &d).scene);
}

#[test]
fn section_marks() {
    // a section of the front view, cut across y, drawn in the top view as a horizontal line
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = Plan::new(&s);
    four_views(&mut p);
    p.add_view(section_of(0, ViewKind::Front, 101, 10, "B"), &s)
        .unwrap();
    let ev = run(&p, &d);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    let top = ev.views.iter().find(|v| v.name == "top").unwrap();
    let line = ev
        .scene
        .prims
        .iter()
        .find_map(|p| match (&p.layer[..], &p.shape) {
            ("SECTION", Shape::Line { a, b }) => Some((*a, *b)),
            _ => None,
        })
        .unwrap();
    // horizontal, 10 mm up the top view at the scale of the sheet, a little longer than the view
    assert_eq!(line.0 .1, line.1 .1);
    let (n, dd) = (i64::from(ev.scale.0), i64::from(ev.scale.1));
    assert_eq!(line.0 .1, top.origin.1 + 10_000 * n / dd);
    assert_eq!(
        line.1 .0 - line.0 .0,
        top.size.0 + 2 * s.section.overshoot_um
    );
    // a cut across y cannot be drawn in the front view, which looks along y
    let mut q = Plan::new(&s);
    four_views(&mut q);
    q.add_view(section_of(0, ViewKind::Front, 100, 10, "B"), &s)
        .unwrap();
    let ev = run(&q, &d);
    assert!(ev
        .issues
        .iter()
        .any(|i| i.code == "PHOENIX-CAD-059" && i.detail.contains("does not show the axis")));
}

#[test]
fn section_bad() {
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = Plan::new(&s);
    four_views(&mut p);
    // a plane that misses the solid is reported and the rest of the sheet is drawn
    p.add_view(section_of(0, ViewKind::Right, 100, 90, "A"), &s)
        .unwrap();
    let ev = run(&p, &d);
    assert_eq!(ev.issues.len(), 1);
    assert_eq!(ev.issues[0].code, "PHOENIX-CAD-059");
    assert_eq!(ev.views.len(), 4);
    // the file refuses what cannot be a section
    let mut q = Plan::new(&s);
    four_views(&mut q);
    q.add_view(section_of(0, ViewKind::Right, 100, 15, "A"), &s)
        .unwrap();
    for bad in [
        section_of(1, ViewKind::Right, 100, 15, "A"),
        section_of(1, ViewKind::Right, 999, 15, "C"),
        section_of(1, ViewKind::Iso, 100, 15, "C"),
        section_of(1, ViewKind::Right, 100, 15, "c1"),
        section_of(1, ViewKind::Right, 100, 15, "ABCD"),
    ] {
        assert_eq!(code(q.add_view(bad, &s).unwrap_err()), "PHOENIX-CAD-059");
    }
    let mut hatched = view(9, ViewKind::Front, false);
    hatched.hatch = Some("steel".to_string());
    assert_eq!(
        code(q.add_view(hatched, &s).unwrap_err()),
        "PHOENIX-CAD-059"
    );
    // a section is not also a detail
    let mut both = section_of(1, ViewKind::Right, 100, 15, "D");
    both.detail = Some(Detail {
        parent: id(100),
        label: "E".into(),
        centre: (0, 0),
        shape: DetailShape::Circle { radius_nm: 1 },
    });
    assert_eq!(code(q.add_view(both, &s).unwrap_err()), "PHOENIX-CAD-059");
    // the letters are given in order, without the ones that look like digits
    assert_eq!(next_label(&q), "B");
    for k in 0..30u16 {
        let l = next_label(&q);
        assert!(!l.contains(['I', 'O', 'Q']), "{l}");
        let mut v = section_of(20 + k, ViewKind::Top, 100, 5, &l);
        v.id = id(400 + k);
        if q.add_view(v, &s).is_err() {
            break;
        }
    }
}

fn detail_plan(s: &SheetSettings) -> Plan {
    let mut p = Plan::new(s);
    four_views(&mut p);
    let mut det = View::plain(id(120), "detail", ViewKind::Front, Source::Tip, false);
    det.scale = Some((4, 1));
    det.detail = Some(Detail {
        parent: id(100),
        label: "B".into(),
        centre: (10_000_000, 0),
        shape: DetailShape::Circle {
            radius_nm: 3_000_000,
        },
    });
    p.add_view(det, s).unwrap();
    p
}

#[test]
fn detail_view() {
    let s = sheet();
    let d = plain_box();
    let p = detail_plan(&s);
    let ev = run(&p, &d);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    // two edges of the corner are inside the circle; the circle is drawn in the view it was taken
    // from and again as the border of the detail
    let det = ev.views.iter().find(|v| v.name == "detail").unwrap();
    assert_eq!((det.visible, det.hidden), (2, 0));
    assert_eq!(det.scale, (4, 1));
    let circles = ev
        .scene
        .prims
        .iter()
        .filter(|p| p.layer == "DETAIL" && matches!(p.shape, Shape::Circle { .. }))
        .count();
    assert_eq!(circles, 2);
    assert!(has_text(&ev, "B (4:1)"));
    // a rectangle does the same with lines
    let mut q = p.clone();
    q.views.last_mut().unwrap().detail.as_mut().unwrap().shape = DetailShape::Rect {
        width_nm: 6_000_000,
        height_nm: 6_000_000,
    };
    let ev = run(&q, &d);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    assert_eq!(count_layer(&ev, "DETAIL"), 8);
    // a shape with nothing of the view in it is a problem of the detail
    let mut r = p.clone();
    r.views.last_mut().unwrap().detail.as_mut().unwrap().centre = (5_000_000, 5_000_000);
    let ev = run(&r, &d);
    assert!(ev
        .issues
        .iter()
        .any(|i| i.code == "PHOENIX-CAD-059" && i.detail.contains("nothing of the view")));
    // the enlarged view comes first, and has the same kind
    let mut bad = detail_plan(&s);
    let mut v = bad.views.pop().unwrap();
    v.kind = ViewKind::Top;
    assert_eq!(code(bad.add_view(v, &s).unwrap_err()), "PHOENIX-CAD-059");
    let mut early = Plan::new(&s);
    let mut v = detail_plan(&s).views.pop().unwrap();
    v.id = id(130);
    assert_eq!(code(early.add_view(v, &s).unwrap_err()), "PHOENIX-CAD-059");
    // the derived views stay where the person puts them
    let mut placed = detail_plan(&s);
    placed.views.last_mut().unwrap().place = Place::At(20_000, 20_000);
    let ev = run(&placed, &d);
    let det = ev.views.iter().find(|v| v.name == "detail").unwrap();
    assert_eq!(det.origin, (20_000, 20_000));
}

fn rich_plan() -> Plan {
    let s = sheet();
    let mut p = full_plan();
    p.add_view(section_of(0, ViewKind::Right, 100, 15, "A"), &s)
        .unwrap();
    p.views.last_mut().unwrap().hatch = Some("steel".into());
    let mut det = View::plain(id(120), "detail", ViewKind::Front, Source::Tip, true);
    det.scale = Some((4, 1));
    det.detail = Some(Detail {
        parent: id(100),
        label: "B".into(),
        centre: (10_000_000, -5),
        shape: DetailShape::Rect {
            width_nm: 6_000_000,
            height_nm: 3_000_000,
        },
    });
    p.add_view(det, &s).unwrap();
    p.set_dim_tol(id(201), Some(Tol::Class("H7/g6".into())), &s)
        .unwrap();
    p.dims[0].step_um = Some(9000);
    p.add_dim(
        dim(
            5,
            DimKind::Chain,
            vec![
                face(2, Axis::X, Side::Min),
                face(3, Axis::X, Side::Min),
                face(3, Axis::X, Side::Max),
            ],
            id(100),
        ),
        &s,
    )
    .unwrap();
    p.dims[1].tol = Some(Tol::Dev {
        upper_nm: 100_000,
        lower_nm: -20_000,
    });
    p.add_dim(
        dim(
            6,
            DimKind::Ordinate,
            vec![
                face(2, Axis::Z, Side::Min),
                face(3, Axis::Z, Side::Min),
                face(3, Axis::Z, Side::Max),
            ],
            id(100),
        ),
        &s,
    )
    .unwrap();
    p.add_revision("2026-01-05", "First issue", "A. Author", &s)
        .unwrap();
    p.add_revision("", "Hole moved", "", &s).unwrap();
    p.set_title_template(Some("compact"), &s).unwrap();
    p.set_line_group(Some(500), &s).unwrap();
    p
}

#[test]
fn file_second() {
    let s = sheet();
    let p = rich_plan();
    let bytes = p.to_bytes();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for line in [
        "nextrevision 3",
        "titleblock compact",
        "linegroup 500",
        "section ",
        "detail ",
        " rect 10000000 -5 6000000 3000000",
        "hatch steel",
        "step 9000",
        "tol class H7/g6",
        "tol dev 100000 -20000",
        "revision 1",
        "date 2026-01-05",
        "description First issue",
        "author A. Author",
        "revision 2",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    // a revision with no date and no author has only its description
    assert!(text.contains("revision 2\ndescription Hole moved\n"));
    assert_eq!(Plan::from_bytes(&bytes, &s).unwrap(), p);
    // a sheet with none of the new records is written as before
    let old = full_plan().to_bytes();
    let old_text = String::from_utf8(old).unwrap();
    for word in [
        "nextrevision",
        "titleblock",
        "linegroup",
        "section",
        "detail",
        "hatch",
        "tol ",
    ] {
        assert!(!old_text.contains(word), "{word}");
    }
    // a line of a newer program after a revision is kept with it
    let with_x = text.replace(
        "description Hole moved\n",
        "description Hole moved\nx-newer 1\n",
    );
    let back = Plan::from_bytes(with_x.as_bytes(), &s).unwrap();
    assert_eq!(back.revisions[1].extra, vec!["x-newer 1".to_string()]);
    assert_eq!(back.to_bytes(), with_x.as_bytes());
    // what is wrong is refused with the code of the thing
    let broken = |from: &str, to: &str| {
        Plan::from_bytes(text.replace(from, to).as_bytes(), &s)
            .unwrap_err()
            .code()
    };
    assert_eq!(
        broken("tol class H7/g6", "tol class H7/H7"),
        "PHOENIX-CAD-058"
    );
    assert_eq!(
        broken("tol dev 100000 -20000", "tol dev -5 5"),
        "PHOENIX-CAD-058"
    );
    assert_eq!(broken("section ", "section 9 "), "PHOENIX-CAD-054");
    assert_eq!(broken("revision 2\n", "revision 1\n"), "PHOENIX-CAD-060");
    assert_eq!(
        broken("titleblock compact", "titleblock nothing"),
        "PHOENIX-CAD-054"
    );
    assert_eq!(broken("linegroup 500", "linegroup 400"), "PHOENIX-CAD-054");
    assert_eq!(
        broken("nextrevision 3", "nextrevision 2"),
        "PHOENIX-CAD-060"
    );
    assert_eq!(
        broken("hatch steel", "hatch steel\nhatch wood"),
        "PHOENIX-CAD-054"
    );
}

#[test]
fn tol_text() {
    let table = crate::iso286::Table::shipped();
    let size = 25_000_000;
    let note = |style, tol: &Tol| annotate("25.00", tol, size, &table, style, 3).unwrap();
    let h7 = Tol::Class("H7".into());
    assert_eq!(note(TolDisplay::Class, &h7), "25.00 H7");
    assert_eq!(note(TolDisplay::Deviation, &h7), "25.00 +0.021/0");
    assert_eq!(note(TolDisplay::Both, &h7), "25.00 H7 (+0.021/0)");
    // a fit gives the hole and then the shaft, from the table
    let fit = Tol::Class("H7/g6".into());
    let t = note(TolDisplay::Both, &fit);
    assert!(t.starts_with("25.00 H7/g6 (+0.021/0 | -"), "{t}");
    let a = apply(&fit, size, &table).unwrap();
    let want = crate::iso286::fit(&table, 25.0, "H7/g6").unwrap();
    assert_eq!(a.devs[0], (want.hole.upper_dev_nm, want.hole.lower_dev_nm));
    assert_eq!(
        a.devs[1],
        (want.shaft.upper_dev_nm, want.shaft.lower_dev_nm)
    );
    // deviations by hand: alike on both sides, or not
    let sym = Tol::Dev {
        upper_nm: 100_000,
        lower_nm: -100_000,
    };
    assert_eq!(note(TolDisplay::Class, &sym), "25.00 \u{b1}0.100");
    let asym = Tol::Dev {
        upper_nm: 0,
        lower_nm: -50_000,
    };
    assert_eq!(note(TolDisplay::Deviation, &asym), "25.00 0/-0.050");
    assert_eq!(pair_text(5000, -2000, 3), "+0.005/-0.002");
    assert_eq!(pair_text(0, 0, 3), "0/0");
}

#[test]
fn tol_bad() {
    let table = crate::iso286::Table::shipped();
    let lin = DimKind::Linear;
    assert!(check(&Tol::Class("H7".into()), lin).is_ok());
    for bad in [
        "H7/H7", "g6/H7", "X7", "H4", "H", "7", "H7 ", "H7/g6/h6", "",
    ] {
        assert!(check(&Tol::Class(bad.into()), lin).is_err(), "{bad}");
    }
    assert!(check(&Tol::Class("H7".into()), DimKind::Angle).is_err());
    assert!(check(
        &Tol::Dev {
            upper_nm: -1,
            lower_nm: 1
        },
        lin
    )
    .is_err());
    // the combinations only the size can tell: a letter the grade has no value for, a size above
    // the table, a size that is nothing
    assert!(apply(&Tol::Class("k8".into()), 10_000_000, &table).is_err());
    assert!(apply(&Tol::Class("H7".into()), 600_000_000, &table).is_err());
    assert!(apply(&Tol::Class("H7".into()), 0, &table).is_err());
    assert!(apply(&Tol::Class("H7".into()), 25_000_000, &table).is_ok());
}

#[test]
fn tol_in_sheet() {
    let s = SheetSettings::with_user_config("[sheet]\ndim.tol_style = deviation\n").unwrap();
    let d = plain_box();
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
    p.set_dim_tol(id(201), Some(Tol::Class("H7".into())), &s)
        .unwrap();
    let run_with = |p: &Plan| {
        evaluate(
            p,
            &Model {
                dag: Some(&d),
                sketches: &[],
            },
            &cfg(),
            &s,
            Lang::En,
        )
        .unwrap()
    };
    let ev = run_with(&p);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    assert_eq!(ev.dims[0].texts, vec!["10.00 +0.015/0".to_string()]);
    assert!(has_text(&ev, "10.00 +0.015/0"));
    // the table that ships is not checked, and the panel says so
    assert!(ev.unverified_table);
    let lines = lines_of(&p, &ev, &s, Lang::En);
    assert_eq!(lines.notes.len(), 1);
    assert!(lines.notes[0].text.contains("not"), "{:?}", lines.notes);
    // a class that cannot be worked out at that size makes the dimension a problem, not a drawing
    p.set_dim_tol(id(201), Some(Tol::Class("k8".into())), &s)
        .unwrap();
    let ev = run_with(&p);
    assert!(ev.dims.is_empty());
    assert_eq!(ev.issues[0].code, "PHOENIX-CAD-058");
    // a hole over a hole is refused when it is set
    let e = p
        .set_dim_tol(id(201), Some(Tol::Class("H7/H7".into())), &s)
        .unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-058");
    // an angle takes none
    let e = p
        .add_dim(
            Dim {
                tol: Some(Tol::Class("H7".into())),
                ..dim(
                    7,
                    DimKind::Angle,
                    vec![
                        Ref::Element {
                            sketch: "plate".into(),
                            element: 1,
                        },
                        Ref::Element {
                            sketch: "plate".into(),
                            element: 2,
                        },
                    ],
                    id(100),
                )
            },
            &s,
        )
        .unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-058");
    // no tolerance, no note
    let q = Plan::new(&s);
    assert!(!run_with(&q).unverified_table);
}

#[test]
fn dim_extended() {
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = Plan::new(&s);
    four_views(&mut p);
    let xs = vec![
        face(2, Axis::X, Side::Min),
        face(3, Axis::X, Side::Min),
        face(3, Axis::X, Side::Max),
        face(2, Axis::X, Side::Max),
    ];
    for (n, kind) in [DimKind::Chain, DimKind::Ordinate, DimKind::Cumulative]
        .into_iter()
        .enumerate()
    {
        let mut dm = dim(10 + n as u16, kind, xs.clone(), id(100));
        dm.offset_um = Some(10_000 + 30_000 * n as i64);
        p.add_dim(dm, &s).unwrap();
    }
    let ev = run(&p, &d);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    let texts = |name: &str| {
        ev.dims
            .iter()
            .find(|x| x.name == name)
            .unwrap()
            .texts
            .clone()
    };
    // the chain: each face to the next one; the ordinate: each face from the first; the
    // cumulative: the same distances, stacked
    assert_eq!(texts("d10"), ["10.00", "10.00", "20.00"]);
    assert_eq!(texts("d11"), ["10.00", "20.00", "40.00"]);
    assert_eq!(texts("d12"), ["10.00", "20.00", "40.00"]);
    // the ordinate writes its origin and has no arrows; the chain has an arrow at each end of each run
    assert!(has_text(&ev, "0"));
    assert!(count_layer(&ev, "DIMENSION") > 20);
    // an ordinate that runs along z is drawn up the left of the view, with its text turned
    let mut q = Plan::new(&s);
    four_views(&mut q);
    q.add_dim(
        dim(
            20,
            DimKind::Ordinate,
            vec![
                face(2, Axis::Z, Side::Min),
                face(3, Axis::Z, Side::Min),
                face(3, Axis::Z, Side::Max),
            ],
            id(100),
        ),
        &s,
    )
    .unwrap();
    let ev = run(&q, &d);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    assert!(ev.scene.prims.iter().any(|p| matches!(
        &p.shape,
        Shape::Text { text, turn, .. } if text == "8.00" && *turn == 90.0
    )));
    // not in the iso view, and not across two axes
    let mut bad = Plan::new(&s);
    four_views(&mut bad);
    bad.add_dim(dim(21, DimKind::Chain, xs.clone(), id(103)), &s)
        .unwrap();
    let ev = run(&bad, &d);
    assert_eq!(ev.issues[0].code, "PHOENIX-CAD-056");
    // three faces at least
    assert_eq!(
        code(
            bad.add_dim(dim(22, DimKind::Ordinate, xs[..2].to_vec(), id(100)), &s)
                .unwrap_err()
        ),
        "PHOENIX-CAD-056"
    );
}

#[test]
fn dim_levels() {
    let slot = |n: u16, lo: i64, hi: i64, lines: i64| Slot {
        id: id(n),
        view: id(100),
        horizontal: true,
        lo,
        hi,
        step: 7000,
        lines,
    };
    // overlapping ones climb, ones apart share the first line, and the room between them counts
    let got = place_dims(
        &[
            slot(1, 0, 40_000, 1),
            slot(2, 10_000, 20_000, 1),
            slot(3, 22_000, 30_000, 1),
            slot(4, 45_000, 60_000, 1),
            slot(5, 41_000, 44_000, 2),
        ],
        2000,
    );
    assert_eq!(got[&id(1)], 0);
    assert_eq!(got[&id(2)], 7000);
    assert_eq!(got[&id(3)], 7000);
    assert_eq!(got[&id(4)], 0);
    // 5 meets 1 (41 < 40 + 2) and 4 (44 + 2 > 45): it goes above both
    assert_eq!(got[&id(5)], 7000);
    // another view or the other side does not meet them
    let mut other = slot(6, 0, 40_000, 1);
    other.view = id(101);
    let mut vertical = slot(7, 0, 40_000, 1);
    vertical.horizontal = false;
    let got = place_dims(&[slot(1, 0, 40_000, 1), other, vertical], 2000);
    assert!(got.values().all(|v| *v == 0));
    // the same in the sheet: the second dimension over the first one is one step further out
    let s = sheet();
    let d = hollow("40 mm");
    let mut p = Plan::new(&s);
    four_views(&mut p);
    for (n, (a, b)) in [(2, 2), (3, 3)].into_iter().enumerate() {
        p.add_dim(
            dim(
                30 + n as u16,
                DimKind::Linear,
                vec![face(a, Axis::X, Side::Min), face(b, Axis::X, Side::Max)],
                id(100),
            ),
            &s,
        )
        .unwrap();
    }
    let ev = run(&p, &d);
    let level = |i: usize| ev.dims[i].line.unwrap().level_um;
    assert_eq!(level(0), s.dim.offset_um);
    assert_eq!(level(1), s.dim.offset_um + s.dim.step_um);
    // one that was put by hand stays where it was put and takes no line
    p.dims[1].offset_um = Some(30_000);
    let ev = run(&p, &d);
    assert_eq!(ev.dims[1].line.unwrap().level_um, 30_000);
    assert_eq!(ev.dims[0].line.unwrap().level_um, s.dim.offset_um);
}

#[test]
fn dim_drag() {
    let line = DimLine {
        horizontal: true,
        view_origin: (50_000, 80_000),
        level_um: 10_000,
        lo: 50_000,
        hi: 90_000,
    };
    assert_eq!(line.line_at(), 70_000);
    assert!(line.hit((70_000, 70_500), 1000));
    assert!(line.hit((50_000 - 500, 70_000), 1000));
    assert!(!line.hit((70_000, 72_000), 1000));
    assert!(!line.hit((95_000, 70_000), 1000));
    // the pointer sets the distance from the view, on a multiple of the snap, within a metre
    assert_eq!(line.offset_for((70_000, 64_400), 1000), 16_000);
    assert_eq!(line.offset_for((70_000, 64_600), 1000), 15_000);
    assert_eq!(line.offset_for((70_000, 90_000), 1000), -10_000);
    assert_eq!(line.offset_for((0, -900_000_000), 1000), 1_000_000);
    let side = DimLine {
        horizontal: false,
        view_origin: (50_000, 80_000),
        level_um: 12_000,
        lo: 80_000,
        hi: 100_000,
    };
    assert_eq!(side.line_at(), 38_000);
    assert!(side.hit((38_000, 90_000), 500));
    assert_eq!(side.offset_for((30_000, 90_000), 1000), 20_000);
    // in the panel: the nearest line under the pointer is taken and dragged, and the sheet is
    // evaluated again with the dimension where it was dropped
    let d = hollow("40 mm");
    let s = sheet();
    let mut st = PlanState::with_defaults();
    st.load_bytes(&project_bytes(&d, None), None, Lang::En)
        .unwrap();
    st.create_sheet(Lang::En).unwrap();
    let front = st.plan.as_ref().unwrap().views[0].id;
    st.plan
        .as_mut()
        .unwrap()
        .add_dim(
            dim(
                1,
                DimKind::Linear,
                vec![face(2, Axis::X, Side::Min), face(2, Axis::X, Side::Max)],
                front,
            ),
            &s,
        )
        .unwrap();
    st.evaluate(Lang::En).unwrap();
    let l = st.eval.as_ref().unwrap().dims[0].line.unwrap();
    let on = ((l.lo + l.hi) / 2, l.line_at());
    assert_eq!(st.pick_dim(on, 1000), Some(id(201)));
    assert_eq!(st.pick_dim((on.0, on.1 + 50_000), 1000), None);
    assert!(!st.begin_drag((0, 0)));
    assert!(st.begin_drag(on));
    let to = (on.0, l.view_origin.1 - 25_300);
    assert_eq!(st.drag_to(to, Lang::En).unwrap(), Some(25_000));
    assert_eq!(st.plan.as_ref().unwrap().dims[0].offset_um, Some(25_000));
    assert_eq!(
        st.eval.as_ref().unwrap().dims[0].line.unwrap().level_um,
        25_000
    );
    assert!(st.dirty);
    st.end_drag();
    assert_eq!(st.drag_to(to, Lang::En).unwrap(), None);
    // a locked layer cannot be dragged
    st.toggle_locked("DIMENSION", Lang::En).unwrap();
    assert!(st.begin_drag((on.0, l.view_origin.1 - 25_000)));
    assert_eq!(
        st.drag_to(to, Lang::En).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
}

#[test]
fn revisions() {
    let s = sheet();
    let mut p = Plan::new(&s);
    assert_eq!(
        p.add_revision("2026-02-01", "First issue", "A", &s)
            .unwrap(),
        1
    );
    assert_eq!(p.add_revision("", "Second", "", &s).unwrap(), 2);
    assert_eq!(p.add_revision("later", "Third", "B", &s).unwrap(), 3);
    // a number is never given twice, even after a revision is taken away
    p.remove_revision(2, &s).unwrap();
    assert_eq!(p.add_revision("", "Fourth", "", &s).unwrap(), 4);
    let numbers: Vec<u32> = p.revisions.iter().map(|r| r.number).collect();
    assert_eq!(numbers, [1, 3, 4]);
    assert_eq!(
        p.remove_revision(2, &s).unwrap_err().code(),
        "PHOENIX-CAD-014"
    );
    // letters or numbers, past Z
    let letters = RevisionNumbering::Letters;
    assert_eq!(revision_label(1, letters), "A");
    assert_eq!(revision_label(26, letters), "Z");
    assert_eq!(revision_label(27, letters), "AA");
    assert_eq!(revision_label(53, letters), "BA");
    assert_eq!(revision_label(3, RevisionNumbering::Numbers), "3");
    // a text of two lines, an empty description, a locked frame and too many are refused
    for (date, what, who) in [
        ("a\nb", "x", ""),
        ("", "", ""),
        ("", " x", ""),
        ("", "x", "a\tb"),
    ] {
        assert_eq!(
            p.add_revision(date, what, who, &s).unwrap_err().code(),
            "PHOENIX-CAD-060"
        );
    }
    assert_eq!(p.next_revision, 5);
    let small = SheetSettings::with_user_config("[sheet]\nrevisions.max = 2\n").unwrap();
    let mut q = Plan::new(&small);
    q.add_revision("", "a", "", &small).unwrap();
    q.add_revision("", "b", "", &small).unwrap();
    assert_eq!(
        q.add_revision("", "c", "", &small).unwrap_err().code(),
        "PHOENIX-CAD-060"
    );
    p.set_layer_locked("FRAME", true, &s).unwrap();
    assert_eq!(
        p.add_revision("", "z", "", &s).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
    p.set_layer_locked("FRAME", false, &s).unwrap();
    // the table is drawn at the upper right corner with the oldest revision first, and the views
    // leave room for it
    let d = plain_box();
    let mut sheet_plan = Plan::new(&s);
    four_views(&mut sheet_plan);
    let before = run(&sheet_plan, &d);
    for r in &p.revisions {
        sheet_plan
            .add_revision(&r.date, &r.description, &r.author, &s)
            .unwrap();
    }
    let ev = run(&sheet_plan, &d);
    assert!(ev.is_clean(), "{:?}", ev.issues);
    for t in [
        "Revision",
        "Date",
        "First issue",
        "Third",
        "Fourth",
        "A",
        "B",
        "C",
        "later",
    ] {
        assert!(has_text(&ev, t), "{t}");
    }
    let top = |ev: &Evaluation| {
        ev.views
            .iter()
            .map(|v| v.origin.1 + v.size.1)
            .max()
            .unwrap()
    };
    assert!(top(&ev) <= top(&before));
    let row = s.revisions.row_um;
    let header = ev
        .scene
        .prims
        .iter()
        .filter_map(|p| match &p.shape {
            Shape::Text { text, at, .. } if text == "Revision" => Some(*at),
            _ => None,
        })
        .max_by_key(|at| at.1)
        .unwrap();
    assert!(header.1 > ev.scene.height_um - s.margin_um - row);
    // and it can be turned off, or in the panel
    let off = SheetSettings::with_user_config("[sheet]\nrevisions.table = 0\n").unwrap();
    let ev2 = evaluate(
        &sheet_plan,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &cfg(),
        &off,
        Lang::En,
    )
    .unwrap();
    assert!(!has_text(&ev2, "First issue"));
    let lines = lines_of(&sheet_plan, &ev, &s, Lang::En);
    assert_eq!(lines.revisions.len(), 3);
    assert_eq!(
        lines.revisions[0].text,
        "Revision A: 2026-02-01, First issue (A)"
    );
}

#[test]
fn title_blocks() {
    let s = sheet();
    let d = plain_box();
    let mut p = Plan::new(&s);
    four_views(&mut p);
    let count_frame = |p: &Plan| count_layer(&run(p, &d), "FRAME");
    let wide = count_frame(&p);
    p.set_title_template(Some("compact"), &s).unwrap();
    let ev = run(&p, &d);
    // the compact block has four fields in two columns: fewer rectangles than the eight of the default
    assert!(count_layer(&ev, "FRAME") < wide);
    assert!(has_text(&ev, "Title") && !has_text(&ev, "Author"));
    p.set_title_template(Some("default"), &s).unwrap();
    assert_eq!(p.title_template, None);
    assert_eq!(count_frame(&p), wide);
    assert_eq!(
        p.set_title_template(Some("missing"), &s)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-054"
    );
    // the settings can make another block the one of a new sheet, and refuse one that is not there
    let u = SheetSettings::with_user_config("[sheet]\ntitleblock.template = compact\n").unwrap();
    assert_eq!(u.title_block_of(None).unwrap().fields.len(), 4);
    assert!(SheetSettings::with_user_config("[sheet]\ntitleblock.template = nothing\n").is_err());
    let more = "[sheet]\ntitleblock.templates = compact long\ntitleblock.template.long.width = 100 mm\ntitleblock.template.long.row = 5 mm\ntitleblock.template.long.columns = 1\ntitleblock.template.long.fields = title\n";
    let m = SheetSettings::with_user_config(more).unwrap();
    assert_eq!(m.title_block_of(Some("long")).unwrap().columns, 1);
}

#[test]
fn line_groups() {
    let s = sheet();
    let mut p = Plan::new(&s);
    assert_eq!(p.layer_width_um("OUTLINE", &s), 350);
    assert_eq!(p.layer_width_um("DIMENSION", &s), 175);
    // another group of the standard changes the thick, the thin and the extra thick lines
    p.set_line_group(Some(700), &s).unwrap();
    assert_eq!(p.layer_width_um("OUTLINE", &s), 700);
    assert_eq!(p.layer_width_um("DIMENSION", &s), 350);
    let wide = SheetSettings::with_user_config("[sheet]\nlayer.SECTION.width = extra\n").unwrap();
    assert_eq!(p.layer_width_um("SECTION", &wide), 1400);
    // a width in micrometres stays, and what the person set for a layer wins
    let fixed = SheetSettings::with_user_config("[sheet]\nlayer.HATCH.width = 0.2 mm\n").unwrap();
    assert_eq!(p.layer_width_um("HATCH", &fixed), 200);
    p.set_layer_look("OUTLINE", "000000", 600, &s).unwrap();
    assert_eq!(p.layer_width_um("OUTLINE", &s), 600);
    // only the groups of the standard
    assert_eq!(
        p.set_line_group(Some(400), &s).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
    p.set_line_group(Some(350), &s).unwrap();
    assert_eq!(p.line_group_um, None);
    assert!(SheetSettings::with_user_config("[sheet]\nline_group = 0.4 mm\n").is_err());
    assert!(SheetSettings::with_user_config("[sheet]\nline_group = 0.5 mm\n").is_ok());
    // the scene carries the sheet's widths, and the patterns of the dashes are the settings' own
    let d = plain_box();
    let mut q = Plan::new(&s);
    four_views(&mut q);
    q.set_line_group(Some(1000), &s).unwrap();
    let ev = run(&q, &d);
    assert_eq!(ev.scene.layers["OUTLINE"].1, 1000);
    assert_eq!(ev.scene.dashed.dash, 80);
    let slow = SheetSettings::with_user_config("[sheet]\nlinetype.dashed = 200 50\n").unwrap();
    let ev = evaluate(
        &q,
        &Model {
            dag: Some(&d),
            sketches: &[],
        },
        &cfg(),
        &slow,
        Lang::En,
    )
    .unwrap();
    assert_eq!((ev.scene.dashed.dash, ev.scene.dashed.gap), (200, 50));
    assert!(out::to_svg(&ev.scene, "t").contains("stroke-dasharray"));
    for bad in [
        "linetype.dashed = 80",
        "linetype.dashdot = 1 2 3",
        "linetype.dashed = 0 4",
    ] {
        assert!(SheetSettings::with_user_config(&format!("[sheet]\n{bad}\n")).is_err());
    }
}

#[test]
fn settings_more() {
    let s = sheet();
    assert_eq!(s.layers.len(), 12);
    assert_eq!(s.hatch.default.angle_deg, 45);
    assert_eq!(s.hatch.materials["aluminium"].angle_deg, 135);
    assert_eq!(s.section.end_length_um, 6000);
    assert_eq!(s.dim.tol_style, TolDisplay::Class);
    assert_eq!(s.revisions.numbering, RevisionNumbering::Letters);
    assert_eq!(s.line_groups.len(), 7);
    for bad in [
        "hatch.angle = 180",
        "hatch.spacing = 0.1 mm",
        "hatch.materials = steel steel",
        "hatch.materials = Steel",
        "hatch.materials = copper",
        "dim.tol_style = both and more",
        "revisions.numbering = roman",
        "section.text_height = 0 mm",
        "titleblock.templates = default",
        "dim.drag_snap = 1 um",
    ] {
        assert!(
            SheetSettings::with_user_config(&format!("[sheet]\n{bad}\n")).is_err(),
            "{bad}"
        );
    }
    let copper = "[sheet]\nhatch.materials = copper\nhatch.material.copper.angle = 30\nhatch.material.copper.spacing = 2 mm\n";
    assert_eq!(
        SheetSettings::with_user_config(copper)
            .unwrap()
            .hatch
            .materials["copper"],
        HatchStyle {
            angle_deg: 30,
            spacing_um: 2000
        }
    );
}

#[test]
fn fuzz_second() {
    let s = sheet();
    let base = rich_plan().to_bytes();
    let mut rng = Rng(0x5eed_0022);
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
    // the hatching: lines inside the box of the rectangles, the same for the same input, and
    // never more than the limit
    for _ in 0..300 {
        let rects: Vec<((i64, i64), (i64, i64))> = (0..1 + rng.below(4))
            .map(|_| {
                let (x, y) = (rng.below(20_000) as i64, rng.below(20_000) as i64);
                (
                    (x, y),
                    (
                        x + 1 + rng.below(8000) as i64,
                        y + 1 + rng.below(8000) as i64,
                    ),
                )
            })
            .collect();
        let style = HatchStyle {
            angle_deg: rng.below(180) as u32,
            spacing_um: 200 + rng.below(3000) as i64,
        };
        let lo = (
            rects.iter().map(|r| r.0 .0).min().unwrap() - 1,
            rects.iter().map(|r| r.0 .1).min().unwrap() - 1,
        );
        let hi = (
            rects.iter().map(|r| r.1 .0).max().unwrap() + 1,
            rects.iter().map(|r| r.1 .1).max().unwrap() + 1,
        );
        match hatch_lines(&rects, style, 400) {
            Ok(lines) => {
                assert!(lines.len() <= 400);
                for (a, b) in &lines {
                    for p in [a, b] {
                        assert!(p.0 >= lo.0 && p.0 <= hi.0 && p.1 >= lo.1 && p.1 <= hi.1);
                    }
                }
                assert_eq!(lines, hatch_lines(&rects, style, 400).unwrap());
            }
            Err(n) => assert!(n > 400),
        }
    }
    // the clipping: what is kept is inside the shape and inside the box of the segment
    for _ in 0..2000 {
        let rnd = |r: &mut Rng| r.below(2001) as i64 - 1000;
        let (a, b) = (
            (rnd(&mut rng), rnd(&mut rng)),
            (rnd(&mut rng), rnd(&mut rng)),
        );
        let r = 1 + rng.below(800) as i64;
        if let Some((p, q)) = clip_to_circle(a, b, (0, 0), r) {
            for e in [p, q] {
                let d = ((e.0 * e.0 + e.1 * e.1) as f64).sqrt();
                assert!(d <= r as f64 + 1.5, "{e:?} {r}");
                assert!(e.0 >= a.0.min(b.0) - 1 && e.0 <= a.0.max(b.0) + 1);
            }
        }
        let rect = ((-r, -r / 2), (r, r));
        if let Some((p, q)) = clip_to_rect(a, b, rect) {
            for e in [p, q] {
                assert!(e.0 >= -r - 1 && e.0 <= r + 1 && e.1 >= -r / 2 - 1 && e.1 <= r + 1);
            }
        }
    }
}

mod cc23;
