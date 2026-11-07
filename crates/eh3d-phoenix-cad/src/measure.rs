//! Measurements between references of the model: the distance between two points, a point and an
//! edge or two edges, and the angle between three points or between two edges.
//!
//! A reference is a point (the centre of a hole or of a post, a corner of the box) or an edge,
//! in millimetres in the frame of the model. The numbers are floating point and are a reading for
//! the person: nothing here decides anything about the model. A measurement keeps the lines that
//! show what was measured, so that the views can draw them.

use crate::projection::Point3;
use crate::CadError;

/// Anything shorter than this, in millimetres, counts as no length at all.
pub const EPSILON: f64 = 1e-9;

/// What a reference is, which the window turns into words in the language of the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// The centre of a hole of a face, by its name.
    Hole(String),
    /// The centre of a post, by its name.
    Post(String),
    /// A corner of the outer box.
    OuterCorner,
    /// A corner of the inner space.
    InnerCorner,
    /// An edge of the outer box.
    OuterEdge,
    /// An edge of the inner space.
    InnerEdge,
}

/// The shape of a reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geometry {
    /// A point.
    Point(Point3),
    /// An edge, from one end to the other.
    Edge(Point3, Point3),
}

/// A point or an edge of the model that can be measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// What it is.
    pub label: Label,
    /// Where it is.
    pub geometry: Geometry,
}

impl Reference {
    /// A point.
    pub fn point(label: Label, at: Point3) -> Reference {
        Reference {
            label,
            geometry: Geometry::Point(at),
        }
    }

    /// An edge; refused when its two ends are the same point.
    pub fn edge(label: Label, a: Point3, b: Point3) -> Result<Reference, CadError> {
        if length(sub(b, a)) <= EPSILON {
            return Err(CadError::Measure("an edge has no length".into()));
        }
        Ok(Reference {
            label,
            geometry: Geometry::Edge(a, b),
        })
    }

    /// Whether it is a point.
    pub fn is_point(&self) -> bool {
        matches!(self.geometry, Geometry::Point(_))
    }
}

/// What a measurement gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// A distance, in millimetres.
    Distance,
    /// An angle, in degrees.
    Angle,
}

/// The result of a measurement and the lines that show it.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// A distance or an angle.
    pub quantity: Quantity,
    /// What was measured, in the order it was picked.
    pub refs: Vec<Reference>,
    /// Millimetres for a distance, degrees for an angle.
    pub value: f64,
    /// For a distance, the steps along x, y and z from the first reference to the second, in
    /// millimetres; zeros for an angle.
    pub delta: [f64; 3],
    /// The lines that show it: the shortest line between the two references, or the two legs of
    /// the angle.
    pub lines: Vec<(Point3, Point3)>,
}

fn sub(a: Point3, b: Point3) -> Point3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Point3, b: Point3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Point3, b: Point3) -> Point3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: Point3) -> f64 {
    dot(a, a).sqrt()
}

fn along(a: Point3, d: Point3, t: f64) -> Point3 {
    [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t]
}

/// The distance between two points, in millimetres.
pub fn distance_points(a: Point3, b: Point3) -> f64 {
    length(sub(b, a))
}

/// The point of the edge `a`-`b` that is nearest to `p`.
pub fn nearest_on_edge(p: Point3, a: Point3, b: Point3) -> Point3 {
    let d = sub(b, a);
    let len2 = dot(d, d);
    if len2 <= EPSILON * EPSILON {
        return a;
    }
    let t = (dot(sub(p, a), d) / len2).clamp(0.0, 1.0);
    along(a, d, t)
}

/// The nearest points of two edges, one on each (the standard way to find them: the two
/// parameters of the closest points of the lines, brought back into the edges one at a time).
pub fn nearest_between_edges(p1: Point3, q1: Point3, p2: Point3, q2: Point3) -> (Point3, Point3) {
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);
    if a <= EPSILON * EPSILON && e <= EPSILON * EPSILON {
        return (p1, p2);
    }
    let (s, t) = if a <= EPSILON * EPSILON {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot(d1, r);
        if e <= EPSILON * EPSILON {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let s0 = if denom > EPSILON * a * e {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t0 > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s0, t0)
            }
        }
    };
    (along(p1, d1, s), along(p2, d2, t))
}

