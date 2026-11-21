// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests of the broken view, the notes with a leader, the free attributes and the buttons and
//! dragging of the panel. Every random case has a fixed seed.

use super::*;
use crate::plans::bom::to_csv_cols;
use crate::plans::brk::{clip, zigzag, Warp};
use crate::plans::drag::{pick_balloon, pick_note, snap, BalloonPart, CutLine};
use crate::plans::measure::Model;
use crate::plans::panel::{
    parse_mm, section_kinds_for, DragTarget, FormAction, PlanForm, PlanState,
};
use crate::plans::prim::Shape;
use crate::plans::scene::Evaluation;
use crate::CadError;

fn bar(len_mm: i64) -> FeatureDag {
    let c = cfg();
    let mut d = FeatureDag::new(HistorySettings::defaults());
    d.add_feature(
        id(2),
        "bar",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "0 mm"),
            ("sx", &format!("{len_mm} mm")),
            ("sy", "20 mm"),
            ("sz", "10 mm"),
        ],
        &c,
    )
    .unwrap();
    d.rebuild(&c).unwrap();
    d
}

fn eval_of(p: &Plan, d: &FeatureDag) -> Evaluation {
    let model = Model {
        dag: Some(d),
        sketches: &[],
    };
    evaluate(p, &model, &cfg(), &sheet(), Lang::En).unwrap()
}

