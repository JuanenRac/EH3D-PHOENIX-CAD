//! The 2D sketch: geometry on a plane of the box, the constraints that tie it together, and a
//! solver that moves the geometry until the constraints hold.
//!
//! * a sketch lives on a [`SketchPlane`], a face of the box with an origin and the axes of the
//!   face; the coordinates inside it are [`Length`]s (integer nanometres);
//! * the geometry is made of points, lines (between two points), circles (a centre and a radius)
//!   and arcs (a centre and two points, counter-clockwise from the start to the end); a polyline
//!   and a rectangle are made of those, with the constraints that keep them what they are;
//! * the constraints are geometric (coincident, point on object, horizontal, vertical, parallel,
//!   perpendicular, tangent, equal, symmetric, midpoint, fixed) or dimensional (distances,
//!   length, radius, diameter, angle); a dimension holds a value and, if wanted, an expression
//!   over the variables of the project that gives that value;
//! * the identifiers are counters inside the sketch (`e1`, `c1`), so the same steps give the
//!   same identifiers, and a removed identifier is never given again;
//! * [`solver`] solves the sketch by least squares and reports the degrees of freedom left, the
//!   constraints that are redundant or in conflict, or that it did not converge;
//! * [`settings`] holds the tuned values of the solver and of the profiles (a file outside the
//!   code), [`file`] the text file of a sketch (a way in and out) and [`domain`] the domain
//!   `sketch` of the project file, where the sketches of a project are kept;
//! * the editing operations on a sketch (chamfering a corner, trimming and extending a line, repeating
//!   elements along a line or round a point, offsetting an outline) are methods of [`Sketch`].
//!
//! The solver works on floating point numbers in millimetres and rounds the result to the
//! nanometre; it uses only the operations that every platform computes the same way (sums,
//! products, quotients and square roots), so the same sketch gives the same result bit for bit.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::config::Settings;
use crate::datum::{Datum, Point};
use crate::enclosure::{Enclosure, Face};
use crate::expr::{parse, Names, Value};
use crate::units::{div_round_half_away, Angle};
use crate::vars::Variables;
use crate::{CadError, Length};

pub mod domain;
pub mod file;
mod ops;
pub mod settings;
pub mod solver;

/// The identifier of an element of a sketch (`e1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementId(pub u32);

/// The identifier of a constraint of a sketch (`c1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintId(pub u32);

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "e{}", self.0)
    }
}

impl fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c{}", self.0)
    }
}

/// The plane of a sketch: a face of the box, an origin on it given as a distance along each of
/// its two axes, and an offset along its normal (positive outwards).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SketchPlane {
    /// The face.
    pub face: Face,
    /// The origin along the first axis of the face.
    pub u0: Length,
    /// The origin along the second axis of the face.
    pub v0: Length,
    /// The offset along the normal of the face, outwards.
    pub offset: Length,
}

impl SketchPlane {
    /// The plane on a face, with its origin at the corner of the face and no offset.
    pub fn on_face(face: Face) -> SketchPlane {
        SketchPlane {
            face,
            u0: Length::ZERO,
            v0: Length::ZERO,
            offset: Length::ZERO,
        }
    }

    /// The reference frame of the plane in the model: the origin and the axes of the face.
    pub fn datum(&self, enclosure: &Enclosure) -> Result<Datum, CadError> {
        let face = Datum::of_face(enclosure, self.face);
        let origin = face.to_global([self.u0, self.v0, self.offset])?;
        Ok(Datum {
            origin,
            rotation: face.rotation,
        })
    }

    /// The point of the model at a point of the sketch.
    pub fn to_model(&self, enclosure: &Enclosure, x: Length, y: Length) -> Result<Point, CadError> {
        self.datum(enclosure)?.to_global([x, y, Length::ZERO])
    }

    /// The point of the sketch at a point of the model that lies in the plane; a point outside
    /// the plane is refused.
    pub fn from_model(
        &self,
        enclosure: &Enclosure,
        point: Point,
    ) -> Result<(Length, Length), CadError> {
        let local = self.datum(enclosure)?.to_local(point)?;
        if local[2] != Length::ZERO {
            return Err(CadError::SketchElement(
                "the point is not in the plane of the sketch".into(),
            ));
        }
        Ok((local[0], local[1]))
    }
}

/// An element of the geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    /// A point.
    Point {
        /// Along the first axis of the plane.
        x: Length,
        /// Along the second axis of the plane.
        y: Length,
    },
    /// A line between two points.
    Line {
        /// The first point.
        a: ElementId,
        /// The second point.
        b: ElementId,
    },
    /// A circle.
    Circle {
        /// The centre, a point.
        center: ElementId,
        /// The radius.
        radius: Length,
    },
    /// An arc, counter-clockwise from the start to the end; its radius is the distance from the
    /// centre to the start, and the end is kept at the same distance.
    Arc {
        /// The centre, a point.
        center: ElementId,
        /// The start, a point.
        start: ElementId,
        /// The end, a point.
        end: ElementId,
    },
}

/// What an element is, without its data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A point.
    Point,
    /// A line.
    Line,
    /// A circle.
    Circle,
    /// An arc.
    Arc,
}

impl Element {
    /// What the element is.
    pub fn kind(&self) -> Kind {
        match self {
            Element::Point { .. } => Kind::Point,
            Element::Line { .. } => Kind::Line,
            Element::Circle { .. } => Kind::Circle,
            Element::Arc { .. } => Kind::Arc,
        }
    }

    /// The elements this one is made of.
    pub fn parts(&self) -> Vec<ElementId> {
        match *self {
            Element::Point { .. } => Vec::new(),
            Element::Line { a, b } => vec![a, b],
            Element::Circle { center, .. } => vec![center],
            Element::Arc { center, start, end } => vec![center, start, end],
        }
    }
}

/// An element with its identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The identifier.
    pub id: ElementId,
    /// The element.
    pub element: Element,
    /// Construction geometry guides the drawing and takes part in the constraints, but is not part
    /// of the profile.
    pub construction: bool,
}

/// A value of a dimension and, if wanted, the expression that gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dim<T> {
    /// The value, the one the solver aims at.
    pub value: T,
    /// An expression over the variables of the project and the constants of the settings; when
    /// there is one, [`Sketch::resolve_dimensions`] sets the value from it. A single variable name
    /// is an expression.
    pub expr: Option<String>,
}

impl<T> Dim<T> {
    /// A dimension with a fixed value.
    pub fn fixed(value: T) -> Dim<T> {
        Dim { value, expr: None }
    }

    /// A dimension that is the result of an expression; `value` is its value until the
    /// expression is calculated.
    pub fn with_expr(value: T, expr: &str) -> Dim<T> {
        Dim {
            value,
            expr: Some(expr.to_string()),
        }
    }
}

/// A constraint. The elements named by a constraint are checked when it is added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    /// Two points are the same point.
    Coincident(ElementId, ElementId),
    /// A point lies on a line (the whole line, not only the segment), a circle or the circle of an
    /// arc.
    PointOnObject {
        /// The point.
        point: ElementId,
        /// The line, circle or arc.
        object: ElementId,
    },
    /// A line is parallel to the first axis.
    Horizontal(ElementId),
    /// A line is parallel to the second axis.
    Vertical(ElementId),
    /// Two points have the same second coordinate.
    HorizontalPoints(ElementId, ElementId),
    /// Two points have the same first coordinate.
    VerticalPoints(ElementId, ElementId),
    /// Two lines are parallel.
    Parallel(ElementId, ElementId),
    /// Two lines are perpendicular.
    Perpendicular(ElementId, ElementId),
    /// A line touches a circle or an arc, or two circles or arcs touch, from outside or inside
    /// as they are when the sketch is solved.
    Tangent(ElementId, ElementId),
    /// Two lines have the same length, or two circles or arcs the same radius.
    Equal(ElementId, ElementId),
    /// Two lines lie on one and the same line (they are parallel and one is on the other).
    Collinear(ElementId, ElementId),
    /// Two circles or arcs have the same centre.
    Concentric(ElementId, ElementId),
    /// Three points lie on one line.
    CollinearPoints(ElementId, ElementId, ElementId),
    /// The distance from `a` to `b` is the distance from `c` to `d`.
    EqualDistance {
        /// The first point of the first pair.
        a: ElementId,
        /// The second point of the first pair.
        b: ElementId,
        /// The first point of the second pair.
        c: ElementId,
        /// The second point of the second pair.
        d: ElementId,
    },
    /// A point is as far from `a` as it is from `b` (it lies on the perpendicular bisector of
    /// the two).
    Equidistant {
        /// The point that is as far from both.
        point: ElementId,
        /// The first of the two points.
        a: ElementId,
        /// The second of the two points.
        b: ElementId,
    },
    /// The angle from the line `a` to the line `b` is the angle from the line `c` to the line `d`.
    EqualAngle {
        /// The first line of the first pair.
        a: ElementId,
        /// The second line of the first pair.
        b: ElementId,
        /// The first line of the second pair.
        c: ElementId,
        /// The second line of the second pair.
        d: ElementId,
    },
    /// The length of the line `a` is to the length of the line `b` as `num` is to `den` (the
    /// radius of a circle or an arc, for two of those): `a * den = b * num`.
    Proportional {
        /// The first line, circle or arc.
        a: ElementId,
        /// The second line, circle or arc.
        b: ElementId,
        /// The part of the ratio that belongs to `a` (1 to 1000).
        num: u32,
        /// The part of the ratio that belongs to `b` (1 to 1000).
        den: u32,
    },
    /// The line `bisector` halves the angle from the line `a` to the line `b`: the angle from `a`
    /// to the bisector is the angle from the bisector to `b`.
    Bisector {
        /// The line that halves the angle.
        bisector: ElementId,
        /// The first side of the angle.
        a: ElementId,
        /// The second side of the angle.
        b: ElementId,
    },
    /// Two points are mirror images across a line.
    SymmetricLine {
        /// The first point.
        a: ElementId,
        /// The second point.
        b: ElementId,
        /// The line of symmetry.
        axis: ElementId,
    },
    /// Two lines are mirror images across a line: the start of the first is the mirror image of
    /// the start of the second, and the end of the first of the end of the second.
    SymmetricLines {
        /// The first line.
        a: ElementId,
        /// The second line.
        b: ElementId,
        /// The line of symmetry.
        axis: ElementId,
    },
    /// Two circles or arcs are mirror images across a line: their centres are, and their radii
    /// are equal.
    SymmetricCircles {
        /// The first circle or arc.
        a: ElementId,
        /// The second circle or arc.
        b: ElementId,
        /// The line of symmetry.
        axis: ElementId,
    },
    /// Two points are mirror images through a point.
    SymmetricPoint {
        /// The first point.
        a: ElementId,
        /// The second point.
        b: ElementId,
        /// The centre of symmetry.
        center: ElementId,
    },
    /// A point is the midpoint of a line.
    Midpoint {
        /// The point.
        point: ElementId,
        /// The line.
        line: ElementId,
    },
    /// A point is held where it is.
    Fixed {
        /// The point.
        point: ElementId,
        /// Where it is held, along the first axis.
        x: Length,
        /// Where it is held, along the second axis.
        y: Length,
    },
    /// The distance between two points.
    Distance {
        /// The first point.
        a: ElementId,
        /// The second point.
        b: ElementId,
        /// The distance.
        dim: Dim<Length>,
    },
    /// The first coordinate of `b` minus the one of `a` (it has a sign).
    DistanceX {
        /// The first point.
        a: ElementId,
        /// The second point.
        b: ElementId,
        /// The difference.
        dim: Dim<Length>,
    },
    /// The second coordinate of `b` minus the one of `a` (it has a sign).
    DistanceY {
        /// The first point.
        a: ElementId,
        /// The second point.
        b: ElementId,
        /// The difference.
        dim: Dim<Length>,
    },
    /// The length of a line.
    LineLength {
        /// The line.
        line: ElementId,
        /// The length.
        dim: Dim<Length>,
    },
    /// The radius of a circle or an arc.
    Radius {
        /// The circle or arc.
        round: ElementId,
        /// The radius.
        dim: Dim<Length>,
    },
    /// The diameter of a circle or an arc.
    Diameter {
        /// The circle or arc.
        round: ElementId,
        /// The diameter.
        dim: Dim<Length>,
    },
    /// The angle from the direction of the first line to the direction of the second, counted
    /// counter-clockwise. A line has no preferred sense, so an angle and the same angle plus a half
    /// turn are the same constraint; the solver keeps the one the sketch is nearer to.
    AngleBetween {
        /// The first line.
        a: ElementId,
        /// The second line.
        b: ElementId,
        /// The angle.
        dim: Dim<Angle>,
    },
    /// The distance from a point to the line through a line (the whole line, not only the
    /// segment). The point keeps the side of the line it is on when the solver starts (a point
    /// on the line itself counts as being on the left).
    PointLineDistance {
        /// The point.
        point: ElementId,
        /// The line.
        line: ElementId,
        /// The distance.
        dim: Dim<Length>,
    },
    /// The angle from the first axis to the direction of a line, counted counter-clockwise. A
    /// line has no preferred sense, so an angle and the same angle plus a half turn are the same
    /// constraint.
    LineAngle {
        /// The line.
        line: ElementId,
        /// The angle.
        dim: Dim<Angle>,
    },
}

