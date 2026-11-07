//! The enclosure as an OpenSCAD-style script.
//!
//! The text is a pure function of the model: the same enclosure always gives the same text. It
//! holds no date, no host and no program version. Lengths are written in millimetres with the
//! digits they have (at most six decimals, which is exactly one nanometre), never rounded.

use std::fmt::Write as _;

use crate::catalog::{Head, PartInstance, Shape};
use crate::enclosure::{Enclosure, Face, Hole, HoleStyle, Treatment};
use crate::fins::FinSet;
use crate::mount::{Lid, Lip, Post};

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

/// A post: a cylinder that starts a little inside the floor so that the two solids overlap, with
/// the cone of its foot and the bore cut from its top.
fn post_text(enclosure: &Enclosure, post: &Post) -> String {
    let p = enclosure.params();
    let sunk = p.floor.get().min(1_000_000);
    let height = post.height.get() + sunk;
    let mut out = String::new();
    let _ = writeln!(out, "  // post {}", post.name);
    let _ = writeln!(
        out,
        "  translate([{}, {}, {}]) difference() {{",
        mm_text(p.wall.get() + post.x.get()),
        mm_text(p.wall.get() + post.y.get()),
        mm_text(p.floor.get() - sunk)
    );
    out.push_str("    union() {\n");
    let _ = writeln!(
        out,
        "      cylinder(h = {}, d = {}, $fn = {CIRCLE_SEGMENTS});",
        mm_text(height),
        mm_text(post.outer_diameter.get())
    );
    if let Some(base) = post.base {
        let _ = writeln!(
            out,
            "      translate([0, 0, {}]) cylinder(h = {}, d1 = {}, d2 = {}, $fn = {CIRCLE_SEGMENTS});",
            mm_text(sunk),
            mm_text(base.height.get()),
            mm_text(base.diameter.get()),
            mm_text(post.outer_diameter.get())
        );
    }
    out.push_str("    }\n");
    let _ = writeln!(
        out,
        "    translate([0, 0, {}]) cylinder(h = {}, d = {}, $fn = {CIRCLE_SEGMENTS});",
        mm_text(height - post.bore_depth.get()),
        mm_text(post.bore_depth.get() + 1_000_000),
        mm_text(post.bore_diameter.get())
    );
    out.push_str("  }\n");
    out
}

/// The centring lip on the underside of the lid: a rectangular ring, one clearance inside the
/// walls, that starts a little inside the lid so that the two solids overlap.
fn lip_text(enclosure: &Enclosure, lid: &Lid, lip: &Lip) -> String {
    let p = enclosure.params();
    let (c, t, depth) = (lid.clearance.get(), lip.thickness.get(), lip.depth.get());
    let (length, width) = (p.inner_length.get() - 2 * c, p.inner_width.get() - 2 * c);
    let overlap = p.lid.get().min(1_000_000);
    let mut out = String::new();
    out.push_str("  // centring lip under the lid\n");
    let _ = writeln!(
        out,
        "  translate([{}, {}, {}]) difference() {{",
        mm_text(p.wall.get() + c),
        mm_text(p.wall.get() + c),
        mm_text(p.floor.get() + p.inner_height.get() - depth)
    );
    let _ = writeln!(
        out,
        "    cube([{}, {}, {}]);",
        mm_text(length),
        mm_text(width),
        mm_text(depth + overlap)
    );
    let _ = writeln!(
        out,
        "    translate([{}, {}, -1]) cube([{}, {}, {}]);",
        mm_text(t),
        mm_text(t),
        mm_text(length - 2 * t),
        mm_text(width - 2 * t),
        mm_text(depth + overlap + 2_000_000)
    );
    out.push_str("  }\n");
    out
}