fn texts_on(e: &Evaluation, layer: &str) -> Vec<String> {
    e.scene
        .prims
        .iter()
        .filter(|p| p.layer == layer)
        .filter_map(|p| match &p.shape {
            Shape::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn lines_on(e: &Evaluation, layer: &str) -> usize {
    e.scene
        .prims
        .iter()
        .filter(|p| p.layer == layer && matches!(p.shape, Shape::Line { .. }))
        .count()
}

fn front_of_bar(s: &SheetSettings) -> Plan {
    let mut p = Plan::new(s);
    p.add_view(view(0, ViewKind::Front, false), s).unwrap();
    p
}

fn cut(from_mm: i64, to_mm: i64) -> BreakCut {
    BreakCut {
        along_u: true,
        from_nm: from_mm * 1_000_000,
        to_nm: to_mm * 1_000_000,
    }
}

// ----- the broken view -----

#[test]
fn brk_squeeze() {
    let w = Warp {
        along_u: true,
        from: 100,
        to: 400,
        gap: 50,
    };
    assert_eq!(w.coord(50), 50);
    assert_eq!(w.coord(100), 100);
    assert_eq!(w.coord(250), 100);
    assert_eq!(w.coord(400), 150);
    assert_eq!(w.coord(1000), 750);
    assert_eq!(w.removed(), 250);
    assert_eq!(w.point((1000, 7)), (750, 7));
    let v = Warp {
        along_u: false,
        ..w
    };
    assert_eq!(v.point((7, 1000)), (7, 750));
    // an edge that lies in the range goes, one that crosses a side is cut there, one that crosses
    // the whole range is two, one at a side stays
    let seg = |a: i64, b: i64| Seg {
        a: (a, 5),
        b: (b, 5),
        hidden: false,
    };
    let out = clip(
        &[
            seg(10, 90),
            seg(150, 300),
            seg(50, 200),
            seg(300, 500),
            seg(0, 900),
        ],
        &w,
    );
    let spans: Vec<(i64, i64)> = out.iter().map(|g| (g.a.0, g.b.0)).collect();
    assert_eq!(
        spans,
        vec![(10, 90), (50, 100), (400, 500), (0, 100), (400, 900)]
    );
    let across = Seg {
        a: (100, 0),
        b: (100, 50),
        hidden: true,
    };
    let inside = Seg {
        a: (200, 0),
        b: (200, 50),
        hidden: false,
    };
    let end = Seg {
        a: (400, 0),
        b: (400, 50),
        hidden: false,
    };
    assert_eq!(clip(&[across, inside, end], &w), vec![across, end]);
}

#[test]
fn brk_props() {
    let mut rng = Rng(0xb4ea_c023);
    let mut crossed = 0;
    for _ in 0..3000 {
        let from = rng.below(1000) as i64 - 500;
        let len = 1 + rng.below(1000) as i64;
        let to = from + len;
        let gap = rng.below(len as usize) as i64;
        let w = Warp {
            along_u: rng.below(2) == 0,
            from,
            to,
            gap,
        };
        let mut segs = Vec::new();
        for _ in 0..8 {
            let (a, b) = (rng.below(2400) as i64 - 1200, rng.below(2400) as i64 - 1200);
            let y = rng.below(100) as i64;
            let (pa, pb) = if w.along_u {
                ((a, y), (b, y))
            } else {
                ((y, a), (y, b))
            };
            segs.push(Seg {
                a: pa,
                b: pb,
                hidden: rng.below(2) == 0,
            });
        }
        let out = clip(&segs, &w);
        let along = |p: (i64, i64)| if w.along_u { p.0 } else { p.1 };
        // nothing is left inside the range, and what is left is exactly what lay outside it
        let mut want = 0;
        for g in &segs {
            let (lo, hi) = (along(g.a).min(along(g.b)), along(g.a).max(along(g.b)));
            let overlap = (hi.min(to) - lo.max(from)).max(0);
            want += hi - lo - overlap;
            if lo < from && hi > to {
                crossed += 1;
            }
        }
        let mut got = 0;
        for g in &out {
            for e in [g.a, g.b] {
                assert!(along(e) <= from || along(e) >= to, "{e:?} {from} {to}");
            }
            got += (along(g.a) - along(g.b)).abs();
        }
        assert_eq!(got, want);
        // the squeeze never turns the order round and keeps the lengths outside the range
        let mut cs: Vec<i64> = (0..40).map(|_| rng.below(2400) as i64 - 1200).collect();
        cs.sort_unstable();
        for pair in cs.windows(2) {
            assert!(w.coord(pair[0]) <= w.coord(pair[1]));
            if pair[1] <= from || pair[0] >= to {
                assert_eq!(w.coord(pair[1]) - w.coord(pair[0]), pair[1] - pair[0]);
            }
        }
    }
    assert!(crossed > 100);
    // a mark is a zigzag of `2 x teeth` pieces that starts and ends on the line
    let s = sheet();
    let z = zigzag("BREAK", true, 5000, 0, 60_000, &s.brk);
    assert_eq!(z.len(), 2 * s.brk.teeth as usize);
    let ends = |p: &crate::plans::prim::Prim| match p.shape {
        Shape::Line { a, b } => (a, b),
        _ => unreachable!(),
    };
    assert_eq!(ends(&z[0]).0, (5000, 0));
    assert_eq!(ends(&z[z.len() - 1]).1, (5000, 60_000));
}

#[test]
fn brk_view() {
    let (s, d) = (sheet(), bar(200));
    let mut p = front_of_bar(&s);
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
    let whole = eval_of(&p, &d);
    assert!(whole.is_clean(), "{:?}", whole.issues);
    let (lo, hi) = whole.views[0].picture;
    assert_eq!(hi.0 - lo.0, 200_000_000);
    assert_eq!(lines_on(&whole, "BREAK"), 0);
    // 80 mm of the length leave, and the 20 mm of the gap stay
    p.set_break(id(100), Some(cut(60, 140)), &s).unwrap();
    let e = eval_of(&p, &d);
    assert!(e.is_clean(), "{:?}", e.issues);
    let (lo, hi) = e.views[0].picture;
    assert_eq!(hi.0 - lo.0, 140_000_000);
    assert!(e.views[0].size.0 < whole.views[0].size.0);
    // two marks of a zigzag each
    assert_eq!(lines_on(&e, "BREAK"), 2 * 2 * s.brk.teeth as usize);
    // the dimension across the break still says the real length
    assert!(
        e.dims[0].texts[0].contains("200.00"),
        "{:?}",
        e.dims[0].texts
    );
    // the dimension line is as long as the picture, not as the part
    let l = e.dims[0].line.unwrap();
    assert_eq!(l.hi - l.lo, e.views[0].size.0);
    // the file keeps the break, in version 2 of the text
    let bytes = p.to_bytes();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("drawing 2\n"));
    assert!(text.contains("\nbreak u 60000000 140000000\n"));
    assert_eq!(Plan::from_bytes(&bytes, &s).unwrap(), p);
    // taking the break away goes back to version 1
    p.set_break(id(100), None, &s).unwrap();
    assert!(p.to_bytes().starts_with(b"drawing 1\n"));
}

#[test]
fn brk_refused() {
    let (s, d) = (sheet(), bar(200));
    let mut p = front_of_bar(&s);
    let code = |r: Result<(), CadError>| r.unwrap_err().code();
    // a range that ends before it starts, or is not longer than the gap
    assert_eq!(
        code(p.set_break(id(100), Some(cut(140, 60)), &s)),
        "PHOENIX-CAD-061"
    );
    assert_eq!(
        code(p.set_break(id(100), Some(cut(60, 80)), &s)),
        "PHOENIX-CAD-061"
    );
    assert_eq!(
        code(p.set_break(id(100), Some(cut(60, 6_000_000_000)), &s)),
        "PHOENIX-CAD-061"
    );
    assert_eq!(
        code(p.set_break(id(999), Some(cut(60, 140)), &s)),
        "PHOENIX-CAD-014"
    );
    // not the isometric view
    let mut iso = Plan::new(&s);
    four_views(&mut iso);
    assert_eq!(
        code(iso.set_break(id(103), Some(cut(60, 140)), &s)),
        "PHOENIX-CAD-061"
    );
    // a locked layer
    p.set_layer_locked("OUTLINE", true, &s).unwrap();
    assert_eq!(
        code(p.set_break(id(100), Some(cut(60, 140)), &s)),
        "PHOENIX-CAD-054"
    );
    p.set_layer_locked("OUTLINE", false, &s).unwrap();
    // a range that is not inside the picture is reported when the sheet is worked out
    let mut p = front_of_bar(&s);
    p.set_break(id(100), Some(cut(-10, 100)), &s).unwrap();
    let e = eval_of(&p, &d);
    assert_eq!(e.issues.len(), 1);
    assert_eq!(e.issues[0].code, "PHOENIX-CAD-061");
    assert!(e.issues[0].detail.contains("not inside the picture"));
    // a view that does not fit the frame even with the break is told with the same code
    let long = bar(500);
    let mut p = front_of_bar(&s);
    p.set_break(id(100), Some(cut(100, 200)), &s).unwrap();
    let e = eval_of(&p, &long);
    assert!(
        e.issues.iter().any(|i| i.code == "PHOENIX-CAD-061"),
        "{:?}",
        e.issues
    );
    p.set_break(id(100), Some(cut(100, 400)), &s).unwrap();
    assert!(eval_of(&p, &long).is_clean());
    // a detail cannot enlarge a broken view, and a broken view cannot be a detail
    let mut det = view(5, ViewKind::Front, false);
    det.detail = Some(Detail {
        parent: id(100),
        label: "B".into(),
        centre: (10_000_000, 5_000_000),
        shape: DetailShape::Circle {
            radius_nm: 3_000_000,
        },
    });
    assert_eq!(
        p.add_view(det.clone(), &s).unwrap_err().code(),
        "PHOENIX-CAD-061"
    );
    let mut q = front_of_bar(&s);
    q.add_view(det, &s).unwrap();
    assert_eq!(
        code(q.set_break(id(105), Some(cut(60, 140)), &s)),
        "PHOENIX-CAD-061"
    );
}

#[test]
fn brk_cut_line() {
    // the line of a cut in a broken view follows the squeeze, and cannot be dragged
    let (s, d) = (sheet(), hollow("200 mm"));
    let mut p = Plan::new(&s);
    p.scale = SheetScale::Auto;
    p.add_view(view(0, ViewKind::Top, false), &s).unwrap();
    let mut sec = view(1, ViewKind::Front, false);
    sec.section = Some(SectionCut {
        parent: id(100),
        at_nm: 10_000_000,
        label: "A".into(),
    });
    p.add_view(sec, &s).unwrap();
    let e = eval_of(&p, &d);
    assert!(e.is_clean(), "{:?}", e.issues);
    assert!(e.cuts[0].draggable);
    p.set_break(id(100), Some(cut(60, 140)), &s).unwrap();
    let e = eval_of(&p, &d);
    assert!(e.is_clean(), "{:?}", e.issues);
    assert!(!e.cuts[0].draggable);
}

// ----- the notes -----

fn note(n: u16, text: &str, anchor: NoteAnchor) -> Note {
    Note {
        id: id(400 + n),
        text: text.to_string(),
        anchor,
        at: (150_000, 120_000),
        centre: false,
        layer: "NOTE".to_string(),
        extra: Vec::new(),
    }
}

#[test]
fn note_flow() {
    let (s, d) = (sheet(), hollow("40 mm"));
    let mut p = Plan::new(&s);
    four_views(&mut p);
    let at_view = NoteAnchor::View {
        view: id(100),
        u_nm: 20_000_000,
        v_nm: 5_000_000,
    };
    p.add_note(note(1, "Deburr all edges", at_view), &s)
        .unwrap();
    let mut datum = note(
        2,
        "Datum",
        NoteAnchor::Face {
            view: id(100),
            node: id(2),
            axis: Axis::X,
            side: Side::Min,
        },
    );
    datum.centre = true;
    p.add_note(datum, &s).unwrap();
    let e = eval_of(&p, &d);
    assert!(e.is_clean(), "{:?}", e.issues);
    assert_eq!(e.notes.len(), 2);
    assert!(texts_on(&e, "NOTE").contains(&"Deburr all edges".to_string()));
    // the leader of the second note ends on the low `x` face of the front view, with a cross
    let tip = e.notes[1].tip.unwrap();
    assert_eq!(tip.0, e.views[0].origin.0);
    let m = s.note.centre_mark_um;
    let cross = e
        .scene
        .prims
        .iter()
        .filter(|q| q.layer == "NOTE")
        .filter(|q| {
            matches!(q.shape, Shape::Line { a, b }
                if (a.0 == tip.0 && b.0 == tip.0 && (b.1 - a.1).abs() == 2 * m)
                    || (a.1 == tip.1 && b.1 == tip.1 && (b.0 - a.0).abs() == 2 * m))
        })
        .count();
    assert_eq!(cross, 2);
    // the file
    let bytes = p.to_bytes();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("drawing 2\n"));
    assert!(text.contains("\nnote "));
    assert!(text.contains("\ncentre 1\n"));
    assert_eq!(Plan::from_bytes(&bytes, &s).unwrap(), p);
    // lines of a newer program stay in the record of a note
    let kept = text.replacen("layer NOTE\n", "layer NOTE\nx-future kept\n", 1);
    let mut again = Plan::from_bytes(kept.as_bytes(), &s).unwrap();
    assert_eq!(again.notes[0].extra, vec!["x-future kept".to_string()]);
    assert_eq!(again.to_bytes(), kept.as_bytes());
    // move and remove
    again.move_note(id(401), (10_000, 20_000), &s).unwrap();
    assert_eq!(again.notes[0].at, (10_000, 20_000));
    assert!(again.move_note(id(401), (900_000_000, 0), &s).is_err());
    again.remove_note(id(401), &s).unwrap();
    assert_eq!(again.notes.len(), 1);
    assert!(again.remove_note(id(401), &s).is_err());
    // a face whose node is gone is an orphan: reported, not drawn
    let mut q = Plan::new(&s);
    four_views(&mut q);
    q.add_note(
        note(
            3,
            "Gone",
            NoteAnchor::Face {
                view: id(100),
                node: id(77),
                axis: Axis::X,
                side: Side::Max,
            },
        ),
        &s,
    )
    .unwrap();
    let e = eval_of(&q, &d);
    assert_eq!(e.issues.len(), 1);
    assert_eq!(e.issues[0].code, "PHOENIX-CAD-055");
    assert!(texts_on(&e, "NOTE").is_empty());
    assert_eq!(e.notes[0].tip, None);
}

#[test]
fn note_refused() {
    let s = sheet();
    let mut p = Plan::new(&s);
    four_views(&mut p);
    let at = |view: u16| NoteAnchor::View {
        view: id(view),
        u_nm: 0,
        v_nm: 0,
    };
    let code = |p: &mut Plan, n: Note| p.add_note(n, &s).unwrap_err().code();
    assert_eq!(code(&mut p, note(1, "", at(100))), "PHOENIX-CAD-062");
    assert_eq!(
        code(&mut p, note(1, "two\nlines", at(100))),
        "PHOENIX-CAD-062"
    );
    assert_eq!(code(&mut p, note(1, " padded", at(100))), "PHOENIX-CAD-062");
    assert_eq!(
        code(&mut p, note(1, &"x".repeat(201), at(100))),
        "PHOENIX-CAD-062"
    );
    assert_eq!(code(&mut p, note(1, "no view", at(900))), "PHOENIX-CAD-062");
    let face_in_iso = NoteAnchor::Face {
        view: id(103),
        node: id(2),
        axis: Axis::X,
        side: Side::Min,
    };
    assert_eq!(code(&mut p, note(1, "iso", face_in_iso)), "PHOENIX-CAD-062");
    let mut far = note(1, "far", at(100));
    far.at = (900_000_000, 0);
    assert_eq!(code(&mut p, far), "PHOENIX-CAD-062");
    let mut odd = note(1, "layer", at(100));
    odd.layer = "NOPE".into();
    assert_eq!(code(&mut p, odd), "PHOENIX-CAD-054");
    // an id in use
    p.add_note(note(1, "one", at(100)), &s).unwrap();
    assert!(p.add_note(note(1, "again", at(100)), &s).is_err());
    // the layer of the notes can be locked, and the most notes can be set
    p.set_layer_locked("NOTE", true, &s).unwrap();
    assert_eq!(code(&mut p, note(2, "locked", at(100))), "PHOENIX-CAD-054");
    assert!(p.remove_note(id(401), &s).is_err());
    p.set_layer_locked("NOTE", false, &s).unwrap();
    let few = SheetSettings::with_user_config("[sheet]\nmax_notes = 1\n").unwrap();
    assert_eq!(
        p.add_note(note(2, "two", at(100)), &few)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-062"
    );
    // a note in a file of version 1 is refused (what is read is what is written)
    let text = String::from_utf8(p.to_bytes()).unwrap();
    let old = text.replacen("drawing 2", "drawing 1", 1);
    assert_eq!(
        Plan::from_bytes(old.as_bytes(), &s).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
    let bad = text.replacen("anchor view", "anchor point", 1);
    assert_eq!(
        Plan::from_bytes(bad.as_bytes(), &s).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
}

// ----- the attributes -----

#[test]
fn attr_flow() {
    let s = sheet();
    let mut p = Plan::new(&s);
    p.set_attr(id(2), "partno", "A-100", &s).unwrap();
    p.set_attr(id(2), "finish", "anodised black", &s).unwrap();
    p.set_attr(id(2), "partno", "A-101", &s).unwrap();
    assert_eq!(p.attr(id(2), "partno"), Some("A-101"));
    assert_eq!(p.attr(id(2), "nothing"), None);
    // the lines are sorted by key and the text is in version 2
    let text = String::from_utf8(p.to_bytes()).unwrap();
    assert!(text.starts_with("drawing 2\n"));
    assert!(text.contains("part "));
    assert!(text.contains("\nattr finish anodised black\nattr partno A-101\n"));
    assert_eq!(Plan::from_bytes(text.as_bytes(), &s).unwrap(), p);
    // a text with the same attribute twice is refused
    let twice = text.replacen(
        "attr partno A-101\n",
        "attr partno A-101\nattr partno B\n",
        1,
    );
    assert_eq!(
        Plan::from_bytes(twice.as_bytes(), &s).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
    // taking away: the part record goes with its last attribute
    p.remove_attr(id(2), "finish").unwrap();
    p.remove_attr(id(2), "partno").unwrap();
    assert!(p.parts.is_empty());
    assert!(p.remove_attr(id(2), "partno").is_err());
    assert!(p.to_bytes().starts_with(b"drawing 1\n"));
    // the material and the description keep the record
    p.set_part(id(2), "steel", "", &s).unwrap();
    p.set_attr(id(2), "partno", "X", &s).unwrap();
    p.remove_attr(id(2), "partno").unwrap();
    assert_eq!(p.parts.len(), 1);
}

#[test]
fn attr_limits() {
    let s = sheet();
    let mut p = Plan::new(&s);
    let code = |p: &mut Plan, k: &str, v: &str| p.set_attr(id(2), k, v, &s).unwrap_err().code();
    assert_eq!(code(&mut p, "Part", "x"), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, "", "x"), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, "part no", "x"), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, &"k".repeat(25), "x"), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, "k", ""), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, "k", " x"), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, "k", "a\tb"), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, "k", "a\nb"), "PHOENIX-CAD-063");
    assert_eq!(code(&mut p, "k", &"v".repeat(81)), "PHOENIX-CAD-063");
    assert!(p.parts.is_empty());
    // any character that is not a control one is a value, unless the settings say ASCII only
    p.set_attr(id(2), "diameter", "Ø20 ±0.1", &s).unwrap();
    let ascii = SheetSettings::with_user_config("[sheet]\nattr.value_charset = ascii\n").unwrap();
    assert_eq!(
        p.set_attr(id(2), "other", "Ø20", &ascii)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-063"
    );
    assert_eq!(p.validate(&ascii).unwrap_err().code(), "PHOENIX-CAD-063");
    // the most attributes of a part
    let two = SheetSettings::with_user_config("[sheet]\nattr.max_per_node = 2\n").unwrap();
    let mut q = Plan::new(&two);
    q.set_attr(id(2), "a", "1", &two).unwrap();
    q.set_attr(id(2), "b", "1", &two).unwrap();
    q.set_attr(id(2), "b", "2", &two).unwrap();
    assert_eq!(
        q.set_attr(id(2), "c", "1", &two).unwrap_err().code(),
        "PHOENIX-CAD-063"
    );
    // another node has its own count; the most parts is the one of the sheet
    q.set_attr(id(3), "a", "1", &two).unwrap();
    let one = SheetSettings::with_user_config("[sheet]\nmax_parts = 1\n").unwrap();
    assert!(q.set_attr(id(4), "a", "1", &one).is_err());
    // the characters of a key are a setting
    let digits =
        SheetSettings::with_user_config("[sheet]\nattr.key_charset = 0123456789\n").unwrap();
    let mut r = Plan::new(&digits);
    r.set_attr(id(2), "42", "x", &digits).unwrap();
    assert_eq!(
        r.set_attr(id(2), "ab", "x", &digits).unwrap_err().code(),
        "PHOENIX-CAD-063"
    );
}

