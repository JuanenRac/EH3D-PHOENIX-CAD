//! The enclosure as an OpenSCAD-style script.
//!
//! The text is a pure function of the model: the same enclosure always gives the same text. It
//! holds no date, no host and no program version. Lengths are written in millimetres with the
//! digits they have (at most six decimals, which is exactly one nanometre), never rounded.

use std::fmt::Write as _;

use crate::enclosure::{Enclosure, Face, Hole, HoleStyle, Treatment};

/// A length in nanometres as millimetres, with no trailing zeros: `1500000` is `1.5`.
pub fn mm_text(nm: i64) -> String {
    let sign = if nm < 0 { "-" } else { "" };
    let abs = nm.unsigned_abs();
    let (whole, frac) = (abs / 1_000_000, abs % 1_000_000);
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let digits = format!("{frac:06}");
        format!("{sign}{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// The number of segments of a circle in the script.
pub const CIRCLE_SEGMENTS: u32 = 64;

/// A direction along the axes of the box.
type Dir = (i8, i8, i8);

/// The placement of a local frame in the box: the text of a `multmatrix` whose columns are the
/// three local axes and the origin, so that what is drawn in the frame lands where it belongs.
fn frame_text(origin: (i64, i64, i64), axes: [Dir; 3]) -> String {
    let [a, b, c] = axes;
    format!(
        "multmatrix([[{}, {}, {}, {}], [{}, {}, {}, {}], [{}, {}, {}, {}], [0, 0, 0, 1]])",
        a.0,
        b.0,
        c.0,
        mm_text(origin.0),
        a.1,
        b.1,
        c.1,
        mm_text(origin.1),
        a.2,
        b.2,
        c.2,
        mm_text(origin.2)
    )
}

/// An edge of the box that a treatment cuts: where its corner is, the two local axes that run
/// along the two faces from the corner into the box, the local axis along the edge and its length.
struct Edge {
    label: &'static str,
    origin: (i64, i64, i64),
    axes: [Dir; 3],
    length: i64,
}

/// The flat figure that is taken away from the corner of an edge, drawn in the frame of the edge.
fn cut_profile(t: Treatment) -> String {
    let s = mm_text(t.size().get());
    match t {
        Treatment::Sharp => String::new(),
        Treatment::Chamfer(_) => format!("polygon([[0, 0], [{s}, 0], [0, {s}]])"),
        Treatment::Round(_) => format!(
            "difference() {{ square([{s}, {s}]); translate([{s}, {s}]) circle(r = {s}, $fn = {CIRCLE_SEGMENTS}); }}"
        ),
    }
}

/// The text that takes the treatment of one group of edges away from the box.
fn edge_cutters_text(group: &str, t: Treatment, edges: &[Edge]) -> String {
    let mut out = String::new();
    if t.is_sharp() {
        return out;
    }
    for edge in edges {
        let _ = writeln!(out, "  // {group} edge, {}", edge.label);
        let _ = writeln!(
            out,
            "  {} translate([0, 0, -1]) linear_extrude(height = {}) {};",
            frame_text(edge.origin, edge.axes),
            mm_text(edge.length + 2_000_000),
            cut_profile(t)
        );
    }
    out
}

/// The four edges around the lid (`down` is -1: the faces run down from the edge) or around the
/// floor (`down` is 1), whose corners are at height `at`.
fn rim_edges(x: i64, y: i64, at: i64, down: i8) -> [Edge; 4] {
    [
        Edge {
            label: "front",
            origin: (0, 0, at),
            axes: [(0, 1, 0), (0, 0, down), (1, 0, 0)],
            length: x,
        },
        Edge {
            label: "back",
            origin: (0, y, at),
            axes: [(0, -1, 0), (0, 0, down), (1, 0, 0)],
            length: x,
        },
        Edge {
            label: "left",
            origin: (0, 0, at),
            axes: [(1, 0, 0), (0, 0, down), (0, 1, 0)],
            length: y,
        },
        Edge {
            label: "right",
            origin: (x, 0, at),
            axes: [(-1, 0, 0), (0, 0, down), (0, 1, 0)],
            length: y,
        },
    ]
}

/// The rounded and chamfered edges, as shapes taken away from the box. The cuts run the whole
/// length of their edge, so at a corner where two of them meet they overlap.
fn edge_cutters(enclosure: &Enclosure) -> String {
    let edging = enclosure.edging();
    let (x, y, z) = enclosure.params().outer();
    let (x, y, z) = (x.get(), y.get(), z.get());
    let vertical = [
        Edge {
            label: "front left",
            origin: (0, 0, 0),
            axes: [(1, 0, 0), (0, 1, 0), (0, 0, 1)],
            length: z,
        },
        Edge {
            label: "front right",
            origin: (x, 0, 0),
            axes: [(-1, 0, 0), (0, 1, 0), (0, 0, 1)],
            length: z,
        },
        Edge {
            label: "back left",
            origin: (0, y, 0),
            axes: [(1, 0, 0), (0, -1, 0), (0, 0, 1)],
            length: z,
        },
        Edge {
            label: "back right",
            origin: (x, y, 0),
            axes: [(-1, 0, 0), (0, -1, 0), (0, 0, 1)],
            length: z,
        },
    ];
    let mut out = String::new();
    out.push_str(&edge_cutters_text("vertical", edging.vertical, &vertical));
    out.push_str(&edge_cutters_text(
        "lid",
        edging.top,
        &rim_edges(x, y, z, -1),
    ));
    out.push_str(&edge_cutters_text(
        "floor",
        edging.bottom,
        &rim_edges(x, y, 0, 1),
    ));
    out
}

/// Where the centre of a hole is on the outer face and the frame of the hole: the first axis runs
/// along `u`, the second along `v` and the third goes into the material.
fn hole_frame(enclosure: &Enclosure, hole: &Hole) -> ((i64, i64, i64), [Dir; 3]) {
    let (x, y, z) = enclosure.params().outer();
    let (x, y, z) = (x.get(), y.get(), z.get());
    let (u, v) = (hole.u.get(), hole.v.get());
    match hole.face {
        Face::Front => ((u, 0, v), [(1, 0, 0), (0, 0, 1), (0, 1, 0)]),
        Face::Back => ((x - u, y, v), [(-1, 0, 0), (0, 0, 1), (0, -1, 0)]),
        Face::Left => ((0, y - u, v), [(0, -1, 0), (0, 0, 1), (1, 0, 0)]),
        Face::Right => ((x, u, v), [(0, 1, 0), (0, 0, 1), (-1, 0, 0)]),
        Face::Top => ((u, v, z), [(1, 0, 0), (0, 1, 0), (0, 0, -1)]),
        Face::Bottom => ((u, y - v, 0), [(1, 0, 0), (0, -1, 0), (0, 0, 1)]),
    }
}

/// A hole that is not a plain round one, drawn in its own frame: the shapes start 1 mm outside the
/// face and run 1 mm past the thickness so that the cut is clean.
fn styled_hole_text(enclosure: &Enclosure, hole: &Hole) -> String {
    let thickness = match hole.face {
        Face::Front | Face::Back | Face::Left | Face::Right => enclosure.params().wall.get(),
        Face::Top => enclosure.params().lid.get(),
        Face::Bottom => enclosure.params().floor.get(),
    };
    let (origin, axes) = hole_frame(enclosure, hole);
    let long = mm_text(thickness + 2_000_000);
    let d = hole.diameter.get();
    let round = |diameter: i64, height: &str| {
        format!(
            "    translate([0, 0, -1]) cylinder(h = {height}, d = {}, $fn = {CIRCLE_SEGMENTS});\n",
            mm_text(diameter)
        )
    };
    let mut body = String::new();
    match hole.style {
        HoleStyle::Plain => body.push_str(&round(d, &long)),
        HoleStyle::Countersunk { head_diameter } => {
            body.push_str(&round(d, &long));
            // a cone of 90 degrees: 1 mm outside the face it is 2 mm wider than on the face
            let wide = mm_text(head_diameter.get() + 2_000_000);
            let _ = writeln!(
                body,
                "    translate([0, 0, -1]) cylinder(h = ({wide} - {}) / 2, d1 = {wide}, d2 = {}, $fn = {CIRCLE_SEGMENTS});",
                mm_text(d),
                mm_text(d)
            );
        }
        HoleStyle::Counterbored {
            head_diameter,
            depth,
        } => {
            body.push_str(&round(d, &long));
            body.push_str(&round(
                head_diameter.get(),
                &mm_text(depth.get() + 1_000_000),
            ));
        }
        HoleStyle::Rectangular { height } => {
            let _ = writeln!(
                body,
                "    translate([-{w} / 2, -{h} / 2, -1]) cube([{w}, {h}, {long}]);",
                w = mm_text(d),
                h = mm_text(height.get())
            );
        }
        HoleStyle::Oval { length, vertical } => {
            let shift = format!("({} - {}) / 2", mm_text(length.get()), mm_text(d));
            let (first, second) = if vertical {
                (format!("[0, -{shift}, -1]"), format!("[0, {shift}, -1]"))
            } else {
                (format!("[-{shift}, 0, -1]"), format!("[{shift}, 0, -1]"))
            };
            let _ = writeln!(
                body,
                "    hull() {{ translate({first}) cylinder(h = {long}, d = {}, $fn = {CIRCLE_SEGMENTS}); translate({second}) cylinder(h = {long}, d = {}, $fn = {CIRCLE_SEGMENTS}); }}",
                mm_text(d),
                mm_text(d)
            );
        }
    }
    let mut out = String::new();
    let _ = writeln!(out, "  // hole {}, {} face", hole.name, hole.face.name());
    let _ = writeln!(out, "  {} {{", frame_text(origin, axes));
    out.push_str(&body);
    out.push_str("  }\n");
    out
}

fn hole_text(enclosure: &Enclosure, hole: &Hole) -> String {
    if hole.style == HoleStyle::Plain {
        plain_hole_text(enclosure, hole)
    } else {
        styled_hole_text(enclosure, hole)
    }
}

fn plain_hole_text(enclosure: &Enclosure, hole: &Hole) -> String {
    let p = enclosure.params();
    let (x, y, z) = p.outer();
    let (x, y, z) = (x.get(), y.get(), z.get());
    let (u, v) = (hole.u.get(), hole.v.get());
    // The point on the face, the rotation that points the cylinder through the material (the
    // faces are seen from outside, u to the right and v up) and the thickness it crosses.
    let (at, rotation, thickness) = match hole.face {
        Face::Front => ((u, -1_000_000, v), (-90, 0, 0), p.wall.get()),
        Face::Back => ((x - u, y + 1_000_000, v), (90, 0, 0), p.wall.get()),
        Face::Left => ((-1_000_000, y - u, v), (0, 90, 0), p.wall.get()),
        Face::Right => ((x + 1_000_000, u, v), (0, -90, 0), p.wall.get()),
        Face::Top => ((u, v, z + 1_000_000), (180, 0, 0), p.lid.get()),
        Face::Bottom => ((u, y - v, -1_000_000), (0, 0, 0), p.floor.get()),
    };
    let mut out = String::new();
    let _ = writeln!(out, "  // hole {}, {} face", hole.name, hole.face.name());
    let _ = write!(
        out,
        "  translate([{}, {}, {}])",
        mm_text(at.0),
        mm_text(at.1),
        mm_text(at.2)
    );
    if rotation != (0, 0, 0) {
        let _ = write!(
            out,
            " rotate([{}, {}, {}])",
            rotation.0, rotation.1, rotation.2
        );
    }
    let _ = writeln!(
        out,
        " cylinder(h = {}, d = {}, $fn = {CIRCLE_SEGMENTS});",
        mm_text(thickness + 2_000_000),
        mm_text(hole.diameter.get())
    );
    out
}

/// The script of the enclosure.
pub fn to_scad(enclosure: &Enclosure) -> String {
    let p = enclosure.params();
    let (x, y, z) = p.outer();
    let mut out = String::new();
    out.push_str(
        "// Enclosure: the same model always gives this same text. Lengths are in millimetres.\n",
    );
    for (name, v) in [
        ("inner_length", p.inner_length),
        ("inner_width", p.inner_width),
        ("inner_height", p.inner_height),
        ("wall", p.wall),
        ("floor", p.floor),
        ("lid", p.lid),
    ] {
        let _ = writeln!(out, "{name} = {};", mm_text(v.get()));
    }
    out.push_str("difference() {\n");
    let _ = writeln!(
        out,
        "  cube([{}, {}, {}]);",
        mm_text(x.get()),
        mm_text(y.get()),
        mm_text(z.get())
    );
    out.push_str(
        "  translate([wall, wall, floor]) cube([inner_length, inner_width, inner_height]);\n",
    );
    out.push_str(&edge_cutters(enclosure));
    for hole in enclosure.holes().values() {
        out.push_str(&hole_text(enclosure, hole));
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, Operation};
    use crate::Length;
    use eh3d_container::ObjectId;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn enclosure() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    fn add(e: &mut Enclosure, name: &str, face: Face, u: i64, v: i64, d: i64) {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in name.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, (h & 0x0fff) as u16, h).unwrap(),
            hole: Hole {
                name: name.into(),
                face,
                u: mm(u),
                v: mm(v),
                diameter: mm(d),
                style: crate::enclosure::HoleStyle::Plain,
            },
        })
        .unwrap();
    }

    #[test]
    fn lengths_are_written_exactly() {
        assert_eq!(mm_text(0), "0");
        assert_eq!(mm_text(1_500_000), "1.5");
        assert_eq!(mm_text(-2_000_000), "-2");
        assert_eq!(mm_text(1), "0.000001");
        assert_eq!(mm_text(-1_234_567), "-1.234567");
        assert_eq!(mm_text(104_000_000), "104");
    }

    #[test]
    fn the_empty_box_is_a_difference_of_two_cubes() {
        let text = to_scad(&enclosure());
        assert!(text.contains("inner_length = 100;"));
        assert!(text.contains("  cube([104, 64, 35]);"));
        assert!(text.contains(
            "translate([wall, wall, floor]) cube([inner_length, inner_width, inner_height]);"
        ));
        assert!(text.ends_with("}\n"));
        assert!(!text.contains("hole"));
    }

    #[test]
    fn each_face_has_its_own_placement() {
        let mut e = enclosure();
        add(&mut e, "front", Face::Front, 20, 17, 6);
        add(&mut e, "back", Face::Back, 20, 17, 6);
        add(&mut e, "left", Face::Left, 20, 17, 6);
        add(&mut e, "right", Face::Right, 20, 17, 6);
        add(&mut e, "top", Face::Top, 20, 17, 6);
        add(&mut e, "bottom", Face::Bottom, 20, 17, 6);
        let t = to_scad(&e);
        assert!(t.contains(
            "translate([20, -1, 17]) rotate([-90, 0, 0]) cylinder(h = 4, d = 6, $fn = 64);"
        ));
        assert!(t.contains(
            "translate([84, 65, 17]) rotate([90, 0, 0]) cylinder(h = 4, d = 6, $fn = 64);"
        ));
        assert!(t.contains(
            "translate([-1, 44, 17]) rotate([0, 90, 0]) cylinder(h = 4, d = 6, $fn = 64);"
        ));
        assert!(t.contains(
            "translate([105, 20, 17]) rotate([0, -90, 0]) cylinder(h = 4, d = 6, $fn = 64);"
        ));
        assert!(t.contains(
            "translate([20, 17, 36]) rotate([180, 0, 0]) cylinder(h = 5, d = 6, $fn = 64);"
        ));
        assert!(t.contains("translate([20, 47, -1]) cylinder(h = 4, d = 6, $fn = 64);"));
    }

    #[test]
    fn the_same_model_gives_the_same_text_whatever_the_order_of_the_operations() {
        let mut a = enclosure();
        add(&mut a, "one", Face::Front, 20, 17, 6);
        add(&mut a, "two", Face::Back, 30, 17, 6);
        let mut b = enclosure();
        add(&mut b, "two", Face::Back, 30, 17, 6);
        add(&mut b, "one", Face::Front, 20, 17, 6);
        assert_eq!(to_scad(&a), to_scad(&b));
        let text = to_scad(&a);
        assert!(text.contains("hole one") && text.contains("hole two"));
    }

    fn one_hole(face: Face, diameter: i64, style: HoleStyle) -> String {
        let mut e = enclosure();
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: Hole {
                name: "s".into(),
                face,
                u: mm(30),
                v: mm(17),
                diameter: Length::new(diameter).unwrap(),
                style,
            },
        })
        .unwrap();
        to_scad(&e)
    }

    #[test]
    fn a_countersink_is_a_cone_of_ninety_degrees_over_the_straight_hole() {
        let t = one_hole(
            Face::Front,
            3_400_000,
            HoleStyle::Countersunk {
                head_diameter: Length::new(6_300_000).unwrap(),
            },
        );
        assert!(
            t.contains("multmatrix([[1, 0, 0, 30], [0, 0, 1, 0], [0, 1, 0, 17], [0, 0, 0, 1]]) {")
        );
        assert!(t.contains("translate([0, 0, -1]) cylinder(h = 4, d = 3.4, $fn = 64);"));
        assert!(t.contains("cylinder(h = (8.3 - 3.4) / 2, d1 = 8.3, d2 = 3.4, $fn = 64);"));
    }

    #[test]
    fn a_counterbore_is_a_wide_cylinder_as_deep_as_the_recess() {
        let t = one_hole(
            Face::Top,
            3_400_000,
            HoleStyle::Counterbored {
                head_diameter: mm(6),
                depth: mm(1),
            },
        );
        assert!(t.contains(
            "multmatrix([[1, 0, 0, 30], [0, 1, 0, 17], [0, 0, -1, 35], [0, 0, 0, 1]]) {"
        ));
        assert!(t.contains("cylinder(h = 5, d = 3.4, $fn = 64);"));
        assert!(t.contains("cylinder(h = 2, d = 6, $fn = 64);"));
    }

    #[test]
    fn a_rectangle_and_a_slot_are_drawn_across_u_and_v() {
        let rect = one_hole(
            Face::Front,
            10_000_000,
            HoleStyle::Rectangular { height: mm(4) },
        );
        assert!(rect.contains("translate([-10 / 2, -4 / 2, -1]) cube([10, 4, 4]);"));
        let slot = one_hole(
            Face::Front,
            4_000_000,
            HoleStyle::Oval {
                length: mm(12),
                vertical: false,
            },
        );
        assert!(slot.contains("hull() { translate([-(12 - 4) / 2, 0, -1]) cylinder("));
        assert!(slot.contains("translate([(12 - 4) / 2, 0, -1]) cylinder(h = 4, d = 4, $fn = 64);"));
        let upright = one_hole(
            Face::Front,
            4_000_000,
            HoleStyle::Oval {
                length: mm(12),
                vertical: true,
            },
        );
        assert!(upright.contains("hull() { translate([0, -(12 - 4) / 2, -1]) cylinder("));
    }

    #[test]
    fn rounded_and_chamfered_edges_are_cut_along_their_whole_length() {
        let sharp = to_scad(&enclosure());
        assert!(!sharp.contains("linear_extrude"));
        let mut e = enclosure();
        e.apply(Operation::SetEdging(crate::enclosure::Edging {
            vertical: Treatment::Round(mm(5)),
            top: Treatment::Chamfer(mm(2)),
            bottom: Treatment::Sharp,
        }))
        .unwrap();
        let t = to_scad(&e);
        assert!(t.contains("// vertical edge, front left"));
        assert!(t.contains(
            "multmatrix([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]) translate([0, 0, -1]) linear_extrude(height = 37) difference() { square([5, 5]); translate([5, 5]) circle(r = 5, $fn = 64); };"
        ));
        assert!(t.contains("multmatrix([[-1, 0, 0, 104], [0, 1, 0, 0]"));
        assert_eq!(t.matches("// vertical edge").count(), 4);
        assert_eq!(t.matches("// lid edge").count(), 4);
        assert!(!t.contains("// floor edge"));
        assert!(t.contains(
            "multmatrix([[0, 0, 1, 0], [1, 0, 0, 0], [0, -1, 0, 35], [0, 0, 0, 1]]) translate([0, 0, -1]) linear_extrude(height = 106) polygon([[0, 0], [2, 0], [0, 2]]);"
        ));
        // the cuts come before the holes and the text is the same for the same model
        assert_eq!(t, to_scad(&e));
    }

    #[test]
    fn the_frame_of_every_face_goes_into_the_material() {
        let e = enclosure();
        let hole = |face: Face| Hole {
            name: "f".into(),
            face,
            u: mm(30),
            v: mm(17),
            diameter: mm(4),
            style: HoleStyle::Plain,
        };
        // the third axis of the frame points from the face into the box
        for (face, inward) in [
            (Face::Front, (0, 1, 0)),
            (Face::Back, (0, -1, 0)),
            (Face::Left, (1, 0, 0)),
            (Face::Right, (-1, 0, 0)),
            (Face::Top, (0, 0, -1)),
            (Face::Bottom, (0, 0, 1)),
        ] {
            let (_, axes) = hole_frame(&e, &hole(face));
            assert_eq!(axes[2], inward, "{face:?}");
        }
    }
}