/// The nearest points of two references, one on each.
fn nearest(a: &Geometry, b: &Geometry) -> (Point3, Point3) {
    match (*a, *b) {
        (Geometry::Point(p), Geometry::Point(q)) => (p, q),
        (Geometry::Point(p), Geometry::Edge(x, y)) => (p, nearest_on_edge(p, x, y)),
        (Geometry::Edge(x, y), Geometry::Point(q)) => (nearest_on_edge(q, x, y), q),
        (Geometry::Edge(x, y), Geometry::Edge(u, v)) => nearest_between_edges(x, y, u, v),
    }
}

/// The shortest distance between two references: the length of the line that joins the nearest
/// points, and its steps along the three axes.
pub fn distance(a: &Reference, b: &Reference) -> Measurement {
    let (p, q) = nearest(&a.geometry, &b.geometry);
    let delta = sub(q, p);
    Measurement {
        quantity: Quantity::Distance,
        refs: vec![a.clone(), b.clone()],
        value: length(delta),
        delta,
        lines: vec![(p, q)],
    }
}

/// The angle in degrees, from 0 to 180, between two directions; refused when one has no length.
fn between(u: Point3, v: Point3) -> Result<f64, CadError> {
    if length(u) <= EPSILON || length(v) <= EPSILON {
        return Err(CadError::Measure(
            "an angle needs two lines that have a length".into(),
        ));
    }
    Ok(length(cross(u, v)).atan2(dot(u, v)).to_degrees())
}

/// The angle at `vertex` between the lines to `a` and to `b`, from 0 to 180 degrees.
pub fn angle_at(a: &Reference, vertex: &Reference, b: &Reference) -> Result<Measurement, CadError> {
    let point = |r: &Reference| match r.geometry {
        Geometry::Point(p) => Ok(p),
        Geometry::Edge(..) => Err(CadError::Measure(
            "an angle at a vertex needs three points".into(),
        )),
    };
    let (pa, pv, pb) = (point(a)?, point(vertex)?, point(b)?);
    let value = between(sub(pa, pv), sub(pb, pv))?;
    Ok(Measurement {
        quantity: Quantity::Angle,
        refs: vec![a.clone(), vertex.clone(), b.clone()],
        value,
        delta: [0.0; 3],
        lines: vec![(pv, pa), (pv, pb)],
    })
}

/// The angle between two edges taken as lines, from 0 to 90 degrees: an edge has no direction of
/// its own, so the acute angle is the one that is read.
pub fn angle_between_edges(a: &Reference, b: &Reference) -> Result<Measurement, CadError> {
    let edge = |r: &Reference| match r.geometry {
        Geometry::Edge(x, y) => Ok((x, y)),
        Geometry::Point(_) => Err(CadError::Measure("this angle needs two edges".into())),
    };
    let ((a0, a1), (b0, b1)) = (edge(a)?, edge(b)?);
    let turn = between(sub(a1, a0), sub(b1, b0))?;
    Ok(Measurement {
        quantity: Quantity::Angle,
        refs: vec![a.clone(), b.clone()],
        value: turn.min(180.0 - turn),
        delta: [0.0; 3],
        lines: vec![(a0, a1), (b0, b1)],
    })
}

/// The angle of what was picked: two edges, or three points (the second is the vertex).
pub fn angle(refs: &[Reference]) -> Result<Measurement, CadError> {
    match refs {
        [a, b] => angle_between_edges(a, b),
        [a, v, b] => angle_at(a, v, b),
        _ => Err(CadError::Measure(
            "an angle needs two edges or three points".into(),
        )),
    }
}