#[test]
fn attr_fuzz() {
    let s = sheet();
    let mut rng = Rng(0x00a7_7123);
    let alphabet: Vec<char> = "abcxyz09_-AZ \t\nØé字 ./".chars().collect();
    let mut p = Plan::new(&s);
    let (mut ok, mut refused) = (0, 0);
    for _ in 0..4000 {
        // most keys and values are good, a quarter have something wrong
        let mk = |rng: &mut Rng, max: usize, good: &str| -> String {
            let mut out: String = (0..1 + rng.below(max))
                .map(|_| {
                    good.chars()
                        .nth(rng.below(good.chars().count()))
                        .unwrap_or('a')
                })
                .collect();
            if rng.below(4) == 0 {
                let at = rng.below(out.chars().count() + 1);
                let bad = alphabet[rng.below(alphabet.len())];
                out = out
                    .chars()
                    .take(at)
                    .chain([bad])
                    .chain(out.chars().skip(at))
                    .collect();
            }
            out
        };
        let (k, v) = (
            mk(&mut rng, 26, "abcdefghijklmnopqrstuvwxyz0123456789_-"),
            mk(&mut rng, 90, "abcdefghijklmnopqrstuvwxyz 0123456789.,/"),
        );
        let node = id(2 + rng.below(3) as u16);
        match p.set_attr(node, &k, &v, &s) {
            Ok(()) => {
                ok += 1;
                p.validate(&s).unwrap();
                let bytes = p.to_bytes();
                assert_eq!(Plan::from_bytes(&bytes, &s).unwrap(), p);
            }
            Err(e) => {
                refused += 1;
                assert_eq!(e.code(), "PHOENIX-CAD-063", "{k:?} {v:?}");
            }
        }
        if rng.below(7) == 0 {
            let _ = p.remove_attr(node, &k);
        }
    }
    assert!(ok > 50 && refused > 50, "{ok} {refused}");
    // a damaged file never panics, and what is accepted is read back to the same bytes
    let base = p.to_bytes();
    let mut accepted = 0;
    for _ in 0..3000 {
        let input = mutate(&mut rng, &base);
        if let Ok(q) = Plan::from_bytes(&input, &s) {
            accepted += 1;
            assert_eq!(q.to_bytes(), input);
        }
    }
    assert!(accepted < 3000);
}