impl Constraint {
    /// The name of the kind, as used in the file and in messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Constraint::Coincident(..) => "coincident",
            Constraint::PointOnObject { .. } => "point_on_object",
            Constraint::Horizontal(..) => "horizontal",
            Constraint::Vertical(..) => "vertical",
            Constraint::HorizontalPoints(..) => "horizontal_points",
            Constraint::VerticalPoints(..) => "vertical_points",
            Constraint::Parallel(..) => "parallel",
            Constraint::Perpendicular(..) => "perpendicular",
            Constraint::Tangent(..) => "tangent",
            Constraint::Equal(..) => "equal",
            Constraint::Collinear(..) => "collinear",
            Constraint::Concentric(..) => "concentric",
            Constraint::CollinearPoints(..) => "collinear_points",
            Constraint::EqualDistance { .. } => "equal_distance",
            Constraint::Equidistant { .. } => "equidistant",
            Constraint::EqualAngle { .. } => "equal_angle",
            Constraint::Proportional { .. } => "proportional",
            Constraint::Bisector { .. } => "bisector",
            Constraint::SymmetricLine { .. } => "symmetric_line",
            Constraint::SymmetricLines { .. } => "symmetric_lines",
            Constraint::SymmetricCircles { .. } => "symmetric_circles",
            Constraint::SymmetricPoint { .. } => "symmetric_point",
            Constraint::Midpoint { .. } => "midpoint",
            Constraint::Fixed { .. } => "fixed",
            Constraint::Distance { .. } => "distance",
            Constraint::DistanceX { .. } => "distance_x",
            Constraint::DistanceY { .. } => "distance_y",
            Constraint::LineLength { .. } => "length",
            Constraint::Radius { .. } => "radius",
            Constraint::Diameter { .. } => "diameter",
            Constraint::AngleBetween { .. } => "angle",
            Constraint::PointLineDistance { .. } => "point_line_distance",
            Constraint::LineAngle { .. } => "line_angle",
        }
    }

    /// The elements the constraint is about.
    pub fn elements(&self) -> Vec<ElementId> {
        match self {
            Constraint::Coincident(a, b)
            | Constraint::HorizontalPoints(a, b)
            | Constraint::VerticalPoints(a, b)
            | Constraint::Parallel(a, b)
            | Constraint::Perpendicular(a, b)
            | Constraint::Tangent(a, b)
            | Constraint::Equal(a, b)
            | Constraint::Collinear(a, b)
            | Constraint::Concentric(a, b)
            | Constraint::Distance { a, b, .. }
            | Constraint::DistanceX { a, b, .. }
            | Constraint::DistanceY { a, b, .. }
            | Constraint::Proportional { a, b, .. }
            | Constraint::AngleBetween { a, b, .. } => vec![*a, *b],
            Constraint::PointOnObject { point, object } => vec![*point, *object],
            Constraint::CollinearPoints(a, b, c) => vec![*a, *b, *c],
            Constraint::EqualDistance { a, b, c, d } | Constraint::EqualAngle { a, b, c, d } => {
                vec![*a, *b, *c, *d]
            }
            Constraint::Equidistant { point, a, b } => vec![*point, *a, *b],
            Constraint::Bisector { bisector, a, b } => vec![*bisector, *a, *b],
            Constraint::Horizontal(l) | Constraint::Vertical(l) => vec![*l],
            Constraint::SymmetricLine { a, b, axis }
            | Constraint::SymmetricLines { a, b, axis }
            | Constraint::SymmetricCircles { a, b, axis } => vec![*a, *b, *axis],
            Constraint::SymmetricPoint { a, b, center } => vec![*a, *b, *center],
            Constraint::Midpoint { point, line } => vec![*point, *line],
            Constraint::Fixed { point, .. } => vec![*point],
            Constraint::LineLength { line, .. } | Constraint::LineAngle { line, .. } => {
                vec![*line]
            }
            Constraint::PointLineDistance { point, line, .. } => vec![*point, *line],
            Constraint::Radius { round, .. } | Constraint::Diameter { round, .. } => {
                vec![*round]
            }
        }
    }

    /// Whether the constraint is a dimension whose value follows an expression.
    pub fn has_expression(&self) -> bool {
        match self {
            Constraint::Distance { dim, .. }
            | Constraint::DistanceX { dim, .. }
            | Constraint::DistanceY { dim, .. }
            | Constraint::LineLength { dim, .. }
            | Constraint::PointLineDistance { dim, .. }
            | Constraint::Radius { dim, .. }
            | Constraint::Diameter { dim, .. } => dim.expr.is_some(),
            Constraint::AngleBetween { dim, .. } | Constraint::LineAngle { dim, .. } => {
                dim.expr.is_some()
            }
            _ => false,
        }
    }

    /// Whether the constraint is a dimension (it has a value).
    pub fn is_dimension(&self) -> bool {
        matches!(
            self,
            Constraint::Distance { .. }
                | Constraint::DistanceX { .. }
                | Constraint::DistanceY { .. }
                | Constraint::LineLength { .. }
                | Constraint::Radius { .. }
                | Constraint::Diameter { .. }
                | Constraint::AngleBetween { .. }
                | Constraint::PointLineDistance { .. }
                | Constraint::LineAngle { .. }
        )
    }
}

/// A constraint with its identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintEntry {
    /// The identifier.
    pub id: ConstraintId,
    /// The constraint.
    pub constraint: Constraint,
    /// A constraint that is not enabled is kept but the solver ignores it.
    pub enabled: bool,
    /// A reference dimension: it shows what the drawing measures and holds nothing, so the solver
    /// ignores it. Only a dimension can be one, and it has no expression.
    pub reference: bool,
}

impl ConstraintEntry {
    /// Whether the solver takes the constraint into account: enabled and not a reference.
    pub fn drives(&self) -> bool {
        self.enabled && !self.reference
    }
}

/// The four points and four lines of a rectangle made by [`Sketch::add_rectangle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rectangle {
    /// The corners, counter-clockwise from the one with the smallest coordinates.
    pub points: [ElementId; 4],
    /// The sides: from the first corner to the second (bottom), second to third (right), third to
    /// fourth (top) and fourth to first (left).
    pub lines: [ElementId; 4],
}

/// What [`Sketch::round_corner`] made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundedCorner {
    /// The arc that rounds the corner.
    pub arc: ElementId,
    /// The radius dimension of the arc.
    pub radius: ConstraintId,
}

/// A sketch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sketch {
    name: String,
    plane: SketchPlane,
    elements: Vec<Entry>,
    constraints: Vec<ConstraintEntry>,
    next_element: u32,
    next_constraint: u32,
}

/// The longest name of a sketch.
pub const MAX_NAME_BYTES: usize = 64;

/// Checks the name of a sketch: letters, digits, `_` and `-`.
pub fn check_sketch_name(name: &str) -> Result<(), CadError> {
    if !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(())
    } else {
        Err(CadError::InvalidName(name.to_string()))
    }
}

fn bad_element(why: impl Into<String>) -> CadError {
    CadError::SketchElement(why.into())
}

fn bad_constraint(why: impl Into<String>) -> CadError {
    CadError::SketchConstraint(why.into())
}

fn check_expr_text(text: &str) -> Result<(), CadError> {
    if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
        Err(bad_constraint(
            "an expression is one line of at most 512 characters",
        ))
    } else {
        Ok(())
    }
}

impl Sketch {
    /// An empty sketch on a plane.
    pub fn new(name: &str, plane: SketchPlane) -> Result<Sketch, CadError> {
        check_sketch_name(name)?;
        Ok(Sketch {
            name: name.to_string(),
            plane,
            elements: Vec::new(),
            constraints: Vec::new(),
            next_element: 1,
            next_constraint: 1,
        })
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gives the sketch another name: letters, digits, `_` and `-`.
    pub fn set_name(&mut self, name: &str) -> Result<(), CadError> {
        check_sketch_name(name)?;
        self.name = name.to_string();
        Ok(())
    }

    /// The plane.
    pub fn plane(&self) -> &SketchPlane {
        &self.plane
    }

    /// Moves the sketch to another plane.
    pub fn set_plane(&mut self, plane: SketchPlane) {
        self.plane = plane;
    }

    /// The elements, in the order they were added.
    pub fn elements(&self) -> &[Entry] {
        &self.elements
    }

    /// The constraints, in the order they were added.
    pub fn constraints(&self) -> &[ConstraintEntry] {
        &self.constraints
    }

    /// The counters of the next identifiers, kept in the file so that a removed identifier is
    /// never given again.
    pub fn next_ids(&self) -> (u32, u32) {
        (self.next_element, self.next_constraint)
    }

    /// An element by identifier.
    pub fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements
            .iter()
            .find(|e| e.id == id)
            .map(|e| &e.element)
    }

    /// A constraint by identifier.
    pub fn constraint(&self, id: ConstraintId) -> Option<&ConstraintEntry> {
        self.constraints.iter().find(|c| c.id == id)
    }

    /// The position of a point.
    pub fn point(&self, id: ElementId) -> Result<(Length, Length), CadError> {
        match self.element(id) {
            Some(Element::Point { x, y }) => Ok((*x, *y)),
            Some(_) => Err(bad_element(format!("{id} is not a point"))),
            None => Err(CadError::UnknownName(id.to_string())),
        }
    }

