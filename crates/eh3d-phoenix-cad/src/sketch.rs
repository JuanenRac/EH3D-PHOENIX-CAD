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
//! * [`settings`] holds the tuned values of the solver (a file outside the code) and [`file`] the
//!   text file of a sketch, kept next to the project file.
//!
//! The solver works on floating point numbers in millimetres and rounds the result to the
//! nanometre; it uses only the operations that every platform computes the same way (sums,
//! products, quotients and square roots), so the same sketch gives the same result bit for bit.

use std::collections::BTreeSet;
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
    /// Two points are mirror images across a line.
    SymmetricLine {
        /// The first point.
        a: ElementId,
        /// The second point.
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
            Constraint::SymmetricLine { .. } => "symmetric_line",
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
            | Constraint::Distance { a, b, .. }
            | Constraint::DistanceX { a, b, .. }
            | Constraint::DistanceY { a, b, .. }
            | Constraint::AngleBetween { a, b, .. } => vec![*a, *b],
            Constraint::PointOnObject { point, object } => vec![*point, *object],
            Constraint::Horizontal(l) | Constraint::Vertical(l) => vec![*l],
            Constraint::SymmetricLine { a, b, axis } => vec![*a, *b, *axis],
            Constraint::SymmetricPoint { a, b, center } => vec![*a, *b, *center],
            Constraint::Midpoint { point, line } => vec![*point, *line],
            Constraint::Fixed { point, .. } => vec![*point],
            Constraint::LineLength { line, .. } => vec![*line],
            Constraint::Radius { round, .. } | Constraint::Diameter { round, .. } => {
                vec![*round]
            }
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
            Constraint::SymmetricLine { a, b, axis } => {
                Self::check_distinct(*a, *b)?;
                self.need(*a, &[Point], "a point")?;
                self.need(*b, &[Point], "a point")?;
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
            Constraint::Radius { round, dim } | Constraint::Diameter { round, dim } => {
                self.need(*round, &[Circle, Arc], "a circle or an arc")?;
                self.check_dim_length(dim, true)?;
            }
        }
        if let Constraint::AngleBetween { dim, .. } = constraint {
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

    /// Changes the value of a dimension that is a length (and drops its expression).
    pub fn set_length_dimension(
        &mut self,
        id: ConstraintId,
        value: Length,
    ) -> Result<(), CadError> {
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
            | Constraint::Radius { dim, .. }
            | Constraint::Diameter { dim, .. } => &mut dim.expr,
            Constraint::AngleBetween { dim, .. } => &mut dim.expr,
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
                | Constraint::Radius { dim, .. }
                | Constraint::Diameter { dim, .. } => (dim.expr.as_deref(), false),
                Constraint::AngleBetween { dim, .. } => (dim.expr.as_deref(), true),
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
                    | Constraint::Radius { dim, .. }
                    | Constraint::Diameter { dim, .. },
                    New::Length(v),
                ) => dim.value = v,
                (Constraint::AngleBetween { dim, .. }, New::Angle(v)) => dim.value = v,
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
}