#[test]
fn attr_feeds() {
    let user = "[sheet]\nbom.attr_columns = partno vendor\ntitleblock.from_attr = number:partno\ntitleblock.fields = title number\n";
    let s = SheetSettings::with_user_config(user).unwrap();
    assert_eq!(
        s.bom_attr_columns,
        vec!["partno".to_string(), "vendor".to_string()]
    );
    assert_eq!(
        s.title_from_attr,
        vec![("number".to_string(), "partno".to_string())]
    );
    let d = plain_box();
    let mut p = Plan::new(&s);
    p.add_view(view(0, ViewKind::Front, false), &s).unwrap();
    p.set_attr(id(2), "partno", "A-100", &s).unwrap();
    let model = Model {
        dag: Some(&d),
        sketches: &[],
    };
    let e = evaluate(&p, &model, &cfg(), &s, Lang::En).unwrap();
    assert!(e.is_clean(), "{:?}", e.issues);
    assert_eq!(e.bom[0].attrs, vec!["A-100".to_string(), String::new()]);
    // the table has the columns and the title block takes the number from the attribute
    let texts = texts_on(&e, "TEXT");
    assert_eq!(texts.iter().filter(|t| *t == "A-100").count(), 2);
    assert!(texts.contains(&"vendor".to_string()));
    let csv = to_csv_cols(&e.bom, &s.bom_attr_columns);
    assert!(csv.starts_with("item,quantity,description,material,partno,vendor\n"));
    assert!(csv.contains(",A-100,\n"));
    // a value the sheet gives for the field wins over the attribute
    p.set_field("number", "DWG-1", &s).unwrap();
    let e = evaluate(&p, &model, &cfg(), &s, Lang::En).unwrap();
    let texts = texts_on(&e, "TEXT");
    assert!(texts.contains(&"DWG-1".to_string()));
    assert_eq!(texts.iter().filter(|t| *t == "A-100").count(), 1);
    // with no column the bill is the four columns of before
    let plain = sheet();
    let e = evaluate(&p, &model, &cfg(), &plain, Lang::En).unwrap();
    assert!(to_csv_cols(&e.bom, &[]).starts_with("item,quantity,description,material\n"));
}