/// What is added to the box after its holes are cut: the posts and the lip.
/// The point of a face for a position `u`, `v` on it and a height `w` above it (outwards), in
/// the frame of the script (the faces are seen from outside, `u` to the right and `v` up).
fn face_point(enclosure: &Enclosure, face: Face, u: i64, v: i64, w: i64) -> [i64; 3] {
    let (x, y, z) = enclosure.params().outer();
    let (x, y, z) = (x.get(), y.get(), z.get());
    match face {
        Face::Front => [u, -w, v],
        Face::Back => [x - u, y + w, v],
        Face::Left => [-w, y - u, v],
        Face::Right => [x + w, u, v],
        Face::Top => [u, v, z + w],
        Face::Bottom => [u, y - v, -w],
    }
}

/// A set of cooling fins: one box for each fin, from a little inside the wall (so that the two
/// solids overlap) to the height of the fin.
fn fins_text(enclosure: &Enclosure, set: &FinSet) -> String {
    let p = enclosure.params();
    let thickness = match set.face {
        Face::Front | Face::Back | Face::Left | Face::Right => p.wall.get(),
        Face::Top => p.lid.get(),
        Face::Bottom => p.floor.get(),
    };
    let sunk = thickness.min(1_000_000);
    let height = set.height.get();
    let mut out = String::new();
    let _ = writeln!(out, "  // fins {}, {} face", set.name, set.face.name());
    for r in set.rects() {
        // the footprints are in nanometres doubled: halves are dropped
        let (u0, u1) = (r.u0.div_euclid(2) as i64, r.u1.div_euclid(2) as i64);
        let (v0, v1) = (r.v0.div_euclid(2) as i64, r.v1.div_euclid(2) as i64);
        let a = face_point(enclosure, set.face, u0, v0, -sunk);
        let b = face_point(enclosure, set.face, u1, v1, height);
        let lo = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let hi = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        let _ = writeln!(
            out,
            "  translate([{}, {}, {}]) cube([{}, {}, {}]);",
            mm_text(lo[0]),
            mm_text(lo[1]),
            mm_text(lo[2]),
            mm_text(hi[0] - lo[0]),
            mm_text(hi[1] - lo[1]),
            mm_text(hi[2] - lo[2])
        );
    }
    out
}

/// A part of the catalog: its shape in its own frame (the smallest corner of its bounding box at
/// the origin), placed with the `multmatrix` of its orientation so that the smallest corner of the
/// turned part lies at its position.
fn part_text(part: &PartInstance) -> String {
    let (_, shift) = part.extent();
    let cols = part.orientation.columns();
    let dir = |c: [i8; 3]| -> Dir { (c[0], c[1], c[2]) };
    let origin = (
        part.at[0].get() + shift[0],
        part.at[1].get() + shift[1],
        part.at[2].get() + shift[2],
    );
    let frame = frame_text(origin, [dir(cols[0]), dir(cols[1]), dir(cols[2])]);
    let size = part.part.size(part.cut);
    let [sx, sy, sz] = size.map(mm_text);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "  // part {}, {} of the library {}",
        part.name, part.part.name, part.library
    );
    let _ = writeln!(out, "  {frame} {{");
    match &part.part.shape {
        Shape::Profile { wall, .. } => {
            if wall.get() > 0 {
                let t = wall.get();
                let _ = writeln!(
                    out,
                    "    difference() {{ cube([{sx}, {sy}, {sz}]); translate([{}, {}, -1]) cube([{}, {}, {} + 2]); }}",
                    mm_text(t),
                    mm_text(t),
                    mm_text(size[0] - 2 * t),
                    mm_text(size[1] - 2 * t),
                    sz
                );
            } else {
                let _ = writeln!(out, "    cube([{sx}, {sy}, {sz}]);");
            }
        }
        Shape::Screw {
            diameter,
            head,
            head_diameter,
            head_height,
            ..
        } => {
            let (d, hd, hh) = (
                mm_text(diameter.get()),
                mm_text(head_diameter.get()),
                mm_text(head_height.get()),
            );
            let _ = writeln!(out, "    translate([{hd} / 2, {hd} / 2, 0]) {{");
            let _ = match head {
                Head::Cylinder => writeln!(
                    out,
                    "      cylinder(d = {hd}, h = {hh}, $fn = {CIRCLE_SEGMENTS});"
                ),
                Head::Countersunk => writeln!(
                    out,
                    "      cylinder(d1 = {hd}, d2 = {d}, h = {hh}, $fn = {CIRCLE_SEGMENTS});"
                ),
                Head::Hex => writeln!(
                    out,
                    "      cylinder(d = {hd} / cos(30), h = {hh}, $fn = 6);"
                ),
            };
            let _ = writeln!(
                out,
                "      cylinder(d = {d}, h = {sz}, $fn = {CIRCLE_SEGMENTS});"
            );
            out.push_str("    }\n");
        }
        Shape::Bracket {
            leg_a,
            leg_b,
            width,
            thickness,
            hole_diameter,
        } => {
            let (a, b, w, t) = (
                mm_text(leg_a.get()),
                mm_text(leg_b.get()),
                mm_text(width.get()),
                mm_text(thickness.get()),
            );
            let legs = format!("union() {{ cube([{a}, {w}, {t}]); cube([{t}, {w}, {b}]); }}");
            if hole_diameter.get() > 0 {
                let h = mm_text(hole_diameter.get());
                let _ = writeln!(out, "    difference() {{");
                let _ = writeln!(out, "      {legs}");
                let _ = writeln!(
                    out,
                    "      translate([{t} + ({a} - {t}) / 2, {w} / 2, -1]) cylinder(d = {h}, h = {t} + 2, $fn = {CIRCLE_SEGMENTS});"
                );
                let _ = writeln!(
                    out,
                    "      translate([-1, {w} / 2, {t} + ({b} - {t}) / 2]) rotate([0, 90, 0]) cylinder(d = {h}, h = {t} + 2, $fn = {CIRCLE_SEGMENTS});"
                );
                let _ = writeln!(out, "    }}");
            } else {
                let _ = writeln!(out, "    {legs}");
            }
        }
    }
    out.push_str("  }\n");
    out
}

