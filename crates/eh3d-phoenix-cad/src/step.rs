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
//! Only the box with straight round holes and straight posts with a plain bore is drawn:
//! [`to_step`] writes that and nothing else, so for a model with rounded or chamfered edges, with a
//! countersunk, counterbored, rectangular or oval hole, with the reinforced foot of a post or with
//! the centring lip of a lid use [`export_step`], which refuses such a model (`PHOENIX-CAD-022`)
//! instead of writing a file that would not show it. The script of [`crate::scad`] draws all of
//! them. A post is part of the void: its wall, its top ring, the wall of its bore and the bottom of
//! the bore close the void, and the floor of the inner space has a circle where the post stands.
//!
//! The file has only been checked for its structure (header, entity references, counts); no CAD
//! program has opened it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::enclosure::{Enclosure, Face, HoleStyle};
use crate::CadError;

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

/// The four circles of a post (its foot, its top, the top of its bore and the bottom of its bore)
/// and the centres and diameters that its faces need.
struct PostRing {
    top_centre: P,
    bore_centre: P,
    outer: i128,
    bore: i128,
    foot_edge: usize,
    top_edge: usize,
    bore_top_edge: usize,
    bore_bottom_edge: usize,
}

/// One face of the box: which face, its four corners in order and the direction its normal points to.
type FaceSpec = (Face, [usize; 4], (i8, i8, i8));