#[test]
fn cfg_new() {
    let s = sheet();
    assert_eq!(s.brk.teeth, 3);
    assert_eq!(s.brk.gap_nm, 20_000_000);
    assert_eq!(s.note.max_chars, 200);
    assert_eq!(s.attr.key_max, 24);
    assert!(s.bom_attr_columns.is_empty() && s.title_from_attr.is_empty());
    assert!(s.layer("BREAK").is_some() && s.layer("NOTE").is_some());
    for bad in [
        "break.teeth = 0",
        "break.teeth = 17",
        "break.gap = 0.5 mm",
        "note.max_chars = 4",
        "attr.key_charset = aa",
        "attr.key_charset =",
        "attr.value_charset = latin",
        "bom.attr_columns = A!",
        "bom.attr_columns = a b c d e",
        "bom.attr_columns = a a",
        "titleblock.from_attr = number",
        "titleblock.from_attr = number:A! ",
        "max_notes = 0",
    ] {
        let text = format!("[sheet]\n{bad}\n");
        assert!(SheetSettings::with_user_config(&text).is_err(), "{bad}");
    }
}

// ----- dragging and the buttons -----

#[test]
fn drag_pure() {
    let line = CutLine {
        view: id(1),
        vertical: true,
        at_um: 70_000,
        span: (10_000, 50_000),
        at_nm: 40_000_000,
        lo_nm: 0,
        origin_um: 50_000,
        scale: (1, 2),
        draggable: true,
    };
    assert!(line.hit((70_300, 30_000), 500));
    assert!(!line.hit((71_000, 30_000), 500));
    assert!(!line.hit((70_000, 60_000), 500));
    assert_eq!(line.distance((70_400, 0)), 400);
    // 10 mm on the sheet at 1:2 are 20 mm of the model; the snap of 1 mm of the sheet is 2 mm
    assert_eq!(line.at_nm_for((60_000, 0), 1000), 20_000_000);
    assert_eq!(line.at_nm_for((60_400, 0), 1000), 20_000_000);
    assert_eq!(line.at_nm_for((60_600, 0), 1000), 22_000_000);
    let flat = CutLine {
        vertical: false,
        scale: (2, 1),
        ..line
    };
    assert_eq!(flat.at_nm_for((0, 60_000), 1000), 5_000_000);
    assert_eq!(
        flat.at_nm_for((0, -900_000_000_000), 1000),
        -1_000_000_000_000
    );
    // a point lands on the grid
    assert_eq!(snap((12_400, -12_600), 1000), (12_000, -13_000));
    assert_eq!(snap((5, 5), 0), (5, 5));
    // balloons: the end of the leader wins over the circle, the nearest wins, far away is nothing
    let b = |n: u16, at: (i64, i64), to: (i64, i64)| Balloon {
        id: id(300 + n),
        name: format!("b{n}"),
        part: id(2),
        at,
        to,
        extra: Vec::new(),
    };
    let list = vec![
        b(1, (50_000, 50_000), (52_000, 50_000)),
        b(2, (90_000, 50_000), (100_000, 60_000)),
    ];
    let r = 5000;
    assert_eq!(
        pick_balloon(&list, r, (52_100, 50_000), 1000),
        Some((id(301), BalloonPart::Tip))
    );
    assert_eq!(
        pick_balloon(&list, r, (47_000, 50_000), 1000),
        Some((id(301), BalloonPart::Circle))
    );
    assert_eq!(
        pick_balloon(&list, r, (90_000, 54_000), 1000),
        Some((id(302), BalloonPart::Circle))
    );
    assert_eq!(
        pick_balloon(&list, r, (100_000, 59_500), 1000),
        Some((id(302), BalloonPart::Tip))
    );
    assert_eq!(pick_balloon(&list, r, (0, 0), 1000), None);
    // notes: inside the box of the text
    let notes = vec![note(
        1,
        "hello",
        NoteAnchor::View {
            view: id(100),
            u_nm: 0,
            v_nm: 0,
        },
    )];
    assert_eq!(
        pick_note(&notes, 3500, (150_500, 121_000), 500),
        Some(id(401))
    );
    assert_eq!(
        pick_note(&notes, 3500, (152_000, 123_800), 500),
        Some(id(401))
    );
    assert_eq!(pick_note(&notes, 3500, (170_000, 121_000), 500), None);
}

