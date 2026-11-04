//! The enclosure as an OpenSCAD-style script.
//!
//! The text is a pure function of the model: the same enclosure always gives the same text. It
//! holds no date, no host and no program version. Lengths are written in millimetres with the
//! digits they have (at most six decimals, which is exactly one nanometre), never rounded.

use std::fmt::Write as _;

use crate::enclosure::{Enclosure, Face, Hole};

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

fn hole_text(enclosure: &Enclosure, hole: &Hole) -> String {
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
}