/// The corners of a box are numbered `x + 2 y + 4 z` with 0 for the minimum and 1 for the maximum.
const FACES: [FaceSpec; 6] = [
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
    let all_holes = enclosure.all_holes();
    for hole in &all_holes {
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
    // the circles of the posts: the foot is a bound of the floor of the inner space
    let mut rings: Vec<PostRing> = Vec::new();
    for post in enclosure.posts().values() {
        let cx = 2 * i128::from(p.wall.get() + post.x.get());
        let cy = 2 * i128::from(p.wall.get() + post.y.get());
        let floor = i128::from(p.floor.get());
        let top_z = 2 * (floor + i128::from(post.height.get()));
        let bore_z = 2 * (floor + i128::from(post.height.get() - post.bore_depth.get()));
        let (outer, bore) = (
            i128::from(post.outer_diameter.get()),
            i128::from(post.bore_diameter.get()),
        );
        let up = (0, 0, 1);
        let foot_edge = circle_edge(&mut b, (cx, cy, 2 * floor), up, outer);
        let top_edge = circle_edge(&mut b, (cx, cy, top_z), up, outer);
        let bore_top_edge = circle_edge(&mut b, (cx, cy, top_z), up, bore);
        let bore_bottom_edge = circle_edge(&mut b, (cx, cy, bore_z), up, bore);
        inner_circles
            .entry(Face::Bottom)
            .or_default()
            .push(foot_edge);
        rings.push(PostRing {
            top_centre: (cx, cy, top_z),
            bore_centre: (cx, cy, bore_z),
            outer,
            bore,
            foot_edge,
            top_edge,
            bore_top_edge,
            bore_bottom_edge,
        });
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
    let mut cavity = box_faces(&mut b, inner_min, inner_max, &inner_circles);

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

    // the faces of the posts close the void: the wall of the post and the wall of its bore are
    // cylinders, the top ring and the bottom of the bore are planes whose normal points down
    for ring in rings {
        let along = (0, 0, 1);
        let down = (0, 0, -1);
        let across = (1, 0, 0);
        let wall_axis = b.axis(ring.top_centre, along, across);
        let wall = b.add(format!(
            "CYLINDRICAL_SURFACE('',#{wall_axis},{})",
            real2(ring.outer)
        ));
        let l1 = b.loop_of(&[(ring.top_edge, true)]);
        let l2 = b.loop_of(&[(ring.foot_edge, true)]);
        let b1 = b.add(format!("FACE_OUTER_BOUND('',#{l1},.T.)"));
        let b2 = b.add(format!("FACE_BOUND('',#{l2},.T.)"));
        cavity.push(b.add(format!("ADVANCED_FACE('',(#{b1},#{b2}),#{wall},.F.)")));

        let ring_axis = b.axis(ring.top_centre, down, across);
        let ring_plane = b.add(format!("PLANE('',#{ring_axis})"));
        let l3 = b.loop_of(&[(ring.top_edge, false)]);
        let l4 = b.loop_of(&[(ring.bore_top_edge, false)]);
        let b3 = b.add(format!("FACE_OUTER_BOUND('',#{l3},.T.)"));
        let b4 = b.add(format!("FACE_BOUND('',#{l4},.T.)"));
        cavity.push(b.add(format!("ADVANCED_FACE('',(#{b3},#{b4}),#{ring_plane},.T.)")));

        let bore_axis = b.axis(ring.top_centre, along, across);
        let bore_wall = b.add(format!(
            "CYLINDRICAL_SURFACE('',#{bore_axis},{})",
            real2(ring.bore)
        ));
        let l5 = b.loop_of(&[(ring.bore_top_edge, true)]);
        let l6 = b.loop_of(&[(ring.bore_bottom_edge, true)]);
        let b5 = b.add(format!("FACE_OUTER_BOUND('',#{l5},.T.)"));
        let b6 = b.add(format!("FACE_BOUND('',#{l6},.T.)"));
        cavity.push(b.add(format!("ADVANCED_FACE('',(#{b5},#{b6}),#{bore_wall},.T.)")));

        let bottom_axis = b.axis(ring.bore_centre, down, across);
        let bottom_plane = b.add(format!("PLANE('',#{bottom_axis})"));
        let l7 = b.loop_of(&[(ring.bore_bottom_edge, false)]);
        let b7 = b.add(format!("FACE_OUTER_BOUND('',#{l7},.T.)"));
        cavity.push(b.add(format!("ADVANCED_FACE('',(#{b7}),#{bottom_plane},.T.)")));
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

/// The enclosure as STEP text, or the error `PHOENIX-CAD-022` when the model has a shape that the
/// file does not draw: a rounded or chamfered edge, a hole that is not a straight round one (the
/// holes of the lid fixing included), the reinforced foot of a post or the centring lip of a lid.
pub fn export_step(enclosure: &Enclosure) -> Result<String, CadError> {
    if !enclosure.edging().is_sharp() {
        return Err(CadError::NotExported("rounded or chamfered edges".into()));
    }
    if let Some(hole) = enclosure
        .all_holes()
        .into_iter()
        .find(|h| h.style != HoleStyle::Plain)
    {
        return Err(CadError::NotExported(format!(
            "the shape of hole {}",
            hole.name
        )));
    }
    if let Some(post) = enclosure.posts().values().find(|p| p.base.is_some()) {
        return Err(CadError::NotExported(format!(
            "the reinforced foot of post {}",
            post.name
        )));
    }
    if enclosure.lid().is_some_and(|lid| lid.lip.is_some()) {
        return Err(CadError::NotExported("the centring lip of the lid".into()));
    }
    Ok(to_step(enclosure))
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
                    style: crate::enclosure::HoleStyle::Plain,
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

    #[test]
    fn a_shape_that_the_file_does_not_draw_is_refused_instead_of_left_out() {
        let plain = enclosure(&[(Face::Front, 20, 15, 8)]);
        assert_eq!(export_step(&plain).unwrap(), to_step(&plain));
        let mut rounded = enclosure(&[]);
        rounded
            .apply(Operation::SetEdging(crate::enclosure::Edging {
                vertical: crate::enclosure::Treatment::Round(mm(5)),
                ..crate::enclosure::Edging::default()
            }))
            .unwrap();
        let err = export_step(&rounded).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-022");
        let mut slot = enclosure(&[]);
        slot.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: Hole {
                name: "slot".into(),
                face: Face::Front,
                u: mm(40),
                v: mm(17),
                diameter: mm(4),
                style: HoleStyle::Oval {
                    length: mm(12),
                    vertical: false,
                },
            },
        })
        .unwrap();
        let err = export_step(&slot).unwrap_err();
        assert_eq!(err, CadError::NotExported("the shape of hole slot".into()));
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
            id: ObjectId::from_parts(1_700_000_000_000, n as u16, n + 40).unwrap(),
            post: p,
        })
        .unwrap();
    }

    fn a_lid(style: HoleStyle, lip: Option<Lip>) -> Lid {
        Lid {
            fixing: LidFixing::Auto { inset: mm(6) },
            screw: "M3".into(),
            hole_diameter: Length::new(3_400_000).unwrap(),
            style,
            clearance: Length::new(200_000).unwrap(),
            lip,
        }
    }

    /// Every entity is numbered in order and refers only to earlier ones.
    fn assert_references_in_order(text: &str) {
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
            defined = id;
        }
    }

    #[test]
    fn a_post_closes_the_void_with_four_faces_two_cylinders_and_four_circles() {
        let mut e = enclosure(&[]);
        add_post(&mut e, 1, a_post("p", 25, 15));
        let text = to_step(&e);
        // the box has 12 faces; the post adds its wall, its top ring, the wall of its bore and
        // the bottom of its bore
        assert_eq!(count(&text, "ADVANCED_FACE"), 12 + 4);
        assert_eq!(count(&text, "CYLINDRICAL_SURFACE"), 2);
        assert_eq!(count(&text, "PLANE"), 12 + 2);
        // four circles, each a closed edge with its own vertex
        assert_eq!(count(&text, "CIRCLE"), 4);
        assert_eq!(count(&text, "EDGE_CURVE"), 24 + 4);
        assert_eq!(count(&text, "VERTEX_POINT"), 16 + 4);
        // the foot of the post is a bound of the floor; the wall, the ring and the wall of the
        // bore each have one more; the four faces of the post have an outer bound each
        assert_eq!(count(&text, "FACE_BOUND"), 4);
        assert_eq!(count(&text, "FACE_OUTER_BOUND"), 12 + 4);
        assert_eq!(count(&text, "CLOSED_SHELL"), 2);
        assert_eq!(count(&text, "BREP_WITH_VOIDS"), 1);
        // the centre of the post is at 2 + 25 and 2 + 15 mm; the floor is 2 mm thick, the post 6 mm
        // high and the bore 5 mm deep, so its bottom is 3 mm above the bottom of the box
        assert!(text.contains("CARTESIAN_POINT('',(27.,17.,2.))"));
        assert!(text.contains("CARTESIAN_POINT('',(27.,17.,8.))"));
        assert!(text.contains("CARTESIAN_POINT('',(27.,17.,3.))"));
        // the radii: 4 mm for the post, 2.1 mm for the bore
        let radii: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("CYLINDRICAL_SURFACE"))
            .map(|l| l.rsplit(',').next().unwrap_or(""))
            .collect();
        assert_eq!(radii, vec!["4.);", "2.1);"]);
        assert_references_in_order(&text);
    }

    #[test]
    fn each_post_adds_its_own_faces_and_the_text_does_not_depend_on_the_order() {
        let build = |order: [u64; 3]| {
            let mut e = enclosure(&[(Face::Front, 20, 15, 8)]);
            for n in order {
                add_post(&mut e, n, a_post(&format!("p{n}"), 20 * n as i64, 40));
            }
            e
        };
        let a = to_step(&build([1, 2, 3]));
        let b = to_step(&build([3, 1, 2]));
        assert_eq!(a, b);
        // the hole of the front adds one face and one cylinder
        assert_eq!(count(&a, "ADVANCED_FACE"), 12 + 1 + 3 * 4);
        assert_eq!(count(&a, "CYLINDRICAL_SURFACE"), 1 + 3 * 2);
        assert_references_in_order(&a);
    }

    #[test]
    fn a_post_is_exported_and_what_the_file_cannot_draw_is_refused() {
        let mut e = enclosure(&[]);
        add_post(&mut e, 1, a_post("p", 25, 15));
        assert_eq!(export_step(&e).unwrap(), to_step(&e));
        // the foot of a post is a cone
        let mut footed = enclosure(&[]);
        let mut p = a_post("p", 25, 15);
        p.base = Some(Base {
            diameter: mm(12),
            height: mm(2),
        });
        add_post(&mut footed, 1, p);
        let err = export_step(&footed).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-022");
        assert_eq!(
            err,
            CadError::NotExported("the reinforced foot of post p".into())
        );
        // the lip is a ring that the file does not draw
        let mut lipped = enclosure(&[]);
        let lip = Lip {
            thickness: Length::new(1_200_000).unwrap(),
            depth: mm(3),
        };
        lipped
            .apply(Operation::SetLid(Some(a_lid(HoleStyle::Plain, Some(lip)))))
            .unwrap();
        assert_eq!(
            export_step(&lipped).unwrap_err(),
            CadError::NotExported("the centring lip of the lid".into())
        );
    }

    #[test]
    fn the_holes_of_the_lid_are_drawn_when_they_are_plain_and_refused_when_they_are_not() {
        let mut plain = enclosure(&[]);
        plain
            .apply(Operation::SetLid(Some(a_lid(HoleStyle::Plain, None))))
            .unwrap();
        let text = export_step(&plain).unwrap();
        // the four corner holes, each a cylinder with its face
        assert_eq!(count(&text, "ADVANCED_FACE"), 12 + 4);
        assert_eq!(count(&text, "CYLINDRICAL_SURFACE"), 4);
        assert!(text.contains("CARTESIAN_POINT('',(8.,8.,35.))"));
        assert_references_in_order(&text);
        let mut sunk = enclosure(&[]);
        let head = HoleStyle::Countersunk {
            head_diameter: Length::new(6_300_000).unwrap(),
        };
        sunk.apply(Operation::SetLid(Some(a_lid(head, None))))
            .unwrap();
        assert_eq!(
            export_step(&sunk).unwrap_err(),
            CadError::NotExported("the shape of hole lid screw front left".into())
        );
    }
}