fn loaded() -> (PlanState, FeatureDag) {
    let d = hollow("40 mm");
    let mut st = PlanState::with_defaults();
    st.load_bytes(&project_bytes(&d, None), None, Lang::En)
        .unwrap();
    st.create_sheet(Lang::En).unwrap();
    (st, d)
}

#[test]
fn drag_cut() {
    let (mut st, _) = loaded();
    let top = st.plan.as_ref().unwrap().views[1].id;
    assert_eq!(
        section_kinds_for(ViewKind::Top),
        vec![ViewKind::Front, ViewKind::Right]
    );
    assert_eq!(
        section_kinds_for(ViewKind::Front),
        vec![ViewKind::Top, ViewKind::Right]
    );
    assert!(section_kinds_for(ViewKind::Iso).is_empty());
    let label = st
        .add_section(top, ViewKind::Front, "10", Lang::En)
        .unwrap();
    assert_eq!(label, "A");
    assert!(
        st.eval.as_ref().unwrap().is_clean(),
        "{:?}",
        st.eval.as_ref().unwrap().issues
    );
    // a cut that the parent does not show is refused, and so is a position that is no length
    assert_eq!(
        st.add_section(top, ViewKind::Top, "10", Lang::En)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-059"
    );
    assert!(st
        .add_section(top, ViewKind::Front, "ten", Lang::En)
        .is_err());
    let line = st.eval.as_ref().unwrap().cuts[0];
    assert!(line.draggable && !line.vertical);
    let mid = ((line.span.0 + line.span.1) / 2, line.at_um);
    let target = st.pick(mid);
    assert_eq!(target, Some(DragTarget::Cut(line.view)));
    assert!(st.begin_drag(mid));
    let to = (mid.0, line.at_um + 3000);
    let want = line.at_nm_for(to, st.sheet.note.drag_snap_um);
    assert_ne!(want, line.at_nm);
    assert_eq!(st.drag_to(to, Lang::En).unwrap(), Some(want));
    let cut_now = st
        .plan
        .as_ref()
        .unwrap()
        .view(line.view)
        .unwrap()
        .section
        .clone()
        .unwrap();
    assert_eq!(cut_now.at_nm, want);
    assert_eq!(st.eval.as_ref().unwrap().cuts[0].at_nm, want);
    assert!(st.dirty);
    // the line moved on the sheet as far as the pointer went (within the snap)
    let moved = st.eval.as_ref().unwrap().cuts[0].at_um - line.at_um;
    let grid = st.sheet.note.drag_snap_um;
    assert!((moved - 3000).abs() <= grid, "{moved}");
    // a locked layer SECTION refuses
    st.toggle_locked("SECTION", Lang::En).unwrap();
    assert_eq!(
        st.drag_to(to, Lang::En).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
}

#[test]
fn drag_things() {
    let (mut st, _) = loaded();
    let s = st.sheet.clone();
    let front = st.plan.as_ref().unwrap().views[0].id;
    st.plan
        .as_mut()
        .unwrap()
        .add_balloon(
            Balloon {
                id: id(300),
                name: "b".into(),
                part: id(2),
                at: (100_000, 100_000),
                to: (120_000, 110_000),
                extra: Vec::new(),
            },
            &s,
        )
        .unwrap();
    st.evaluate(Lang::En).unwrap();
    // the circle moves, the end of the leader stays
    assert!(st.begin_drag((100_200, 100_100)));
    assert_eq!(
        st.drag,
        Some(DragTarget::Balloon(id(300), BalloonPart::Circle))
    );
    assert_eq!(st.drag_to((130_400, 90_000), Lang::En).unwrap(), Some(0));
    let b = st.plan.as_ref().unwrap().balloons[0].clone();
    assert_eq!((b.at, b.to), ((130_000, 90_000), (120_000, 110_000)));
    // the end of the leader moves, the circle stays
    st.end_drag();
    assert!(st.begin_drag((120_100, 110_000)));
    assert_eq!(
        st.drag,
        Some(DragTarget::Balloon(id(300), BalloonPart::Tip))
    );
    st.drag_to((60_000, 40_000), Lang::En).unwrap();
    let b = st.plan.as_ref().unwrap().balloons[0].clone();
    assert_eq!((b.at, b.to), ((130_000, 90_000), (60_000, 40_000)));
    st.end_drag();
    // a locked layer BALLOON refuses
    st.toggle_locked("BALLOON", Lang::En).unwrap();
    assert!(st.begin_drag((130_000, 90_000)));
    assert_eq!(
        st.drag_to((140_000, 90_000), Lang::En).unwrap_err().code(),
        "PHOENIX-CAD-054"
    );
    st.end_drag();
    // a note: added to the middle of the view, taken by its text and dropped on the grid
    let n = st.add_note("Check fit", front, Lang::En).unwrap();
    let before = st.plan.as_ref().unwrap().notes[0].at;
    let tip = st.eval.as_ref().unwrap().notes[0].tip.unwrap();
    let view = st.eval.as_ref().unwrap().views[0].clone();
    assert!(tip.0 > view.origin.0 && tip.0 < view.origin.0 + view.size.0);
    assert!(st.begin_drag((before.0 + 500, before.1 + 500)));
    assert_eq!(st.drag, Some(DragTarget::Note(n)));
    st.drag_to((50_300, 40_700), Lang::En).unwrap();
    assert_eq!(st.plan.as_ref().unwrap().notes[0].at, (50_000, 41_000));
    st.end_drag();
    assert!(!st.begin_drag((290_000, 5_000)));
    // the panel lists the note
    let lines = st.lines(Lang::En);
    assert_eq!(lines.note_items.len(), 1);
    assert!(lines.note_items[0].text.contains("Check fit"));
}

#[test]
fn panel_form() {
    let (mut st, _) = loaded();
    let mut f = PlanForm::default();
    // no description is no revision
    assert_eq!(
        f.apply(&mut st, FormAction::Revision, Lang::En)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-060"
    );
    f.description = "First issue".into();
    f.author = "JR".into();
    let l = f.apply(&mut st, FormAction::Revision, Lang::En).unwrap();
    assert!(l.text.contains('A'));
    assert_eq!(st.plan.as_ref().unwrap().revisions.len(), 1);
    assert!(f.description.is_empty());
    // the two overall sizes of the first view
    f.view = 0;
    f.apply(&mut st, FormAction::Dims, Lang::En).unwrap();
    assert_eq!(st.plan.as_ref().unwrap().dims.len(), 2);
    let texts: Vec<String> = st
        .eval
        .as_ref()
        .unwrap()
        .dims
        .iter()
        .flat_map(|d| d.texts.clone())
        .collect();
    assert!(texts.iter().any(|t| t.contains("40.00")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("10.00")), "{texts:?}");
    // the isometric view cannot be dimensioned
    f.view = 3;
    assert_eq!(
        f.apply(&mut st, FormAction::Dims, Lang::En)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-056"
    );
    assert_eq!(st.plan.as_ref().unwrap().dims.len(), 2);
    // a section from the top view, and none from the isometric one
    f.view = 1;
    f.position = "10".into();
    f.kind = 1;
    assert_eq!(f.kinds(&st), vec![ViewKind::Front, ViewKind::Right]);
    f.apply(&mut st, FormAction::Section, Lang::En).unwrap();
    let plan = st.plan.as_ref().unwrap();
    assert_eq!(plan.views.len(), 5);
    assert_eq!(plan.views[4].kind, ViewKind::Right);
    f.view = 3;
    assert_eq!(
        f.apply(&mut st, FormAction::Section, Lang::En)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-059"
    );
    // a note
    f.view = 0;
    f.text = "Sharp edges removed".into();
    f.apply(&mut st, FormAction::Note, Lang::En).unwrap();
    assert!(f.text.is_empty());
    assert_eq!(st.plan.as_ref().unwrap().notes.len(), 1);
    f.text = "two\nlines".into();
    assert_eq!(
        f.apply(&mut st, FormAction::Note, Lang::En)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-062"
    );
    // a break and its removal
    f.from = "10".into();
    f.to = "35 mm".into();
    f.along_u = true;
    f.apply(&mut st, FormAction::Break, Lang::En).unwrap();
    assert!(st.plan.as_ref().unwrap().views[0].brk.is_some());
    let line = st.lines(Lang::En).views[0].text.clone();
    assert!(line.contains("broken view, 5.000 mm left out"), "{line}");
    f.to = "5".into();
    assert_eq!(
        f.apply(&mut st, FormAction::Break, Lang::En)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-061"
    );
    f.apply(&mut st, FormAction::Unbreak, Lang::En).unwrap();
    assert!(st.plan.as_ref().unwrap().views[0].brk.is_none());
    // the sheet is saved only when asked, and what was added is in the file
    assert!(st.dirty);
    assert_eq!(parse_mm("12.5").unwrap(), 12_500_000);
    assert_eq!(parse_mm(" 2 in ").unwrap(), 50_800_000);
    assert!(parse_mm("").is_err());
    // no sheet: nothing to do
    let mut empty = PlanState::with_defaults();
    assert!(f.apply(&mut empty, FormAction::Note, Lang::En).is_err());
}

#[test]
fn texts_x8() {
    // every new word of the panel and every new diagnostic has its eight texts
    use crate::lang::Msg;
    for m in [
        Msg::PlNotes,
        Msg::PlNoteRow,
        Msg::PlBroken,
        Msg::PlEditSheet,
        Msg::PlAddRevision,
        Msg::PlAddDims,
        Msg::PlAddSection,
        Msg::PlAddNote,
        Msg::PlAddBreak,
        Msg::PlPosition,
        Msg::PlFrom,
        Msg::PlTo,
        Msg::PlText,
        Msg::PlViewOne,
        Msg::PlAdded,
    ] {
        let en = m.tr(Lang::En);
        for l in Lang::ALL {
            assert!(!m.tr(l).trim().is_empty());
            assert_eq!(m.tr(l).matches('{').count(), en.matches('{').count());
        }
        assert_ne!(m.tr(Lang::Ja), en);
        assert_ne!(m.tr(Lang::Zh), en);
    }
    for code in ["PHOENIX-CAD-061", "PHOENIX-CAD-062", "PHOENIX-CAD-063"] {
        for l in ["en", "es", "de", "fr", "it", "ja", "zh", "pt"] {
            assert!(crate::messages::message(code, l).is_some());
        }
    }
}
