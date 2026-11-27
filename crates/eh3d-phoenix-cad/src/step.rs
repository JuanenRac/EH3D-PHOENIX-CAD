//! The enclosure as a STEP file (ISO 10303-21, a boundary representation).
//!
//! The solid is the outer box with the inner space as a void (`BREP_WITH_VOIDS`) and the holes of
//! the model through its walls. A round hole is a cylindrical face between two circular edges, one
//! on the outer face and one on the face of the inner space; a countersink is a conical face and a
//! counterbore a second cylinder with the flat ring at its bottom; a rectangular hole is four flat
//! faces and a slot two flat faces and two half cylinders. The four vertical edges of the box may
//! be rounded (quarter cylinders) or chamfered (flat faces), a post may have its conical foot and
//! the lid its centring lip. Planar faces have their outline as outer bound and the openings of
//! their holes as further bounds.
//!
//! The text is a pure function of the model: no date, no host, no program version. Lengths are in
//! millimetres with the digits they have (at most six decimals, and seven for a radius of an odd
//! number of nanometres), never rounded. Entities are numbered in the order they are made.
//!
//! The extrusions and pockets of sketches, the cooling fins, the placed parts, the turned and
//! swept bodies and the treatment of the edges around the lid or the floor are not drawn:
//! [`to_step`] leaves them out, so use [`export_step`], which refuses such a model
//! (`PHOENIX-CAD-022`) instead of writing a file that would not show it. The script of
//! [`crate::scad`] says what it draws. A post is part of the void: its wall (and the cone of its
//! foot), its top ring, the wall of its bore and the bottom of the bore close the void, and the
//! floor of the inner space has a circle where the post stands. The centring lip is part of the
//! void too: the ceiling of the inner space is split in the part round the lip and the part inside
//! it.
//!
//! The file has only been checked for its structure (header, entity references, counts); no CAD
//! program has opened it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::enclosure::{Enclosure, Face, HoleStyle, Treatment};
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