fn mounting_text(enclosure: &Enclosure) -> String {
    let mut out = String::new();
    for post in enclosure.posts().values() {
        out.push_str(&post_text(enclosure, post));
    }
    for set in enclosure.fins().values() {
        out.push_str(&fins_text(enclosure, set));
    }
    for part in enclosure.placed_parts().values() {
        out.push_str(&part_text(part));
    }
    if let Some(lid) = enclosure.lid() {
        if let Some(lip) = &lid.lip {
            out.push_str(&lip_text(enclosure, lid, lip));
        }
    }
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
    if !enclosure.features().is_empty() {
        out.push_str("// The extrusions and pockets of sketches are not drawn in this script.\n");
    }
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
    let mut solid = String::new();
    solid.push_str("difference() {\n");
    let _ = writeln!(
        solid,
        "  cube([{}, {}, {}]);",
        mm_text(x.get()),
        mm_text(y.get()),
        mm_text(z.get())
    );
    solid.push_str(
        "  translate([wall, wall, floor]) cube([inner_length, inner_width, inner_height]);\n",
    );
    solid.push_str(&edge_cutters(enclosure));
    for hole in enclosure.all_holes() {
        solid.push_str(&hole_text(enclosure, &hole));
    }
    solid.push_str("}\n");
    let added = mounting_text(enclosure);
    if added.is_empty() {
        out.push_str(&solid);
    } else {
        out.push_str("union() {\n");
        out.push_str(&solid);
        out.push_str(&added);
        out.push_str("}\n");
    }
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

    use crate::mount::{Base, BoreKind, Lid, LidFixing, Lip, Post};

    fn a_post(name: &str, x: i64, y: i64) -> Post {
        Post {
            name: name.into(),
            board_hole: None,
            x: mm(x),
            y: mm(y),
            outer_diameter: mm(8),
            height: mm(6),
            kind: BoreKind::Insert,
            bore_diameter: Length::new(4_200_000).unwrap(),
            bore_depth: mm(5),
            base: None,
        }
    }

    fn add_post(e: &mut Enclosure, n: u64, p: Post) {
        e.apply(Operation::AddPost {
            id: ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap(),
            post: p,
        })
        .unwrap();
    }

    fn a_lid(lip: Option<Lip>) -> Lid {
        Lid {
            fixing: LidFixing::Auto { inset: mm(6) },
            screw: "M3".into(),
            hole_diameter: Length::new(3_400_000).unwrap(),
            style: HoleStyle::Countersunk {
                head_diameter: Length::new(6_300_000).unwrap(),
            },
            clearance: Length::new(200_000).unwrap(),
            lip,
        }
    }

    #[test]
    fn a_box_without_posts_or_a_lip_is_not_wrapped_in_a_union() {
        assert!(!to_scad(&enclosure()).contains("union"));
    }

    #[test]
    fn a_post_is_a_cylinder_with_its_bore_added_to_the_box() {
        let mut e = enclosure();
        add_post(&mut e, 1, a_post("p", 25, 15));
        let t = to_scad(&e);
        // 2 mm of wall and 25 mm, 2 mm of wall and 15 mm, and the post starts 1 mm inside the
        // floor, which is 2 mm thick, so it is 7 mm long; its bore is 5 mm deep, from the top
        assert!(t.contains("  // post p\n"));
        assert!(t.contains("  translate([27, 17, 1]) difference() {\n"));
        assert!(t.contains("      cylinder(h = 7, d = 8, $fn = 64);\n"));
        assert!(t.contains("    translate([0, 0, 2]) cylinder(h = 6, d = 4.2, $fn = 64);\n"));
        assert!(!t.contains("d1 ="));
        assert!(t.contains("union() {\ndifference() {\n"));
        assert!(t.ends_with("  }\n}\n"));
    }

    #[test]
    fn the_foot_of_a_post_is_a_cone_from_the_floor() {
        let mut e = enclosure();
        let mut p = a_post("p", 25, 15);
        p.base = Some(Base {
            diameter: mm(12),
            height: mm(2),
        });
        add_post(&mut e, 1, p);
        let t = to_scad(&e);
        assert!(
            t.contains("      translate([0, 0, 1]) cylinder(h = 2, d1 = 12, d2 = 8, $fn = 64);\n")
        );
    }

    #[test]
    fn a_post_sinks_only_as_far_as_a_thin_floor_is_thick() {
        let mut e = Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: Length::new(500_000).unwrap(),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap();
        add_post(&mut e, 1, a_post("p", 25, 15));
        let t = to_scad(&e);
        // the post starts at the underside of the floor and is 6 + 0.5 mm long
        assert!(t.contains("  translate([27, 17, 0]) difference() {\n"));
        assert!(t.contains("      cylinder(h = 6.5, d = 8, $fn = 64);\n"));
    }

    #[test]
    fn the_centring_lip_is_a_ring_under_the_lid_one_clearance_inside_the_walls() {
        let mut e = enclosure();
        let lip = Lip {
            thickness: Length::new(1_200_000).unwrap(),
            depth: mm(3),
        };
        e.apply(Operation::SetLid(Some(a_lid(Some(lip))))).unwrap();
        let t = to_scad(&e);
        // 2 + 0.2 mm from the outside; it starts 2 + 30 - 3 mm above the bottom; the box inside
        // the walls is 100 by 60 mm, 0.4 mm less across the ring; the ring is 1.2 mm wide, so the
        // opening is 2.4 mm less again; the ring runs 1 mm into the lid
        assert!(t.contains("  // centring lip under the lid\n"));
        assert!(t.contains("  translate([2.2, 2.2, 29]) difference() {\n"));
        assert!(t.contains("    cube([99.6, 59.6, 4]);\n"));
        assert!(t.contains("    translate([1.2, 1.2, -1]) cube([97.2, 57.2, 6]);\n"));
        // without the lip there is nothing to add: the four corner holes alone
        let mut f = enclosure();
        f.apply(Operation::SetLid(Some(a_lid(None)))).unwrap();
        assert!(!to_scad(&f).contains("union"));
    }

    #[test]
    fn the_holes_of_the_lid_are_drawn_like_the_other_holes() {
        let mut e = enclosure();
        e.apply(Operation::SetLid(Some(a_lid(None)))).unwrap();
        let t = to_scad(&e);
        assert_eq!(t.matches("// hole lid screw").count(), 4);
        assert!(t.contains("// hole lid screw front left, top face\n"));
        // the top face, 8 mm from the left and from the front, on a box 35 mm high
        assert!(
            t.contains("multmatrix([[1, 0, 0, 8], [0, 1, 0, 8], [0, 0, -1, 35], [0, 0, 0, 1]]) {")
        );
        assert!(t.contains(
            "multmatrix([[1, 0, 0, 96], [0, 1, 0, 56], [0, 0, -1, 35], [0, 0, 0, 1]]) {"
        ));
    }

    #[test]
    fn the_same_posts_give_the_same_text_whatever_the_order_they_were_added_in() {
        let build = |order: [u64; 3]| {
            let mut e = enclosure();
            for n in order {
                add_post(&mut e, n, a_post(&format!("p{n}"), 20 * n as i64, 15));
            }
            e.apply(Operation::SetLid(Some(a_lid(None)))).unwrap();
            e
        };
        let (a, b) = (build([1, 2, 3]), build([3, 2, 1]));
        assert_eq!(a, b);
        assert_eq!(to_scad(&a), to_scad(&b));
        let t = to_scad(&a);
        let first = t.find("// post p1").unwrap();
        let second = t.find("// post p2").unwrap();
        let third = t.find("// post p3").unwrap();
        assert!(first < second && second < third);
        assert_eq!(t.matches("// hole lid screw p").count(), 3);
    }

    fn catalog_part(kind: &str) -> crate::catalog::Part {
        use crate::catalog::{Head, Part, Shape};
        let um = |v: i64| Length::from_um(v).unwrap();
        match kind {
            "tube" => Part {
                name: "tube-20x20".into(),
                material: "aluminium".into(),
                shape: Shape::Profile {
                    width: um(20_000),
                    height: um(20_000),
                    wall: um(1_500),
                    default_cut: um(300_000),
                    max_cut: um(2_000_000),
                },
            },
            "bolt" => Part {
                name: "bolt".into(),
                material: "steel".into(),
                shape: Shape::Screw {
                    metric: "M3".into(),
                    diameter: um(3_000),
                    length: um(10_000),
                    head: Head::Cylinder,
                    head_diameter: um(5_500),
                    head_height: um(3_000),
                },
            },
            "flat" => Part {
                name: "flat".into(),
                material: "steel".into(),
                shape: Shape::Screw {
                    metric: "M3".into(),
                    diameter: um(3_000),
                    length: um(8_000),
                    head: Head::Countersunk,
                    head_diameter: um(6_000),
                    head_height: um(1_700),
                },
            },
            "hex" => Part {
                name: "hex".into(),
                material: "stainless".into(),
                shape: Shape::Screw {
                    metric: "M4".into(),
                    diameter: um(4_000),
                    length: um(16_000),
                    head: Head::Hex,
                    head_diameter: um(7_000),
                    head_height: um(2_800),
                },
            },
            _ => Part {
                name: "angle-30".into(),
                material: "aluminium".into(),
                shape: Shape::Bracket {
                    leg_a: um(30_000),
                    leg_b: um(30_000),
                    width: um(20_000),
                    thickness: um(3_000),
                    hole_diameter: um(4_500),
                },
            },
        }
    }

    fn place(
        e: &mut Enclosure,
        n: u64,
        kind: &str,
        at: [i64; 3],
        axis: crate::catalog::Axis,
        roll: u8,
        cut: Option<i64>,
    ) {
        use crate::catalog::{Orientation, PartInstance};
        e.apply(Operation::AddPart {
            id: ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap(),
            part: PartInstance {
                name: format!("piece {n}"),
                library: "lib".into(),
                part: catalog_part(kind),
                at: at.map(mm),
                orientation: Orientation { axis, roll },
                cut: cut.map(mm),
            },
        })
        .unwrap();
    }

    #[test]
    fn a_hollow_profile_is_placed_with_the_matrix_of_its_orientation() {
        use crate::catalog::Axis;
        let mut e = enclosure();
        // the length runs along +x and the section is rolled a quarter turn: the local x goes to
        // +z, the local y to -y (so the part is moved up by its 20 mm there) and the local z to +x
        place(&mut e, 1, "tube", [10, -5, 2], Axis::PlusX, 1, Some(250));
        let text = to_scad(&e);
        let expect = "  // part piece 1, tube-20x20 of the library lib\n\
             \x20 multmatrix([[0, 0, 1, 10], [0, -1, 0, 15], [1, 0, 0, 2], [0, 0, 0, 1]]) {\n\
             \x20   difference() { cube([20, 20, 250]); translate([1.5, 1.5, -1]) cube([17, 17, 250 + 2]); }\n\
             \x20 }\n";
        assert!(text.contains(expect), "{text}");
        // a part makes the box a union with it
        assert!(text.contains("union() {\n"), "{text}");
        // the same model gives the same text
        assert_eq!(text, to_scad(&e.clone()));
    }

    #[test]
    fn a_solid_bar_is_one_cube_and_a_part_turned_the_other_way_is_moved_by_its_size() {
        use crate::catalog::{Axis, Part, Shape};
        let mut e = enclosure();
        let bar = Part {
            name: "bar".into(),
            material: "aluminium".into(),
            shape: Shape::Profile {
                width: mm(10),
                height: mm(30),
                wall: Length::ZERO,
                default_cut: mm(100),
                max_cut: mm(1000),
            },
        };
        e.apply(Operation::AddPart {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            part: crate::catalog::PartInstance {
                name: "bar".into(),
                library: "lib".into(),
                part: bar,
                at: [mm(1), mm(2), mm(3)],
                orientation: crate::catalog::Orientation {
                    axis: Axis::MinusZ,
                    roll: 0,
                },
                cut: Some(mm(100)),
            },
        })
        .unwrap();
        let text = to_scad(&e);
        // -z/0 turns the local y to -y and the local z to -z: moved by 30 in y and 100 in z
        assert!(
            text.contains("multmatrix([[1, 0, 0, 1], [0, -1, 0, 32], [0, 0, -1, 103], [0, 0, 0, 1]]) {\n    cube([10, 30, 100]);\n  }\n"),
            "{text}"
        );
    }

    #[test]
    fn the_three_heads_of_a_screw_and_the_bracket_with_its_holes() {
        use crate::catalog::Axis;
        let mut e = enclosure();
        place(&mut e, 1, "bolt", [0, 0, 0], Axis::PlusZ, 0, None);
        place(&mut e, 2, "flat", [20, 0, 0], Axis::PlusZ, 0, None);
        place(&mut e, 3, "hex", [40, 0, 0], Axis::PlusZ, 0, None);
        place(&mut e, 4, "angle", [60, 0, 0], Axis::PlusZ, 0, None);
        let text = to_scad(&e);
        for line in [
            "    translate([5.5 / 2, 5.5 / 2, 0]) {\n      cylinder(d = 5.5, h = 3, $fn = 64);\n      cylinder(d = 3, h = 13, $fn = 64);\n    }\n",
            "      cylinder(d1 = 6, d2 = 3, h = 1.7, $fn = 64);\n      cylinder(d = 3, h = 9.7, $fn = 64);\n",
            "      cylinder(d = 7 / cos(30), h = 2.8, $fn = 6);\n      cylinder(d = 4, h = 18.8, $fn = 64);\n",
            "      union() { cube([30, 20, 3]); cube([3, 20, 30]); }\n",
            "      translate([3 + (30 - 3) / 2, 20 / 2, -1]) cylinder(d = 4.5, h = 3 + 2, $fn = 64);\n",
            "      translate([-1, 20 / 2, 3 + (30 - 3) / 2]) rotate([0, 90, 0]) cylinder(d = 4.5, h = 3 + 2, $fn = 64);\n",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        // every block that is opened is closed
        assert_eq!(text.matches('{').count(), text.matches('}').count());
    }
}
