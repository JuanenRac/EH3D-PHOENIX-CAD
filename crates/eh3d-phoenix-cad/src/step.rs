//! The enclosure as a STEP file (ISO 10303-21, a boundary representation).
//!
//! The solid is the outer box with the inner space as a void (`BREP_WITH_VOIDS`) and a round hole
//! through the wall for each hole of the model: the hole is a cylindrical face between two circular
//! edges, one on the outer face and one on the face of the inner space. Planar faces have the
//! rectangle as their outer bound and the circles of their holes as further bounds.
//!
//! The text is a pure function of the model: no date, no host, no program version. Lengths are in
//! millimetres with the digits they have (at most six decimals, and seven for a radius of an odd
//! number of nanometres), never rounded. Entities are numbered in the order they are made.
//!
//! The file has only been checked for its structure (header, entity references, counts); no CAD
//! program has opened it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::enclosure::{Enclosure, Face};

/// A point in twice its nanometre coordinates, so that a radius of half a nanometre is exact.
type P = (i128, i128, i128);

/// A coordinate given in twice the nanometres, as a STEP real in millimetres.
fn real2(twice: i128) -> String {
    let sign = if twice < 0 { "-" } else { "" };
    let a = twice.unsigned_abs();
    let (nm, half) = (a / 2, a % 2);
    let (whole, frac) = (nm / 1_000_000, nm % 1_000_000);
    let digits = format!("{frac:06}{}", if half == 1 { "5" } else { "" });
    let digits = digits.trim_end_matches('0');
    format!("{sign}{whole}.{digits}")
}

fn unit(v: i8) -> &'static str {
    match v {
        -1 => "-1.",
        0 => "0.",
        _ => "1.",
    }
}

#[derive(Default)]
struct Builder {
    lines: Vec<String>,
}

impl Builder {
    fn add(&mut self, body: String) -> usize {
        self.lines.push(body);
        self.lines.len()
    }

    fn point(&mut self, p: P) -> usize {
        self.add(format!(
            "CARTESIAN_POINT('',({},{},{}))",
            real2(p.0),
            real2(p.1),
            real2(p.2)
        ))
    }

    fn direction(&mut self, d: (i8, i8, i8)) -> usize {
        self.add(format!(
            "DIRECTION('',({},{},{}))",
            unit(d.0),
            unit(d.1),
            unit(d.2)
        ))
    }

    fn axis(&mut self, origin: P, axis: (i8, i8, i8), refdir: (i8, i8, i8)) -> usize {
        let (p, a, r) = (
            self.point(origin),
            self.direction(axis),
            self.direction(refdir),
        );
        self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{a},#{r})"))
    }

    fn vertex(&mut self, p: P) -> usize {
        let point = self.point(p);
        self.add(format!("VERTEX_POINT('',#{point})"))
    }

    /// A loop of one closed edge, traversed forwards or backwards.
    fn loop_of(&mut self, edges: &[(usize, bool)]) -> usize {
        let oriented: Vec<String> = edges
            .iter()
            .map(|(edge, forward)| {
                let id = self.add(format!(
                    "ORIENTED_EDGE('',*,*,#{edge},{})",
                    if *forward { ".T." } else { ".F." }
                ));
                format!("#{id}")
            })
            .collect();
        self.add(format!("EDGE_LOOP('',({}))", oriented.join(",")))
    }
}

/// The unit direction and the position of a hole: the point on the outer face, the outward normal
/// and the thickness the hole crosses.
fn hole_place(e: &Enclosure, h: &crate::enclosure::Hole) -> (P, (i8, i8, i8), i64) {
    let p = e.params();
    let (x, y, z) = p.outer();
    let (x, y, z) = (
        i128::from(x.get()),
        i128::from(y.get()),
        i128::from(z.get()),
    );
    let (u, v) = (i128::from(h.u.get()), i128::from(h.v.get()));
    let (at, n, t) = match h.face {
        Face::Front => ((u, 0, v), (0, -1, 0), p.wall.get()),
        Face::Back => ((x - u, y, v), (0, 1, 0), p.wall.get()),
        Face::Left => ((0, y - u, v), (-1, 0, 0), p.wall.get()),
        Face::Right => ((x, u, v), (1, 0, 0), p.wall.get()),
        Face::Top => ((u, v, z), (0, 0, 1), p.lid.get()),
        Face::Bottom => ((u, y - v, 0), (0, 0, -1), p.floor.get()),
    };
    ((2 * at.0, 2 * at.1, 2 * at.2), n, t)
}