    fn kind_of(&self, id: ElementId) -> Result<Kind, CadError> {
        self.element(id)
            .map(Element::kind)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))
    }

    fn need(&self, id: ElementId, allowed: &[Kind], what: &str) -> Result<Kind, CadError> {
        let kind = self.kind_of(id)?;
        if allowed.contains(&kind) {
            Ok(kind)
        } else {
            Err(bad_constraint(format!("{id} is not {what}")))
        }
    }

    fn take_element_id(&mut self) -> Result<ElementId, CadError> {
        let id = ElementId(self.next_element);
        self.next_element = self
            .next_element
            .checked_add(1)
            .ok_or(CadError::Overflow("element identifiers"))?;
        Ok(id)
    }

    fn push_element(
        &mut self,
        element: Element,
        construction: bool,
    ) -> Result<ElementId, CadError> {
        let id = self.take_element_id()?;
        self.elements.push(Entry {
            id,
            element,
            construction,
        });
        Ok(id)
    }

    /// Adds a point.
    pub fn add_point(&mut self, x: Length, y: Length) -> Result<ElementId, CadError> {
        self.push_element(Element::Point { x, y }, false)
    }

    /// Adds a line between two points that exist and differ.
    pub fn add_line(&mut self, a: ElementId, b: ElementId) -> Result<ElementId, CadError> {
        self.check_line(a, b)?;
        self.push_element(Element::Line { a, b }, false)
    }

    fn check_line(&self, a: ElementId, b: ElementId) -> Result<(), CadError> {
        self.need_point(a)?;
        self.need_point(b)?;
        if a == b {
            return Err(bad_element("a line needs two different points"));
        }
        Ok(())
    }

    fn need_point(&self, id: ElementId) -> Result<(), CadError> {
        match self.kind_of(id)? {
            Kind::Point => Ok(()),
            _ => Err(bad_element(format!("{id} is not a point"))),
        }
    }

    /// Adds a circle around a point, with a positive radius.
    pub fn add_circle(&mut self, center: ElementId, radius: Length) -> Result<ElementId, CadError> {
        self.need_point(center)?;
        if radius <= Length::ZERO {
            return Err(bad_element("the radius of a circle is positive"));
        }
        self.push_element(Element::Circle { center, radius }, false)
    }

    /// Adds an arc from three different points.
    pub fn add_arc(
        &mut self,
        center: ElementId,
        start: ElementId,
        end: ElementId,
    ) -> Result<ElementId, CadError> {
        for id in [center, start, end] {
            self.need_point(id)?;
        }
        if center == start || center == end || start == end {
            return Err(bad_element("an arc needs three different points"));
        }
        self.push_element(Element::Arc { center, start, end }, false)
    }

    /// Adds a polyline through the points: the points, and a line between each two that follow
    /// each other (the last to the first when `closed`). The lines share their points. Returns the
    /// lines. At least two points, three when closed.
    pub fn add_polyline(
        &mut self,
        points: &[(Length, Length)],
        closed: bool,
    ) -> Result<Vec<ElementId>, CadError> {
        let least = if closed { 3 } else { 2 };
        if points.len() < least {
            return Err(bad_element(format!("a polyline needs {least} points")));
        }
        let saved = self.clone();
        let result = (|| {
            let ids = points
                .iter()
                .map(|(x, y)| self.add_point(*x, *y))
                .collect::<Result<Vec<_>, _>>()?;
            let mut lines = Vec::new();
            for pair in ids.windows(2) {
                lines.push(self.add_line(pair[0], pair[1])?);
            }
            if closed {
                lines.push(self.add_line(ids[ids.len() - 1], ids[0])?);
            }
            Ok(lines)
        })();
        if result.is_err() {
            *self = saved;
        }
        result
    }

    /// Adds a rectangle between two opposite corners: four points, four lines and the four
    /// constraints that keep the sides horizontal and vertical. It leaves four degrees of freedom
    /// (the position of a corner, the width and the height).
    pub fn add_rectangle(
        &mut self,
        corner: (Length, Length),
        opposite: (Length, Length),
    ) -> Result<Rectangle, CadError> {
        if corner.0 == opposite.0 || corner.1 == opposite.1 {
            return Err(bad_element("a rectangle has a width and a height"));
        }
        let (x0, x1) = (corner.0.min(opposite.0), corner.0.max(opposite.0));
        let (y0, y1) = (corner.1.min(opposite.1), corner.1.max(opposite.1));
        let saved = self.clone();
        let result = (|| {
            let p = [
                self.add_point(x0, y0)?,
                self.add_point(x1, y0)?,
                self.add_point(x1, y1)?,
                self.add_point(x0, y1)?,
            ];
            let l = [
                self.add_line(p[0], p[1])?,
                self.add_line(p[1], p[2])?,
                self.add_line(p[2], p[3])?,
                self.add_line(p[3], p[0])?,
            ];
            self.add_constraint(Constraint::Horizontal(l[0]))?;
            self.add_constraint(Constraint::Vertical(l[1]))?;
            self.add_constraint(Constraint::Horizontal(l[2]))?;
            self.add_constraint(Constraint::Vertical(l[3]))?;
            Ok(Rectangle {
                points: p,
                lines: l,
            })
        })();
        if result.is_err() {
            *self = saved;
        }
        result
    }

    /// Marks an element as construction geometry or as part of the profile.
    pub fn set_construction(&mut self, id: ElementId, construction: bool) -> Result<(), CadError> {
        let entry = self
            .elements
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        entry.construction = construction;
        Ok(())
    }

    /// Moves a point; the solver moves points too, so this is where the drawing starts from.
    pub fn move_point(&mut self, id: ElementId, x: Length, y: Length) -> Result<(), CadError> {
        self.need_point(id)?;
        if let Some(entry) = self.elements.iter_mut().find(|e| e.id == id) {
            entry.element = Element::Point { x, y };
        }
        Ok(())
    }

    /// Changes the radius of a circle (the radius of an arc is the distance of its start).
    pub fn set_circle_radius(&mut self, id: ElementId, radius: Length) -> Result<(), CadError> {
        if radius <= Length::ZERO {
            return Err(bad_element("the radius of a circle is positive"));
        }
        match self.elements.iter_mut().find(|e| e.id == id) {
            Some(Entry {
                element: Element::Circle { radius: r, .. },
                ..
            }) => {
                *r = radius;
                Ok(())
            }
            Some(_) => Err(bad_element(format!("{id} is not a circle"))),
            None => Err(CadError::UnknownName(id.to_string())),
        }
    }

    /// Removes an element that nothing else is made of and no constraint is about.
    pub fn remove_element(&mut self, id: ElementId) -> Result<(), CadError> {
        if self.element(id).is_none() {
            return Err(CadError::UnknownName(id.to_string()));
        }
        let used_by_element = self
            .elements
            .iter()
            .any(|e| e.id != id && e.element.parts().contains(&id));
        let used_by_constraint = self
            .constraints
            .iter()
            .any(|c| c.constraint.elements().contains(&id));
        if used_by_element || used_by_constraint {
            return Err(CadError::InUse(id.to_string()));
        }
        self.elements.retain(|e| e.id != id);
        Ok(())
    }

    /// The two points of a line.
    fn line_points(&self, id: ElementId) -> Result<(ElementId, ElementId), CadError> {
        match self.element(id) {
            Some(Element::Line { a, b }) => Ok((*a, *b)),
            Some(_) => Err(bad_element(format!("{id} is not a line"))),
            None => Err(CadError::UnknownName(id.to_string())),
        }
    }

    /// Makes mirror images of `items` across the line `axis`: a point becomes a point, a line a
    /// line between the images of its ends, a circle a circle of the same radius about the image
    /// of its centre, and an arc an arc whose start and end are the images of its end and start
    /// (a mirror image turns the other way round). A point of an item that lies on the axis is its
    /// own image, so the image is joined to the original there. Each point has one image, however
    /// many items share it, and an item that would be drawn on top of itself (a line along the
    /// axis, a circle centred on it, a point on it) has none. The images keep the construction flag of the
    /// original. With `constrain` each image point is made the mirror image of its original with
    /// a symmetric constraint and each circle gets an equal radius, so the images follow the
    /// originals when the sketch is solved again; the sketch is left as it was when anything is
    /// refused. Returns the new items, in the order of `items`.
    pub fn mirror(
        &mut self,
        items: &[ElementId],
        axis: ElementId,
        constrain: bool,
    ) -> Result<Vec<ElementId>, CadError> {
        let saved = self.clone();
        let result = self.mirror_inner(items, axis, constrain);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn mirror_inner(
        &mut self,
        items: &[ElementId],
        axis: ElementId,
        constrain: bool,
    ) -> Result<Vec<ElementId>, CadError> {
        if items.is_empty() {
            return Err(bad_element("there is nothing to mirror"));
        }
        let (a0, a1) = self.line_points(axis)?;
        if items.contains(&axis) {
            return Err(bad_element("the axis of a mirror image is not mirrored"));
        }
        for (i, id) in items.iter().enumerate() {
            if self.element(*id).is_none() {
                return Err(CadError::UnknownName(id.to_string()));
            }
            if items[..i].contains(id) {
                return Err(bad_element(format!("{id} is named twice")));
            }
        }
        let at = |sk: &Sketch, id: ElementId| -> Result<(f64, f64), CadError> {
            let (x, y) = sk.point(id)?;
            Ok((x.get() as f64, y.get() as f64))
        };
        let (p, q) = (at(self, a0)?, at(self, a1)?);
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = dx.hypot(dy);
        if len < 1.0 {
            return Err(bad_element("the axis of a mirror image has no length"));
        }
        let (dx, dy) = (dx / len, dy / len);
        let reflect = |v: (f64, f64)| -> (f64, f64) {
            let t = (v.0 - p.0) * dx + (v.1 - p.1) * dy;
            (2.0 * (p.0 + t * dx) - v.0, 2.0 * (p.1 + t * dy) - v.1)
        };
        // the points the items are made of, in the order of the items
        let mut needed: Vec<ElementId> = Vec::new();
        for id in items {
            let parts = match self.element(*id) {
                Some(Element::Point { .. }) => vec![*id],
                Some(e) => e.parts(),
                None => Vec::new(),
            };
            for part in parts {
                if !needed.contains(&part) {
                    needed.push(part);
                }
            }
        }
        let mut image: BTreeMap<ElementId, ElementId> = BTreeMap::new();
        let mut moved: Vec<(ElementId, ElementId)> = Vec::new();
        for id in needed {
            let here = self.point(id)?;
            let (x, y) = reflect(at(self, id)?);
            let there = (
                Length::new(x.round() as i64)?,
                Length::new(y.round() as i64)?,
            );
            if there == here {
                image.insert(id, id);
            } else {
                let new = self.add_point(there.0, there.1)?;
                image.insert(id, new);
                moved.push((id, new));
            }
        }
        let mut made = Vec::new();
        for id in items {
            let Some(entry) = self.elements.iter().find(|e| e.id == *id).cloned() else {
                continue;
            };
            let new = match entry.element {
                Element::Point { .. } => (image[id] != *id).then_some(image[id]),
                Element::Line { a, b } => {
                    let (ia, ib) = (image[&a], image[&b]);
                    if (ia, ib) == (a, b) || (ia, ib) == (b, a) {
                        None
                    } else {
                        Some(self.add_line(ia, ib)?)
                    }
                }
                Element::Circle { center, radius } => {
                    if image[&center] == center {
                        None
                    } else {
                        let new = self.add_circle(image[&center], radius)?;
                        if constrain {
                            self.add_constraint(Constraint::Equal(*id, new))?;
                        }
                        Some(new)
                    }
                }
                Element::Arc { center, start, end } => {
                    Some(self.add_arc(image[&center], image[&end], image[&start])?)
                }
            };
            if let Some(new) = new {
                if entry.construction {
                    self.set_construction(new, true)?;
                }
                made.push(new);
            }
        }
        if constrain {
            for (original, copy) in moved {
                self.add_constraint(Constraint::SymmetricLine {
                    a: original,
                    b: copy,
                    axis,
                })?;
            }
        }
        Ok(made)
    }

    /// Rounds the corner where two lines meet with an arc of `radius`: the lines are shortened to
    /// the points where the arc touches them, and a new arc joins those points with a radius
    /// dimension of its own, so the radius can be edited later (the arc starts and ends on the
    /// lines, so it is tangent to them as it is made, but no tangent constraint is added: the
    /// solver finds one at the end of a line redundant). The lines must share exactly one
    /// point, the corner; that point must belong to nothing else and no constraint may be about
    /// it. A line that has a constraint other than horizontal, vertical, parallel, perpendicular
    /// or collinear is refused (shortening it would break the others), and so are two lines that
    /// are collinear and a radius that needs more of a line than it has (a millimetre is always
    /// left). The sketch is left as it was when the corner is refused.
    pub fn round_corner(
        &mut self,
        a: ElementId,
        b: ElementId,
        radius: Length,
    ) -> Result<RoundedCorner, CadError> {
        let saved = self.clone();
        let result = self.round_corner_inner(a, b, radius);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn round_corner_inner(
        &mut self,
        a: ElementId,
        b: ElementId,
        radius: Length,
    ) -> Result<RoundedCorner, CadError> {
        if a == b {
            return Err(bad_element("a corner is made of two different lines"));
        }
        if radius <= Length::ZERO {
            return Err(bad_element("the radius of a corner is positive"));
        }
        let (a0, a1) = self.line_points(a)?;
        let (b0, b1) = self.line_points(b)?;
        let shared: Vec<ElementId> = [a0, a1]
            .into_iter()
            .filter(|p| *p == b0 || *p == b1)
            .collect();
        let [corner] = shared[..] else {
            return Err(bad_element(
                "the two lines must share one point, the corner",
            ));
        };
        let far_a = if a0 == corner { a1 } else { a0 };
        let far_b = if b0 == corner { b1 } else { b0 };
        if self
            .elements
            .iter()
            .any(|e| e.id != a && e.id != b && e.element.parts().contains(&corner))
        {
            return Err(bad_element("the corner point belongs to something else"));
        }
        for c in &self.constraints {
            let about = c.constraint.elements();
            let on_corner = about.contains(&corner);
            let on_lines = about.contains(&a) || about.contains(&b);
            let keeps = matches!(
                c.constraint,
                Constraint::Horizontal(_)
                    | Constraint::Vertical(_)
                    | Constraint::Parallel(..)
                    | Constraint::Perpendicular(..)
                    | Constraint::Collinear(..)
            );
            if on_corner || (on_lines && !keeps) {
                return Err(bad_constraint(
                    "a constraint is about the corner or about the length of a line",
                ));
            }
        }
        let at = |id: ElementId| -> Result<(f64, f64), CadError> {
            let (x, y) = self.point(id)?;
            Ok((x.get() as f64, y.get() as f64))
        };
        let (p, pa, pb) = (at(corner)?, at(far_a)?, at(far_b)?);
        let (da, db) = ((pa.0 - p.0, pa.1 - p.1), (pb.0 - p.0, pb.1 - p.1));
        let (la, lb) = (da.0.hypot(da.1), db.0.hypot(db.1));
        let (u, v) = ((da.0 / la, da.1 / la), (db.0 / lb, db.1 / lb));
        let cross = u.0 * v.1 - u.1 * v.0;
        if cross.abs() < 1e-9 {
            return Err(bad_element(
                "the two lines are on one line: there is no corner",
            ));
        }
        let half = (u.0 * v.0 + u.1 * v.1).clamp(-1.0, 1.0).acos() / 2.0;
        let r = radius.get() as f64;
        // how far from the corner the arc touches each line, and how far its centre is
        let touch = r / half.tan();
        let to_centre = r / half.sin();
        let room = 1_000_000.0;
        if touch + room > la || touch + room > lb {
            return Err(bad_element(
                "the lines are too short for this radius (a millimetre of each must be left)",
            ));
        }
        let bis = (u.0 + v.0, u.1 + v.1);
        let bl = bis.0.hypot(bis.1);
        let place = |x: f64, y: f64| -> Result<(Length, Length), CadError> {
            Ok((
                Length::new(x.round() as i64)?,
                Length::new(y.round() as i64)?,
            ))
        };
        let t1 = place(p.0 + u.0 * touch, p.1 + u.1 * touch)?;
        let t2 = place(p.0 + v.0 * touch, p.1 + v.1 * touch)?;
        let centre = (p.0 + bis.0 / bl * to_centre, p.1 + bis.1 / bl * to_centre);
        let c = place(centre.0, centre.1)?;
        let (t1_id, t2_id, c_id) = (
            self.add_point(t1.0, t1.1)?,
            self.add_point(t2.0, t2.1)?,
            self.add_point(c.0, c.1)?,
        );
        // a line comes after its points in the list: the new points go before the first of the two
        let first_line = self
            .elements
            .iter()
            .position(|e| e.id == a || e.id == b)
            .unwrap_or(0);
        let new_points = self.elements.split_off(self.elements.len() - 3);
        self.elements.splice(first_line..first_line, new_points);
        for (line, new) in [(a, t1_id), (b, t2_id)] {
            if let Some(Entry {
                element: Element::Line { a, b },
                ..
            }) = self.elements.iter_mut().find(|e| e.id == line)
            {
                if *a == corner {
                    *a = new;
                } else {
                    *b = new;
                }
            }
        }
        // the arc runs counter-clockwise: from the touch point whose turn about the centre is
        // the earlier one
        let turn = (t1.0.get() as f64 - c.0.get() as f64) * (t2.1.get() as f64 - c.1.get() as f64)
            - (t1.1.get() as f64 - c.1.get() as f64) * (t2.0.get() as f64 - c.0.get() as f64);
        let (start, end) = if turn > 0.0 {
            (t1_id, t2_id)
        } else {
            (t2_id, t1_id)
        };
        let arc = self.add_arc(c_id, start, end)?;
        self.elements.retain(|e| e.id != corner);
        let dimension = self.add_constraint(Constraint::Radius {
            round: arc,
            dim: Dim::fixed(radius),
        })?;
        Ok(RoundedCorner {
            arc,
            radius: dimension,
        })
    }

    fn check_distinct(a: ElementId, b: ElementId) -> Result<(), CadError> {
        if a == b {
            Err(bad_constraint(format!("{a} cannot be tied to itself")))
        } else {
            Ok(())
        }
    }

    fn check_dim_length(&self, dim: &Dim<Length>, positive: bool) -> Result<(), CadError> {
        if let Some(text) = &dim.expr {
            check_expr_text(text)?;
        }
        if positive && dim.value <= Length::ZERO {
            return Err(bad_constraint("this dimension is positive"));
        }
        Ok(())
    }

    /// Checks that a constraint is about elements of the right kinds.
    pub fn check_constraint(&self, constraint: &Constraint) -> Result<(), CadError> {
        use Kind::{Arc, Circle, Line, Point};
        match constraint {
            Constraint::Coincident(a, b)
            | Constraint::HorizontalPoints(a, b)
            | Constraint::VerticalPoints(a, b) => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Point], "a point")?;
                self.need(*b, &[Point], "a point")?;
            }
            Constraint::PointOnObject { point, object } => {
                self.need(*point, &[Point], "a point")?;
                self.need(*object, &[Line, Circle, Arc], "a line, a circle or an arc")?;
            }
            Constraint::Horizontal(l) | Constraint::Vertical(l) => {
                self.need(*l, &[Line], "a line")?;
            }
            Constraint::Parallel(a, b)
            | Constraint::Perpendicular(a, b)
            | Constraint::Collinear(a, b)
            | Constraint::AngleBetween { a, b, .. } => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Line], "a line")?;
                self.need(*b, &[Line], "a line")?;
            }
            Constraint::Tangent(a, b) => {
                Self::check_distinct(*a, *b)?;
                let ka = self.need(*a, &[Line, Circle, Arc], "a line, a circle or an arc")?;
                let kb = self.need(*b, &[Line, Circle, Arc], "a line, a circle or an arc")?;
                if ka == Line && kb == Line {
                    return Err(bad_constraint("two lines cannot be tangent"));
                }
            }
            Constraint::Equal(a, b) => {
                Self::check_distinct(*a, *b)?;
                let ka = self.need(*a, &[Line, Circle, Arc], "a line, a circle or an arc")?;
                let kb = self.need(*b, &[Line, Circle, Arc], "a line, a circle or an arc")?;
                if (ka == Line) != (kb == Line) {
                    return Err(bad_constraint(
                        "equal ties two lines or two circles or arcs, not one of each",
                    ));
                }
            }
            Constraint::Proportional { a, b, num, den } => {
                Self::check_distinct(*a, *b)?;
                let ka = self.need(*a, &[Line, Circle, Arc], "a line, a circle or an arc")?;
                let kb = self.need(*b, &[Line, Circle, Arc], "a line, a circle or an arc")?;
                if (ka == Line) != (kb == Line) {
                    return Err(bad_constraint(
                        "a proportion ties two lines or two circles or arcs, not one of each",
                    ));
                }
                if !(1..=1000).contains(num) || !(1..=1000).contains(den) {
                    return Err(bad_constraint(
                        "the two parts of a proportion are whole numbers from 1 to 1000"
                            .to_string(),
                    ));
                }
            }
            Constraint::Bisector { bisector, a, b } => {
                Self::check_distinct(*a, *b)?;
                Self::check_distinct(*bisector, *a)?;
                Self::check_distinct(*bisector, *b)?;
                for id in [bisector, a, b] {
                    self.need(*id, &[Line], "a line")?;
                }
            }
            Constraint::Concentric(a, b) => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Circle, Arc], "a circle or an arc")?;
                self.need(*b, &[Circle, Arc], "a circle or an arc")?;
            }
            Constraint::CollinearPoints(a, b, c) => {
                Self::check_distinct(*a, *b)?;
                Self::check_distinct(*a, *c)?;
                Self::check_distinct(*b, *c)?;
                for id in [a, b, c] {
                    self.need(*id, &[Point], "a point")?;
                }
            }
            Constraint::EqualDistance { a, b, c, d } => {
                Self::check_distinct(*a, *b)?;
                Self::check_distinct(*c, *d)?;
                if (a, b) == (c, d) || (a, b) == (d, c) {
                    return Err(bad_constraint(
                        "the two pairs of points are the same pair".to_string(),
                    ));
                }
                for id in [a, b, c, d] {
                    self.need(*id, &[Point], "a point")?;
                }
            }
            Constraint::Equidistant { point, a, b } => {
                Self::check_distinct(*a, *b)?;
                Self::check_distinct(*point, *a)?;
                Self::check_distinct(*point, *b)?;
                for id in [point, a, b] {
                    self.need(*id, &[Point], "a point")?;
                }
            }
            Constraint::EqualAngle { a, b, c, d } => {
                Self::check_distinct(*a, *b)?;
                Self::check_distinct(*c, *d)?;
                if (a, b) == (c, d) {
                    return Err(bad_constraint(
                        "the two pairs of lines are the same pair".to_string(),
                    ));
                }
                for id in [a, b, c, d] {
                    self.need(*id, &[Line], "a line")?;
                }
            }
            Constraint::SymmetricLine { a, b, axis } => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Point], "a point")?;
                self.need(*b, &[Point], "a point")?;
                self.need(*axis, &[Line], "a line")?;
            }
            Constraint::SymmetricLines { a, b, axis } => {
                Self::check_distinct(*a, *b)?;
                Self::check_distinct(*a, *axis)?;
                Self::check_distinct(*b, *axis)?;
                for id in [a, b, axis] {
                    self.need(*id, &[Line], "a line")?;
                }
            }
            Constraint::SymmetricCircles { a, b, axis } => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Circle, Arc], "a circle or an arc")?;
                self.need(*b, &[Circle, Arc], "a circle or an arc")?;
                self.need(*axis, &[Line], "a line")?;
            }
            Constraint::SymmetricPoint { a, b, center } => {
                Self::check_distinct(*a, *b)?;
                Self::check_distinct(*a, *center)?;
                Self::check_distinct(*b, *center)?;
                for id in [a, b, center] {
                    self.need(*id, &[Point], "a point")?;
                }
            }
            Constraint::Midpoint { point, line } => {
                self.need(*point, &[Point], "a point")?;
                self.need(*line, &[Line], "a line")?;
            }
            Constraint::Fixed { point, .. } => {
                self.need(*point, &[Point], "a point")?;
            }
            Constraint::Distance { a, b, dim } => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Point], "a point")?;
                self.need(*b, &[Point], "a point")?;
                self.check_dim_length(dim, true)?;
            }
            Constraint::DistanceX { a, b, dim } | Constraint::DistanceY { a, b, dim } => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Point], "a point")?;
                self.need(*b, &[Point], "a point")?;
                self.check_dim_length(dim, false)?;
            }
            Constraint::LineLength { line, dim } => {
                self.need(*line, &[Line], "a line")?;
                self.check_dim_length(dim, true)?;
            }
            Constraint::PointLineDistance { point, line, dim } => {
                self.need(*point, &[Point], "a point")?;
                self.need(*line, &[Line], "a line")?;
                self.check_dim_length(dim, true)?;
            }
            Constraint::LineAngle { line, .. } => {
                self.need(*line, &[Line], "a line")?;
            }
            Constraint::Radius { round, dim } | Constraint::Diameter { round, dim } => {
                self.need(*round, &[Circle, Arc], "a circle or an arc")?;
                self.check_dim_length(dim, true)?;
            }
        }
        if let Constraint::AngleBetween { dim, .. } | Constraint::LineAngle { dim, .. } = constraint
        {
            if let Some(text) = &dim.expr {
                check_expr_text(text)?;
            }
        }
        Ok(())
    }

    /// Adds a constraint, enabled.
    pub fn add_constraint(&mut self, constraint: Constraint) -> Result<ConstraintId, CadError> {
        self.check_constraint(&constraint)?;
        let id = ConstraintId(self.next_constraint);
        self.next_constraint = self
            .next_constraint
            .checked_add(1)
            .ok_or(CadError::Overflow("constraint identifiers"))?;
        self.constraints.push(ConstraintEntry {
            id,
            constraint,
            enabled: true,
            reference: false,
        });
        Ok(id)
    }

    /// Holds a point where it is now.
    pub fn fix_point(&mut self, point: ElementId) -> Result<ConstraintId, CadError> {
        let (x, y) = self.point(point)?;
        self.add_constraint(Constraint::Fixed { point, x, y })
    }

    /// Removes a constraint.
    pub fn remove_constraint(&mut self, id: ConstraintId) -> Result<(), CadError> {
        let before = self.constraints.len();
        self.constraints.retain(|c| c.id != id);
        if self.constraints.len() == before {
            Err(CadError::UnknownName(id.to_string()))
        } else {
            Ok(())
        }
    }

    /// Turns a constraint on or off.
    pub fn set_enabled(&mut self, id: ConstraintId, enabled: bool) -> Result<(), CadError> {
        let entry = self
            .constraints
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        entry.enabled = enabled;
        Ok(())
    }

    /// The dimension `id` with the value that the drawing measures now, with no expression. A
    /// constraint that is not a dimension is refused. An angle is given from 0 to less than a half
    /// turn (a line has no preferred sense) and a distance from a point to a line without a side.
    pub fn measured(&self, id: ConstraintId) -> Result<Constraint, CadError> {
        let entry = self
            .constraint(id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        let at = |p: ElementId| -> Result<(f64, f64), CadError> {
            let (x, y) = self.point(p)?;
            Ok((x.get() as f64, y.get() as f64))
        };
        let distance = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
        let ends = |line: ElementId| -> Result<((f64, f64), (f64, f64)), CadError> {
            let (a, b) = self.line_points(line)?;
            Ok((at(a)?, at(b)?))
        };
        let radius = |round: ElementId| -> Result<f64, CadError> {
            match self.element(round) {
                Some(Element::Circle { radius, .. }) => Ok(radius.get() as f64),
                Some(Element::Arc { center, start, .. }) => Ok(distance(at(*center)?, at(*start)?)),
                _ => Err(bad_element(format!("{round} is not a circle or an arc"))),
            }
        };
        let length = |nm: f64| -> Result<Dim<Length>, CadError> {
            Ok(Dim::fixed(Length::new(nm.round() as i64)?))
        };
        let angle = |radians: f64| -> Result<Dim<Angle>, CadError> {
            let half_turn: i64 = 180_000_000_000;
            let nanodegrees = (radians.to_degrees() * 1e9).round() as i64;
            Ok(Dim::fixed(Angle::new(nanodegrees.rem_euclid(half_turn))?))
        };
        Ok(match entry.constraint {
            Constraint::Distance { a, b, .. } => Constraint::Distance {
                a,
                b,
                dim: length(distance(at(a)?, at(b)?))?,
            },
            Constraint::DistanceX { a, b, .. } => Constraint::DistanceX {
                a,
                b,
                dim: length(at(b)?.0 - at(a)?.0)?,
            },
            Constraint::DistanceY { a, b, .. } => Constraint::DistanceY {
                a,
                b,
                dim: length(at(b)?.1 - at(a)?.1)?,
            },
            Constraint::LineLength { line, .. } => {
                let (a, b) = ends(line)?;
                Constraint::LineLength {
                    line,
                    dim: length(distance(a, b))?,
                }
            }
            Constraint::Radius { round, .. } => Constraint::Radius {
                round,
                dim: length(radius(round)?)?,
            },
            Constraint::Diameter { round, .. } => Constraint::Diameter {
                round,
                dim: length(2.0 * radius(round)?)?,
            },
            Constraint::PointLineDistance { point, line, .. } => {
                let (a, b) = ends(line)?;
                let p = at(point)?;
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let cross = dx * (p.1 - a.1) - dy * (p.0 - a.0);
                Constraint::PointLineDistance {
                    point,
                    line,
                    dim: length(cross.abs() / dx.hypot(dy).max(1.0))?,
                }
            }
            Constraint::AngleBetween { a, b, .. } => {
                let ((p1, q1), (p2, q2)) = (ends(a)?, ends(b)?);
                let (d1, d2) = ((q1.0 - p1.0, q1.1 - p1.1), (q2.0 - p2.0, q2.1 - p2.1));
                let cross = d1.0 * d2.1 - d1.1 * d2.0;
                let dot = d1.0 * d2.0 + d1.1 * d2.1;
                Constraint::AngleBetween {
                    a,
                    b,
                    dim: angle(cross.atan2(dot))?,
                }
            }
            Constraint::LineAngle { line, .. } => {
                let (a, b) = ends(line)?;
                Constraint::LineAngle {
                    line,
                    dim: angle((b.1 - a.1).atan2(b.0 - a.0))?,
                }
            }
            _ => return Err(bad_constraint(format!("{id} is not a dimension"))),
        })
    }

    /// Makes a dimension a reference (it shows what the drawing measures and holds nothing) or
    /// makes it drive the drawing again. Either way it takes the value measured now and loses its
    /// expression, so nothing moves. A constraint that is not a dimension, and a distance of zero,
    /// are refused.
    pub fn set_reference(&mut self, id: ConstraintId, reference: bool) -> Result<(), CadError> {
        let measured = self.measured(id)?;
        if self
            .constraint(id)
            .is_some_and(|c| c.reference == reference)
        {
            return Ok(());
        }
        self.check_constraint(&measured)?;
        if let Some(entry) = self.constraints.iter_mut().find(|c| c.id == id) {
            entry.constraint = measured;
            entry.reference = reference;
        }
        Ok(())
    }

    /// Refuses to give a value to a dimension that is a reference.
    fn need_driving(&self, id: ConstraintId) -> Result<(), CadError> {
        if self.constraint(id).is_some_and(|c| c.reference) {
            Err(bad_constraint(format!(
                "{id} is a reference dimension: it is read, not set"
            )))
        } else {
            Ok(())
        }
    }

    /// Changes the value of a dimension that is a length (and drops its expression).
    pub fn set_length_dimension(
        &mut self,
        id: ConstraintId,
        value: Length,
    ) -> Result<(), CadError> {
        self.need_driving(id)?;
        let entry = self
            .constraints
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        let positive = !matches!(
            entry.constraint,
            Constraint::DistanceX { .. } | Constraint::DistanceY { .. }
        );
        match &mut entry.constraint {
            Constraint::Distance { dim, .. }
            | Constraint::DistanceX { dim, .. }
            | Constraint::DistanceY { dim, .. }
            | Constraint::LineLength { dim, .. }
            | Constraint::PointLineDistance { dim, .. }
            | Constraint::Radius { dim, .. }
            | Constraint::Diameter { dim, .. } => {
                if positive && value <= Length::ZERO {
                    return Err(bad_constraint("this dimension is positive"));
                }
                dim.value = value;
                dim.expr = None;
                Ok(())
            }
            _ => Err(bad_constraint(format!("{id} is not a dimension of length"))),
        }
    }

    /// Ties a dimension to an expression (a variable name, or a formula over variables and
    /// constants); the value changes when [`Sketch::resolve_dimensions`] is called.
    pub fn set_dimension_expr(
        &mut self,
        id: ConstraintId,
        expr: Option<&str>,
    ) -> Result<(), CadError> {
        if let Some(text) = expr {
            check_expr_text(text)?;
        }
        self.need_driving(id)?;
        let entry = self
            .constraints
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        let slot = match &mut entry.constraint {
            Constraint::Distance { dim, .. }
            | Constraint::DistanceX { dim, .. }
            | Constraint::DistanceY { dim, .. }
            | Constraint::LineLength { dim, .. }
            | Constraint::PointLineDistance { dim, .. }
            | Constraint::Radius { dim, .. }
            | Constraint::Diameter { dim, .. } => &mut dim.expr,
            Constraint::AngleBetween { dim, .. } | Constraint::LineAngle { dim, .. } => {
                &mut dim.expr
            }
            _ => return Err(bad_constraint(format!("{id} is not a dimension"))),
        };
        *slot = expr.map(str::to_string);
        Ok(())
    }

    /// Calculates the expression of every dimension that has one, with the variables of the
    /// project and the constants of the settings, and sets the values. All or nothing: when one
    /// expression fails nothing changes and the error says which dimension.
    pub fn resolve_dimensions(
        &mut self,
        variables: &Variables,
        settings: &Settings,
    ) -> Result<(), CadError> {
        struct Env<'a> {
            variables: &'a Variables,
            settings: &'a Settings,
        }
        impl Names for Env<'_> {
            fn lookup(&self, name: &str) -> Option<Value> {
                self.variables
                    .get(name)
                    .map(Value::from_length)
                    .or_else(|| self.settings.constants.get(name))
            }
        }
        let env = Env {
            variables,
            settings,
        };
        let limits = &settings.expr;
        enum New {
            Length(Length),
            Angle(Angle),
        }
        let mut changes: Vec<(usize, New)> = Vec::new();
        for (i, entry) in self.constraints.iter().enumerate() {
            let (text, wants_angle) = match &entry.constraint {
                Constraint::Distance { dim, .. }
                | Constraint::DistanceX { dim, .. }
                | Constraint::DistanceY { dim, .. }
                | Constraint::LineLength { dim, .. }
                | Constraint::PointLineDistance { dim, .. }
                | Constraint::Radius { dim, .. }
                | Constraint::Diameter { dim, .. } => (dim.expr.as_deref(), false),
                Constraint::AngleBetween { dim, .. } | Constraint::LineAngle { dim, .. } => {
                    (dim.expr.as_deref(), true)
                }
                _ => (None, false),
            };
            let Some(text) = text else { continue };
            let wrap = |e: CadError| bad_constraint(format!("{}: {e}", entry.id));
            let value = parse(text, limits)
                .and_then(|expr| expr.eval(&env, limits))
                .map_err(wrap)?;
            let new = if wants_angle {
                match value {
                    Value::Angle(r) => {
                        let nd = div_round_half_away(r.numer(), r.denom())
                            .map_err(|e| bad_constraint(format!("{}: {e}", entry.id)))?;
                        let nd = i64::try_from(nd).map_err(|_| {
                            bad_constraint(format!("{}: angle too large", entry.id))
                        })?;
                        New::Angle(
                            Angle::new(nd)
                                .map_err(|e| bad_constraint(format!("{}: {e}", entry.id)))?,
                        )
                    }
                    other => {
                        return Err(bad_constraint(format!(
                            "{}: an angle was expected but the result is a {}",
                            entry.id,
                            other.kind_name()
                        )))
                    }
                }
            } else {
                New::Length(
                    value
                        .to_length(0)
                        .map_err(|e| bad_constraint(format!("{}: {e}", entry.id)))?,
                )
            };
            changes.push((i, new));
        }
        // check the signs before anything is written
        for (i, new) in &changes {
            if let (New::Length(v), c) = (new, &self.constraints[*i].constraint) {
                let signed = matches!(
                    c,
                    Constraint::DistanceX { .. } | Constraint::DistanceY { .. }
                );
                if !signed && *v <= Length::ZERO {
                    return Err(bad_constraint(format!(
                        "{}: this dimension is positive",
                        self.constraints[*i].id
                    )));
                }
            }
        }
        for (i, new) in changes {
            match (&mut self.constraints[i].constraint, new) {
                (
                    Constraint::Distance { dim, .. }
                    | Constraint::DistanceX { dim, .. }
                    | Constraint::DistanceY { dim, .. }
                    | Constraint::LineLength { dim, .. }
                    | Constraint::PointLineDistance { dim, .. }
                    | Constraint::Radius { dim, .. }
                    | Constraint::Diameter { dim, .. },
                    New::Length(v),
                ) => dim.value = v,
                (
                    Constraint::AngleBetween { dim, .. } | Constraint::LineAngle { dim, .. },
                    New::Angle(v),
                ) => dim.value = v,
                _ => {}
            }
        }
        Ok(())
    }

    /// The identifiers of the constraints that are about an element.
    pub fn constraints_of(&self, id: ElementId) -> Vec<ConstraintId> {
        self.constraints
            .iter()
            .filter(|c| c.constraint.elements().contains(&id))
            .map(|c| c.id)
            .collect()
    }

    /// Checks the whole sketch: every element made of elements that exist and are of the right
    /// kind, every constraint about elements of the right kind, identifiers that are unique and
    /// below the counters. Used when a file is read.
    pub fn validate(&self) -> Result<(), CadError> {
        let mut seen = BTreeSet::new();
        for e in &self.elements {
            if !seen.insert(e.id) || e.id.0 >= self.next_element {
                return Err(bad_element(format!("{} is repeated or out of order", e.id)));
            }
        }
        let mut earlier = BTreeSet::new();
        for e in &self.elements {
            // a line or an arc is made of elements that come before it
            for part in e.element.parts() {
                if !earlier.contains(&part) {
                    return Err(bad_element(format!("{} needs {part} before it", e.id)));
                }
                if self.kind_of(part)? != Kind::Point {
                    return Err(bad_element(format!("{part} is not a point")));
                }
            }
            if let Element::Line { a, b } = e.element {
                if a == b {
                    return Err(bad_element("a line needs two different points"));
                }
            }
            if let Element::Circle { radius, .. } = e.element {
                if radius <= Length::ZERO {
                    return Err(bad_element("the radius of a circle is positive"));
                }
            }
            earlier.insert(e.id);
        }
        let mut seen_c = BTreeSet::new();
        for c in &self.constraints {
            if !seen_c.insert(c.id) || c.id.0 >= self.next_constraint {
                return Err(bad_constraint(format!(
                    "{} is repeated or out of order",
                    c.id
                )));
            }
            self.check_constraint(&c.constraint)?;
            if c.reference && (!c.constraint.is_dimension() || c.constraint.has_expression()) {
                return Err(bad_constraint(format!(
                    "{} cannot be a reference: only a dimension with no expression can",
                    c.id
                )));
            }
        }
        Ok(())
    }

    /// A sketch built from its parts, as the file gives them, and checked.
    pub fn from_parts(
        name: &str,
        plane: SketchPlane,
        elements: Vec<Entry>,
        constraints: Vec<ConstraintEntry>,
        next: (u32, u32),
    ) -> Result<Sketch, CadError> {
        check_sketch_name(name)?;
        let sketch = Sketch {
            name: name.to_string(),
            plane,
            elements,
            constraints,
            next_element: next.0,
            next_constraint: next.1,
        };
        sketch.validate()?;
        Ok(sketch)
    }

    pub(crate) fn set_point_exact(&mut self, id: ElementId, x: Length, y: Length) {
        if let Some(e) = self.elements.iter_mut().find(|e| e.id == id) {
            e.element = Element::Point { x, y };
        }
    }

    pub(crate) fn set_radius_exact(&mut self, id: ElementId, radius: Length) {
        if let Some(Entry {
            element: Element::Circle { radius: r, .. },
            ..
        }) = self.elements.iter_mut().find(|e| e.id == id)
        {
            *r = radius;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    pub(crate) fn sketch() -> Sketch {
        Sketch::new("s1", SketchPlane::on_face(Face::Top)).unwrap()
    }

    #[test]
    fn identifiers_are_counters_that_are_not_given_again() {
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(1), mm(0)).unwrap();
        assert_eq!((a, b), (ElementId(1), ElementId(2)));
        assert_eq!(a.to_string(), "e1");
        s.remove_element(b).unwrap();
        let c = s.add_point(mm(2), mm(0)).unwrap();
        assert_eq!(c, ElementId(3));
        let k = s
            .add_constraint(Constraint::HorizontalPoints(a, c))
            .unwrap();
        assert_eq!(k.to_string(), "c1");
        s.remove_constraint(k).unwrap();
        let k2 = s.add_constraint(Constraint::VerticalPoints(a, c)).unwrap();
        assert_eq!(k2, ConstraintId(2));
        assert_eq!(s.next_ids(), (4, 3));
    }

    #[test]
    fn elements_are_checked_when_they_are_added() {
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(5), mm(0)).unwrap();
        let line = s.add_line(a, b).unwrap();
        assert_eq!(s.add_line(a, a).unwrap_err().code(), "PHOENIX-CAD-031");
        assert_eq!(s.add_line(a, line).unwrap_err().code(), "PHOENIX-CAD-031");
        assert_eq!(
            s.add_line(a, ElementId(99)).unwrap_err(),
            CadError::UnknownName("e99".into())
        );
        assert!(s.add_circle(a, Length::ZERO).is_err());
        assert!(s.add_arc(a, a, b).is_err());
        let c = s.add_circle(a, mm(3)).unwrap();
        assert!(s.add_circle(c, mm(3)).is_err());
        // a point that a line is made of cannot be removed, nor one a constraint is about
        assert_eq!(s.remove_element(a), Err(CadError::InUse("e1".into())));
        s.remove_element(c).unwrap();
        s.add_constraint(Constraint::Horizontal(line)).unwrap();
        assert_eq!(s.remove_element(line), Err(CadError::InUse("e3".into())));
    }

    #[test]
    fn constraints_are_checked_for_the_kinds_they_tie() {
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(5), mm(0)).unwrap();
        let c = s.add_point(mm(5), mm(5)).unwrap();
        let l1 = s.add_line(a, b).unwrap();
        let l2 = s.add_line(b, c).unwrap();
        let circle = s.add_circle(a, mm(2)).unwrap();
        for bad in [
            Constraint::Coincident(a, a),
            Constraint::Coincident(a, l1),
            Constraint::Horizontal(a),
            Constraint::Parallel(l1, l1),
            Constraint::Tangent(l1, l2),
            Constraint::Tangent(a, circle),
            Constraint::Equal(l1, circle),
            Constraint::SymmetricPoint { a, b, center: a },
            Constraint::Midpoint {
                point: l1,
                line: l2,
            },
            Constraint::LineLength {
                line: l1,
                dim: Dim::fixed(Length::ZERO),
            },
            Constraint::Radius {
                round: l1,
                dim: Dim::fixed(mm(1)),
            },
        ] {
            let err = s.add_constraint(bad.clone()).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-032", "{bad:?}");
        }
        assert!(s.constraints().is_empty());
        for good in [
            Constraint::Coincident(a, c),
            Constraint::PointOnObject {
                point: c,
                object: circle,
            },
            Constraint::Tangent(l1, circle),
            Constraint::Equal(l1, l2),
            Constraint::DistanceX {
                a,
                b,
                dim: Dim::fixed(mm(-5)),
            },
        ] {
            s.add_constraint(good).unwrap();
        }
        assert_eq!(s.constraints().len(), 5);
    }

    #[test]
    fn a_rectangle_is_four_points_four_lines_and_four_constraints() {
        let mut s = sketch();
        let r = s.add_rectangle((mm(10), mm(8)), (mm(-10), mm(-8))).unwrap();
        assert_eq!(s.elements().len(), 8);
        assert_eq!(s.constraints().len(), 4);
        // the corners are put in order whatever the order of the two given
        assert_eq!(s.point(r.points[0]).unwrap(), (mm(-10), mm(-8)));
        assert_eq!(s.point(r.points[2]).unwrap(), (mm(10), mm(8)));
        assert!(s.add_rectangle((mm(0), mm(0)), (mm(0), mm(5))).is_err());
        assert_eq!(s.elements().len(), 8);
    }

    #[test]
    fn a_polyline_shares_its_points_and_a_failed_one_leaves_nothing() {
        let mut s = sketch();
        let lines = s
            .add_polyline(&[(mm(0), mm(0)), (mm(4), mm(0)), (mm(4), mm(3))], true)
            .unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(s.elements().len(), 6);
        let before = s.clone();
        assert!(s.add_polyline(&[(mm(0), mm(0))], false).is_err());
        assert!(s
            .add_polyline(&[(mm(0), mm(0)), (mm(1), mm(1))], true)
            .is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn the_plane_places_a_point_of_the_sketch_in_the_model() {
        use crate::enclosure::EnclosureParams;
        let e = Enclosure::new(EnclosureParams {
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
        // the outer box is 104 x 64 x 35; the top face has its origin at (0, 0, 35)
        let plane = SketchPlane {
            face: Face::Top,
            u0: mm(10),
            v0: mm(20),
            offset: mm(0),
        };
        let p = plane.to_model(&e, mm(5), mm(7)).unwrap();
        assert_eq!(p, [mm(15), mm(27), mm(35)]);
        assert_eq!(plane.from_model(&e, p).unwrap(), (mm(5), mm(7)));
        // a point off the plane is refused
        assert!(plane.from_model(&e, [mm(15), mm(27), mm(30)]).is_err());
        // the front face has its second axis going up
        let front = SketchPlane::on_face(Face::Front);
        assert_eq!(
            front.to_model(&e, mm(3), mm(4)).unwrap(),
            [mm(3), mm(0), mm(4)]
        );
        // the offset moves the plane outwards
        let lifted = SketchPlane {
            offset: mm(2),
            ..front
        };
        assert_eq!(
            lifted.to_model(&e, mm(3), mm(4)).unwrap(),
            [mm(3), mm(-2), mm(4)]
        );
    }

    #[test]
    fn a_reference_dimension_reads_the_drawing_and_holds_nothing() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(30), mm(40)).unwrap();
        let c = k.add_point(mm(0), mm(10)).unwrap();
        let ab = k.add_line(a, b).unwrap();
        let ac = k.add_line(a, c).unwrap();
        let circle = k.add_circle(c, mm(3)).unwrap();
        let distance = k
            .add_constraint(Constraint::Distance {
                a,
                b,
                dim: Dim::fixed(mm(7)),
            })
            .unwrap();
        let value = |k: &Sketch, id| match k.measured(id).unwrap() {
            Constraint::Distance { dim, .. }
            | Constraint::DistanceX { dim, .. }
            | Constraint::DistanceY { dim, .. }
            | Constraint::LineLength { dim, .. }
            | Constraint::Radius { dim, .. }
            | Constraint::Diameter { dim, .. }
            | Constraint::PointLineDistance { dim, .. } => dim.value.get(),
            Constraint::AngleBetween { dim, .. } | Constraint::LineAngle { dim, .. } => {
                dim.value.get()
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(value(&k, distance), 50_000_000);
        for (constraint, expected) in [
            (
                Constraint::DistanceX {
                    a: b,
                    b: a,
                    dim: Dim::fixed(mm(1)),
                },
                -30_000_000,
            ),
            (
                Constraint::DistanceY {
                    a,
                    b,
                    dim: Dim::fixed(mm(1)),
                },
                40_000_000,
            ),
            (
                Constraint::LineLength {
                    line: ac,
                    dim: Dim::fixed(mm(1)),
                },
                10_000_000,
            ),
            (
                Constraint::Radius {
                    round: circle,
                    dim: Dim::fixed(mm(1)),
                },
                3_000_000,
            ),
            (
                Constraint::Diameter {
                    round: circle,
                    dim: Dim::fixed(mm(1)),
                },
                6_000_000,
            ),
            (
                Constraint::PointLineDistance {
                    point: b,
                    line: ac,
                    dim: Dim::fixed(mm(1)),
                },
                30_000_000,
            ),
            (
                Constraint::LineAngle {
                    line: ac,
                    dim: Dim::fixed(Angle::new(0).unwrap()),
                },
                90_000_000_000,
            ),
        ] {
            let id = k.add_constraint(constraint).unwrap();
            assert_eq!(value(&k, id), expected, "{id}");
            k.remove_constraint(id).unwrap();
        }
        let angle = k
            .add_constraint(Constraint::AngleBetween {
                a: ab,
                b: ac,
                dim: Dim::fixed(Angle::new(1).unwrap()),
            })
            .unwrap();
        // the line a-b is at 53.13 degrees and the line a-c at 90
        let between = value(&k, angle);
        assert!((between - 36_869_897_646).abs() < 5, "{between}");
        k.remove_constraint(angle).unwrap();

        // a reference takes the value of the drawing, loses the formula and holds nothing
        k.set_dimension_expr(distance, Some("width")).unwrap();
        k.set_reference(distance, true).unwrap();
        let entry = k.constraint(distance).unwrap().clone();
        assert!(entry.reference && entry.enabled && !entry.drives());
        assert!(!entry.constraint.has_expression());
        assert_eq!(value(&k, distance), 50_000_000);
        assert!(k.set_length_dimension(distance, mm(9)).is_err());
        assert!(k.set_dimension_expr(distance, Some("width")).is_err());
        k.move_point(b, mm(60), mm(80)).unwrap();
        assert_eq!(value(&k, distance), 100_000_000);
        assert!(solved(&mut k));
        assert_eq!(k.point(b).unwrap(), (mm(60), mm(80)));
        // driving again, it holds the value of now
        k.set_reference(distance, false).unwrap();
        let entry = k.constraint(distance).unwrap();
        assert!(!entry.reference);
        assert!(matches!(
            &entry.constraint,
            Constraint::Distance { dim, .. } if dim.value == mm(100)
        ));
        k.validate().unwrap();

        // only a dimension can be a reference, and a distance of nothing cannot
        let level = k.add_constraint(Constraint::Horizontal(ac)).unwrap();
        assert!(k.set_reference(level, true).is_err());
        assert!(k.set_reference(ConstraintId(99), true).is_err());
        k.remove_constraint(level).unwrap();
        k.move_point(b, mm(0), mm(0)).unwrap();
        assert!(k.set_reference(distance, true).is_err());
        assert!(!k.constraint(distance).unwrap().reference);
    }

    #[test]
    fn names_of_sketches_are_plain_words() {
        for ok in ["s1", "front-panel", "A_b"] {
            assert!(check_sketch_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a b", "x/y", &"a".repeat(65)] {
            assert_eq!(
                check_sketch_name(bad).unwrap_err().code(),
                "PHOENIX-CAD-017"
            );
        }
    }

    #[test]
    fn dimensions_follow_the_variables_all_or_nothing() {
        let settings = Settings::defaults();
        let mut vars = Variables::new();
        vars.set("width", mm(80)).unwrap();
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(30), mm(0)).unwrap();
        let k = s
            .add_constraint(Constraint::DistanceX {
                a,
                b,
                dim: Dim::with_expr(mm(1), "width / 2 + 2 mm"),
            })
            .unwrap();
        let ang_a = s.add_point(mm(0), mm(5)).unwrap();
        let ang_b = s.add_point(mm(9), mm(5)).unwrap();
        let l1 = s.add_line(a, b).unwrap();
        let l2 = s.add_line(ang_a, ang_b).unwrap();
        let ka = s
            .add_constraint(Constraint::AngleBetween {
                a: l1,
                b: l2,
                dim: Dim::with_expr(Angle::ZERO, "30 deg + 15 deg"),
            })
            .unwrap();
        s.resolve_dimensions(&vars, &settings).unwrap();
        match &s.constraint(k).unwrap().constraint {
            Constraint::DistanceX { dim, .. } => assert_eq!(dim.value, mm(42)),
            other => panic!("{other:?}"),
        }
        match &s.constraint(ka).unwrap().constraint {
            Constraint::AngleBetween { dim, .. } => {
                assert_eq!(dim.value, Angle::from_deg(45).unwrap());
            }
            other => panic!("{other:?}"),
        }
        // an unknown variable: nothing changes and the error names the dimension
        vars.remove("width").unwrap();
        let before = s.clone();
        let err = s.resolve_dimensions(&vars, &settings).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-032");
        assert!(err.to_string().contains("c1"), "{err}");
        assert_eq!(s, before);
        // a length where an angle is wanted, and a negative length
        s.set_dimension_expr(ka, Some("3 mm")).unwrap();
        vars.set("width", mm(80)).unwrap();
        assert!(s.resolve_dimensions(&vars, &settings).is_err());
        s.set_dimension_expr(ka, None).unwrap();
        s.set_dimension_expr(k, Some("0 mm - 3 mm")).unwrap();
        s.resolve_dimensions(&vars, &settings).unwrap();
        s.set_dimension_expr(k, Some("\n")).unwrap_err();
    }

    #[test]
    fn the_collinear_and_the_line_dimensions_are_checked_and_follow_their_expressions() {
        let settings = Settings::defaults();
        let vars = Variables::new();
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(30), mm(0)).unwrap();
        let c = s.add_point(mm(40), mm(1)).unwrap();
        let d = s.add_point(mm(60), mm(2)).unwrap();
        let l1 = s.add_line(a, b).unwrap();
        let l2 = s.add_line(c, d).unwrap();
        // collinear ties two different lines
        s.add_constraint(Constraint::Collinear(l1, l2)).unwrap();
        assert!(s.add_constraint(Constraint::Collinear(l1, l1)).is_err());
        assert!(s.add_constraint(Constraint::Collinear(l1, a)).is_err());
        // a point and a line, and a distance that is positive
        let ok = Constraint::PointLineDistance {
            point: c,
            line: l1,
            dim: Dim::fixed(mm(4)),
        };
        let k = s.add_constraint(ok).unwrap();
        for bad in [
            Constraint::PointLineDistance {
                point: l2,
                line: l1,
                dim: Dim::fixed(mm(4)),
            },
            Constraint::PointLineDistance {
                point: c,
                line: d,
                dim: Dim::fixed(mm(4)),
            },
            Constraint::PointLineDistance {
                point: c,
                line: l1,
                dim: Dim::fixed(mm(0)),
            },
            Constraint::LineAngle {
                line: a,
                dim: Dim::fixed(Angle::ZERO),
            },
        ] {
            assert!(s.add_constraint(bad).is_err());
        }
        s.set_length_dimension(k, mm(7)).unwrap();
        assert!(s.set_length_dimension(k, mm(0)).is_err());
        let ka = s
            .add_constraint(Constraint::LineAngle {
                line: l2,
                dim: Dim::with_expr(Angle::ZERO, "30 deg + 15 deg"),
            })
            .unwrap();
        s.set_dimension_expr(k, Some("3 mm + 4 mm")).unwrap();
        s.resolve_dimensions(&vars, &settings).unwrap();
        match &s.constraint(k).unwrap().constraint {
            Constraint::PointLineDistance { dim, .. } => assert_eq!(dim.value, mm(7)),
            other => panic!("{other:?}"),
        }
        match &s.constraint(ka).unwrap().constraint {
            Constraint::LineAngle { dim, .. } => {
                assert_eq!(dim.value, Angle::from_deg(45).unwrap());
            }
            other => panic!("{other:?}"),
        }
        // a length where an angle is wanted
        s.set_dimension_expr(ka, Some("3 mm")).unwrap();
        assert!(s.resolve_dimensions(&vars, &settings).is_err());
        // names, elements and the kinds of dimension
        let kinds: Vec<(&str, bool, usize)> = s
            .constraints()
            .iter()
            .map(|e| {
                (
                    e.constraint.kind_name(),
                    e.constraint.is_dimension(),
                    e.constraint.elements().len(),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("collinear", false, 2),
                ("point_line_distance", true, 2),
                ("line_angle", true, 1)
            ]
        );
    }

    #[test]
    fn a_right_angle_is_rounded_with_an_arc_that_touches_both_lines() {
        let mut s = sketch();
        let p = s.add_point(mm(0), mm(0)).unwrap();
        let pa = s.add_point(mm(10), mm(0)).unwrap();
        let pb = s.add_point(mm(0), mm(10)).unwrap();
        let a = s.add_line(p, pa).unwrap();
        let b = s.add_line(p, pb).unwrap();
        let corner = s.round_corner(a, b, mm(2)).unwrap();
        // the corner point is gone, and the lines end where the arc touches them
        assert!(s.element(p).is_none());
        let ends = |s: &Sketch, line: ElementId| match s.element(line) {
            Some(Element::Line { a, b }) => (s.point(*a).unwrap(), s.point(*b).unwrap()),
            other => panic!("{other:?}"),
        };
        assert_eq!(ends(&s, a), ((mm(2), mm(0)), (mm(10), mm(0))));
        assert_eq!(ends(&s, b), ((mm(0), mm(2)), (mm(0), mm(10))));
        // the arc has its centre 2 mm from both lines and runs counter-clockwise from the touch
        // point on the vertical line to the touch point on the horizontal one
        let Some(Element::Arc { center, start, end }) = s.element(corner.arc).copied() else {
            panic!("not an arc");
        };
        assert_eq!(s.point(center).unwrap(), (mm(2), mm(2)));
        assert_eq!(s.point(start).unwrap(), (mm(0), mm(2)));
        assert_eq!(s.point(end).unwrap(), (mm(2), mm(0)));
        // the radius is kept with the arc as a dimension that can be edited
        assert_eq!(
            s.constraint(corner.radius).unwrap().constraint,
            Constraint::Radius {
                round: corner.arc,
                dim: Dim::fixed(mm(2))
            }
        );
        s.validate().unwrap();
        // and the solver agrees: nothing moves and the sketch is solved
        let before = s.clone();
        let outcome = s
            .solve(&crate::sketch::settings::SketchSettings::defaults())
            .unwrap();
        assert!(
            matches!(outcome, crate::sketch::solver::Outcome::Solved { .. }),
            "{outcome:?}"
        );
        assert_eq!(s.point(center).unwrap(), before.point(center).unwrap());
    }

    #[test]
    fn an_acute_and_an_obtuse_corner_are_rounded_with_the_arc_inside_them() {
        for (x, y) in [
            (10, 10),
            (-10, 10),
            (10, -10),
            (-10, -10),
            (40, 6),
            (-6, 40),
        ] {
            let mut s = sketch();
            let p = s.add_point(mm(0), mm(0)).unwrap();
            let pa = s.add_point(mm(40), mm(0)).unwrap();
            let pb = s.add_point(mm(x), mm(y)).unwrap();
            let a = s.add_line(pa, p).unwrap();
            let b = s.add_line(p, pb).unwrap();
            let corner = s.round_corner(a, b, mm(1)).unwrap();
            let Some(Element::Arc { center, start, end }) = s.element(corner.arc).copied() else {
                panic!("not an arc");
            };
            let dist = |p: ElementId, q: ElementId| {
                let (a, b) = (s.point(p).unwrap(), s.point(q).unwrap());
                let (dx, dy) = (
                    (a.0.get() - b.0.get()) as f64,
                    (a.1.get() - b.1.get()) as f64,
                );
                dx.hypot(dy)
            };
            // the radius is the 1 mm that was asked for, to the rounding of a nanometre
            assert!((dist(center, start) - 1e6).abs() < 2.0, "{x},{y}");
            assert!((dist(center, end) - 1e6).abs() < 2.0, "{x},{y}");
            // the sketch still holds together and the solver takes it as it is
            s.validate().unwrap();
            let outcome = s
                .solve(&crate::sketch::settings::SketchSettings::defaults())
                .unwrap();
            assert!(
                matches!(outcome, crate::sketch::solver::Outcome::Solved { .. }),
                "{x},{y}: {outcome:?}"
            );
        }
    }

    #[test]
    fn a_corner_that_cannot_be_rounded_leaves_the_sketch_as_it_was() {
        let mut s = sketch();
        let p = s.add_point(mm(0), mm(0)).unwrap();
        let pa = s.add_point(mm(10), mm(0)).unwrap();
        let pb = s.add_point(mm(0), mm(10)).unwrap();
        let pc = s.add_point(mm(30), mm(30)).unwrap();
        let a = s.add_line(p, pa).unwrap();
        let b = s.add_line(p, pb).unwrap();
        let far = s.add_line(pa, pc).unwrap();
        let line_pa_pb = s.add_line(pa, pb).unwrap();
        let before = s.clone();
        // a radius that needs more of a line than it has, or none at all
        assert!(s.round_corner(a, b, mm(10)).is_err());
        assert!(s.round_corner(a, b, mm(0)).is_err());
        // lines that do not meet, the same line twice, something that is not a line
        assert!(s.round_corner(b, far, mm(1)).is_err());
        assert!(s.round_corner(a, a, mm(1)).is_err());
        assert!(s.round_corner(a, p, mm(1)).is_err());
        // lines that share two points
        let again = s.add_line(pb, pa).unwrap();
        let before2 = s.clone();
        assert!(s.round_corner(line_pa_pb, again, mm(1)).is_err());
        assert_eq!(s, before2);
        // and nothing was left behind by the refusals before
        assert_eq!(before.elements().len() + 1, s.elements().len());
    }

    #[test]
    fn the_corner_of_a_polyline_is_rounded_and_the_other_corners_are_kept() {
        let mut s = sketch();
        let lines = s
            .add_polyline(
                &[
                    (mm(0), mm(0)),
                    (mm(20), mm(0)),
                    (mm(20), mm(20)),
                    (mm(0), mm(20)),
                ],
                true,
            )
            .unwrap();
        let corner = s.round_corner(lines[0], lines[1], mm(3)).unwrap();
        // the rounded corner left a point less and an arc more; the rest of the square is whole
        assert_eq!(s.elements().len(), 4 + 4 - 1 + 3 + 1);
        s.validate().unwrap();
        // a second corner of the same square can be rounded too
        s.round_corner(lines[1], lines[2], mm(3)).unwrap();
        assert_eq!(
            s.elements()
                .iter()
                .filter(|e| e.element.kind() == Kind::Arc)
                .count(),
            2
        );
        let _ = corner;
    }

    #[test]
    fn a_constraint_about_the_corner_or_the_length_of_a_line_refuses_the_rounding() {
        let build = |extra: fn(&mut Sketch, ElementId, ElementId, ElementId, ElementId)| {
            let mut s = sketch();
            let p = s.add_point(mm(0), mm(0)).unwrap();
            let pa = s.add_point(mm(10), mm(0)).unwrap();
            let pb = s.add_point(mm(0), mm(10)).unwrap();
            let a = s.add_line(p, pa).unwrap();
            let b = s.add_line(p, pb).unwrap();
            extra(&mut s, p, pa, a, b);
            let r = s.round_corner(a, b, mm(1));
            (s, r)
        };
        // horizontal, vertical and perpendicular stay true when the lines are shortened
        let (_, ok) = build(|s, _, _, a, b| {
            s.add_constraint(Constraint::Horizontal(a)).unwrap();
            s.add_constraint(Constraint::Perpendicular(a, b)).unwrap();
        });
        assert!(ok.is_ok());
        // a constraint about the corner point does not
        let (_, no) = build(|s, p, _, _, _| {
            s.fix_point(p).unwrap();
        });
        assert!(no.is_err());
        // nor does one about the length of a line
        let (_, no) = build(|s, _, _, a, _| {
            s.add_constraint(Constraint::LineLength {
                line: a,
                dim: Dim::fixed(mm(10)),
            })
            .unwrap();
        });
        assert!(no.is_err());
    }

    fn solved(s: &mut Sketch) -> bool {
        matches!(
            s.solve(&crate::sketch::settings::SketchSettings::defaults()),
            Ok(crate::sketch::solver::Outcome::Solved { .. })
        )
    }

    #[test]
    fn a_line_and_a_circle_are_mirrored_across_a_vertical_axis() {
        let mut s = sketch();
        let o = s.add_point(mm(0), mm(0)).unwrap();
        let top = s.add_point(mm(0), mm(10)).unwrap();
        let axis = s.add_line(o, top).unwrap();
        let p = s.add_point(mm(3), mm(1)).unwrap();
        let q = s.add_point(mm(8), mm(6)).unwrap();
        let line = s.add_line(p, q).unwrap();
        let c = s.add_point(mm(5), mm(2)).unwrap();
        let circle = s.add_circle(c, mm(2)).unwrap();
        let made = s.mirror(&[line, circle], axis, false).unwrap();
        assert_eq!(made.len(), 2);
        let Some(Element::Line { a, b }) = s.element(made[0]).copied() else {
            panic!("not a line");
        };
        assert_eq!(s.point(a).unwrap(), (mm(-3), mm(1)));
        assert_eq!(s.point(b).unwrap(), (mm(-8), mm(6)));
        let Some(Element::Circle { center, radius }) = s.element(made[1]).copied() else {
            panic!("not a circle");
        };
        assert_eq!(s.point(center).unwrap(), (mm(-5), mm(2)));
        assert_eq!(radius, mm(2));
        // without the constraints nothing was tied
        assert!(s.constraints().is_empty());
        s.validate().unwrap();
    }

    #[test]
    fn a_sketch_is_renamed_and_a_circle_gets_another_radius_only_when_it_is_valid() {
        let mut s = sketch();
        s.set_name("lid_2-b").unwrap();
        assert_eq!(s.name(), "lid_2-b");
        for bad in [
            "",
            "has space",
            "a/b",
            "\u{e9}",
            &"x".repeat(MAX_NAME_BYTES + 1),
        ] {
            let err = s.set_name(bad).unwrap_err();
            assert_eq!(
                err.code(),
                CadError::InvalidName(String::new()).code(),
                "{bad:?}"
            );
            assert_eq!(
                s.name(),
                "lid_2-b",
                "a refused name leaves the sketch as it was"
            );
        }
        s.set_name(&"x".repeat(MAX_NAME_BYTES)).unwrap();
        let c = s.add_point(mm(0), mm(0)).unwrap();
        let circle = s.add_circle(c, mm(2)).unwrap();
        s.set_circle_radius(circle, mm(7)).unwrap();
        match s.element(circle) {
            Some(Element::Circle { radius, .. }) => assert_eq!(*radius, mm(7)),
            other => panic!("{other:?}"),
        }
        // zero and negative radii, a point and an unknown identifier are refused unchanged
        assert!(s.set_circle_radius(circle, Length::ZERO).is_err());
        assert!(s.set_circle_radius(circle, mm(-1)).is_err());
        assert!(s.set_circle_radius(c, mm(1)).is_err());
        assert_eq!(
            s.set_circle_radius(ElementId(99), mm(1))
                .unwrap_err()
                .code(),
            CadError::UnknownName(String::new()).code()
        );
        match s.element(circle) {
            Some(Element::Circle { radius, .. }) => assert_eq!(*radius, mm(7)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn collinear_points_and_equal_distances_are_checked_when_they_are_added() {
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(5), mm(0)).unwrap();
        let c = s.add_point(mm(5), mm(5)).unwrap();
        let d = s.add_point(mm(9), mm(9)).unwrap();
        let line = s.add_line(a, b).unwrap();
        for bad in [
            Constraint::CollinearPoints(a, a, c),
            Constraint::CollinearPoints(a, c, a),
            Constraint::CollinearPoints(c, b, b),
            Constraint::CollinearPoints(a, b, line),
            Constraint::EqualDistance { a, b: a, c, d },
            Constraint::EqualDistance { a, b, c, d: c },
            Constraint::EqualDistance { a, b, c: a, d: b },
            Constraint::EqualDistance { a, b, c: b, d: a },
            Constraint::EqualDistance { a, b, c, d: line },
        ] {
            let err = s.add_constraint(bad.clone()).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-032", "{bad:?}");
        }
        assert!(s.constraints().is_empty());
        let on = s
            .add_constraint(Constraint::CollinearPoints(a, b, d))
            .unwrap();
        let equal = s
            .add_constraint(Constraint::EqualDistance { a, b, c, d })
            .unwrap();
        let kinds: Vec<_> = s
            .constraints()
            .iter()
            .map(|c| {
                (
                    c.id,
                    c.constraint.kind_name(),
                    c.constraint.elements().len(),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            vec![(on, "collinear_points", 3), (equal, "equal_distance", 4)]
        );
        assert!(!s.constraints()[0].constraint.is_dimension());
        assert!(!s.constraints()[1].constraint.is_dimension());
    }

    #[test]
    fn equidistant_and_equal_angle_are_checked_when_they_are_added() {
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(10), mm(0)).unwrap();
        let c = s.add_point(mm(5), mm(5)).unwrap();
        let d = s.add_point(mm(0), mm(10)).unwrap();
        let l1 = s.add_line(a, b).unwrap();
        let l2 = s.add_line(a, c).unwrap();
        let l3 = s.add_line(b, c).unwrap();
        let l4 = s.add_line(c, d).unwrap();
        for bad in [
            Constraint::Equidistant { point: a, a, b },
            Constraint::Equidistant { point: b, a, b },
            Constraint::Equidistant { point: c, a: b, b },
            Constraint::Equidistant { point: c, a, b: l1 },
            Constraint::Equidistant { point: l1, a, b },
            Constraint::EqualAngle {
                a: l1,
                b: l1,
                c: l2,
                d: l3,
            },
            Constraint::EqualAngle {
                a: l1,
                b: l2,
                c: l3,
                d: l3,
            },
            Constraint::EqualAngle {
                a: l1,
                b: l2,
                c: l1,
                d: l2,
            },
            Constraint::EqualAngle {
                a: l1,
                b: l2,
                c: l3,
                d: a,
            },
            Constraint::EqualAngle {
                a: l1,
                b: l2,
                c: a,
                d: l4,
            },
        ] {
            let err = s.add_constraint(bad.clone()).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-032", "{bad:?}");
        }
        assert!(s.constraints().is_empty());
        let eq = s
            .add_constraint(Constraint::Equidistant { point: c, a, b })
            .unwrap();
        let ang = s
            .add_constraint(Constraint::EqualAngle {
                a: l1,
                b: l2,
                c: l3,
                d: l4,
            })
            .unwrap();
        // the same pair of lines in the other order is another angle, so it is allowed
        s.add_constraint(Constraint::EqualAngle {
            a: l2,
            b: l1,
            c: l1,
            d: l2,
        })
        .unwrap();
        let kinds: Vec<_> = s
            .constraints()
            .iter()
            .take(2)
            .map(|c| {
                (
                    c.id,
                    c.constraint.kind_name(),
                    c.constraint.elements().len(),
                    c.constraint.is_dimension(),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (eq, "equidistant", 3, false),
                (ang, "equal_angle", 4, false)
            ]
        );
    }

    #[test]
    fn proportional_and_bisector_are_checked_when_they_are_added() {
        let mut s = sketch();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(10), mm(0)).unwrap();
        let c = s.add_point(mm(0), mm(10)).unwrap();
        let l1 = s.add_line(a, b).unwrap();
        let l2 = s.add_line(a, c).unwrap();
        let l3 = s.add_line(b, c).unwrap();
        let o1 = s.add_circle(a, mm(3)).unwrap();
        let o2 = s.add_circle(b, mm(2)).unwrap();
        let ratio = |a: ElementId, b: ElementId, num: u32, den: u32| Constraint::Proportional {
            a,
            b,
            num,
            den,
        };
        for bad in [
            ratio(l1, l1, 2, 1),
            ratio(l1, l2, 0, 1),
            ratio(l1, l2, 1, 0),
            ratio(l1, l2, 1001, 1),
            ratio(l1, l2, 1, 1001),
            ratio(l1, o1, 2, 1),
            ratio(o1, l1, 2, 1),
            ratio(a, l1, 2, 1),
            ratio(l1, b, 2, 1),
            Constraint::Bisector {
                bisector: l1,
                a: l1,
                b: l2,
            },
            Constraint::Bisector {
                bisector: l1,
                a: l2,
                b: l1,
            },
            Constraint::Bisector {
                bisector: l3,
                a: l2,
                b: l2,
            },
            Constraint::Bisector {
                bisector: a,
                a: l2,
                b: l3,
            },
            Constraint::Bisector {
                bisector: l1,
                a: o1,
                b: l3,
            },
        ] {
            let err = s.add_constraint(bad.clone()).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-032", "{bad:?}");
        }
        assert!(s.constraints().is_empty());
        let p = s.add_constraint(ratio(l1, l2, 3, 2)).unwrap();
        let q = s.add_constraint(ratio(o1, o2, 1000, 1)).unwrap();
        let r = s
            .add_constraint(Constraint::Bisector {
                bisector: l3,
                a: l1,
                b: l2,
            })
            .unwrap();
        let kinds: Vec<_> = s
            .constraints()
            .iter()
            .map(|c| {
                (
                    c.id,
                    c.constraint.kind_name(),
                    c.constraint.elements(),
                    c.constraint.is_dimension(),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (p, "proportional", vec![l1, l2], false),
                (q, "proportional", vec![o1, o2], false),
                (r, "bisector", vec![l3, l1, l2], false),
            ]
        );
    }

    #[test]
    fn the_images_are_tied_with_symmetric_constraints_that_the_solver_keeps() {
        let mut s = sketch();
        let o = s.add_point(mm(0), mm(0)).unwrap();
        let q0 = s.add_point(mm(10), mm(10)).unwrap();
        // an axis at 45 degrees, held where it is
        let axis = s.add_line(o, q0).unwrap();
        s.fix_point(o).unwrap();
        s.fix_point(q0).unwrap();
        let p = s.add_point(mm(6), mm(0)).unwrap();
        let r = s.add_point(mm(9), mm(3)).unwrap();
        let line = s.add_line(p, r).unwrap();
        let c = s.add_point(mm(7), mm(1)).unwrap();
        let circle = s.add_circle(c, mm(1)).unwrap();
        let made = s.mirror(&[line, circle], axis, true).unwrap();
        // one symmetric constraint for each of the three points that moved, one equal radius
        let kinds: Vec<_> = s
            .constraints()
            .iter()
            .map(|k| k.constraint.kind_name())
            .collect();
        assert_eq!(
            kinds.iter().filter(|k| **k == "symmetric_line").count(),
            3,
            "{kinds:?}"
        );
        assert_eq!(kinds.iter().filter(|k| **k == "equal").count(), 1);
        let Some(Element::Line { a, b }) = s.element(made[0]).copied() else {
            panic!("not a line");
        };
        // the image of (6, 0) across y = x is (0, 6)
        assert_eq!(s.point(a).unwrap(), (mm(0), mm(6)));
        assert_eq!(s.point(b).unwrap(), (mm(3), mm(9)));
        s.validate().unwrap();
        assert!(solved(&mut s));
        // after the solver the images are still where they were
        assert_eq!(s.point(a).unwrap(), (mm(0), mm(6)));
        // an original that is held where it is moved drags its image with it
        s.move_point(p, mm(8), mm(0)).unwrap();
        s.fix_point(p).unwrap();
        assert!(solved(&mut s));
        let (x, y) = s.point(a).unwrap();
        let moved = (x.get(), y.get());
        assert!(
            moved.0.abs() < 1000 && (moved.1 - 8_000_000).abs() < 1000,
            "{moved:?}"
        );
    }

    #[test]
    fn an_arc_keeps_its_turn_and_the_points_on_the_axis_are_shared() {
        let mut s = sketch();
        let o = s.add_point(mm(0), mm(0)).unwrap();
        let top = s.add_point(mm(0), mm(10)).unwrap();
        let axis = s.add_line(o, top).unwrap();
        // a quarter of a circle about (0, 0) from (5, 0) to (0, 5): it ends on the axis, and its
        // image is the quarter from (0, 5) to (-5, 0), joined to it at the axis
        let c = o;
        let start = s.add_point(mm(5), mm(0)).unwrap();
        let end = s.add_point(mm(0), mm(5)).unwrap();
        let arc = s.add_arc(c, start, end).unwrap();
        let made = s.mirror(&[arc], axis, false).unwrap();
        let Some(Element::Arc {
            center,
            start: s1,
            end: e1,
        }) = s.element(made[0]).copied()
        else {
            panic!("not an arc");
        };
        assert_eq!((center, s1), (o, end));
        assert_eq!(s.point(e1).unwrap(), (mm(-5), mm(0)));
        // an arc about another centre
        let mut s = sketch();
        let o = s.add_point(mm(0), mm(0)).unwrap();
        let top = s.add_point(mm(0), mm(10)).unwrap();
        let axis = s.add_line(o, top).unwrap();
        let c = s.add_point(mm(4), mm(4)).unwrap();
        let start = s.add_point(mm(6), mm(4)).unwrap();
        let end = s.add_point(mm(4), mm(6)).unwrap();
        let arc = s.add_arc(c, start, end).unwrap();
        let made = s.mirror(&[arc], axis, false).unwrap();
        let Some(Element::Arc {
            center,
            start: s2,
            end: e2,
        }) = s.element(made[0]).copied()
        else {
            panic!("not an arc");
        };
        assert_eq!(s.point(center).unwrap(), (mm(-4), mm(4)));
        // the start of the image is the image of the end, so it still runs counter-clockwise
        assert_eq!(s.point(s2).unwrap(), (mm(-4), mm(6)));
        assert_eq!(s.point(e2).unwrap(), (mm(-6), mm(4)));
        // a line that ends on the axis is joined to its image there
        let on = s.add_point(mm(0), mm(7)).unwrap();
        let away = s.add_point(mm(5), mm(9)).unwrap();
        let line = s.add_line(on, away).unwrap();
        let made = s.mirror(&[line], axis, true).unwrap();
        let Some(Element::Line { a, .. }) = s.element(made[0]).copied() else {
            panic!("not a line");
        };
        assert_eq!(a, on);
        // only the far end got a symmetric constraint
        assert_eq!(
            s.constraints()
                .iter()
                .filter(|k| k.constraint.kind_name() == "symmetric_line")
                .count(),
            1
        );
    }

    #[test]
    fn a_mirror_that_is_refused_leaves_the_sketch_as_it_was() {
        let mut s = sketch();
        let o = s.add_point(mm(0), mm(0)).unwrap();
        let top = s.add_point(mm(0), mm(10)).unwrap();
        let axis = s.add_line(o, top).unwrap();
        let p = s.add_point(mm(3), mm(1)).unwrap();
        let q = s.add_point(mm(8), mm(6)).unwrap();
        let line = s.add_line(p, q).unwrap();
        let before = s.clone();
        assert!(s.mirror(&[], axis, true).is_err());
        assert!(s.mirror(&[line, axis], axis, true).is_err());
        assert!(s.mirror(&[line, line], axis, true).is_err());
        assert!(s.mirror(&[line], p, true).is_err());
        assert!(s.mirror(&[ElementId(99)], axis, true).is_err());
        // the axis along itself has no image, and an axis of no length cannot be used
        assert_eq!(s.mirror(&[o], axis, true).unwrap(), Vec::<ElementId>::new());
        assert_eq!(s, before);
    }
}