/// How many references an angle starting with `first` needs: two when it starts with an edge,
/// three when it starts with a point.
pub fn angle_needs(first: &Reference) -> usize {
    if first.is_point() {
        3
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn pt(x: f64, y: f64, z: f64) -> Reference {
        Reference::point(Label::OuterCorner, [x, y, z])
    }

    fn edge(a: Point3, b: Point3) -> Reference {
        Reference::edge(Label::OuterEdge, a, b).unwrap()
    }

    #[test]
    fn the_distance_between_two_points_is_the_length_of_the_line_that_joins_them() {
        // 3, 4 and 12 make 13 (9 + 16 + 144 = 169)
        let m = distance(&pt(0.0, 0.0, 0.0), &pt(3.0, 4.0, 12.0));
        assert_eq!(m.quantity, Quantity::Distance);
        assert!(close(m.value, 13.0), "{}", m.value);
        assert_eq!(m.delta, [3.0, 4.0, 12.0]);
        assert_eq!(m.lines, vec![([0.0, 0.0, 0.0], [3.0, 4.0, 12.0])]);
        // the other way round the steps change their sign
        let back = distance(&pt(3.0, 4.0, 12.0), &pt(0.0, 0.0, 0.0));
        assert!(close(back.value, 13.0));
        assert_eq!(back.delta, [-3.0, -4.0, -12.0]);
    }

    #[test]
    fn the_distance_to_an_edge_is_to_its_nearest_point() {
        let e = edge([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        // above the middle: 3 mm up
        let m = distance(&pt(4.0, 3.0, 0.0), &e);
        assert!(close(m.value, 3.0));
        assert_eq!(m.lines, vec![([4.0, 3.0, 0.0], [4.0, 0.0, 0.0])]);
        // beyond the end the nearest point is the end: 3, 4 make 5
        let m = distance(&e, &pt(13.0, 4.0, 0.0));
        assert!(close(m.value, 5.0), "{}", m.value);
        assert_eq!(m.lines, vec![([10.0, 0.0, 0.0], [13.0, 4.0, 0.0])]);
        // before the start
        let m = distance(&pt(-3.0, 0.0, 4.0), &e);
        assert!(close(m.value, 5.0));
    }

    #[test]
    fn the_distance_between_two_edges_is_that_of_their_nearest_points() {
        let x = edge([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        // parallel, 3 up and 4 across: 5
        let parallel = edge([2.0, 3.0, 4.0], [8.0, 3.0, 4.0]);
        assert!(close(distance(&x, &parallel).value, 5.0));
        // parallel and apart along their length: from (10, 0, 0) to (20, 3, 4), 100 + 9 + 16
        let apart = edge([20.0, 3.0, 4.0], [30.0, 3.0, 4.0]);
        let m = distance(&x, &apart);
        assert!(close(m.value, 125f64.sqrt()), "{}", m.value);
        assert_eq!(m.lines, vec![([10.0, 0.0, 0.0], [20.0, 3.0, 4.0])]);
        // skew: the second is above the middle of the first and across it
        let skew = edge([5.0, -2.0, 4.0], [5.0, 6.0, 4.0]);
        let m = distance(&x, &skew);
        assert!(close(m.value, 4.0), "{}", m.value);
        assert_eq!(m.lines, vec![([5.0, 0.0, 0.0], [5.0, 0.0, 4.0])]);
        // edges that cross are 0 apart
        let cross = edge([5.0, -2.0, 0.0], [5.0, 6.0, 0.0]);
        assert!(close(distance(&x, &cross).value, 0.0));
        // the nearest points are on the edges, not on their lines: the end of a short edge
        let short = edge([12.0, 1.0, 0.0], [12.0, 5.0, 0.0]);
        let m = distance(&x, &short);
        assert!(close(m.value, 5f64.sqrt()), "{}", m.value);
        assert_eq!(m.lines, vec![([10.0, 0.0, 0.0], [12.0, 1.0, 0.0])]);
    }

    #[test]
    fn an_edge_needs_a_length() {
        let e = Reference::edge(Label::InnerEdge, [1.0, 2.0, 3.0], [1.0, 2.0, 3.0]);
        assert_eq!(e.unwrap_err().code(), "PHOENIX-CAD-038");
    }

    #[test]
    fn the_angle_at_a_vertex_is_read_between_the_lines_to_the_other_two_points() {
        let o = pt(0.0, 0.0, 0.0);
        let right = angle_at(&pt(1.0, 0.0, 0.0), &o, &pt(0.0, 1.0, 0.0)).unwrap();
        assert_eq!(right.quantity, Quantity::Angle);
        assert!(close(right.value, 90.0), "{}", right.value);
        assert_eq!(
            right.lines,
            vec![([0.0; 3], [1.0, 0.0, 0.0]), ([0.0; 3], [0.0, 1.0, 0.0])]
        );
        assert!(close(
            angle_at(&pt(1.0, 0.0, 0.0), &o, &pt(1.0, 1.0, 0.0))
                .unwrap()
                .value,
            45.0
        ));
        assert!(close(
            angle_at(&pt(1.0, 0.0, 0.0), &o, &pt(-5.0, 0.0, 0.0))
                .unwrap()
                .value,
            180.0
        ));
        assert!(close(
            angle_at(&pt(1.0, 0.0, 0.0), &o, &pt(7.0, 0.0, 0.0))
                .unwrap()
                .value,
            0.0
        ));
        // (1, 1, 1) against (1, 0, 0): cos = 1 / sqrt 3
        let skew = angle_at(&pt(1.0, 1.0, 1.0), &o, &pt(1.0, 0.0, 0.0)).unwrap();
        assert!(
            (skew.value - 54.735_610_317_245_35).abs() < 1e-9,
            "{}",
            skew.value
        );
    }

    #[test]
    fn an_angle_needs_the_lines_to_have_a_length() {
        let o = pt(0.0, 0.0, 0.0);
        let same = angle_at(&pt(1.0, 0.0, 0.0), &o, &o).unwrap_err();
        assert_eq!(same.code(), "PHOENIX-CAD-038");
        let edges = angle_at(&edge([0.0; 3], [1.0, 0.0, 0.0]), &o, &o).unwrap_err();
        assert_eq!(edges.code(), "PHOENIX-CAD-038");
        assert!(angle_between_edges(&o, &o).is_err());
    }

    #[test]
    fn the_angle_between_edges_is_the_acute_one() {
        let x = edge([0.0; 3], [1.0, 0.0, 0.0]);
        // a diagonal in the plane, the same diagonal the other way round, and one in space
        let up = edge([0.0; 3], [1.0, 1.0, 0.0]);
        let down = edge([1.0, 1.0, 0.0], [0.0; 3]);
        assert!(close(angle_between_edges(&x, &up).unwrap().value, 45.0));
        assert!(close(angle_between_edges(&x, &down).unwrap().value, 45.0));
        let space = edge([0.0; 3], [1.0, 1.0, 1.0]);
        let m = angle_between_edges(&x, &space).unwrap();
        assert!((m.value - 54.735_610_317_245_35).abs() < 1e-9);
        assert_eq!(
            m.lines,
            vec![([0.0; 3], [1.0, 0.0, 0.0]), ([0.0; 3], [1.0, 1.0, 1.0])]
        );
        // parallel edges, whichever way they run: 0
        let back = edge([5.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
        assert!(close(angle_between_edges(&x, &back).unwrap().value, 0.0));
        // perpendicular: 90
        let y = edge([0.0; 3], [0.0, 3.0, 0.0]);
        assert!(close(angle_between_edges(&x, &y).unwrap().value, 90.0));
    }

    #[test]
    fn the_angle_of_what_was_picked_follows_how_many_were_picked() {
        let o = pt(0.0, 0.0, 0.0);
        let a = pt(1.0, 0.0, 0.0);
        let b = pt(0.0, 2.0, 0.0);
        assert!(close(
            angle(&[a.clone(), o.clone(), b]).unwrap().value,
            90.0
        ));
        let x = edge([0.0; 3], [1.0, 0.0, 0.0]);
        let y = edge([0.0; 3], [0.0, 1.0, 0.0]);
        assert!(close(angle(&[x.clone(), y]).unwrap().value, 90.0));
        assert_eq!(
            angle(std::slice::from_ref(&x)).unwrap_err().code(),
            "PHOENIX-CAD-038"
        );
        assert_eq!(
            angle(&[a.clone(), o.clone()]).unwrap_err().code(),
            "PHOENIX-CAD-038"
        );
        assert_eq!(angle_needs(&a), 3);
        assert_eq!(angle_needs(&x), 2);
    }
}