/// A real number of the file with nine decimals at most and no trailing zeros (`1.`, `0.5`).
fn real_f(v: f64) -> String {
    let text = format!("{:.9}", v + 0.0);
    let trimmed = text.trim_end_matches('0');
    if trimmed == "-0." {
        "0.".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// A name that fits between the quotes of a STEP text: printable ASCII without quotes and
/// backslashes, `prism` when nothing is left.
fn step_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '\'' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim_matches('_').is_empty() {
        "prism".to_owned()
    } else {
        cleaned
    }
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

    fn direction_f(&mut self, d: (f64, f64, f64)) -> usize {
        self.add(format!(
            "DIRECTION('',({},{},{}))",
            real_f(d.0),
            real_f(d.1),
            real_f(d.2)
        ))
    }

    fn axis_f(&mut self, origin: P, axis: (f64, f64, f64), refdir: (f64, f64, f64)) -> usize {
        let (p, a, r) = (
            self.point(origin),
            self.direction_f(axis),
            self.direction_f(refdir),
        );
        self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{a},#{r})"))
    }

    /// A straight edge between two vertices that lie at `from` and `to` (twice the nanometres).
    fn line_edge(&mut self, va: usize, vb: usize, from: P, to: P) -> usize {
        let d = (
            (to.0 - from.0) as f64,
            (to.1 - from.1) as f64,
            (to.2 - from.2) as f64,
        );
        let length = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt();
        let start = self.point(from);
        let dir = self.direction_f((d.0 / length, d.1 / length, d.2 / length));
        let vector = self.add(format!("VECTOR('',#{dir},{})", real_f(length / 2.0e6)));
        let line = self.add(format!("LINE('',#{start},#{vector})"));
        self.add(format!("EDGE_CURVE('',#{va},#{vb},#{line},.T.)"))
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

    /// Turns a face of the cavity round: its normal and the way of each of its bounds. A void
    /// shell is written with its faces as those of a solid; in the one shell of a box with holes
    /// they look the other way.
    fn turn_face(&mut self, face: usize) {
        let turned = |line: &str| match line.strip_suffix(".T.)") {
            Some(rest) => format!("{rest}.F.)"),
            None => format!("{}.T.)", line.strip_suffix(".F.)").unwrap_or(line)),
        };
        let line = self.lines[face - 1].clone();
        let bounds = line
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once("(#"))
            .and_then(|(_, rest)| rest.split_once(')'))
            .map_or("", |(list, _)| list);
        for id in bounds.split(",#") {
            if let Ok(id) = id.parse::<usize>() {
                self.lines[id - 1] = turned(&self.lines[id - 1]);
            }
        }
        self.lines[face - 1] = turned(&line);
    }
}

/// A point of the plane of a face: `u` to the right and `v` up, seen from outside, in twice the
/// nanometres.
type P2 = (i128, i128);

/// The way a face of the box lies: where its `u` and `v` start and go, and its outward normal.
#[derive(Debug, Clone, Copy)]
struct FaceFrame {
    origin: P,
    u: (i8, i8, i8),
    v: (i8, i8, i8),
    n: (i8, i8, i8),
}

impl FaceFrame {
    /// The frame of a face of a box of these outer sizes (twice the nanometres).
    fn of(face: Face, outer: P) -> FaceFrame {
        let (x, y, z) = outer;
        let (origin, u, v, n) = match face {
            Face::Front => ((0, 0, 0), (1, 0, 0), (0, 0, 1), (0, -1, 0)),
            Face::Back => ((x, y, 0), (-1, 0, 0), (0, 0, 1), (0, 1, 0)),
            Face::Left => ((0, y, 0), (0, -1, 0), (0, 0, 1), (-1, 0, 0)),
            Face::Right => ((x, 0, 0), (0, 1, 0), (0, 0, 1), (1, 0, 0)),
            Face::Top => ((0, 0, z), (1, 0, 0), (0, 1, 0), (0, 0, 1)),
            Face::Bottom => ((0, y, 0), (1, 0, 0), (0, -1, 0), (0, 0, -1)),
        };
        FaceFrame { origin, u, v, n }
    }

    /// The point at `p`, a depth into the material from the face.
    fn at(&self, p: P2, depth: i128) -> P {
        let along = |o: i128, u: i8, v: i8, n: i8| {
            o + i128::from(u) * p.0 + i128::from(v) * p.1 - i128::from(n) * depth
        };
        (
            along(self.origin.0, self.u.0, self.v.0, self.n.0),
            along(self.origin.1, self.u.1, self.v.1, self.n.1),
            along(self.origin.2, self.u.2, self.v.2, self.n.2),
        )
    }
}

/// A closed outline on the plane of a face.
#[derive(Debug, Clone)]
enum Opening {
    Circle {
        centre: P2,
        radius2: i128,
    },
    /// The corners counter-clockwise seen from outside, each with the centre and the radius of the
    /// arc to the next one, or none for a straight side.
    Path(Vec<(P2, Option<(P2, i128)>)>),
}

impl Opening {
    fn rectangle(low: P2, high: P2) -> Opening {
        Opening::Path(vec![
            ((low.0, low.1), None),
            ((high.0, low.1), None),
            ((high.0, high.1), None),
            ((low.0, high.1), None),
        ])
    }

    /// The smallest rectangle that holds it: low u, low v, high u, high v.
    fn bounds(&self) -> [i128; 4] {
        let mut out = [i128::MAX, i128::MAX, i128::MIN, i128::MIN];
        let mut take = |c: P2, r: i128| {
            out[0] = out[0].min(c.0 - r);
            out[1] = out[1].min(c.1 - r);
            out[2] = out[2].max(c.0 + r);
            out[3] = out[3].max(c.1 + r);
        };
        match self {
            Opening::Circle { centre, radius2 } => take(*centre, *radius2),
            Opening::Path(corners) => {
                for (point, arc) in corners {
                    take(*point, 0);
                    if let Some((centre, radius2)) = arc {
                        take(*centre, *radius2);
                    }
                }
            }
        }
        out
    }
}

/// An outline made into edges at one depth of its face.
struct Ring {
    depth: i128,
    edges: Vec<usize>,
    /// The vertices and their places, for the straight edges that join two rings; none for a
    /// circle.
    vertices: Vec<(usize, P)>,
}

impl Ring {
    /// The edges of a further bound of a face that looks the way of the outline: the other way
    /// round.
    fn against(&self) -> Vec<(usize, bool)> {
        self.edges.iter().rev().map(|e| (*e, false)).collect()
    }

    fn along(&self) -> Vec<(usize, bool)> {
        self.edges.iter().map(|e| (*e, true)).collect()
    }
}

fn ring(b: &mut Builder, frame: &FaceFrame, opening: &Opening, depth: i128) -> Ring {
    match opening {
        Opening::Circle { centre, radius2 } => Ring {
            depth,
            edges: vec![circle_edge(b, frame.at(*centre, depth), frame.n, *radius2)],
            vertices: Vec::new(),
        },
        Opening::Path(corners) => {
            let vertices: Vec<(usize, P)> = corners
                .iter()
                .map(|(p, _)| {
                    let at = frame.at(*p, depth);
                    (b.vertex(at), at)
                })
                .collect();
            let edges = (0..corners.len())
                .map(|i| {
                    let (va, from) = vertices[i];
                    let (vb, to) = vertices[(i + 1) % vertices.len()];
                    match corners[i].1 {
                        None => b.line_edge(va, vb, from, to),
                        Some((centre, radius2)) => {
                            let placement = b.axis(frame.at(centre, depth), frame.n, frame.u);
                            let circle =
                                b.add(format!("CIRCLE('',#{placement},{})", real2(radius2)));
                            b.add(format!("EDGE_CURVE('',#{va},#{vb},#{circle},.T.)"))
                        }
                    }
                })
                .collect();
            Ring {
                depth,
                edges,
                vertices,
            }
        }
    }
}

/// A face from its bounds (the first is the outer one) on a surface.
fn face_of(b: &mut Builder, loops: &[Vec<(usize, bool)>], surface: usize, sense: bool) -> usize {
    let bounds: Vec<String> = loops
        .iter()
        .enumerate()
        .map(|(i, edges)| {
            let id = b.loop_of(edges);
            let kind = if i == 0 {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            format!("#{}", b.add(format!("{kind}('',#{id},.T.)")))
        })
        .collect();
    b.add(format!(
        "ADVANCED_FACE('',({}),#{surface},{})",
        bounds.join(","),
        if sense { ".T." } else { ".F." }
    ))
}

/// The direction an axis placement counts its angles from, for an axis along a coordinate.
fn across_of(n: (i8, i8, i8)) -> (i8, i8, i8) {
    if n.0 != 0 {
        (0, 1, 0)
    } else {
        (1, 0, 0)
    }
}

/// The walls between two rings of the same shape: their normals look inside the outline when the
/// second ring is deeper than the first, and outside it when it is shallower.
fn walls(
    b: &mut Builder,
    frame: &FaceFrame,
    opening: &Opening,
    from: &Ring,
    to: &Ring,
) -> Vec<usize> {
    match opening {
        Opening::Circle { centre, radius2 } => {
            let placement = b.axis(frame.at(*centre, from.depth), frame.n, across_of(frame.n));
            let cylinder = b.add(format!(
                "CYLINDRICAL_SURFACE('',#{placement},{})",
                real2(*radius2)
            ));
            // the far circle goes the other way: it is on a face of the cavity turned round
            let loops = [vec![(from.edges[0], true)], vec![(to.edges[0], false)]];
            vec![face_of(b, &loops, cylinder, false)]
        }
        Opening::Path(corners) => {
            let count = corners.len();
            let up: Vec<usize> = (0..count)
                .map(|i| {
                    let ((va, a), (vb, c)) = (from.vertices[i], to.vertices[i]);
                    b.line_edge(va, vb, a, c)
                })
                .collect();
            (0..count)
                .map(|i| {
                    let j = (i + 1) % count;
                    let edges = [vec![
                        (from.edges[i], true),
                        (up[j], true),
                        (to.edges[i], false),
                        (up[i], false),
                    ]];
                    match corners[i].1 {
                        None => {
                            let (p, q, r) =
                                (from.vertices[i].1, from.vertices[j].1, to.vertices[i].1);
                            let d = difference(q, p);
                            let rise = difference(r, p);
                            let normal = unit_f((
                                d.1 * rise.2 - d.2 * rise.1,
                                d.2 * rise.0 - d.0 * rise.2,
                                d.0 * rise.1 - d.1 * rise.0,
                            ));
                            let placement = b.axis_f(p, normal, unit_f(d));
                            let plane = b.add(format!("PLANE('',#{placement})"));
                            face_of(b, &edges, plane, true)
                        }
                        Some((centre, radius2)) => {
                            let placement = b.axis(frame.at(centre, from.depth), frame.n, frame.u);
                            let cylinder = b.add(format!(
                                "CYLINDRICAL_SURFACE('',#{placement},{})",
                                real2(radius2)
                            ));
                            face_of(b, &edges, cylinder, false)
                        }
                    }
                })
                .collect()
        }
    }
}

fn difference(a: P, b: P) -> (f64, f64, f64) {
    ((a.0 - b.0) as f64, (a.1 - b.1) as f64, (a.2 - b.2) as f64)
}

fn unit_f(v: (f64, f64, f64)) -> (f64, f64, f64) {
    let length = (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt();
    (v.0 / length, v.1 / length, v.2 / length)
}

/// What joins a ring of a hole to the next one, from its mouth to the inner side.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Join {
    Wall,
    /// The cone of a countersink, from a wider circle to a narrower one.
    Cone,
    /// The flat ring at the bottom of a counterbore.
    Floor,
}

/// A hole made into rings: the face it is on, the outline and the depth of each ring and what
/// joins them.
struct HoleShape {
    face: Face,
    openings: Vec<(Opening, i128)>,
    joins: Vec<Join>,
}

/// The outlines of a hole, from its mouth on the outer face to the inner side.
fn hole_shape(enclosure: &Enclosure, h: &crate::enclosure::Hole) -> HoleShape {
    let p = enclosure.params();
    let thickness = match h.face {
        Face::Front | Face::Back | Face::Left | Face::Right => p.wall,
        Face::Top => p.lid,
        Face::Bottom => p.floor,
    };
    let t = 2 * i128::from(thickness.get());
    let centre: P2 = (2 * i128::from(h.u.get()), 2 * i128::from(h.v.get()));
    let d = i128::from(h.diameter.get());
    let circle = |radius2: i128| Opening::Circle { centre, radius2 };
    let through = |o: Opening| (vec![(o.clone(), 0), (o, t)], vec![Join::Wall]);
    let (openings, joins) = match h.style {
        HoleStyle::Plain => through(circle(d)),
        HoleStyle::Countersunk { head_diameter } => {
            let head = i128::from(head_diameter.get());
            // a cone of 90 degrees: as deep as half the difference of the diameters
            let deep = (head - d).min(t);
            let mut openings = vec![(circle(head), 0), (circle(d), deep)];
            let mut joins = vec![Join::Cone];
            if deep < t {
                openings.push((circle(d), t));
                joins.push(Join::Wall);
            }
            (openings, joins)
        }
        HoleStyle::Counterbored {
            head_diameter,
            depth,
        } => {
            let head = i128::from(head_diameter.get());
            let k = 2 * i128::from(depth.get());
            (
                vec![
                    (circle(head), 0),
                    (circle(head), k),
                    (circle(d), k),
                    (circle(d), t),
                ],
                vec![Join::Wall, Join::Floor, Join::Wall],
            )
        }
        HoleStyle::Rectangular { height } => {
            let high = i128::from(height.get());
            through(Opening::rectangle(
                (centre.0 - d, centre.1 - high),
                (centre.0 + d, centre.1 + high),
            ))
        }
        HoleStyle::Oval { length, vertical } => {
            // twice the distance from the centre of the slot to the centre of each end
            let apart = i128::from(length.get()) - d;
            let (cu, cv) = centre;
            if apart <= 0 {
                through(circle(d))
            } else if vertical {
                let (low, high) = ((cu, cv - apart), (cu, cv + apart));
                through(Opening::Path(vec![
                    ((cu - d, cv - apart), Some((low, d))),
                    ((cu + d, cv - apart), None),
                    ((cu + d, cv + apart), Some((high, d))),
                    ((cu - d, cv + apart), None),
                ]))
            } else {
                let (left, right) = ((cu - apart, cv), (cu + apart, cv));
                through(Opening::Path(vec![
                    ((cu + apart, cv - d), Some((right, d))),
                    ((cu + apart, cv + d), None),
                    ((cu - apart, cv + d), Some((left, d))),
                    ((cu - apart, cv - d), None),
                ]))
            }
        }
    };
    HoleShape {
        face: h.face,
        openings,
        joins,
    }
}

/// The centring lip as seen from above: the rectangle of its outer side and of its inner side
/// (low x, low y, high x, high y), the thickness of the lid and how far it hangs, all in twice the
/// nanometres.
struct LipPlace {
    outer: [i128; 4],
    inner: [i128; 4],
    lid: i128,
    depth: i128,
    clearance: i128,
}

fn lip_place(enclosure: &Enclosure) -> Option<LipPlace> {
    let lid = enclosure.lid()?;
    let lip = lid.lip.as_ref()?;
    let p = enclosure.params();
    let (x, y, _) = p.outer();
    let n = |v: crate::Length| 2 * i128::from(v.get());
    let start = n(p.wall) + n(lid.clearance);
    let t = n(lip.thickness);
    let outer = [
        start,
        start,
        n(x) - n(p.wall) - n(lid.clearance),
        n(y) - n(p.wall) - n(lid.clearance),
    ];
    Some(LipPlace {
        outer,
        inner: [outer[0] + t, outer[1] + t, outer[2] - t, outer[3] - t],
        lid: n(p.lid),
        depth: n(lip.depth),
        clearance: n(lid.clearance),
    })
}

/// Where the opening of a hole of the lid on its under side lies from the lip: inside the ring,
/// clear outside it, or over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LipSide {
    Inside,
    Outside,
    Over,
}

impl LipPlace {
    fn side(&self, [u0, v0, u1, v1]: [i128; 4]) -> LipSide {
        let [a, b, c, d] = self.inner;
        if u0 > a && v0 > b && u1 < c && v1 < d {
            return LipSide::Inside;
        }
        let [a, b, c, d] = self.outer;
        if u1 < a || u0 > c || v1 < b || v0 > d {
            LipSide::Outside
        } else {
            LipSide::Over
        }
    }
}

/// The circles of a post (its foot, the top of its conical foot when it has one, its top, the top
/// of its bore and the bottom of its bore) and the centres and diameters that its faces need.
struct PostRing {
    top_centre: P,
    bore_centre: P,
    outer: i128,
    bore: i128,
    foot_edge: usize,
    /// Where the cone of a reinforced foot ends: its circle, its centre, the diameter of the foot
    /// and the height of the cone.
    shoulder: Option<(usize, P, i128, i128)>,
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

/// The further bounds of the faces of a box, each a loop of edges with the way each one is walked
/// (`true` is the way of the edge).
type Circles = BTreeMap<Face, Vec<Vec<(usize, bool)>>>;

/// The twelve edges, the eight corners and the six faces of a box. Returns the ids of the faces in
/// the order of [`FACES`].
fn box_faces(b: &mut Builder, min: P, max: P, circles: &Circles) -> Vec<usize> {
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
        for further in circles.get(&face).map(Vec::as_slice).unwrap_or(&[]) {
            let hole_loop = b.loop_of(further);
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

/// The outside of a box whose vertical edges are rounded or chamfered: the floor, the lid and the
/// walls round the outline seen from above.
fn treated_faces(b: &mut Builder, outer: P, treatment: Treatment, holes: &Circles) -> Vec<usize> {
    let (x, y, z) = outer;
    let s = 2 * i128::from(treatment.size().get());
    let round = matches!(treatment, Treatment::Round(_));
    // the outline counter-clockwise seen from above: the corners and, from each to the next, the
    // face of a flat side or the centre of the corner that is rounded or chamfered
    let corners: [(i128, i128); 8] = [
        (s, 0),
        (x - s, 0),
        (x, s),
        (x, y - s),
        (x - s, y),
        (s, y),
        (0, y - s),
        (0, s),
    ];
    let sides: [(Option<Face>, (i128, i128)); 8] = [
        (Some(Face::Front), (0, 0)),
        (None, (x - s, s)),
        (Some(Face::Right), (0, 0)),
        (None, (x - s, y - s)),
        (Some(Face::Back), (0, 0)),
        (None, (s, y - s)),
        (Some(Face::Left), (0, 0)),
        (None, (s, s)),
    ];
    // a flat side that has gone to nothing is left out
    let kept: Vec<usize> = (0..8)
        .filter(|&i| corners[i] != corners[(i + 1) % 8])
        .collect();
    let count = kept.len();
    let point = |k: usize, h: i128| -> P { (corners[kept[k]].0, corners[kept[k]].1, h) };
    let level = |b: &mut Builder, h: i128| -> (Vec<usize>, Vec<usize>) {
        let vertices: Vec<usize> = (0..count).map(|k| b.vertex(point(k, h))).collect();
        let edges = (0..count)
            .map(|k| {
                let next = (k + 1) % count;
                let (va, vb) = (vertices[k], vertices[next]);
                let (face, centre) = sides[kept[k]];
                if face.is_none() && round {
                    let placement = b.axis((centre.0, centre.1, h), (0, 0, 1), (1, 0, 0));
                    let circle = b.add(format!("CIRCLE('',#{placement},{})", real2(s)));
                    b.add(format!("EDGE_CURVE('',#{va},#{vb},#{circle},.T.)"))
                } else {
                    b.line_edge(va, vb, point(k, h), point(next, h))
                }
            })
            .collect();
        (vertices, edges)
    };
    let (low, bottom) = level(b, 0);
    let (high, top) = level(b, z);
    let up: Vec<usize> = (0..count)
        .map(|k| b.line_edge(low[k], high[k], point(k, 0), point(k, z)))
        .collect();
    let further = |face: Face| holes.get(&face).cloned().unwrap_or_default();
    let mut faces = Vec::new();
    for (face, h, edges, normal) in [
        (Face::Bottom, 0, &bottom, (0, 0, -1)),
        (Face::Top, z, &top, (0, 0, 1)),
    ] {
        let walk: Vec<(usize, bool)> = if h == 0 {
            edges.iter().rev().map(|e| (*e, false)).collect()
        } else {
            edges.iter().map(|e| (*e, true)).collect()
        };
        let mut loops = vec![walk];
        loops.extend(further(face));
        let placement = b.axis(point(0, h), normal, (1, 0, 0));
        let plane = b.add(format!("PLANE('',#{placement})"));
        faces.push(face_of(b, &loops, plane, true));
    }
    for k in 0..count {
        let next = (k + 1) % count;
        let walk = vec![
            (bottom[k], true),
            (up[next], true),
            (top[k], false),
            (up[k], false),
        ];
        let (face, centre) = sides[kept[k]];
        let mut loops = vec![walk];
        if let Some(face) = face {
            loops.extend(further(face));
        }
        let surface = if face.is_none() && round {
            let placement = b.axis((centre.0, centre.1, 0), (0, 0, 1), (1, 0, 0));
            b.add(format!("CYLINDRICAL_SURFACE('',#{placement},{})", real2(s)))
        } else {
            let d = difference(point(next, 0), point(k, 0));
            let along = unit_f(d);
            let placement = b.axis_f(point(k, 0), (along.1, -along.0, 0.0), along);
            b.add(format!("PLANE('',#{placement})"))
        };
        faces.push(face_of(b, &loops, surface, true));
    }
    faces
}

/// The enclosure as STEP text.
pub fn to_step(enclosure: &Enclosure) -> String {
    let p = enclosure.params();
    let (x, y, z) = p.outer();
    let n = |v: i64| 2 * i128::from(v);
    let mut b = Builder::default();
    let outer_min = (0, 0, 0);
    let outer_max = (n(x.get()), n(y.get()), n(z.get()));
    let lip = lip_place(enclosure);

    // the outlines of the holes: the first on the outer face, the last on the face of the inner
    // space (or on the ceiling inside the centring lip)
    let mut outer_circles = Circles::new();
    let mut inner_circles = Circles::new();
    let mut within_lip: Vec<Vec<(usize, bool)>> = Vec::new();
    let mut hole_rings = Vec::new();
    let all_holes = enclosure.all_holes();
    for hole in &all_holes {
        let shape = hole_shape(enclosure, hole);
        let frame = FaceFrame::of(shape.face, outer_max);
        let rings: Vec<Ring> = shape
            .openings
            .iter()
            .map(|(opening, depth)| ring(&mut b, &frame, opening, *depth))
            .collect();
        if let (Some(mouth), Some(inner)) = (rings.first(), rings.last()) {
            outer_circles
                .entry(hole.face)
                .or_default()
                .push(mouth.against());
            let under_lip = hole.face == Face::Top
                && lip.as_ref().is_some_and(|lip| {
                    shape
                        .openings
                        .last()
                        .is_some_and(|(o, _)| lip.side(o.bounds()) == LipSide::Inside)
                });
            if under_lip {
                within_lip.push(inner.against());
            } else {
                inner_circles
                    .entry(hole.face)
                    .or_default()
                    .push(inner.against());
            }
        }
        hole_rings.push((shape, frame, rings));
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
        let wide = post
            .base
            .map_or(outer, |base| i128::from(base.diameter.get()));
        let foot_edge = circle_edge(&mut b, (cx, cy, 2 * floor), up, wide);
        let shoulder = post.base.map(|base| {
            let height = 2 * i128::from(base.height.get());
            let centre = (cx, cy, 2 * floor + height);
            (circle_edge(&mut b, centre, up, outer), centre, wide, height)
        });
        let top_edge = circle_edge(&mut b, (cx, cy, top_z), up, outer);
        let bore_top_edge = circle_edge(&mut b, (cx, cy, top_z), up, bore);
        let bore_bottom_edge = circle_edge(&mut b, (cx, cy, bore_z), up, bore);
        inner_circles
            .entry(Face::Bottom)
            .or_default()
            .push(vec![(foot_edge, true)]);
        rings.push(PostRing {
            top_centre: (cx, cy, top_z),
            bore_centre: (cx, cy, bore_z),
            outer,
            bore,
            foot_edge,
            shoulder,
            top_edge,
            bore_top_edge,
            bore_bottom_edge,
        });
    }
    // the outlines of the lip: at the ceiling and at its lower side, outer and inner
    let lip_rings = lip.as_ref().map(|lip| {
        let frame = FaceFrame::of(Face::Top, outer_max);
        let outline = |r: [i128; 4]| Opening::rectangle((r[0], r[1]), (r[2], r[3]));
        let (outer, inner) = (outline(lip.outer), outline(lip.inner));
        let low = lip.lid + lip.depth;
        let outer_up = ring(&mut b, &frame, &outer, lip.lid);
        let outer_down = ring(&mut b, &frame, &outer, low);
        let inner_up = ring(&mut b, &frame, &inner, lip.lid);
        let inner_down = ring(&mut b, &frame, &inner, low);
        inner_circles
            .entry(Face::Top)
            .or_default()
            .push(outer_up.against());
        (
            frame,
            [outer, inner],
            [outer_up, outer_down, inner_up, inner_down],
        )
    });

    let inner_min = (n(p.wall.get()), n(p.wall.get()), n(p.floor.get()));
    let inner_max = (
        n(p.wall.get() + p.inner_length.get()),
        n(p.wall.get() + p.inner_width.get()),
        n(p.floor.get() + p.inner_height.get()),
    );
    let vertical = enclosure.edging().vertical;
    let mut faces = if vertical.is_sharp() {
        box_faces(&mut b, outer_min, outer_max, &outer_circles)
    } else {
        treated_faces(&mut b, outer_max, vertical, &outer_circles)
    };
    let mut cavity = box_faces(&mut b, inner_min, inner_max, &inner_circles);

    // the walls of the holes, from their mouths to the inner side
    for (shape, frame, made) in &hole_rings {
        for (i, join) in shape.joins.iter().enumerate() {
            let (from, to) = (&made[i], &made[i + 1]);
            match (join, &shape.openings[i].0, &shape.openings[i + 1].0) {
                (
                    Join::Cone,
                    Opening::Circle { radius2: wide, .. },
                    Opening::Circle {
                        centre,
                        radius2: narrow,
                    },
                ) => {
                    let placement =
                        b.axis(frame.at(*centre, to.depth), frame.n, across_of(frame.n));
                    let angle = ((wide - narrow) as f64).atan2((to.depth - from.depth) as f64);
                    let cone = b.add(format!(
                        "CONICAL_SURFACE('',#{placement},{},{})",
                        real2(*narrow),
                        real_f(angle)
                    ));
                    let loops = [vec![(from.edges[0], true)], vec![(to.edges[0], false)]];
                    faces.push(face_of(&mut b, &loops, cone, false));
                }
                (Join::Floor, Opening::Circle { centre, .. }, _) => {
                    let placement =
                        b.axis(frame.at(*centre, from.depth), frame.n, across_of(frame.n));
                    let plane = b.add(format!("PLANE('',#{placement})"));
                    // the ring looks out of the material, towards the mouth
                    faces.push(face_of(&mut b, &[from.along(), to.against()], plane, true));
                }
                (_, opening, _) => faces.extend(walls(&mut b, frame, opening, from, to)),
            }
        }
    }

    // the faces of the posts close the void: the wall of the post and the wall of its bore are
    // cylinders, the foot a cone, the top ring and the bottom of the bore are planes whose normal
    // points down
    for post in rings {
        let along = (0, 0, 1);
        let down = (0, 0, -1);
        let across = (1, 0, 0);
        let wall_axis = b.axis(post.top_centre, along, across);
        let wall = b.add(format!(
            "CYLINDRICAL_SURFACE('',#{wall_axis},{})",
            real2(post.outer)
        ));
        let low = post.shoulder.map_or(post.foot_edge, |s| s.0);
        let loops = [vec![(post.top_edge, true)], vec![(low, false)]];
        cavity.push(face_of(&mut b, &loops, wall, false));

        if let Some((shoulder, centre, wide, height)) = post.shoulder {
            // the cone grows downwards from where the straight wall ends
            let cone_axis = b.axis(centre, down, across);
            let angle = ((wide - post.outer) as f64).atan2(height as f64);
            let cone = b.add(format!(
                "CONICAL_SURFACE('',#{cone_axis},{},{})",
                real2(post.outer),
                real_f(angle)
            ));
            let loops = [vec![(shoulder, true)], vec![(post.foot_edge, false)]];
            cavity.push(face_of(&mut b, &loops, cone, false));
        }

        let ring_axis = b.axis(post.top_centre, down, across);
        let ring_plane = b.add(format!("PLANE('',#{ring_axis})"));
        let loops = [
            vec![(post.top_edge, false)],
            vec![(post.bore_top_edge, true)],
        ];
        cavity.push(face_of(&mut b, &loops, ring_plane, true));

        let bore_axis = b.axis(post.top_centre, along, across);
        let bore_wall = b.add(format!(
            "CYLINDRICAL_SURFACE('',#{bore_axis},{})",
            real2(post.bore)
        ));
        let loops = [
            vec![(post.bore_top_edge, false)],
            vec![(post.bore_bottom_edge, true)],
        ];
        cavity.push(face_of(&mut b, &loops, bore_wall, true));

        let bottom_axis = b.axis(post.bore_centre, down, across);
        let bottom_plane = b.add(format!("PLANE('',#{bottom_axis})"));
        let loops = [vec![(post.bore_bottom_edge, false)]];
        cavity.push(face_of(&mut b, &loops, bottom_plane, true));
    }

    // the centring lip: its two walls, its lower side and the ceiling inside it
    if let (Some(lip), Some((frame, [outer, inner], made))) = (&lip, &lip_rings) {
        let [outer_up, outer_down, inner_up, inner_down] = made;
        cavity.extend(walls(&mut b, frame, outer, outer_up, outer_down));
        cavity.extend(walls(&mut b, frame, inner, inner_down, inner_up));
        let corner = (lip.outer[0], lip.outer[1]);
        let under = b.axis(frame.at(corner, lip.lid + lip.depth), (0, 0, 1), (1, 0, 0));
        let under = b.add(format!("PLANE('',#{under})"));
        let loops = [outer_down.along(), inner_down.against()];
        cavity.push(face_of(&mut b, &loops, under, true));
        let ceiling = b.axis(frame.at(corner, lip.lid), (0, 0, 1), (1, 0, 0));
        let ceiling = b.add(format!("PLANE('',#{ceiling})"));
        let mut loops = vec![inner_up.along()];
        loops.append(&mut within_lip);
        cavity.push(face_of(&mut b, &loops, ceiling, true));
    }

    let list = |ids: &[usize]| -> String {
        ids.iter()
            .map(|i| format!("#{i}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let brep = if all_holes.is_empty() {
        let shell = b.add(format!("CLOSED_SHELL('',({}))", list(&faces)));
        let void_shell = b.add(format!("CLOSED_SHELL('',({}))", list(&cavity)));
        let oriented_void = b.add(format!("ORIENTED_CLOSED_SHELL('',*,#{void_shell},.F.)"));
        b.add(format!(
            "BREP_WITH_VOIDS('enclosure',#{shell},(#{oriented_void}))"
        ))
    } else {
        // a hole joins the cavity to the outside: the walls are one surface
        for &face in &cavity {
            b.turn_face(face);
        }
        faces.append(&mut cavity);
        let shell = b.add(format!("CLOSED_SHELL('',({}))", list(&faces)));
        b.add(format!("MANIFOLD_SOLID_BREP('enclosure',#{shell})"))
    };

    finish_file(b, &[brep], "enclosure")
}

/// The product structure, the units and the header around a solid: the text of the file.
fn finish_file(mut b: Builder, breps: &[usize], name: &str) -> String {
    let solids: String = breps.iter().map(|i| format!(",#{i}")).collect();
    let origin = b.axis((0, 0, 0), (0, 0, 1), (1, 0, 0));
    let unc_unit = b.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".to_owned());
    let angle = b.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))".to_owned());
    let solid = b.add("(NAMED_UNIT(*) SOLID_ANGLE_UNIT() SI_UNIT($,.STERADIAN.))".to_owned());
    let uncertainty = b.add(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{unc_unit},'distance_accuracy_value','')"
    ));
    let context = b.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unc_unit},#{angle},#{solid})) REPRESENTATION_CONTEXT('{name}','3D'))"
    ));
    let representation = b.add(format!(
        "ADVANCED_BREP_SHAPE_REPRESENTATION('{name}',(#{origin}{solids}),#{context})"
    ));
    let app = b.add(
        "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".to_owned(),
    );
    b.add(format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"
    ));
    let product_context = b.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let product = b.add(format!(
        "PRODUCT('{name}','{name}','',(#{product_context}))"
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
    let _ = writeln!(out, "FILE_DESCRIPTION(('EH3D-PHOENIX-CAD {name}'),'2;1');");
    let _ = writeln!(
        out,
        "FILE_NAME('{name}.step','',(''),(''),'EH3D-PHOENIX-CAD','','');"
    );
    out.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\n");
    out.push_str("ENDSEC;\nDATA;\n");
    for (i, body) in b.lines.iter().enumerate() {
        let _ = writeln!(out, "#{}={};", i + 1, body);
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}

/// The enclosure as STEP text, or the error `PHOENIX-CAD-022` when the model has a shape that the
/// file does not draw: a rounded or chamfered edge around the lid or the floor, an extrusion or a
/// pocket of a sketch, a set of cooling fins, a placed part, a turned body, a swept body, a
/// centring lip with no clearance to the walls or a hole of the lid that opens over the lip.
pub fn export_step(enclosure: &Enclosure) -> Result<String, CadError> {
    if !enclosure.features().is_empty() {
        return Err(CadError::NotExported(
            "the extrusions and pockets of sketches".into(),
        ));
    }
    if !enclosure.fins().is_empty() {
        return Err(CadError::NotExported("cooling fins".into()));
    }
    if !enclosure.parts().is_empty() {
        return Err(CadError::NotExported("placed parts".into()));
    }
    if !enclosure.turned().is_empty() {
        return Err(CadError::NotExported("turned bodies".into()));
    }
    if !enclosure.swept().is_empty() {
        return Err(CadError::NotExported("swept bodies".into()));
    }
    let edging = enclosure.edging();
    if !edging.top.is_sharp() || !edging.bottom.is_sharp() {
        return Err(CadError::NotExported(
            "rounded or chamfered edges around the lid or the floor".into(),
        ));
    }
    if let Some(lip) = lip_place(enclosure) {
        if lip.clearance <= 0 {
            return Err(CadError::NotExported(
                "a centring lip with no clearance to the walls".into(),
            ));
        }
        for hole in enclosure.all_holes() {
            if hole.face != Face::Top {
                continue;
            }
            // the lid is seen from above, so the u and v of its holes are x and y
            let shape = hole_shape(enclosure, &hole);
            if shape
                .openings
                .last()
                .is_some_and(|(o, _)| lip.side(o.bounds()) == LipSide::Over)
            {
                return Err(CadError::NotExported(format!(
                    "hole {} over the centring lip",
                    hole.name
                )));
            }
        }
    }
    Ok(to_step(enclosure))
}

/// A prism as STEP text: the profile of a sketch raised from the plane `z0` by `height` (both in
/// nanometres) along the third axis. Every region of the profile is a solid of its own, with its
/// holes through it; the outlines are the straight lines that stand for them (a circle or an arc
/// is the polygon the profile draws), so every face is flat and the file can be read back by
/// [`crate::step_import::read_step`] to the volume of the polygons.
pub fn prism_to_step(
    name: &str,
    profile: &crate::profile::Profile,
    z0: i64,
    height: i64,
) -> Result<String, CadError> {
    if height <= 0 {
        return Err(CadError::NotPositive("the height of the prism"));
    }
    let z1 = z0
        .checked_add(height)
        .filter(|z| z.abs() < crate::LENGTH_LIMIT_NM)
        .ok_or(CadError::LengthOutOfRange(i64::MAX))?;
    let name = step_name(name);
    let mut b = Builder::default();
    let mut solids = Vec::new();
    for region in &profile.regions {
        let rings: Vec<&[crate::profile::Pt]> = std::iter::once(&region.outer)
            .chain(region.holes.iter())
            .map(|o| o.polygon())
            .collect();
        let at = |p: crate::profile::Pt, z: i64| -> P {
            (2 * i128::from(p.0), 2 * i128::from(p.1), 2 * i128::from(z))
        };
        // for each ring: the edges along the bottom, along the top and the rising ones
        let mut faces: Vec<usize> = Vec::new();
        let mut bottoms: Vec<(Vec<usize>, bool)> = Vec::new();
        let mut tops: Vec<(Vec<usize>, bool)> = Vec::new();
        let mut sides: Vec<usize> = Vec::new();
        for (k, ring) in rings.iter().enumerate() {
            let n = ring.len();
            if n < 3 {
                return Err(CadError::NotExported(
                    "an outline with fewer than three corners".into(),
                ));
            }
            let vb: Vec<usize> = ring.iter().map(|p| b.vertex(at(*p, z0))).collect();
            let vt: Vec<usize> = ring.iter().map(|p| b.vertex(at(*p, z1))).collect();
            let mut edge_bottom = Vec::new();
            let mut edge_top = Vec::new();
            let mut edge_up = Vec::new();
            for i in 0..n {
                let j = (i + 1) % n;
                if ring[i] == ring[j] {
                    return Err(CadError::NotExported(
                        "an outline with a side of no length".into(),
                    ));
                }
                edge_bottom.push(b.line_edge(vb[i], vb[j], at(ring[i], z0), at(ring[j], z0)));
                edge_top.push(b.line_edge(vt[i], vt[j], at(ring[i], z1), at(ring[j], z1)));
                edge_up.push(b.line_edge(vb[i], vt[i], at(ring[i], z0), at(ring[i], z1)));
            }
            for i in 0..n {
                let j = (i + 1) % n;
                let loop_id = b.loop_of(&[
                    (edge_bottom[i], true),
                    (edge_up[j], true),
                    (edge_top[i], false),
                    (edge_up[i], false),
                ]);
                let bound = b.add(format!("FACE_OUTER_BOUND('',#{loop_id},.T.)"));
                let (dx, dy) = (
                    (ring[j].0 - ring[i].0) as f64,
                    (ring[j].1 - ring[i].1) as f64,
                );
                let length = (dx * dx + dy * dy).sqrt();
                let (ux, uy) = (dx / length, dy / length);
                let placement = b.axis_f(at(ring[i], z0), (uy, -ux, 0.0), (ux, uy, 0.0));
                let plane = b.add(format!("PLANE('',#{placement})"));
                sides.push(b.add(format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
            }
            bottoms.push((edge_bottom, k == 0));
            tops.push((edge_top, k == 0));
        }
        // the floor looks down and walks every ring backwards; the roof looks up and walks them as they are
        for (rings_edges, up) in [(&bottoms, false), (&tops, true)] {
            let mut bounds = Vec::new();
            for (edges, outer) in rings_edges {
                let walk: Vec<(usize, bool)> = if up {
                    edges.iter().map(|e| (*e, true)).collect()
                } else {
                    edges.iter().rev().map(|e| (*e, false)).collect()
                };
                let loop_id = b.loop_of(&walk);
                bounds.push(b.add(format!(
                    "{}('',#{loop_id},.T.)",
                    if *outer {
                        "FACE_OUTER_BOUND"
                    } else {
                        "FACE_BOUND"
                    }
                )));
            }
            let z = if up { z1 } else { z0 };
            let placement = b.axis(
                at(rings[0][0], z),
                (0, 0, if up { 1 } else { -1 }),
                (1, 0, 0),
            );
            let plane = b.add(format!("PLANE('',#{placement})"));
            let list: Vec<String> = bounds.iter().map(|i| format!("#{i}")).collect();
            faces.push(b.add(format!(
                "ADVANCED_FACE('',({}),#{plane},.T.)",
                list.join(",")
            )));
        }
        faces.extend(sides);
        let list: Vec<String> = faces.iter().map(|i| format!("#{i}")).collect();
        let shell = b.add(format!("CLOSED_SHELL('',({}))", list.join(",")));
        solids.push(b.add(format!("MANIFOLD_SOLID_BREP('{name}',#{shell})")));
    }
    if solids.is_empty() {
        return Err(CadError::NotExported("a profile with no outline".into()));
    }
    Ok(finish_file(b, &solids, &name))
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

    /// The entities of a file by their number.
    fn by_number(text: &str) -> BTreeMap<usize, &str> {
        text.lines()
            .filter_map(|l| l.strip_prefix('#')?.split_once('='))
            .filter_map(|(id, body)| Some((id.parse().ok()?, body.strip_suffix(';')?)))
            .collect()
    }

    /// The numbers an entity refers to, in order.
    fn references(body: &str) -> Vec<usize> {
        body.split('#')
            .skip(1)
            .filter_map(|part| {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect()
    }

    /// Each shell of a file: every edge is walked once each way by the faces of the shell.
    fn assert_every_edge_once_each_way(text: &str) {
        let map = by_number(text);
        let against = |body: &str| body.ends_with(".F.)");
        let mut shells = 0;
        for shell in map.values().filter(|b| b.starts_with("CLOSED_SHELL")) {
            shells += 1;
            let mut ways: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
            for face in references(shell) {
                let named = references(map[&face]);
                // the last number of a face is its surface
                for bound in &named[..named.len() - 1] {
                    let turned = against(map[bound]);
                    let edges = references(map[&references(map[bound])[0]]);
                    for oriented in edges {
                        let edge = references(map[&oriented])[0];
                        ways.entry(edge)
                            .or_default()
                            .push(against(map[&oriented]) != turned);
                    }
                }
            }
            for (edge, mut seen) in ways {
                seen.sort_unstable();
                assert_eq!(seen, vec![false, true], "edge #{edge}");
            }
        }
        assert!(shells > 0);
    }

    #[test]
    fn a_hole_joins_the_cavity_and_the_outside_in_one_closed_shell() {
        let mut e = enclosure(&[(Face::Front, 20, 15, 8), (Face::Top, 30, 30, 10)]);
        add_post(&mut e, 1, a_post("p", 60, 40));
        let text = to_step(&e);
        assert_eq!(count(&text, "CLOSED_SHELL"), 1);
        assert_eq!(count(&text, "MANIFOLD_SOLID_BREP"), 1);
        assert_eq!(count(&text, "BREP_WITH_VOIDS"), 0);
        assert_every_edge_once_each_way(&text);
        let mut posts_only = enclosure(&[]);
        add_post(&mut posts_only, 1, a_post("p", 60, 40));
        let text = to_step(&posts_only);
        assert_eq!(count(&text, "CLOSED_SHELL"), 2);
        assert_every_edge_once_each_way(&text);
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
        for edging in [
            crate::enclosure::Edging {
                top: crate::enclosure::Treatment::Round(mm(2)),
                ..crate::enclosure::Edging::default()
            },
            crate::enclosure::Edging {
                bottom: crate::enclosure::Treatment::Chamfer(mm(2)),
                ..crate::enclosure::Edging::default()
            },
        ] {
            let mut rounded = enclosure(&[]);
            rounded.apply(Operation::SetEdging(edging)).unwrap();
            let err = export_step(&rounded).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-022");
        }
    }

    fn styled(face: Face, u: i64, v: i64, d: i64, style: HoleStyle) -> Enclosure {
        let mut e = enclosure(&[]);
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: Hole {
                name: "h".into(),
                face,
                u: mm(u),
                v: mm(v),
                diameter: mm(d),
                style,
            },
        })
        .unwrap();
        e
    }

    #[test]
    fn each_style_of_hole_has_its_faces_and_walks_every_edge_once_each_way() {
        // the style, the faces beside the twelve of the box, the cylinders and the cones
        let cases = [
            (
                HoleStyle::Countersunk {
                    head_diameter: mm(7),
                },
                2,
                1,
                1,
            ),
            (
                HoleStyle::Counterbored {
                    head_diameter: mm(8),
                    depth: mm(1),
                },
                3,
                2,
                0,
            ),
            (HoleStyle::Rectangular { height: mm(6) }, 4, 0, 0),
            (
                HoleStyle::Oval {
                    length: mm(12),
                    vertical: false,
                },
                4,
                2,
                0,
            ),
            (
                HoleStyle::Oval {
                    length: mm(12),
                    vertical: true,
                },
                4,
                2,
                0,
            ),
            // a slot as long as it is wide is a round hole
            (
                HoleStyle::Oval {
                    length: mm(4),
                    vertical: false,
                },
                1,
                1,
                0,
            ),
        ];
        for (style, faces, cylinders, cones) in cases {
            for face in [
                Face::Front,
                Face::Back,
                Face::Left,
                Face::Right,
                Face::Top,
                Face::Bottom,
            ] {
                let e = styled(face, 30, 16, 4, style);
                let text = export_step(&e).unwrap();
                assert_eq!(
                    count(&text, "ADVANCED_FACE"),
                    12 + faces,
                    "{style:?} {face:?}"
                );
                assert_eq!(count(&text, "CYLINDRICAL_SURFACE"), cylinders, "{style:?}");
                assert_eq!(count(&text, "CONICAL_SURFACE"), cones, "{style:?}");
                assert_every_edge_once_each_way(&text);
                assert_references_in_order(&text);
            }
        }
    }

    #[test]
    fn rounded_and_chamfered_vertical_edges_are_drawn_with_their_faces() {
        use crate::enclosure::{Edging, Treatment};
        for (treatment, cylinders, planes) in [
            (Treatment::Round(mm(5)), 4, 12),
            (Treatment::Chamfer(mm(3)), 0, 16),
        ] {
            let mut e = enclosure(&[]);
            e.apply(Operation::SetEdging(Edging {
                vertical: treatment,
                ..Edging::default()
            }))
            .unwrap();
            let text = export_step(&e).unwrap();
            // the floor, the lid and eight sides outside; the six faces of the inner space
            assert_eq!(count(&text, "ADVANCED_FACE"), 10 + 6);
            assert_eq!(count(&text, "CYLINDRICAL_SURFACE"), cylinders);
            assert_eq!(count(&text, "PLANE"), planes);
            assert_eq!(count(&text, "BREP_WITH_VOIDS"), 1);
            assert_every_edge_once_each_way(&text);
            assert_references_in_order(&text);
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
    fn the_foot_of_a_post_is_a_cone() {
        let mut footed = enclosure(&[]);
        let mut p = a_post("p", 25, 15);
        p.base = Some(Base {
            diameter: mm(12),
            height: mm(2),
        });
        add_post(&mut footed, 1, p);
        let text = export_step(&footed).unwrap();
        assert_eq!(count(&text, "ADVANCED_FACE"), 12 + 5);
        assert_eq!(count(&text, "CONICAL_SURFACE"), 1);
        assert_eq!(count(&text, "CIRCLE"), 5);
        assert_every_edge_once_each_way(&text);
        assert_references_in_order(&text);
    }

    fn a_lip() -> Lip {
        Lip {
            thickness: Length::new(1_200_000).unwrap(),
            depth: mm(3),
        }
    }

    #[test]
    fn the_centring_lip_splits_the_ceiling_and_its_holes_stay_inside_it() {
        let mut lipped = enclosure(&[]);
        lipped
            .apply(Operation::SetLid(Some(a_lid(
                HoleStyle::Plain,
                Some(a_lip()),
            ))))
            .unwrap();
        let text = export_step(&lipped).unwrap();
        // the box, four holes, the eight walls of the lip, its lower side and the ceiling inside
        assert_eq!(count(&text, "ADVANCED_FACE"), 12 + 4 + 8 + 2);
        assert_every_edge_once_each_way(&text);
        assert_references_in_order(&text);
    }

    #[test]
    fn the_holes_of_the_lid_are_drawn_with_their_recesses() {
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
        let text = export_step(&sunk).unwrap();
        assert_eq!(count(&text, "CONICAL_SURFACE"), 4);
        assert_eq!(count(&text, "ADVANCED_FACE"), 12 + 8);
        assert_every_edge_once_each_way(&text);
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
    fn a_placed_part_is_not_drawn_by_the_step_file_and_the_answer_is_the_code_of_the_shape() {
        use crate::catalog::Axis;
        let mut e = enclosure(&[]);
        assert!(export_step(&e).is_ok());
        place(&mut e, 1, "bolt", [0, 0, 0], Axis::PlusZ, 0, None);
        assert_eq!(
            export_step(&e).unwrap_err(),
            CadError::NotExported("placed parts".into())
        );
    }
}