/// The corners of a box are numbered `x + 2 y + 4 z` with 0 for the minimum and 1 for the maximum.
const FACES: [(Face, [usize; 4], (i8, i8, i8)); 6] = [
    (Face::Bottom, [0, 2, 3, 1], (0, 0, -1)),
    (Face::Top, [4, 5, 7, 6], (0, 0, 1)),
    (Face::Front, [0, 1, 5, 4], (0, -1, 0)),
    (Face::Back, [3, 2, 6, 7], (0, 1, 0)),
    (Face::Left, [0, 4, 6, 2], (-1, 0, 0)),
    (Face::Right, [1, 3, 7, 5], (1, 0, 0)),
];

/// The twelve edges, the eight corners and the six faces of a box. `circles` gives, per face, the
/// closed circular edges of its holes. Returns the ids of the faces in the order of [`FACES`].
fn box_faces(b: &mut Builder, min: P, max: P, circles: &BTreeMap<Face, Vec<usize>>) -> Vec<usize> {
    let corner = |i: usize| -> P {
        (
            if i & 1 != 0 { max.0 } else { min.0 },
            if i & 2 != 0 { max.1 } else { min.1 },
            if i & 4 != 0 { max.2 } else { min.2 },
        )
    };
    let vertices: Vec<usize> = (0..8).map(|i| b.vertex(corner(i))).collect();
    let mut edges: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for a in 0..8usize {
        for bit in [1usize, 2, 4] {
            if a & bit != 0 {
                continue;
            }
            let c = a | bit;
            let (pa, pc) = (corner(a), corner(c));
            let length = (pc.0 - pa.0) + (pc.1 - pa.1) + (pc.2 - pa.2);
            let dir = (i8::from(bit == 1), i8::from(bit == 2), i8::from(bit == 4));
            let start = b.point(pa);
            let d = b.direction(dir);
            let vector = b.add(format!("VECTOR('',#{d},{})", real2(length)));
            let line = b.add(format!("LINE('',#{start},#{vector})"));
            let edge = b.add(format!(
                "EDGE_CURVE('',#{},#{},#{line},.T.)",
                vertices[a], vertices[c]
            ));
            edges.insert((a, c), edge);
        }
    }
    let mut faces = Vec::new();
    for (face, cycle, normal) in FACES {
        let sides: Vec<(usize, bool)> = (0..4)
            .map(|k| {
                let (from, to) = (cycle[k], cycle[(k + 1) % 4]);
                (edges[&(from.min(to), from.max(to))], from < to)
            })
            .collect();
        let outer_loop = b.loop_of(&sides);
        let mut bounds = vec![b.add(format!("FACE_OUTER_BOUND('',#{outer_loop},.T.)"))];
        for circle in circles.get(&face).map(Vec::as_slice).unwrap_or(&[]) {
            let hole_loop = b.loop_of(&[(*circle, false)]);
            bounds.push(b.add(format!("FACE_BOUND('',#{hole_loop},.T.)")));
        }
        let (p0, p1) = (corner(cycle[0]), corner(cycle[1]));
        let sign = |d: i128| i8::from(d > 0) - i8::from(d < 0);
        let refdir = (sign(p1.0 - p0.0), sign(p1.1 - p0.1), sign(p1.2 - p0.2));
        let placement = b.axis(p0, normal, refdir);
        let plane = b.add(format!("PLANE('',#{placement})"));
        let list: Vec<String> = bounds.iter().map(|i| format!("#{i}")).collect();
        faces.push(b.add(format!(
            "ADVANCED_FACE('',({}),#{plane},.T.)",
            list.join(",")
        )));
    }
    faces
}

/// A closed circular edge: its circle and the vertex where it starts and ends.
fn circle_edge(b: &mut Builder, centre: P, normal: (i8, i8, i8), radius2: i128) -> usize {
    let refdir = if normal.0 != 0 { (0, 1, 0) } else { (1, 0, 0) };
    let placement = b.axis(centre, normal, refdir);
    let circle = b.add(format!("CIRCLE('',#{placement},{})", real2(radius2)));
    let start = (
        centre.0 + i128::from(refdir.0) * radius2,
        centre.1 + i128::from(refdir.1) * radius2,
        centre.2 + i128::from(refdir.2) * radius2,
    );
    let vertex = b.vertex(start);
    b.add(format!("EDGE_CURVE('',#{vertex},#{vertex},#{circle},.T.)"))
}

/// The enclosure as STEP text.
pub fn to_step(enclosure: &Enclosure) -> String {
    let p = enclosure.params();
    let (x, y, z) = p.outer();
    let n = |v: i64| 2 * i128::from(v);
    let mut b = Builder::default();

    // the circles of the holes: one on the outer face, one on the face of the inner space
    let mut outer_circles: BTreeMap<Face, Vec<usize>> = BTreeMap::new();
    let mut inner_circles: BTreeMap<Face, Vec<usize>> = BTreeMap::new();
    let mut hole_ends = Vec::new();
    for hole in enclosure.holes().values() {
        let (centre, normal, thickness) = hole_place(enclosure, hole);
        let radius2 = i128::from(hole.diameter.get());
        let inner_centre = (
            centre.0 - i128::from(normal.0) * 2 * i128::from(thickness),
            centre.1 - i128::from(normal.1) * 2 * i128::from(thickness),
            centre.2 - i128::from(normal.2) * 2 * i128::from(thickness),
        );
        let outer = circle_edge(&mut b, centre, normal, radius2);
        let inner = circle_edge(&mut b, inner_centre, normal, radius2);
        outer_circles.entry(hole.face).or_default().push(outer);
        inner_circles.entry(hole.face).or_default().push(inner);
        hole_ends.push((centre, normal, radius2, outer, inner));
    }

    let outer_min = (0, 0, 0);
    let outer_max = (n(x.get()), n(y.get()), n(z.get()));
    let inner_min = (n(p.wall.get()), n(p.wall.get()), n(p.floor.get()));
    let inner_max = (
        n(p.wall.get() + p.inner_length.get()),
        n(p.wall.get() + p.inner_width.get()),
        n(p.floor.get() + p.inner_height.get()),
    );
    let mut faces = box_faces(&mut b, outer_min, outer_max, &outer_circles);
    let cavity = box_faces(&mut b, inner_min, inner_max, &inner_circles);

    // the wall of each hole: a cylinder between the two circles, its normal pointing to the axis
    for (centre, normal, radius2, outer, inner) in hole_ends {
        let placement = b.axis(
            centre,
            normal,
            if normal.0 != 0 { (0, 1, 0) } else { (1, 0, 0) },
        );
        let cylinder = b.add(format!(
            "CYLINDRICAL_SURFACE('',#{placement},{})",
            real2(radius2)
        ));
        let l1 = b.loop_of(&[(outer, true)]);
        let l2 = b.loop_of(&[(inner, true)]);
        let b1 = b.add(format!("FACE_OUTER_BOUND('',#{l1},.T.)"));
        let b2 = b.add(format!("FACE_BOUND('',#{l2},.T.)"));
        faces.push(b.add(format!("ADVANCED_FACE('',(#{b1},#{b2}),#{cylinder},.F.)")));
    }

    let list = |ids: &[usize]| -> String {
        ids.iter()
            .map(|i| format!("#{i}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let shell = b.add(format!("CLOSED_SHELL('',({}))", list(&faces)));
    let void_shell = b.add(format!("CLOSED_SHELL('',({}))", list(&cavity)));
    let oriented_void = b.add(format!("ORIENTED_CLOSED_SHELL('',*,#{void_shell},.F.)"));
    let brep = b.add(format!(
        "BREP_WITH_VOIDS('enclosure',#{shell},(#{oriented_void}))"
    ));

    // the product structure and the units
    let origin = b.axis((0, 0, 0), (0, 0, 1), (1, 0, 0));
    let unc_unit = b.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".to_owned());
    let angle = b.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))".to_owned());
    let solid = b.add("(NAMED_UNIT(*) SOLID_ANGLE_UNIT() SI_UNIT($,.STERADIAN.))".to_owned());
    let uncertainty = b.add(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{unc_unit},'distance_accuracy_value','')"
    ));
    let context = b.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unc_unit},#{angle},#{solid})) REPRESENTATION_CONTEXT('enclosure','3D'))"
    ));
    let representation = b.add(format!(
        "ADVANCED_BREP_SHAPE_REPRESENTATION('enclosure',(#{origin},#{brep}),#{context})"
    ));
    let app = b.add(
        "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".to_owned(),
    );
    b.add(format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"
    ));
    let product_context = b.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let product = b.add(format!(
        "PRODUCT('enclosure','enclosure','',(#{product_context}))"
    ));
    b.add(format!(
        "PRODUCT_RELATED_PRODUCT_CATEGORY('part','',(#{product}))"
    ));
    let formation = b.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
    let definition_context = b.add(format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"
    ));
    let definition = b.add(format!(
        "PRODUCT_DEFINITION('design','',#{formation},#{definition_context})"
    ));
    let shape = b.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
    b.add(format!(
        "SHAPE_DEFINITION_REPRESENTATION(#{shape},#{representation})"
    ));

    let mut out = String::new();
    out.push_str("ISO-10303-21;\nHEADER;\n");
    out.push_str("FILE_DESCRIPTION(('EH3D-PHOENIX-CAD enclosure'),'2;1');\n");
    out.push_str("FILE_NAME('enclosure.step','',(''),(''),'EH3D-PHOENIX-CAD','','');\n");
    out.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\n");
    out.push_str("ENDSEC;\nDATA;\n");
    for (i, body) in b.lines.iter().enumerate() {
        let _ = writeln!(out, "#{}={};", i + 1, body);
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, Hole, Operation};
    use crate::Length;
    use eh3d_container::ObjectId;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn enclosure(holes: &[(Face, i64, i64, i64)]) -> Enclosure {
        let mut e = Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap();
        for (i, (face, u, v, d)) in holes.iter().enumerate() {
            e.apply(Operation::AddHole {
                id: ObjectId::from_parts(1_700_000_000_000, i as u16 + 1, i as u64 + 7).unwrap(),
                hole: Hole {
                    name: format!("h{i}"),
                    face: *face,
                    u: mm(*u),
                    v: mm(*v),
                    diameter: mm(*d),
                },
            })
            .unwrap();
        }
        e
    }

    fn count(text: &str, word: &str) -> usize {
        text.lines()
            .filter(|l| l.split_once('=').is_some_and(|(_, b)| b.starts_with(word)))
            .count()
    }

    #[test]
    fn reals_are_exact() {
        assert_eq!(real2(0), "0.");
        assert_eq!(real2(2_000_000), "1.");
        assert_eq!(real2(3_000_000), "1.5");
        assert_eq!(real2(-3_000_000), "-1.5");
        assert_eq!(real2(1), "0.0000005");
        assert_eq!(real2(2), "0.000001");
        assert_eq!(real2(-2_000_002), "-1.000001");
    }

    #[test]
    fn the_file_has_a_header_a_data_section_and_an_end() {
        let text = to_step(&enclosure(&[]));
        assert!(text.starts_with("ISO-10303-21;\nHEADER;\n"));
        assert!(text.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"));
        assert_eq!(text.matches("DATA;").count(), 1);
        assert!(text.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN"));
    }

    #[test]
    fn a_box_without_holes_has_twelve_faces_and_one_void() {
        let text = to_step(&enclosure(&[]));
        assert_eq!(count(&text, "ADVANCED_FACE"), 12);
        assert_eq!(count(&text, "PLANE"), 12);
        assert_eq!(count(&text, "EDGE_CURVE"), 24);
        assert_eq!(count(&text, "VERTEX_POINT"), 16);
        assert_eq!(count(&text, "BREP_WITH_VOIDS"), 1);
        assert_eq!(count(&text, "ORIENTED_CLOSED_SHELL"), 1);
        assert_eq!(count(&text, "CLOSED_SHELL"), 2);
        assert_eq!(count(&text, "CYLINDRICAL_SURFACE"), 0);
        assert_eq!(count(&text, "FACE_BOUND"), 0);
    }

    #[test]
    fn each_hole_adds_a_cylinder_a_face_and_two_circles() {
        let holes = [
            (Face::Front, 20, 15, 8),
            (Face::Front, 60, 15, 8),
            (Face::Top, 30, 30, 10),
            (Face::Left, 30, 15, 6),
        ];
        let text = to_step(&enclosure(&holes));
        assert_eq!(count(&text, "ADVANCED_FACE"), 12 + 4);
        assert_eq!(count(&text, "CYLINDRICAL_SURFACE"), 4);
        assert_eq!(count(&text, "CIRCLE"), 8);
        // the circles are closed edges: the same vertex at both ends
        let closed = text
            .lines()
            .filter(|l| {
                let Some((_, body)) = l.split_once("EDGE_CURVE('',#") else {
                    return false;
                };
                let mut ids = body.split(',');
                let a = ids.next().unwrap_or("");
                ids.next().is_some_and(|b| b.trim_start_matches('#') == a)
            })
            .count();
        assert_eq!(closed, 8);
        // each circle is a bound of a planar face and of a cylinder: 8 holes bounds on planes,
        // 4 cylinders with two bounds each (one outer bound, one more)
        assert_eq!(count(&text, "FACE_BOUND"), 8 + 4);
        assert_eq!(count(&text, "FACE_OUTER_BOUND"), 12 + 4);
    }

    #[test]
    fn every_reference_points_to_an_entity_made_before_it() {
        let text = to_step(&enclosure(&[
            (Face::Back, 40, 10, 7),
            (Face::Bottom, 50, 30, 12),
        ]));
        let data = text.split("DATA;\n").nth(1).unwrap();
        let mut defined = 0usize;
        for line in data.lines().take_while(|l| *l != "ENDSEC;") {
            let (id, body) = line.split_once('=').unwrap();
            let id: usize = id.trim_start_matches('#').parse().unwrap();
            assert_eq!(id, defined + 1, "numbered in order");
            let mut rest = body;
            while let Some(i) = rest.find('#') {
                let digits: String = rest[i + 1..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                let target: usize = digits.parse().unwrap();
                assert!(target <= defined, "{line} refers to a later entity");
                rest = &rest[i + 1..];
            }
            assert!(line.ends_with(';'));
            defined = id;
        }
        assert!(defined > 100);
    }

    #[test]
    fn the_same_model_gives_the_same_text_whatever_the_order_of_the_operations() {
        let a = to_step(&enclosure(&[
            (Face::Front, 20, 15, 8),
            (Face::Top, 30, 30, 10),
        ]));
        let b = to_step(&enclosure(&[
            (Face::Front, 20, 15, 8),
            (Face::Top, 30, 30, 10),
        ]));
        assert_eq!(a, b);
        let c = to_step(&enclosure(&[(Face::Front, 20, 15, 9)]));
        assert_ne!(a, c);
    }

    #[test]
    fn the_outer_size_is_in_the_corner_points() {
        let text = to_step(&enclosure(&[]));
        // outer size 104 x 64 x 35 mm, inner space from (2, 2, 2) to (102, 62, 32)
        assert!(text.contains("CARTESIAN_POINT('',(104.,64.,35.))"));
        assert!(text.contains("CARTESIAN_POINT('',(2.,2.,2.))"));
        assert!(text.contains("CARTESIAN_POINT('',(102.,62.,32.))"));
    }
}
