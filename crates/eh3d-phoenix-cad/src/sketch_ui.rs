//! The state of the sketch window and everything it decides without a screen: the tools, how a
//! click is snapped to the grid and to what is already drawn, the selection of elements and
//! constraints, the constraints and dimensions that can be added to a selection, dragging with the
//! solver running at each move, the health of the sketch, the history of the changes and the file.
//!
//! The window (`gui`) only draws the list of primitives this state gives ([`SketchEditor::scene`])
//! and passes on what the person does (a click, a drag, the wheel, a key). Every change is a
//! transaction: the sketch is changed first and kept only when the model accepts it, so a refused
//! change leaves the sketch exactly as it was. A constraint that makes the sketch over-constrained
//! is accepted and kept: the state says which constraints repeat or contradict the others and they
//! can be switched off.
//!
//! The tuned values (the grid, the radius of a snap, the angle that counts as horizontal, the
//! limits of the zoom) live in the `[sketch_ui]` section of `phoenix-cad-sketch.default.cfg`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::config::{Config, Settings};
use crate::enclosure::Face;
use crate::history::History;
use crate::lang::{fill, Lang, Msg};
use crate::scad::mm_text;
use crate::sketch::file;
use crate::sketch::settings::{SketchSettings, DEFAULT_SKETCH_CONFIG};
use crate::sketch::solver::Outcome;
use crate::sketch::{Constraint, ConstraintId, Dim, Element, ElementId, Kind, Sketch, SketchPlane};
use crate::units::{div_round_half_away, Angle};
use crate::vars::Variables;
use crate::window::Status;
use crate::{CadError, Length};

/// A point of the screen, in pixels, with the origin at the upper left corner of the canvas.
pub type Px = (f64, f64);

/// A point of the sketch, in millimetres.
pub type Mm = (f64, f64);

const NM_PER_MM: f64 = 1_000_000.0;

// -------------------------------------------------------------------------------------------
// Settings
// -------------------------------------------------------------------------------------------

/// The settings of the sketch window with their types.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchUiSettings {
    /// The distance between the lines of the grid, in millimetres.
    pub grid: f64,
    /// Whether a click is snapped to the grid when the window opens.
    pub grid_snap: bool,
    /// The pixels from a point or a line within which a click snaps to it.
    pub snap_radius: f64,
    /// The pixels from an element within which a click picks it.
    pub pick_radius: f64,
    /// The degrees from horizontal or vertical within which a new line is made so.
    pub auto_tolerance: f64,
    /// Whether the constraints of a new line are proposed when the window opens.
    pub auto_constraints: bool,
    /// The shortest line, radius or side that is drawn, in millimetres.
    pub min_segment: f64,
    /// The pixels for each millimetre when the canvas opens.
    pub scale: f64,
    /// The smallest pixels for each millimetre.
    pub scale_min: f64,
    /// The largest pixels for each millimetre.
    pub scale_max: f64,
    /// The zoom for each unit of the mouse wheel (the exponent of the factor).
    pub wheel_speed: f64,
    /// The segments of a circle.
    pub arc_segments: u32,
    /// Every how many lines of the grid there is a heavier one.
    pub major_every: u32,
    /// The most changes kept to go back to.
    pub undo_depth: usize,
    /// Whether the solver runs at each move of a drag.
    pub live_drag: bool,
    /// The grid is not drawn when its lines would be closer than this many pixels.
    pub min_grid_px: f64,
    /// The pixels between a dimension and what it measures.
    pub label_offset: f64,
    /// The pixels left empty around the drawing when it is fitted to the canvas.
    pub margin: f64,
}

fn missing(key: &str) -> CadError {
    CadError::Config(format!("{key} is missing"))
}

fn real(c: &Config, key: &str, low: f64, high: f64) -> Result<f64, CadError> {
    let text = c.get(key).ok_or_else(|| missing(key))?;
    let value: f64 = text
        .parse()
        .map_err(|_| CadError::Config(format!("{key} is not a number")))?;
    if value.is_finite() && (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(CadError::Config(format!(
            "{key} must be from {low} to {high}"
        )))
    }
}

fn count(c: &Config, key: &str, low: u32, high: u32) -> Result<u32, CadError> {
    let value = c.number(key)?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(CadError::Config(format!(
            "{key} must be from {low} to {high}"
        )))
    }
}

impl SketchUiSettings {
    /// The defaults that ship with the crate.
    pub fn defaults() -> SketchUiSettings {
        let config = Config::parse(DEFAULT_SKETCH_CONFIG).expect("the shipped settings are valid");
        SketchUiSettings::from_config(&config).expect("the shipped settings are complete")
    }

    /// The defaults with the keys of a user file put over them. Keys of other parts are accepted
    /// and ignored here.
    pub fn with_user_config(text: &str) -> Result<SketchUiSettings, CadError> {
        let mut config = Config::parse(DEFAULT_SKETCH_CONFIG)?;
        config.overlay(&Config::parse(text)?);
        SketchUiSettings::from_config(&config)
    }

    /// Reads the typed settings from a complete configuration.
    pub fn from_config(c: &Config) -> Result<SketchUiSettings, CadError> {
        let s = SketchUiSettings {
            grid: real(c, "sketch_ui.grid", 0.001, 1000.0)?,
            grid_snap: count(c, "sketch_ui.grid_snap", 0, 1)? == 1,
            snap_radius: real(c, "sketch_ui.snap_radius", 1.0, 100.0)?,
            pick_radius: real(c, "sketch_ui.pick_radius", 1.0, 100.0)?,
            auto_tolerance: real(c, "sketch_ui.auto_tolerance", 0.0, 45.0)?,
            auto_constraints: count(c, "sketch_ui.auto_constraints", 0, 1)? == 1,
            min_segment: real(c, "sketch_ui.min_segment", 0.000_001, 100.0)?,
            scale: real(c, "sketch_ui.scale", 0.01, 1000.0)?,
            scale_min: real(c, "sketch_ui.scale_min", 0.001, 1000.0)?,
            scale_max: real(c, "sketch_ui.scale_max", 0.01, 100_000.0)?,
            wheel_speed: real(c, "sketch_ui.wheel_speed", 0.000_01, 1.0)?,
            arc_segments: count(c, "sketch_ui.arc_segments", 8, 720)?,
            major_every: count(c, "sketch_ui.major_every", 2, 100)?,
            undo_depth: count(c, "sketch_ui.undo_depth", 1, 10_000)? as usize,
            live_drag: count(c, "sketch_ui.live_drag", 0, 1)? == 1,
            min_grid_px: real(c, "sketch_ui.min_grid_px", 2.0, 200.0)?,
            label_offset: real(c, "sketch_ui.label_offset", 0.0, 200.0)?,
            margin: real(c, "sketch_ui.margin", 0.0, 400.0)?,
        };
        if s.scale_min >= s.scale_max || !(s.scale_min..=s.scale_max).contains(&s.scale) {
            return Err(CadError::Config(
                "sketch_ui.scale must lie between sketch_ui.scale_min and sketch_ui.scale_max"
                    .into(),
            ));
        }
        Ok(s)
    }
}

// -------------------------------------------------------------------------------------------
// The canvas
// -------------------------------------------------------------------------------------------

/// The window onto the sketch: where the centre of the canvas is, how many pixels a millimetre
/// is and how big the canvas is. The second axis of the sketch points up on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View2 {
    /// The point of the sketch at the centre of the canvas, in millimetres.
    pub center: Mm,
    /// Pixels for each millimetre.
    pub scale: f64,
    /// The size of the canvas in pixels.
    pub size: Px,
}

impl View2 {
    /// A view of a canvas of `size` pixels centred on the origin.
    pub fn new(scale: f64, size: Px) -> View2 {
        View2 {
            center: (0.0, 0.0),
            scale,
            size,
        }
    }

    /// The pixel of a point of the sketch.
    pub fn to_px(&self, p: Mm) -> Px {
        (
            self.size.0 / 2.0 + (p.0 - self.center.0) * self.scale,
            self.size.1 / 2.0 - (p.1 - self.center.1) * self.scale,
        )
    }

    /// The point of the sketch at a pixel.
    pub fn to_mm(&self, p: Px) -> Mm {
        (
            self.center.0 + (p.0 - self.size.0 / 2.0) / self.scale,
            self.center.1 - (p.1 - self.size.1 / 2.0) / self.scale,
        )
    }

    /// Multiplies the scale by `factor` (inside the limits) keeping the point under the pixel
    /// `at` where it is.
    pub fn zoom_at(&mut self, at: Px, factor: f64, low: f64, high: f64) {
        let fixed = self.to_mm(at);
        self.scale = (self.scale * factor).clamp(low, high);
        let now = self.to_mm(at);
        self.center = (
            self.center.0 + fixed.0 - now.0,
            self.center.1 + fixed.1 - now.1,
        );
    }

    /// Moves the drawing by `dx` and `dy` pixels.
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        self.center = (
            self.center.0 - dx / self.scale,
            self.center.1 + dy / self.scale,
        );
    }
}

// -------------------------------------------------------------------------------------------
// Tools, selection, snaps
// -------------------------------------------------------------------------------------------

/// What a click on the canvas does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Pick elements and drag them.
    Select,
    /// Draw points.
    Point,
    /// Draw a line from two clicks.
    Line,
    /// Draw lines that share their ends until it is finished.
    Polyline,
    /// Draw a rectangle from two opposite corners.
    Rectangle,
    /// Draw a circle from its centre and a point of it.
    Circle,
    /// Draw an arc from its centre, its start and its end.
    Arc,
}

impl Tool {
    /// Every tool, in the order of the toolbar.
    pub const ALL: [Tool; 7] = [
        Tool::Select,
        Tool::Point,
        Tool::Line,
        Tool::Polyline,
        Tool::Rectangle,
        Tool::Circle,
        Tool::Arc,
    ];

    /// The words that name the tool.
    pub fn msg(self) -> Msg {
        match self {
            Tool::Select => Msg::ToolSelect,
            Tool::Point => Msg::ToolPoint,
            Tool::Line => Msg::ToolLine,
            Tool::Polyline => Msg::ToolPolyline,
            Tool::Rectangle => Msg::ToolRectangle,
            Tool::Circle => Msg::ToolCircle,
            Tool::Arc => Msg::ToolArc,
        }
    }

    /// What to click, as a sentence.
    pub fn hint(self) -> Msg {
        match self {
            Tool::Select => Msg::HintSelect,
            Tool::Point => Msg::HintPoint,
            Tool::Line => Msg::HintLine,
            Tool::Polyline => Msg::HintPolyline,
            Tool::Rectangle => Msg::HintRectangle,
            Tool::Circle => Msg::HintCircle,
            Tool::Arc => Msg::HintArc,
        }
    }
}

/// What is selected: elements and constraints, in the order they were picked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picked {
    /// The elements.
    pub elements: Vec<ElementId>,
    /// The constraints.
    pub constraints: Vec<ConstraintId>,
}

impl Picked {
    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.constraints.is_empty()
    }
}

/// What a click is snapped to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapKind {
    /// Nothing: the place of the pointer.
    Free,
    /// A line of the grid.
    Grid,
    /// A point that exists.
    Point(ElementId),
    /// A line, a circle or an arc that exists.
    Object(ElementId),
}

/// Where a click lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// The place, in millimetres.
    pub at: Mm,
    /// What it was snapped to.
    pub kind: SnapKind,
}

/// A place a new element is tied to: a point that exists, or a new point that may lie on an
/// object that exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Existing(ElementId),
    New {
        x: Length,
        y: Length,
        on: Option<ElementId>,
    },
}

/// The geometric constraints that can be added to a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintTool {
    /// Two points are one.
    Coincident,
    /// A point lies on a line, a circle or an arc.
    PointOnObject,
    /// A line, or two points, are level.
    Horizontal,
    /// A line, or two points, are plumb.
    Vertical,
    /// Two lines are parallel.
    Parallel,
    /// Two lines are perpendicular.
    Perpendicular,
    /// A line and a circle or two circles touch.
    Tangent,
    /// Two lines or two circles are the same size.
    Equal,
    /// Two points are mirror images across a line or through a point.
    Symmetric,
    /// A point is the midpoint of a line.
    Midpoint,
    /// Points are held where they are.
    Fixed,
}

impl ConstraintTool {
    /// Every tool, in the order of the panel.
    pub const ALL: [ConstraintTool; 11] = [
        ConstraintTool::Coincident,
        ConstraintTool::PointOnObject,
        ConstraintTool::Horizontal,
        ConstraintTool::Vertical,
        ConstraintTool::Parallel,
        ConstraintTool::Perpendicular,
        ConstraintTool::Tangent,
        ConstraintTool::Equal,
        ConstraintTool::Symmetric,
        ConstraintTool::Midpoint,
        ConstraintTool::Fixed,
    ];

    /// The words that name the constraint.
    pub fn msg(self) -> Msg {
        match self {
            ConstraintTool::Coincident => Msg::CoCoincident,
            ConstraintTool::PointOnObject => Msg::CoPointOnObject,
            ConstraintTool::Horizontal => Msg::CoHorizontal,
            ConstraintTool::Vertical => Msg::CoVertical,
            ConstraintTool::Parallel => Msg::CoParallel,
            ConstraintTool::Perpendicular => Msg::CoPerpendicular,
            ConstraintTool::Tangent => Msg::CoTangent,
            ConstraintTool::Equal => Msg::CoEqual,
            ConstraintTool::Symmetric => Msg::CoSymmetric,
            ConstraintTool::Midpoint => Msg::CoMidpoint,
            ConstraintTool::Fixed => Msg::CoFixed,
        }
    }
}

/// The dimensions that can be added to a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimTool {
    /// The distance between two points.
    Distance,
    /// The horizontal distance between two points.
    DistanceX,
    /// The vertical distance between two points.
    DistanceY,
    /// The length of a line.
    Length,
    /// The radius of a circle or an arc.
    Radius,
    /// The diameter of a circle or an arc.
    Diameter,
    /// The angle between two lines.
    Angle,
}

impl DimTool {
    /// Every dimension, in the order of the panel.
    pub const ALL: [DimTool; 7] = [
        DimTool::Distance,
        DimTool::DistanceX,
        DimTool::DistanceY,
        DimTool::Length,
        DimTool::Radius,
        DimTool::Diameter,
        DimTool::Angle,
    ];

    /// The words that name the dimension.
    pub fn msg(self) -> Msg {
        match self {
            DimTool::Distance => Msg::DiDistance,
            DimTool::DistanceX => Msg::DiDistanceX,
            DimTool::DistanceY => Msg::DiDistanceY,
            DimTool::Length => Msg::DiLength,
            DimTool::Radius => Msg::DiRadius,
            DimTool::Diameter => Msg::DiDiameter,
            DimTool::Angle => Msg::DiAngle,
        }
    }
}

/// How sound the sketch is, as the colour of its indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Fully constrained.
    Green,
    /// Something is still free (or there is nothing yet).
    Amber,
    /// Over-constrained, or the solver found no solution.
    Red,
}

/// The state of the sketch after the solver ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// There is nothing drawn.
    Empty,
    /// Every degree of freedom is taken.
    Full,
    /// Some degrees of freedom are left.
    Under,
    /// Some constraints repeat or contradict the others.
    Over,
    /// The solver ran out of steps, or the sketch is too large to solve.
    Failed,
}

/// What the solver said about the sketch.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// How sound the sketch is.
    pub health: Health,
    /// The degrees of freedom left, when the sketch was solved.
    pub dof: Option<usize>,
    /// The constraints that repeat others.
    pub redundant: Vec<ConstraintId>,
    /// The constraints that contradict others.
    pub conflicting: Vec<ConstraintId>,
    /// The error, when the sketch could not be solved at all.
    pub error: Option<CadError>,
}

impl Report {
    fn of(sketch: &Sketch, outcome: Result<Outcome, CadError>) -> Report {
        let none = |health, dof| Report {
            health,
            dof,
            redundant: Vec::new(),
            conflicting: Vec::new(),
            error: None,
        };
        match outcome {
            Ok(Outcome::Solved { dof, .. }) => {
                if sketch.elements().is_empty() {
                    none(Health::Empty, Some(0))
                } else if dof == 0 {
                    none(Health::Full, Some(0))
                } else {
                    none(Health::Under, Some(dof))
                }
            }
            Ok(Outcome::OverConstrained {
                redundant,
                conflicting,
            }) => Report {
                health: Health::Over,
                dof: None,
                redundant,
                conflicting,
                error: None,
            },
            Ok(Outcome::NotConverged { .. }) => none(Health::Failed, None),
            Err(e) => Report {
                error: Some(e),
                ..none(Health::Failed, None)
            },
        }
    }

    /// The colour of the indicator.
    pub fn tone(&self) -> Tone {
        match self.health {
            Health::Full => Tone::Green,
            Health::Empty | Health::Under => Tone::Amber,
            Health::Over | Health::Failed => Tone::Red,
        }
    }

    /// Every constraint that adds nothing or contradicts, in order of their identifiers.
    pub fn problems(&self) -> Vec<ConstraintId> {
        let mut all: Vec<ConstraintId> = self
            .conflicting
            .iter()
            .chain(self.redundant.iter())
            .copied()
            .collect();
        all.sort();
        all.dedup();
        all
    }
}

/// How a constraint stands in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowMark {
    /// Nothing is wrong.
    Ok,
    /// It repeats other constraints.
    Redundant,
    /// It contradicts other constraints.
    Conflict,
}

/// A line of the list of constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintRow {
    /// The identifier.
    pub id: ConstraintId,
    /// The words that name its kind.
    pub label: Msg,
    /// The elements it is about, as their identifiers.
    pub elements: String,
    /// The value of a dimension, in millimetres or degrees.
    pub value: Option<String>,
    /// What to type to give the dimension this value: its expression, or the value itself.
    pub edit: Option<String>,
    /// Whether the solver takes it into account.
    pub enabled: bool,
    /// Whether it is selected.
    pub selected: bool,
    /// How it stands.
    pub mark: RowMark,
}

/// A line of the list of elements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementRow {
    /// The identifier.
    pub id: ElementId,
    /// The words that name its kind.
    pub label: Msg,
    /// Where it is, in millimetres.
    pub detail: String,
    /// Whether it is construction geometry.
    pub construction: bool,
    /// Whether it is selected.
    pub selected: bool,
}

// -------------------------------------------------------------------------------------------
// What is drawn
// -------------------------------------------------------------------------------------------

/// How a primitive is drawn; the window gives each a colour and a width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// A line of the grid.
    GridMinor,
    /// A heavier line of the grid.
    GridMajor,
    /// An axis.
    Axis,
    /// Geometry of the profile.
    Normal,
    /// Construction geometry.
    Construction,
    /// Selected.
    Selected,
    /// About the selected constraint.
    Related,
    /// What a constraint that contradicts others is about.
    Conflict,
    /// What a constraint that repeats others is about.
    Redundant,
    /// What is being drawn.
    Preview,
    /// A dimension.
    Dimension,
    /// The sign of a constraint.
    Marker,
}

/// A primitive of the drawing, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    /// A straight segment.
    Segment {
        /// One end.
        a: Px,
        /// The other end.
        b: Px,
        /// How it is drawn.
        stroke: Stroke,
    },
    /// A broken line (a circle or an arc).
    Polyline {
        /// The corners.
        points: Vec<Px>,
        /// How it is drawn.
        stroke: Stroke,
    },
    /// A point.
    Dot {
        /// Where it is.
        at: Px,
        /// How it is drawn.
        stroke: Stroke,
    },
    /// A ring around a place (what a click would snap to).
    Ring {
        /// The centre.
        at: Px,
        /// How it is drawn.
        stroke: Stroke,
    },
    /// A short text.
    Text {
        /// Where it is, its centre.
        at: Px,
        /// The text.
        text: String,
        /// How it is drawn.
        stroke: Stroke,
    },
}

// -------------------------------------------------------------------------------------------
// Geometry of the drawing
// -------------------------------------------------------------------------------------------

fn mm_of(l: Length) -> f64 {
    l.get() as f64 / NM_PER_MM
}

fn to_len(v: f64) -> Result<Length, CadError> {
    if !v.is_finite() {
        return Err(CadError::ValueOutOfRange("position in the sketch"));
    }
    Length::new((v * NM_PER_MM).round() as i64)
}

fn dist(a: Mm, b: Mm) -> f64 {
    let (dx, dy): (f64, f64) = (b.0 - a.0, b.1 - a.1);
    (dx * dx + dy * dy).sqrt()
}

/// The distance from a point to a segment and the nearest point of the segment.
fn segment_distance(p: Mm, a: Mm, b: Mm) -> (f64, Mm) {
    let (dx, dy): (f64, f64) = (b.0 - a.0, b.1 - a.1);
    let len2: f64 = dx * dx + dy * dy;
    let t: f64 = if len2 <= 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let nearest = (a.0 + t * dx, a.1 + t * dy);
    (dist(p, nearest), nearest)
}

/// The distance from a point to a circle and the nearest point of the circle.
fn circle_distance(p: Mm, center: Mm, radius: f64) -> Option<(f64, Mm)> {
    let d = dist(p, center);
    if d <= 0.0 {
        return None;
    }
    let k = radius / d;
    let nearest = (
        center.0 + (p.0 - center.0) * k,
        center.1 + (p.1 - center.1) * k,
    );
    Some(((d - radius).abs(), nearest))
}

fn direction(center: Mm, p: Mm) -> f64 {
    let (dx, dy): (f64, f64) = (p.0 - center.0, p.1 - center.1);
    dy.atan2(dx)
}

/// Whether the direction of `p` from the centre lies on the arc that runs counter-clockwise from
/// `start` to `end`.
fn on_arc(center: Mm, start: Mm, end: Mm, p: Mm) -> bool {
    let turn = std::f64::consts::TAU;
    let sweep = (direction(center, end) - direction(center, start)).rem_euclid(turn);
    let at = (direction(center, p) - direction(center, start)).rem_euclid(turn);
    at <= sweep
}

fn arc_points(center: Mm, start: Mm, end: Mm, segments: u32) -> Vec<Mm> {
    let turn = std::f64::consts::TAU;
    let radius = dist(center, start);
    let first = direction(center, start);
    let sweep = (direction(center, end) - first).rem_euclid(turn);
    let sweep = if sweep <= 0.0 { turn } else { sweep };
    let steps = ((segments as f64) * sweep / turn).ceil().max(2.0) as u32;
    (0..=steps)
        .map(|k| {
            let a = first + sweep * f64::from(k) / f64::from(steps);
            (center.0 + radius * a.cos(), center.1 + radius * a.sin())
        })
        .collect()
}

fn circle_points(center: Mm, radius: f64, segments: u32) -> Vec<Mm> {
    (0..=segments)
        .map(|k| {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(segments);
            (center.0 + radius * a.cos(), center.1 + radius * a.sin())
        })
        .collect()
}

/// The text of an angle in degrees with up to three decimals and no trailing zeros.
pub fn degrees_text(angle: Angle) -> String {
    let thousandths = div_round_half_away(i128::from(angle.get()), 1_000_000).unwrap_or(0);
    let sign = if thousandths < 0 { "-" } else { "" };
    let abs = thousandths.unsigned_abs();
    let (whole, frac) = (abs / 1000, abs % 1000);
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let digits = format!("{frac:03}");
        format!("{sign}{whole}.{}", digits.trim_end_matches('0'))
    }
}

fn constraint_msg(c: &Constraint) -> Msg {
    match c {
        Constraint::Coincident(..) => Msg::CoCoincident,
        Constraint::PointOnObject { .. } => Msg::CoPointOnObject,
        Constraint::Horizontal(..) | Constraint::HorizontalPoints(..) => Msg::CoHorizontal,
        Constraint::Vertical(..) | Constraint::VerticalPoints(..) => Msg::CoVertical,
        Constraint::Parallel(..) => Msg::CoParallel,
        Constraint::Perpendicular(..) => Msg::CoPerpendicular,
        Constraint::Tangent(..) => Msg::CoTangent,
        Constraint::Equal(..) => Msg::CoEqual,
        Constraint::SymmetricLine { .. } | Constraint::SymmetricPoint { .. } => Msg::CoSymmetric,
        Constraint::Midpoint { .. } => Msg::CoMidpoint,
        Constraint::Fixed { .. } => Msg::CoFixed,
        Constraint::Distance { .. } => Msg::DiDistance,
        Constraint::DistanceX { .. } => Msg::DiDistanceX,
        Constraint::DistanceY { .. } => Msg::DiDistanceY,
        Constraint::LineLength { .. } => Msg::DiLength,
        Constraint::Radius { .. } => Msg::DiRadius,
        Constraint::Diameter { .. } => Msg::DiDiameter,
        Constraint::AngleBetween { .. } => Msg::DiAngle,
    }
}

fn kind_msg(kind: Kind) -> Msg {
    match kind {
        Kind::Point => Msg::ElPoint,
        Kind::Line => Msg::ElLine,
        Kind::Circle => Msg::ElCircle,
        Kind::Arc => Msg::ElArc,
    }
}

/// The sign drawn beside a geometric constraint.
fn marker_of(c: &Constraint) -> Option<&'static str> {
    match c {
        Constraint::Horizontal(..) | Constraint::HorizontalPoints(..) => Some("H"),
        Constraint::Vertical(..) | Constraint::VerticalPoints(..) => Some("V"),
        Constraint::Parallel(..) => Some("//"),
        Constraint::Perpendicular(..) => Some("+"),
        Constraint::Tangent(..) => Some("T"),
        Constraint::Equal(..) => Some("="),
        Constraint::Coincident(..) => Some("C"),
        Constraint::PointOnObject { .. } => Some("O"),
        Constraint::SymmetricLine { .. } | Constraint::SymmetricPoint { .. } => Some("S"),
        Constraint::Midpoint { .. } => Some("M"),
        Constraint::Fixed { .. } => Some("F"),
        _ => None,
    }
}

/// What a closure that changes the sketch may use.
struct Ctx<'a> {
    variables: &'a Variables,
    library: &'a Settings,
    solver: &'a SketchSettings,
    ui: &'a SketchUiSettings,
}

/// A drag in progress.
#[derive(Debug, Clone)]
struct Drag {
    before: Sketch,
    targets: Vec<(ElementId, Mm)>,
    start: Mm,
    over: Option<ElementId>,
}

/// The sketch window: the sketch and everything the person sees of it.
#[derive(Debug, Clone)]
pub struct SketchEditor {
    /// The language of the texts.
    pub lang: Lang,
    /// The sketch.
    pub sketch: Sketch,
    /// What a click does.
    pub tool: Tool,
    /// Whether new lines, circles and arcs are construction geometry.
    pub construction_mode: bool,
    /// Whether a click is snapped to the grid.
    pub grid_snap: bool,
    /// Whether the constraints of a new line are proposed and added.
    pub auto_constraints: bool,
    /// What is selected.
    pub picked: Picked,
    /// The window onto the sketch.
    pub view: View2,
    /// What the status bar says.
    pub status: Status,
    /// The file the sketch was opened from or saved to.
    pub path: Option<PathBuf>,
    /// True when the sketch has changes that are not saved.
    pub dirty: bool,
    /// What the solver said about the sketch.
    pub report: Report,
    /// What a click would snap to, where the pointer is.
    pub hover: Option<Snap>,
    /// The text typed for the value of a dimension that is about to be added.
    pub dim_input: String,
    /// The settings of the library (units, the language of the formulas).
    pub library: Settings,
    /// The settings of the solver.
    pub solver: SketchSettings,
    /// The settings of the window.
    pub ui: SketchUiSettings,
    /// The variables of the project, which the dimensions may use.
    pub variables: Variables,
    history: History<Sketch>,
    pending: Vec<Anchor>,
    chain: Option<Sketch>,
    chain_first: Option<ElementId>,
    drag: Option<Drag>,
    cursor: Option<Mm>,
}

fn new_sketch() -> Sketch {
    Sketch::new("sketch", SketchPlane::on_face(Face::Top)).expect("the name is valid")
}

impl SketchEditor {
    /// A window with an empty sketch on the top face.
    pub fn new(
        lang: Lang,
        library: Settings,
        solver: SketchSettings,
        ui: SketchUiSettings,
    ) -> SketchEditor {
        let view = View2::new(ui.scale, (800.0, 600.0));
        let mut editor = SketchEditor {
            lang,
            sketch: new_sketch(),
            tool: Tool::Select,
            construction_mode: false,
            grid_snap: ui.grid_snap,
            auto_constraints: ui.auto_constraints,
            picked: Picked::default(),
            view,
            status: Status::Ready,
            path: None,
            dirty: false,
            report: Report {
                health: Health::Empty,
                dof: Some(0),
                redundant: Vec::new(),
                conflicting: Vec::new(),
                error: None,
            },
            hover: None,
            dim_input: String::new(),
            history: History::new(ui.undo_depth),
            library,
            solver,
            ui,
            variables: Variables::new(),
            pending: Vec::new(),
            chain: None,
            chain_first: None,
            drag: None,
            cursor: None,
        };
        editor.evaluate(false);
        editor
    }

    /// A window with the settings that ship with the crate.
    pub fn with_defaults(lang: Lang) -> SketchEditor {
        SketchEditor::new(
            lang,
            Settings::defaults(),
            SketchSettings::defaults(),
            SketchUiSettings::defaults(),
        )
    }

    /// Starts an empty sketch.
    pub fn new_sketch(&mut self) {
        self.sketch = new_sketch();
        self.reset_session();
        self.path = None;
        self.status = Status::Done(Msg::SketchNew, Vec::new());
        self.evaluate(false);
    }

    fn reset_session(&mut self) {
        self.history.clear();
        self.picked = Picked::default();
        self.pending.clear();
        self.chain = None;
        self.chain_first = None;
        self.drag = None;
        self.dirty = false;
        self.dim_input.clear();
    }

    // ---------------------------------------------------------------------------------------
    // The solver and the transactions
    // ---------------------------------------------------------------------------------------

    /// Runs the solver. With `adopt` the sketch is moved to the solution; without it the sketch
    /// is left alone and only the report is made.
    fn evaluate(&mut self, adopt: bool) {
        let outcome = if adopt {
            self.sketch.solve(&self.solver)
        } else {
            self.sketch.clone().solve(&self.solver)
        };
        self.report = Report::of(&self.sketch, outcome);
    }

    /// Changes the sketch as a transaction: when `f` fails the sketch is as it was and the status
    /// says why; when it succeeds the solver runs and the change is one step of the history (or
    /// part of the polyline being drawn).
    fn edit<T>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Sketch, &Ctx) -> Result<T, CadError>,
    ) -> Option<T> {
        let before = self.sketch.clone();
        let result = {
            let ctx = Ctx {
                variables: &self.variables,
                library: &self.library,
                solver: &self.solver,
                ui: &self.ui,
            };
            f(&mut self.sketch, &ctx)
        };
        match result {
            Ok(value) => {
                self.evaluate(true);
                if self.sketch != before {
                    self.dirty = true;
                    if self.chain.is_none() {
                        self.history.record(name, before);
                    }
                }
                Some(value)
            }
            Err(e) => {
                self.sketch = before;
                self.status = Status::Error(e);
                None
            }
        }
    }

    /// Gives the sketch the variables of the project; the dimensions that use them are
    /// calculated again.
    pub fn set_variables(&mut self, variables: &Variables) {
        if self.variables == *variables {
            return;
        }
        self.variables = variables.clone();
        let before = self.sketch.clone();
        let result = self
            .sketch
            .resolve_dimensions(&self.variables, &self.library);
        match result {
            Ok(()) => {
                self.evaluate(true);
                if self.sketch != before {
                    self.dirty = true;
                }
            }
            Err(e) => {
                self.sketch = before;
                self.status = Status::Error(e);
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Snapping and picking
    // ---------------------------------------------------------------------------------------

    fn position(&self, id: ElementId) -> Option<Mm> {
        match self.sketch.element(id) {
            Some(Element::Point { x, y }) => Some((mm_of(*x), mm_of(*y))),
            _ => None,
        }
    }

    /// The pixels of a point of the sketch.
    fn px(&self, id: ElementId) -> Option<Px> {
        self.position(id).map(|p| self.view.to_px(p))
    }

    fn anchor_position(&self, anchor: &Anchor) -> Option<Mm> {
        match anchor {
            Anchor::Existing(id) => self.position(*id),
            Anchor::New { x, y, .. } => Some((mm_of(*x), mm_of(*y))),
        }
    }

    /// The nearest object (line, circle or arc) to a point and the nearest place of it.
    fn nearest_object(&self, p: Mm, within: f64) -> Option<(ElementId, Mm)> {
        let mut best: Option<(f64, ElementId, Mm)> = None;
        for entry in self.sketch.elements() {
            let found = match entry.element {
                Element::Point { .. } => None,
                Element::Line { a, b } => match (self.position(a), self.position(b)) {
                    (Some(a), Some(b)) => Some(segment_distance(p, a, b)),
                    _ => None,
                },
                Element::Circle { center, radius } => self
                    .position(center)
                    .and_then(|c| circle_distance(p, c, mm_of(radius))),
                Element::Arc { center, start, end } => {
                    match (
                        self.position(center),
                        self.position(start),
                        self.position(end),
                    ) {
                        (Some(c), Some(s), Some(e)) if on_arc(c, s, e, p) => {
                            circle_distance(p, c, dist(c, s))
                        }
                        _ => None,
                    }
                }
            };
            if let Some((d, at)) = found {
                if d <= within && best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, entry.id, at));
                }
            }
        }
        best.map(|(_, id, at)| (id, at))
    }

    /// The nearest point to a place, other than the ones in `skip`.
    fn nearest_point(&self, p: Mm, within: f64, skip: &[ElementId]) -> Option<(ElementId, Mm)> {
        let mut best: Option<(f64, ElementId, Mm)> = None;
        for entry in self.sketch.elements() {
            if skip.contains(&entry.id) {
                continue;
            }
            if let Element::Point { x, y } = entry.element {
                let at = (mm_of(x), mm_of(y));
                let d = dist(p, at);
                if d <= within && best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, entry.id, at));
                }
            }
        }
        best.map(|(_, id, at)| (id, at))
    }

    /// Where a click at a pixel lands: on a point that exists, else on an object that exists,
    /// else on the grid (when it is on), else where the pointer is.
    pub fn snap_at(&self, px: Px) -> Snap {
        self.snap_skipping(px, &[])
    }

    fn snap_skipping(&self, px: Px, skip: &[ElementId]) -> Snap {
        let p = self.view.to_mm(px);
        let within = self.ui.snap_radius / self.view.scale;
        if let Some((id, at)) = self.nearest_point(p, within, skip) {
            return Snap {
                at,
                kind: SnapKind::Point(id),
            };
        }
        if let Some((id, at)) = self.nearest_object(p, within) {
            return Snap {
                at,
                kind: SnapKind::Object(id),
            };
        }
        if self.grid_snap {
            let g = self.ui.grid;
            return Snap {
                at: ((p.0 / g).round() * g, (p.1 / g).round() * g),
                kind: SnapKind::Grid,
            };
        }
        Snap {
            at: p,
            kind: SnapKind::Free,
        }
    }

    /// The element under a pixel: a point first, else the nearest line, circle or arc.
    pub fn hit(&self, px: Px) -> Option<ElementId> {
        let p = self.view.to_mm(px);
        let within = self.ui.pick_radius / self.view.scale;
        if let Some((id, _)) = self.nearest_point(p, within, &[]) {
            return Some(id);
        }
        self.nearest_object(p, within).map(|(id, _)| id)
    }

    fn anchor(&self, snap: &Snap) -> Result<Anchor, CadError> {
        match snap.kind {
            SnapKind::Point(id) => Ok(Anchor::Existing(id)),
            SnapKind::Object(id) => Ok(Anchor::New {
                x: to_len(snap.at.0)?,
                y: to_len(snap.at.1)?,
                on: Some(id),
            }),
            SnapKind::Grid | SnapKind::Free => Ok(Anchor::New {
                x: to_len(snap.at.0)?,
                y: to_len(snap.at.1)?,
                on: None,
            }),
        }
    }

    // ---------------------------------------------------------------------------------------
    // The view
    // ---------------------------------------------------------------------------------------

    /// Tells the size of the canvas.
    pub fn set_canvas(&mut self, size: Px) {
        self.view.size = size;
    }

    /// Zooms with the wheel around a pixel.
    pub fn wheel(&mut self, at: Px, scroll: f64) {
        let factor = (scroll * self.ui.wheel_speed).exp();
        self.view
            .zoom_at(at, factor, self.ui.scale_min, self.ui.scale_max);
    }

    /// Moves the drawing by a drag of the canvas.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.view.pan_by(dx, dy);
    }

    /// Fits the whole drawing to the canvas (the view of an empty sketch is the one it opens
    /// with).
    pub fn fit(&mut self) {
        let mut low = (f64::MAX, f64::MAX);
        let mut high = (f64::MIN, f64::MIN);
        let mut any = false;
        let mut take = |p: Mm, r: f64| {
            low = (low.0.min(p.0 - r), low.1.min(p.1 - r));
            high = (high.0.max(p.0 + r), high.1.max(p.1 + r));
            any = true;
        };
        for entry in self.sketch.elements() {
            match entry.element {
                Element::Point { x, y } => take((mm_of(x), mm_of(y)), 0.0),
                Element::Circle { center, radius } => {
                    if let Some(c) = self.position(center) {
                        take(c, mm_of(radius));
                    }
                }
                _ => {}
            }
        }
        if !any {
            self.view.center = (0.0, 0.0);
            self.view.scale = self.ui.scale;
            return;
        }
        self.view.center = ((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0);
        let room = (
            (self.view.size.0 - 2.0 * self.ui.margin).max(1.0),
            (self.view.size.1 - 2.0 * self.ui.margin).max(1.0),
        );
        let (w, h) = ((high.0 - low.0).max(1e-9), (high.1 - low.1).max(1e-9));
        let scale = (room.0 / w).min(room.1 / h);
        self.view.scale = scale.clamp(self.ui.scale_min, self.ui.scale_max);
    }

    // ---------------------------------------------------------------------------------------
    // Tools
    // ---------------------------------------------------------------------------------------

    /// Chooses what a click does. A drawing that was started is dropped.
    pub fn set_tool(&mut self, tool: Tool) {
        self.cancel();
        self.tool = tool;
        self.status = Status::Done(tool.hint(), Vec::new());
    }

    /// Escape: drops what is being drawn or dragged, or else clears the selection.
    pub fn cancel(&mut self) {
        if let Some(drag) = self.drag.take() {
            self.sketch = drag.before;
            self.evaluate(false);
            return;
        }
        if !self.pending.is_empty() || self.chain.is_some() {
            self.pending.clear();
            self.finish_chain();
            return;
        }
        self.picked = Picked::default();
    }

    /// Enter, or a right click while drawing: ends a polyline.
    pub fn finish(&mut self) {
        self.pending.clear();
        self.finish_chain();
    }

    fn finish_chain(&mut self) {
        self.chain_first = None;
        if let Some(before) = self.chain.take() {
            if self.sketch != before {
                let name = Msg::ToolPolyline.tr(self.lang);
                self.history.record(name, before);
            }
        }
    }

    /// Whether a drawing is half done, and so what the window shows beside the pointer.
    pub fn is_drawing(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The pointer moved over the canvas: what a click would snap to, and the rubber band.
    pub fn pointer_move(&mut self, px: Px) {
        let skip: Vec<ElementId> = self
            .drag
            .as_ref()
            .map(|d| d.targets.iter().map(|t| t.0).collect())
            .unwrap_or_default();
        let snap = self.snap_skipping(px, &skip);
        self.cursor = Some(snap.at);
        self.hover = Some(snap);
    }

    /// The pointer left the canvas.
    pub fn pointer_leave(&mut self) {
        self.hover = None;
        self.cursor = None;
    }

    /// A click with a drawing tool.
    pub fn place(&mut self, px: Px) {
        if self.tool == Tool::Select {
            return;
        }
        let snap = self.snap_at(px);
        let anchor = match self.anchor(&snap) {
            Ok(a) => a,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        match self.tool {
            Tool::Select => {}
            Tool::Point => self.place_point(anchor),
            Tool::Line => self.place_line(anchor, false),
            Tool::Polyline => self.place_line(anchor, true),
            Tool::Rectangle => self.place_rectangle(anchor),
            Tool::Circle => self.place_circle(anchor),
            Tool::Arc => self.place_arc(anchor),
        }
    }

    fn drew(&mut self, tool: Tool) {
        self.status = Status::Done(Msg::ElementDrawn, vec![tool.msg().tr(self.lang).to_owned()]);
    }

    fn too_short(&mut self) {
        self.status = Status::Problem(Msg::TooShort, Vec::new());
    }

    fn place_point(&mut self, anchor: Anchor) {
        if matches!(anchor, Anchor::Existing(_)) {
            self.status = Status::Problem(Msg::AlreadyAPoint, Vec::new());
            return;
        }
        let name = Msg::ToolPoint.tr(self.lang);
        if self
            .edit(name, |s, _| materialize(s, &anchor).map(|_| ()))
            .is_some()
        {
            self.drew(Tool::Point);
        }
    }

    fn place_line(&mut self, anchor: Anchor, chain: bool) {
        let Some(start) = self.pending.first().copied() else {
            self.pending.push(anchor);
            return;
        };
        let (Some(a), Some(b)) = (self.anchor_position(&start), self.anchor_position(&anchor))
        else {
            return;
        };
        if dist(a, b) < self.ui.min_segment {
            self.too_short();
            return;
        }
        if chain && self.chain.is_none() {
            self.chain = Some(self.sketch.clone());
        }
        let construction = self.construction_mode;
        let auto = self.auto_constraints;
        let tool = if chain { Tool::Polyline } else { Tool::Line };
        let name = tool.msg().tr(self.lang);
        let made = self.edit(name, |s, ctx| {
            let p = materialize(s, &start)?;
            let q = materialize(s, &anchor)?;
            let line = s.add_line(p, q)?;
            if construction {
                s.set_construction(line, true)?;
            }
            if auto {
                auto_constrain(s, ctx, line, a, b);
            }
            Ok((p, q))
        });
        let Some((first, last)) = made else {
            return;
        };
        self.drew(tool);
        if chain {
            let closed = self.chain_first == Some(last);
            if self.chain_first.is_none() {
                self.chain_first = Some(first);
            }
            if closed {
                self.finish();
            } else {
                self.pending = vec![Anchor::Existing(last)];
            }
        } else {
            self.pending.clear();
        }
    }

    fn place_rectangle(&mut self, anchor: Anchor) {
        let Some(first) = self.pending.first().copied() else {
            self.pending.push(anchor);
            return;
        };
        let (Some(a), Some(b)) = (self.anchor_position(&first), self.anchor_position(&anchor))
        else {
            return;
        };
        if (a.0 - b.0).abs() < self.ui.min_segment || (a.1 - b.1).abs() < self.ui.min_segment {
            self.too_short();
            return;
        }
        let construction = self.construction_mode;
        let name = Msg::ToolRectangle.tr(self.lang);
        let made = self.edit(name, |s, _| {
            let c1 = anchor_lengths(s, &first)?;
            let c2 = anchor_lengths(s, &anchor)?;
            let rect = s.add_rectangle(c1, c2)?;
            let (x_min, y_min) = (c1.0.min(c2.0), c1.1.min(c2.1));
            for (anchor, at) in [(&first, c1), (&anchor, c2)] {
                let index = match (at.0 == x_min, at.1 == y_min) {
                    (true, true) => 0,
                    (false, true) => 1,
                    (false, false) => 2,
                    (true, false) => 3,
                };
                link(s, rect.points[index], anchor)?;
            }
            if construction {
                for line in rect.lines {
                    s.set_construction(line, true)?;
                }
            }
            Ok(())
        });
        if made.is_some() {
            self.pending.clear();
            self.drew(Tool::Rectangle);
        }
    }

    fn place_circle(&mut self, anchor: Anchor) {
        let Some(center) = self.pending.first().copied() else {
            self.pending.push(anchor);
            return;
        };
        let (Some(c), Some(p)) = (self.anchor_position(&center), self.anchor_position(&anchor))
        else {
            return;
        };
        let radius = dist(c, p);
        if radius < self.ui.min_segment {
            self.too_short();
            return;
        }
        let construction = self.construction_mode;
        let name = Msg::ToolCircle.tr(self.lang);
        let made = self.edit(name, |s, _| {
            let id = materialize(s, &center)?;
            let circle = s.add_circle(id, to_len(radius)?)?;
            if construction {
                s.set_construction(circle, true)?;
            }
            Ok(())
        });
        if made.is_some() {
            self.pending.clear();
            self.drew(Tool::Circle);
        }
    }

    fn place_arc(&mut self, anchor: Anchor) {
        if self.pending.len() < 2 {
            if let (Some(first), Some(here)) = (
                self.pending.first().and_then(|a| self.anchor_position(a)),
                self.anchor_position(&anchor),
            ) {
                if dist(first, here) < self.ui.min_segment {
                    self.too_short();
                    return;
                }
            }
            self.pending.push(anchor);
            return;
        }
        let (center, start) = (self.pending[0], self.pending[1]);
        let (Some(c), Some(s0), Some(p)) = (
            self.anchor_position(&center),
            self.anchor_position(&start),
            self.anchor_position(&anchor),
        ) else {
            return;
        };
        let radius = dist(c, s0);
        let reach = dist(c, p);
        if reach < self.ui.min_segment {
            self.too_short();
            return;
        }
        let end = (
            c.0 + (p.0 - c.0) * radius / reach,
            c.1 + (p.1 - c.1) * radius / reach,
        );
        let construction = self.construction_mode;
        let name = Msg::ToolArc.tr(self.lang);
        let made = self.edit(name, |s, _| {
            let c = materialize(s, &center)?;
            let a = materialize(s, &start)?;
            let end = Anchor::New {
                x: to_len(end.0)?,
                y: to_len(end.1)?,
                on: None,
            };
            let b = materialize(s, &end)?;
            let arc = s.add_arc(c, a, b)?;
            if construction {
                s.set_construction(arc, true)?;
            }
            Ok(())
        });
        if made.is_some() {
            self.pending.clear();
            self.drew(Tool::Arc);
        }
    }

    /// The constraint a line being drawn would get, so that the window can say so before the
    /// second click.
    pub fn proposal(&self) -> Option<Msg> {
        if !self.auto_constraints || !matches!(self.tool, Tool::Line | Tool::Polyline) {
            return None;
        }
        let a = self.pending.first().and_then(|a| self.anchor_position(a))?;
        let b = self.cursor?;
        if dist(a, b) < self.ui.min_segment {
            return None;
        }
        match near_axis(a, b, self.ui.auto_tolerance) {
            Some(Axis::Horizontal) => Some(Msg::HintHorizontal),
            Some(Axis::Vertical) => Some(Msg::HintVertical),
            None => None,
        }
    }

    // ---------------------------------------------------------------------------------------
    // Selection
    // ---------------------------------------------------------------------------------------

    /// A click with the selection tool: picks what is under the pixel, or with `extend` adds it
    /// to (or takes it from) what is picked. A click on nothing clears the selection.
    pub fn select_at(&mut self, px: Px, extend: bool) {
        match self.hit(px) {
            Some(id) => {
                if extend {
                    if let Some(k) = self.picked.elements.iter().position(|e| *e == id) {
                        self.picked.elements.remove(k);
                    } else {
                        self.picked.elements.push(id);
                    }
                } else {
                    self.picked = Picked {
                        elements: vec![id],
                        constraints: Vec::new(),
                    };
                }
            }
            None => {
                if !extend {
                    self.picked = Picked::default();
                }
            }
        }
    }

    /// Picks an element of the list.
    pub fn select_element(&mut self, id: ElementId, extend: bool) {
        if !extend {
            self.picked = Picked::default();
        }
        if let Some(k) = self.picked.elements.iter().position(|e| *e == id) {
            if extend {
                self.picked.elements.remove(k);
            }
        } else {
            self.picked.elements.push(id);
        }
    }

    /// Picks a constraint of the list.
    pub fn select_constraint(&mut self, id: ConstraintId, extend: bool) {
        if !extend {
            self.picked = Picked::default();
        }
        if let Some(k) = self.picked.constraints.iter().position(|c| *c == id) {
            if extend {
                self.picked.constraints.remove(k);
            }
        } else {
            self.picked.constraints.push(id);
        }
    }

    /// Picks every element.
    pub fn select_all(&mut self) {
        self.picked = Picked {
            elements: self.sketch.elements().iter().map(|e| e.id).collect(),
            constraints: Vec::new(),
        };
    }

    fn kind_of(&self, id: ElementId) -> Option<Kind> {
        self.sketch.element(id).map(Element::kind)
    }

    // ---------------------------------------------------------------------------------------
    // Constraints from the selection
    // ---------------------------------------------------------------------------------------

    fn picked_of(&self, kinds: &[Kind]) -> Vec<ElementId> {
        self.picked
            .elements
            .iter()
            .copied()
            .filter(|id| self.kind_of(*id).is_some_and(|k| kinds.contains(&k)))
            .collect()
    }

    /// The constraints a tool makes of the selection, or why it cannot.
    fn build(&self, tool: ConstraintTool) -> Result<Vec<Constraint>, CadError> {
        let all = &self.picked.elements;
        let points = self.picked_of(&[Kind::Point]);
        let lines = self.picked_of(&[Kind::Line]);
        let rounds = self.picked_of(&[Kind::Circle, Kind::Arc]);
        let wrong =
            || CadError::SketchConstraint("the selection does not fit this constraint".into());
        let built = match tool {
            ConstraintTool::Coincident if all.len() == 2 && points.len() == 2 => {
                vec![Constraint::Coincident(points[0], points[1])]
            }
            ConstraintTool::PointOnObject
                if all.len() == 2 && points.len() == 1 && lines.len() + rounds.len() == 1 =>
            {
                let object = lines
                    .first()
                    .or(rounds.first())
                    .copied()
                    .ok_or_else(wrong)?;
                vec![Constraint::PointOnObject {
                    point: points[0],
                    object,
                }]
            }
            ConstraintTool::Horizontal if !all.is_empty() && lines.len() == all.len() => {
                lines.iter().map(|l| Constraint::Horizontal(*l)).collect()
            }
            ConstraintTool::Horizontal if all.len() == 2 && points.len() == 2 => {
                vec![Constraint::HorizontalPoints(points[0], points[1])]
            }
            ConstraintTool::Vertical if !all.is_empty() && lines.len() == all.len() => {
                lines.iter().map(|l| Constraint::Vertical(*l)).collect()
            }
            ConstraintTool::Vertical if all.len() == 2 && points.len() == 2 => {
                vec![Constraint::VerticalPoints(points[0], points[1])]
            }
            ConstraintTool::Parallel if all.len() == 2 && lines.len() == 2 => {
                vec![Constraint::Parallel(lines[0], lines[1])]
            }
            ConstraintTool::Perpendicular if all.len() == 2 && lines.len() == 2 => {
                vec![Constraint::Perpendicular(lines[0], lines[1])]
            }
            ConstraintTool::Tangent if all.len() == 2 && !rounds.is_empty() => {
                vec![Constraint::Tangent(all[0], all[1])]
            }
            ConstraintTool::Equal if all.len() == 2 && (lines.len() == 2 || rounds.len() == 2) => {
                vec![Constraint::Equal(all[0], all[1])]
            }
            ConstraintTool::Symmetric if all.len() == 3 && points.len() == 3 => {
                vec![Constraint::SymmetricPoint {
                    a: points[0],
                    b: points[1],
                    center: points[2],
                }]
            }
            ConstraintTool::Symmetric
                if all.len() == 3 && points.len() == 2 && lines.len() == 1 =>
            {
                vec![Constraint::SymmetricLine {
                    a: points[0],
                    b: points[1],
                    axis: lines[0],
                }]
            }
            ConstraintTool::Midpoint if all.len() == 2 && points.len() == 1 && lines.len() == 1 => {
                vec![Constraint::Midpoint {
                    point: points[0],
                    line: lines[0],
                }]
            }
            ConstraintTool::Fixed if !all.is_empty() && points.len() == all.len() => points
                .iter()
                .map(|p| {
                    let (x, y) = self.sketch.point(*p)?;
                    Ok(Constraint::Fixed { point: *p, x, y })
                })
                .collect::<Result<Vec<_>, CadError>>()?,
            _ => return Err(wrong()),
        };
        for c in &built {
            self.sketch.check_constraint(c)?;
        }
        Ok(built)
    }

    /// The constraints that can be added to the selection.
    pub fn available(&self) -> Vec<ConstraintTool> {
        ConstraintTool::ALL
            .into_iter()
            .filter(|t| self.build(*t).is_ok())
            .collect()
    }

    /// Adds a constraint to the selection. A constraint that makes the sketch over-constrained is
    /// kept: the state says which ones to switch off.
    pub fn add_constraint(&mut self, tool: ConstraintTool) {
        let built = match self.build(tool) {
            Ok(built) => built,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let name = tool.msg().tr(self.lang);
        let added = self.edit(name, |s, _| {
            let mut ids = Vec::new();
            for c in built {
                ids.push(s.add_constraint(c)?);
            }
            Ok(ids)
        });
        if let Some(ids) = added {
            self.status = Status::Done(Msg::ConstraintAdded, vec![name.to_owned()]);
            self.picked.constraints = ids;
        }
    }

    // ---------------------------------------------------------------------------------------
    // Dimensions
    // ---------------------------------------------------------------------------------------

    fn dim_built(&self, tool: DimTool) -> Result<Constraint, CadError> {
        let all = &self.picked.elements;
        let points = self.picked_of(&[Kind::Point]);
        let lines = self.picked_of(&[Kind::Line]);
        let rounds = self.picked_of(&[Kind::Circle, Kind::Arc]);
        let wrong =
            || CadError::SketchConstraint("the selection does not fit this dimension".into());
        let c = match tool {
            DimTool::Distance | DimTool::DistanceX | DimTool::DistanceY
                if all.len() == 2 && points.len() == 2 =>
            {
                let (a, b) = (points[0], points[1]);
                let (pa, pb) = (self.sketch.point(a)?, self.sketch.point(b)?);
                let (pa, pb) = ((mm_of(pa.0), mm_of(pa.1)), (mm_of(pb.0), mm_of(pb.1)));
                match tool {
                    DimTool::Distance => Constraint::Distance {
                        a,
                        b,
                        dim: Dim::fixed(to_len(dist(pa, pb))?),
                    },
                    DimTool::DistanceX => Constraint::DistanceX {
                        a,
                        b,
                        dim: Dim::fixed(to_len(pb.0 - pa.0)?),
                    },
                    _ => Constraint::DistanceY {
                        a,
                        b,
                        dim: Dim::fixed(to_len(pb.1 - pa.1)?),
                    },
                }
            }
            DimTool::Length if all.len() == 1 && lines.len() == 1 => {
                let Some(Element::Line { a, b }) = self.sketch.element(lines[0]).copied() else {
                    return Err(wrong());
                };
                let (pa, pb) = (
                    self.position(a).ok_or_else(wrong)?,
                    self.position(b).ok_or_else(wrong)?,
                );
                Constraint::LineLength {
                    line: lines[0],
                    dim: Dim::fixed(to_len(dist(pa, pb))?),
                }
            }
            DimTool::Radius | DimTool::Diameter if all.len() == 1 && rounds.len() == 1 => {
                let radius = match self.sketch.element(rounds[0]).copied() {
                    Some(Element::Circle { radius, .. }) => radius,
                    Some(Element::Arc { center, start, .. }) => {
                        let (c, s) = (
                            self.position(center).ok_or_else(wrong)?,
                            self.position(start).ok_or_else(wrong)?,
                        );
                        to_len(dist(c, s))?
                    }
                    _ => return Err(wrong()),
                };
                if tool == DimTool::Radius {
                    Constraint::Radius {
                        round: rounds[0],
                        dim: Dim::fixed(radius),
                    }
                } else {
                    let twice = radius
                        .get()
                        .checked_mul(2)
                        .ok_or(CadError::ValueOutOfRange("diameter"))?;
                    Constraint::Diameter {
                        round: rounds[0],
                        dim: Dim::fixed(Length::new(twice)?),
                    }
                }
            }
            DimTool::Angle if all.len() == 2 && lines.len() == 2 => {
                let measured = self.sketch.measured_angle(lines[0], lines[1])?;
                Constraint::AngleBetween {
                    a: lines[0],
                    b: lines[1],
                    dim: Dim::fixed(measured),
                }
            }
            _ => return Err(wrong()),
        };
        self.sketch.check_constraint(&c)?;
        Ok(c)
    }

    /// The dimensions that can be added to the selection.
    pub fn dimensions_available(&self) -> Vec<DimTool> {
        DimTool::ALL
            .into_iter()
            .filter(|t| self.dim_built(*t).is_ok())
            .collect()
    }

    /// Adds a dimension to the selection with the value it has now, or with the text typed in
    /// `dim_input` (a value, a variable or a formula) when there is one.
    pub fn add_dimension(&mut self, tool: DimTool) {
        let built = match self.dim_built(tool) {
            Ok(c) => c,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let text = self.dim_input.trim().to_owned();
        let name = tool.msg().tr(self.lang);
        let added = self.edit(name, |s, ctx| {
            let id = s.add_constraint(built)?;
            if !text.is_empty() {
                apply_text(s, ctx, id, &text)?;
            }
            Ok(id)
        });
        if let Some(id) = added {
            self.picked = Picked {
                elements: Vec::new(),
                constraints: vec![id],
            };
            self.dim_input.clear();
            self.status = Status::Done(Msg::ConstraintAdded, vec![name.to_owned()]);
        }
    }

    /// Gives a dimension the value, variable or formula typed. A refused text leaves the sketch
    /// as it was.
    pub fn edit_dimension(&mut self, id: ConstraintId, text: &str) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            self.status = Status::Problem(Msg::ValueNeeded, Vec::new());
            return;
        }
        let name = self.dimension_name(id);
        let shown = text.clone();
        let done = self.edit(&name, |s, ctx| apply_text(s, ctx, id, &text));
        if done.is_some() {
            self.status = Status::Done(Msg::DimensionChanged, vec![name, shown]);
        }
    }

    fn dimension_name(&self, id: ConstraintId) -> String {
        self.sketch.constraint(id).map_or_else(String::new, |c| {
            constraint_msg(&c.constraint).tr(self.lang).to_owned()
        })
    }

    /// The value of a dimension and what to type to give it the same value again: its expression
    /// when it has one, else the value.
    fn dimension_texts(c: &Constraint) -> Option<(String, String)> {
        let (value, expr) = match c {
            Constraint::Distance { dim, .. }
            | Constraint::DistanceX { dim, .. }
            | Constraint::DistanceY { dim, .. }
            | Constraint::LineLength { dim, .. }
            | Constraint::Radius { dim, .. }
            | Constraint::Diameter { dim, .. } => (mm_text(dim.value.get()), dim.expr.clone()),
            Constraint::AngleBetween { dim, .. } => (degrees_text(dim.value), dim.expr.clone()),
            _ => return None,
        };
        let edit = expr.unwrap_or_else(|| value.clone());
        Some((value, edit))
    }

    // ---------------------------------------------------------------------------------------
    // Lists
    // ---------------------------------------------------------------------------------------

    /// The constraints, as lines of a list.
    pub fn constraint_rows(&self) -> Vec<ConstraintRow> {
        self.sketch
            .constraints()
            .iter()
            .map(|entry| {
                let texts = SketchEditor::dimension_texts(&entry.constraint);
                let ids: Vec<String> = entry
                    .constraint
                    .elements()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                let mark = if self.report.conflicting.contains(&entry.id) {
                    RowMark::Conflict
                } else if self.report.redundant.contains(&entry.id) {
                    RowMark::Redundant
                } else {
                    RowMark::Ok
                };
                ConstraintRow {
                    id: entry.id,
                    label: constraint_msg(&entry.constraint),
                    elements: ids.join(" "),
                    value: texts.as_ref().map(|t| t.0.clone()),
                    edit: texts.map(|t| t.1),
                    enabled: entry.enabled,
                    selected: self.picked.constraints.contains(&entry.id),
                    mark,
                }
            })
            .collect()
    }

    /// The elements, as lines of a list.
    pub fn element_rows(&self) -> Vec<ElementRow> {
        self.sketch
            .elements()
            .iter()
            .map(|e| {
                let detail = match e.element {
                    Element::Point { x, y } => {
                        format!("({}, {})", mm_text(x.get()), mm_text(y.get()))
                    }
                    Element::Line { a, b } => format!("{a} - {b}"),
                    Element::Circle { center, radius } => {
                        format!("{center}, R {}", mm_text(radius.get()))
                    }
                    Element::Arc { center, start, end } => format!("{center}: {start} - {end}"),
                };
                ElementRow {
                    id: e.id,
                    label: kind_msg(e.element.kind()),
                    detail,
                    construction: e.construction,
                    selected: self.picked.elements.contains(&e.id),
                }
            })
            .collect()
    }

    /// The state of the sketch, in words: fully constrained, the degrees of freedom left,
    /// over-constrained or not solved.
    pub fn state_text(&self) -> String {
        match self.report.health {
            Health::Empty => Msg::StateEmpty.tr(self.lang).to_owned(),
            Health::Full => Msg::StateFull.tr(self.lang).to_owned(),
            Health::Under => fill(
                Msg::StateUnder.tr(self.lang),
                &[&self.report.dof.unwrap_or(0).to_string()],
            ),
            Health::Over => Msg::StateOver.tr(self.lang).to_owned(),
            Health::Failed => Msg::StateNotSolved.tr(self.lang).to_owned(),
        }
    }

    /// The constraints that contradict and the ones that repeat, in words.
    pub fn problem_lines(&self) -> Vec<String> {
        let list = |ids: &[ConstraintId]| {
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut lines = Vec::new();
        if !self.report.conflicting.is_empty() {
            lines.push(fill(
                Msg::ConflictList.tr(self.lang),
                &[&list(&self.report.conflicting)],
            ));
        }
        if !self.report.redundant.is_empty() {
            lines.push(fill(
                Msg::RedundantList.tr(self.lang),
                &[&list(&self.report.redundant)],
            ));
        }
        if let Some(e) = &self.report.error {
            lines.push(crate::window::error_text(e, self.lang));
        }
        lines
    }

    // ---------------------------------------------------------------------------------------
    // Editing what is there
    // ---------------------------------------------------------------------------------------

    /// Switches a constraint on or off and solves again.
    pub fn set_enabled(&mut self, id: ConstraintId, enabled: bool) {
        let name = self.dimension_name(id);
        let _ = self.edit(&name, |s, _| s.set_enabled(id, enabled));
    }

    /// Switches off the constraints that repeat or contradict the others: the latest one the
    /// solver names, and again for what is still named, until the sketch holds together.
    pub fn disable_problems(&mut self) {
        if self.report.problems().is_empty() {
            return;
        }
        let name = Msg::DisableProblems.tr(self.lang);
        let done = self.edit(name, |s, ctx| {
            let mut switched = 0usize;
            while switched < s.constraints().len() {
                let outcome = s.clone().solve(ctx.solver);
                let Some(last) = Report::of(s, outcome).problems().last().copied() else {
                    break;
                };
                s.set_enabled(last, false)?;
                switched += 1;
            }
            Ok(switched)
        });
        if let Some(n) = done {
            self.status = Status::Done(Msg::ProblemsDisabled, vec![n.to_string()]);
        }
    }

    /// Removes the selection with what depends on it (the lines of a point, the constraints about
    /// anything removed).
    pub fn delete_selection(&mut self) {
        if self.picked.is_empty() {
            self.status = Status::Problem(Msg::SelectFirst, Vec::new());
            return;
        }
        let picked = self.picked.clone();
        let name = Msg::DeleteSelection.tr(self.lang);
        let removed = self.edit(name, |s, _| {
            let mut ids: BTreeSet<ElementId> = picked.elements.iter().copied().collect();
            loop {
                let before = ids.len();
                for e in s.elements() {
                    if e.element.parts().iter().any(|p| ids.contains(p)) {
                        ids.insert(e.id);
                    }
                }
                if ids.len() == before {
                    break;
                }
            }
            let mut doomed: BTreeSet<ConstraintId> = picked.constraints.iter().copied().collect();
            for c in s.constraints() {
                if c.constraint.elements().iter().any(|e| ids.contains(e)) {
                    doomed.insert(c.id);
                }
            }
            for id in &doomed {
                s.remove_constraint(*id)?;
            }
            for id in ids.iter().rev() {
                s.remove_element(*id)?;
            }
            Ok(doomed.len() + ids.len())
        });
        if let Some(n) = removed {
            self.picked = Picked::default();
            self.status = Status::Done(Msg::ElementsDeleted, vec![n.to_string()]);
        }
    }

    /// Turns the selected elements into construction geometry, or back into geometry of the
    /// profile when every one of them is construction.
    pub fn toggle_construction(&mut self) {
        let ids = self.picked.elements.clone();
        if ids.is_empty() {
            self.status = Status::Problem(Msg::SelectFirst, Vec::new());
            return;
        }
        let name = Msg::ToggleConstruction.tr(self.lang);
        let all_construction = ids.iter().all(|id| {
            self.sketch
                .elements()
                .iter()
                .any(|e| e.id == *id && e.construction)
        });
        let _ = self.edit(name, |s, _| {
            for id in &ids {
                s.set_construction(*id, !all_construction)?;
            }
            Ok(())
        });
    }

    // ---------------------------------------------------------------------------------------
    // Dragging
    // ---------------------------------------------------------------------------------------

    /// Starts to drag what is under the pixel (it becomes the selection when it is not part of
    /// it). The answer is false when there is nothing there.
    pub fn begin_drag(&mut self, px: Px) -> bool {
        let Some(id) = self.hit(px) else {
            return false;
        };
        if !self.picked.elements.contains(&id) {
            self.picked = Picked {
                elements: vec![id],
                constraints: Vec::new(),
            };
        }
        let mut ids: Vec<ElementId> = Vec::new();
        for picked in self.picked.elements.clone() {
            let parts = match self.sketch.element(picked) {
                Some(Element::Point { .. }) => vec![picked],
                Some(Element::Line { a, b }) => vec![*a, *b],
                Some(Element::Circle { center, .. }) => vec![*center],
                Some(Element::Arc { center, start, end }) => vec![*center, *start, *end],
                None => Vec::new(),
            };
            for p in parts {
                if !ids.contains(&p) {
                    ids.push(p);
                }
            }
        }
        let targets: Vec<(ElementId, Mm)> = ids
            .into_iter()
            .filter_map(|p| self.position(p).map(|at| (p, at)))
            .collect();
        if targets.is_empty() {
            return false;
        }
        self.drag = Some(Drag {
            before: self.sketch.clone(),
            targets,
            start: self.view.to_mm(px),
            over: None,
        });
        true
    }

    /// Moves what is dragged to the pixel. The solver runs at each move (when the settings say so)
    /// with the dragged points held at the pointer, so what is tied to them follows and what
    /// cannot move stays: a drag that no solution allows leaves the sketch where it was.
    pub fn drag_to(&mut self, px: Px) {
        let Some(drag) = self.drag.clone() else {
            return;
        };
        let skip: Vec<ElementId> = drag.targets.iter().map(|t| t.0).collect();
        let snap = self.snap_skipping(px, &skip);
        let mut delta = {
            let here = self.view.to_mm(px);
            (here.0 - drag.start.0, here.1 - drag.start.1)
        };
        let mut over = None;
        if let (SnapKind::Point(other), [(_, first)]) = (snap.kind, drag.targets.as_slice()) {
            delta = (snap.at.0 - first.0, snap.at.1 - first.1);
            over = Some(other);
        } else if self.grid_snap && drag.targets.len() == 1 {
            let first = drag.targets[0].1;
            let g = self.ui.grid;
            let target = (
                ((first.0 + delta.0) / g).round() * g,
                ((first.1 + delta.1) / g).round() * g,
            );
            delta = (target.0 - first.0, target.1 - first.1);
        }
        if let Some(d) = self.drag.as_mut() {
            d.over = over;
        }
        if !self.ui.live_drag {
            return;
        }
        // everything that is dragged is held at the pointer; when the constraints between the
        // dragged points forbid it (the two ends of a horizontal line cannot both be held), only
        // the first one is held and the others follow where the constraints put them
        if !self.solve_drag(&drag.targets, delta) && drag.targets.len() > 1 {
            self.solve_drag(&drag.targets[..1], delta);
        }
    }

    /// Solves the sketch with the given points held at their start moved by `delta`, and keeps
    /// the solution. False when there is none (the sketch is not changed).
    fn solve_drag(&mut self, held: &[(ElementId, Mm)], delta: Mm) -> bool {
        let mut trial = self.sketch.clone();
        for (id, at) in held {
            let (Ok(x), Ok(y)) = (to_len(at.0 + delta.0), to_len(at.1 + delta.1)) else {
                return false;
            };
            if trial
                .add_constraint(Constraint::Fixed { point: *id, x, y })
                .is_err()
            {
                return false;
            }
        }
        if !matches!(trial.solve(&self.solver), Ok(Outcome::Solved { .. })) {
            return false;
        }
        for entry in trial.elements() {
            match entry.element {
                Element::Point { x, y } => self.sketch.set_point_exact(entry.id, x, y),
                Element::Circle { radius, .. } => self.sketch.set_radius_exact(entry.id, radius),
                _ => {}
            }
        }
        true
    }

    /// Ends the drag: a point dropped on another point is made coincident with it, and the whole
    /// drag is one step of the history.
    pub fn end_drag(&mut self) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if let (Some(over), [(point, _)]) = (drag.over, drag.targets.as_slice()) {
            if self.position(*point) == self.position(over) {
                let _ = self
                    .sketch
                    .add_constraint(Constraint::Coincident(*point, over));
            }
        }
        self.evaluate(true);
        if self.sketch != drag.before {
            self.dirty = true;
            let name = Msg::HistMove.tr(self.lang);
            self.history.record(name, drag.before);
            self.status = Status::Done(Msg::ElementMoved, Vec::new());
        }
    }

    /// Whether a drag is going on.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    // ---------------------------------------------------------------------------------------
    // History
    // ---------------------------------------------------------------------------------------

    /// Whether there is a change to go back to.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether there is a change to take forward again.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Goes back one change.
    pub fn undo(&mut self) {
        self.finish_chain();
        self.pending.clear();
        match self.history.undo(self.sketch.clone()) {
            Some((name, previous)) => self.restore(previous, Msg::Undone, name),
            None => self.status = Status::Done(Msg::NothingToUndo, Vec::new()),
        }
    }

    /// Goes forward one change that was undone.
    pub fn redo(&mut self) {
        match self.history.redo(self.sketch.clone()) {
            Some((name, next)) => self.restore(next, Msg::Redone, name),
            None => self.status = Status::Done(Msg::NothingToRedo, Vec::new()),
        }
    }

    fn restore(&mut self, sketch: Sketch, message: Msg, name: String) {
        self.sketch = sketch;
        self.dirty = true;
        self.picked
            .elements
            .retain(|id| self.sketch.element(*id).is_some());
        self.picked
            .constraints
            .retain(|id| self.sketch.constraint(*id).is_some());
        self.status = Status::Done(message, vec![name]);
        self.evaluate(false);
    }

    // ---------------------------------------------------------------------------------------
    // Files
    // ---------------------------------------------------------------------------------------

    /// Opens a sketch file. The answer is false (and the status says why) when it could not be
    /// read; the sketch that was open stays as it was.
    pub fn open_file(&mut self, path: &Path) -> bool {
        match file::read_file(path, &self.solver) {
            Ok(sketch) => {
                self.sketch = sketch;
                self.reset_session();
                self.path = Some(path.to_path_buf());
                self.evaluate(false);
                self.fit();
                self.status = Status::Done(Msg::Opened, vec![file_name(path)]);
                true
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// Writes the sketch to `path`, atomically, keeping the copies of the settings.
    pub fn save_to(&mut self, path: &Path) -> bool {
        match file::write_file(path, &self.sketch, self.library.backups) {
            Ok(()) => {
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                self.status = Status::Done(Msg::Saved, vec![file_name(path)]);
                true
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// Writes the sketch to the file it came from. False when it has none yet.
    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(path) => self.save_to(&path),
            None => false,
        }
    }

    // ---------------------------------------------------------------------------------------
    // The drawing
    // ---------------------------------------------------------------------------------------

    fn stroke_of(&self, entry_id: ElementId, construction: bool) -> Stroke {
        if self.picked.elements.contains(&entry_id) {
            return Stroke::Selected;
        }
        for row in &self.picked.constraints {
            if let Some(c) = self.sketch.constraint(*row) {
                if c.constraint.elements().contains(&entry_id) {
                    return Stroke::Related;
                }
            }
        }
        for c in self.sketch.constraints() {
            let mark = if self.report.conflicting.contains(&c.id) {
                Stroke::Conflict
            } else if self.report.redundant.contains(&c.id) {
                Stroke::Redundant
            } else {
                continue;
            };
            if c.constraint.elements().contains(&entry_id) {
                return mark;
            }
        }
        if construction {
            Stroke::Construction
        } else {
            Stroke::Normal
        }
    }

    fn grid_prims(&self, out: &mut Vec<Prim>) {
        let (w, h) = self.view.size;
        let mut step = self.ui.grid;
        let mut guard = 0;
        while step * self.view.scale < self.ui.min_grid_px && guard < 12 {
            step *= 10.0;
            guard += 1;
        }
        let low = self.view.to_mm((0.0, h));
        let high = self.view.to_mm((w, 0.0));
        let first_x = (low.0 / step).floor() as i64;
        let last_x = (high.0 / step).ceil() as i64;
        let first_y = (low.1 / step).floor() as i64;
        let last_y = (high.1 / step).ceil() as i64;
        if (last_x - first_x) > 600 || (last_y - first_y) > 600 {
            return;
        }
        let major = i64::from(self.ui.major_every);
        for k in first_x..=last_x {
            let x = self.view.to_px((k as f64 * step, 0.0)).0;
            let stroke = if k == 0 {
                Stroke::Axis
            } else if k % major == 0 {
                Stroke::GridMajor
            } else {
                Stroke::GridMinor
            };
            out.push(Prim::Segment {
                a: (x, 0.0),
                b: (x, h),
                stroke,
            });
        }
        for k in first_y..=last_y {
            let y = self.view.to_px((0.0, k as f64 * step)).1;
            let stroke = if k == 0 {
                Stroke::Axis
            } else if k % major == 0 {
                Stroke::GridMajor
            } else {
                Stroke::GridMinor
            };
            out.push(Prim::Segment {
                a: (0.0, y),
                b: (w, y),
                stroke,
            });
        }
    }

    fn shape_prims(&self, out: &mut Vec<Prim>) {
        let segments = self.ui.arc_segments;
        let to_px = |points: Vec<Mm>| -> Vec<Px> {
            points.into_iter().map(|p| self.view.to_px(p)).collect()
        };
        // lines, circles and arcs first, so that the points are drawn over them
        for entry in self.sketch.elements() {
            let stroke = self.stroke_of(entry.id, entry.construction);
            match entry.element {
                Element::Point { .. } => {}
                Element::Line { a, b } => {
                    if let (Some(a), Some(b)) = (self.px(a), self.px(b)) {
                        out.push(Prim::Segment { a, b, stroke });
                    }
                }
                Element::Circle { center, radius } => {
                    if let Some(c) = self.position(center) {
                        out.push(Prim::Polyline {
                            points: to_px(circle_points(c, mm_of(radius), segments)),
                            stroke,
                        });
                    }
                }
                Element::Arc { center, start, end } => {
                    if let (Some(c), Some(s), Some(e)) = (
                        self.position(center),
                        self.position(start),
                        self.position(end),
                    ) {
                        out.push(Prim::Polyline {
                            points: to_px(arc_points(c, s, e, segments)),
                            stroke,
                        });
                    }
                }
            }
        }
        for entry in self.sketch.elements() {
            if let Element::Point { .. } = entry.element {
                if let Some(at) = self.px(entry.id) {
                    out.push(Prim::Dot {
                        at,
                        stroke: self.stroke_of(entry.id, false),
                    });
                }
            }
        }
    }

    /// The middle of an element in the sketch, for the signs and the dimensions beside it.
    fn middle(&self, id: ElementId) -> Option<Mm> {
        match self.sketch.element(id)? {
            Element::Point { x, y } => Some((mm_of(*x), mm_of(*y))),
            Element::Line { a, b } => {
                let (a, b) = (self.position(*a)?, self.position(*b)?);
                Some(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
            }
            Element::Circle { center, .. } | Element::Arc { center, .. } => self.position(*center),
        }
    }

    fn label_prims(&self, out: &mut Vec<Prim>) {
        let off = self.ui.label_offset;
        for entry in self.sketch.constraints() {
            if !entry.enabled {
                continue;
            }
            let c = &entry.constraint;
            let stroke = if self.report.conflicting.contains(&entry.id) {
                Stroke::Conflict
            } else if self.report.redundant.contains(&entry.id) {
                Stroke::Redundant
            } else if self.picked.constraints.contains(&entry.id) {
                Stroke::Selected
            } else {
                Stroke::Dimension
            };
            if let Some(sign) = marker_of(c) {
                let on = c.elements().into_iter().find_map(|e| self.middle(e));
                if let Some(m) = on {
                    let at = self.view.to_px(m);
                    out.push(Prim::Text {
                        at: (at.0 + off * 0.6, at.1 - off * 0.6),
                        text: sign.to_owned(),
                        stroke: if stroke == Stroke::Dimension {
                            Stroke::Marker
                        } else {
                            stroke
                        },
                    });
                }
                continue;
            }
            let Some((value, _)) = SketchEditor::dimension_texts(c) else {
                continue;
            };
            let prefix = match c {
                Constraint::DistanceX { .. } => "dx ",
                Constraint::DistanceY { .. } => "dy ",
                Constraint::Radius { .. } => "R ",
                Constraint::Diameter { .. } => "D ",
                _ => "",
            };
            let suffix = if matches!(c, Constraint::AngleBetween { .. }) {
                "\u{b0}"
            } else {
                ""
            };
            let name = match c {
                Constraint::Distance { dim, .. }
                | Constraint::DistanceX { dim, .. }
                | Constraint::DistanceY { dim, .. }
                | Constraint::LineLength { dim, .. }
                | Constraint::Radius { dim, .. }
                | Constraint::Diameter { dim, .. } => dim.expr.clone(),
                Constraint::AngleBetween { dim, .. } => dim.expr.clone(),
                _ => None,
            };
            let text = match name {
                Some(expr) => format!("{prefix}{value}{suffix} ({expr})"),
                None => format!("{prefix}{value}{suffix}"),
            };
            match c {
                Constraint::Distance { a, b, .. }
                | Constraint::DistanceX { a, b, .. }
                | Constraint::DistanceY { a, b, .. } => {
                    if let (Some(pa), Some(pb)) = (self.px(*a), self.px(*b)) {
                        let (pa, pb) = shifted(pa, pb, off);
                        out.push(Prim::Segment {
                            a: pa,
                            b: pb,
                            stroke,
                        });
                        out.push(Prim::Text {
                            at: ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0),
                            text,
                            stroke,
                        });
                    }
                }
                Constraint::LineLength { line, .. } => {
                    if let Some(Element::Line { a, b }) = self.sketch.element(*line) {
                        if let (Some(pa), Some(pb)) = (self.px(*a), self.px(*b)) {
                            let (pa, pb) = shifted(pa, pb, off);
                            out.push(Prim::Segment {
                                a: pa,
                                b: pb,
                                stroke,
                            });
                            out.push(Prim::Text {
                                at: ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0),
                                text,
                                stroke,
                            });
                        }
                    }
                }
                _ => {
                    if let Some(m) = c.elements().first().and_then(|e| self.middle(*e)) {
                        let at = self.view.to_px(m);
                        out.push(Prim::Text {
                            at: (at.0, at.1 - off),
                            text,
                            stroke,
                        });
                    }
                }
            }
        }
    }

    fn preview_prims(&self, out: &mut Vec<Prim>) {
        if let Some(snap) = &self.hover {
            let at = self.view.to_px(snap.at);
            if !matches!(snap.kind, SnapKind::Free) {
                out.push(Prim::Ring {
                    at,
                    stroke: Stroke::Preview,
                });
            }
        }
        let (Some(cursor), false) = (self.cursor, self.pending.is_empty()) else {
            return;
        };
        let here = self.view.to_px(cursor);
        let first = self
            .pending
            .first()
            .and_then(|a| self.anchor_position(a))
            .map(|p| self.view.to_px(p));
        let Some(first) = first else {
            return;
        };
        match self.tool {
            Tool::Line | Tool::Polyline => out.push(Prim::Segment {
                a: first,
                b: here,
                stroke: Stroke::Preview,
            }),
            Tool::Rectangle => {
                let corners = [first, (here.0, first.1), here, (first.0, here.1), first];
                out.push(Prim::Polyline {
                    points: corners.to_vec(),
                    stroke: Stroke::Preview,
                });
            }
            Tool::Circle => {
                let c = self.view.to_mm(first);
                let r = dist(c, cursor);
                out.push(Prim::Polyline {
                    points: circle_points(c, r, self.ui.arc_segments)
                        .into_iter()
                        .map(|p| self.view.to_px(p))
                        .collect(),
                    stroke: Stroke::Preview,
                });
            }
            Tool::Arc => {
                let c = self.view.to_mm(first);
                match self.pending.get(1).and_then(|a| self.anchor_position(a)) {
                    None => out.push(Prim::Segment {
                        a: first,
                        b: here,
                        stroke: Stroke::Preview,
                    }),
                    Some(s) => {
                        let r = dist(c, s);
                        let reach = dist(c, cursor);
                        if reach > 0.0 {
                            let e = (
                                c.0 + (cursor.0 - c.0) * r / reach,
                                c.1 + (cursor.1 - c.1) * r / reach,
                            );
                            out.push(Prim::Polyline {
                                points: arc_points(c, s, e, self.ui.arc_segments)
                                    .into_iter()
                                    .map(|p| self.view.to_px(p))
                                    .collect(),
                                stroke: Stroke::Preview,
                            });
                        }
                    }
                }
            }
            Tool::Point | Tool::Select => {}
        }
    }

    /// Everything the canvas draws, back to front: the grid and the axes, the geometry, the signs
    /// and dimensions of the constraints and what is being drawn.
    pub fn scene(&self) -> Vec<Prim> {
        let mut out = Vec::new();
        self.grid_prims(&mut out);
        self.shape_prims(&mut out);
        self.label_prims(&mut out);
        self.preview_prims(&mut out);
        out
    }
}

/// A segment moved sideways by `off` pixels (to the left of its direction).
fn shifted(a: Px, b: Px, off: f64) -> (Px, Px) {
    let (dx, dy): (f64, f64) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= 0.0 {
        return (a, b);
    }
    let (nx, ny) = (dy / len * off, -dx / len * off);
    ((a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// -------------------------------------------------------------------------------------------
// Pieces of the transactions
// -------------------------------------------------------------------------------------------

/// The point an anchor stands for, added to the sketch when it is new.
fn materialize(s: &mut Sketch, anchor: &Anchor) -> Result<ElementId, CadError> {
    match *anchor {
        Anchor::Existing(id) => Ok(id),
        Anchor::New { x, y, on } => {
            let id = s.add_point(x, y)?;
            if let Some(object) = on {
                s.add_constraint(Constraint::PointOnObject { point: id, object })?;
            }
            Ok(id)
        }
    }
}

fn anchor_lengths(s: &Sketch, anchor: &Anchor) -> Result<(Length, Length), CadError> {
    match *anchor {
        Anchor::Existing(id) => s.point(id),
        Anchor::New { x, y, .. } => Ok((x, y)),
    }
}

/// Ties a point of a new shape to what its corner was clicked on.
fn link(s: &mut Sketch, point: ElementId, anchor: &Anchor) -> Result<(), CadError> {
    match *anchor {
        Anchor::Existing(other) => {
            s.add_constraint(Constraint::Coincident(point, other))?;
        }
        Anchor::New {
            on: Some(object), ..
        } => {
            s.add_constraint(Constraint::PointOnObject { point, object })?;
        }
        Anchor::New { on: None, .. } => {}
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

/// Whether the line from `a` to `b` is within `tolerance` degrees of an axis.
fn near_axis(a: Mm, b: Mm, tolerance: f64) -> Option<Axis> {
    let (dx, dy): (f64, f64) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    let angle = dy.atan2(dx).to_degrees();
    if angle <= tolerance {
        Some(Axis::Horizontal)
    } else if angle >= 90.0 - tolerance {
        Some(Axis::Vertical)
    } else {
        None
    }
}

/// Adds the constraint that a new line close to horizontal or vertical asks for, unless it makes
/// the sketch unsolvable.
fn auto_constrain(s: &mut Sketch, ctx: &Ctx, line: ElementId, a: Mm, b: Mm) {
    let constraint = match near_axis(a, b, ctx.ui.auto_tolerance) {
        Some(Axis::Horizontal) => Constraint::Horizontal(line),
        Some(Axis::Vertical) => Constraint::Vertical(line),
        None => return,
    };
    let mut trial = s.clone();
    if trial.add_constraint(constraint.clone()).is_err() {
        return;
    }
    if matches!(trial.solve(ctx.solver), Ok(Outcome::Solved { .. })) {
        s.add_constraint(constraint).ok();
    }
}

/// Gives a dimension the value, variable or formula typed: a bare number is a length in the unit
/// of the settings (degrees for an angle) and stays a plain value; anything else is a formula
/// over the variables and the constants and is kept with the dimension.
fn apply_text(s: &mut Sketch, ctx: &Ctx, id: ConstraintId, text: &str) -> Result<(), CadError> {
    let angle = matches!(
        s.constraint(id).map(|c| &c.constraint),
        Some(Constraint::AngleBetween { .. })
    );
    let numeric = !text.chars().any(char::is_alphabetic);
    let expr = if numeric {
        let number = text.replace(',', ".");
        if angle {
            format!("{number} deg")
        } else {
            format!("{number} {}", ctx.library.unit.suffix())
        }
    } else {
        text.to_owned()
    };
    s.set_dimension_expr(id, Some(&expr))?;
    s.resolve_dimensions(ctx.variables, ctx.library)?;
    if numeric {
        s.set_dimension_expr(id, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> SketchEditor {
        let mut e = SketchEditor::with_defaults(Lang::En);
        e.set_canvas((800.0, 600.0));
        e
    }

    /// The pixel of a point of the sketch in millimetres.
    fn at(e: &SketchEditor, x: f64, y: f64) -> Px {
        e.view.to_px((x, y))
    }

    fn click(e: &mut SketchEditor, x: f64, y: f64) {
        let p = at(e, x, y);
        e.pointer_move(p);
        e.place(p);
    }

    fn mm(v: f64) -> Length {
        to_len(v).unwrap()
    }

    fn point(e: &SketchEditor, id: u32) -> (Length, Length) {
        e.sketch.point(ElementId(id)).unwrap()
    }

    fn ids(e: &SketchEditor) -> usize {
        e.sketch.elements().len()
    }

    /// A rectangle from (0, 0) to (40, 25) drawn with the tool.
    fn rectangle(e: &mut SketchEditor) {
        e.set_tool(Tool::Rectangle);
        click(e, 0.0, 0.0);
        click(e, 40.0, 25.0);
        e.set_tool(Tool::Select);
    }

    fn pick(e: &mut SketchEditor, list: &[u32]) {
        e.picked = Picked {
            elements: list.iter().map(|n| ElementId(*n)).collect(),
            constraints: Vec::new(),
        };
    }

    #[test]
    fn the_settings_come_from_the_shipped_file_and_a_user_file_changes_its_keys() {
        let s = SketchUiSettings::defaults();
        assert_eq!(s.grid, 1.0);
        assert!(s.grid_snap && s.auto_constraints && s.live_drag);
        assert_eq!(
            (s.snap_radius, s.pick_radius, s.auto_tolerance),
            (10.0, 8.0, 3.0)
        );
        assert_eq!((s.scale, s.scale_min, s.scale_max), (8.0, 0.5, 400.0));
        assert_eq!((s.arc_segments, s.major_every, s.undo_depth), (48, 10, 100));
        assert_eq!(SketchUiSettings::with_user_config("").unwrap(), s);
        let both = format!(
            "{}\n{}\n{}",
            crate::config::DEFAULT_CONFIG,
            crate::uiconfig::default_file(),
            crate::sketch::settings::default_file()
        );
        assert_eq!(SketchUiSettings::with_user_config(&both).unwrap(), s);
        let own =
            SketchUiSettings::with_user_config("[sketch_ui]\ngrid = 2.5\ngrid_snap = 0\n").unwrap();
        assert_eq!(own.grid, 2.5);
        assert!(!own.grid_snap);
        assert_eq!(own.snap_radius, 10.0);
        // the keys of the solver are still read by their own part
        assert_eq!(
            SketchSettings::with_user_config(&both).unwrap(),
            SketchSettings::defaults()
        );
    }

    #[test]
    fn values_outside_their_range_are_refused() {
        for text in [
            "[sketch_ui]\ngrid = 0\n",
            "[sketch_ui]\ngrid_snap = 2\n",
            "[sketch_ui]\nsnap_radius = 500\n",
            "[sketch_ui]\nauto_tolerance = 60\n",
            "[sketch_ui]\nscale_min = 500\n",
            "[sketch_ui]\nscale = 5000\n",
            "[sketch_ui]\narc_segments = 3\n",
            "[sketch_ui]\nmajor_every = one\n",
        ] {
            let error = SketchUiSettings::with_user_config(text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{text:?}: {error}");
        }
    }

    #[test]
    fn the_canvas_maps_pixels_to_the_sketch_and_back() {
        let v = View2::new(10.0, (800.0, 600.0));
        assert_eq!(v.to_px((0.0, 0.0)), (400.0, 300.0));
        assert_eq!(v.to_px((5.0, 2.0)), (450.0, 280.0));
        assert_eq!(v.to_mm((450.0, 280.0)), (5.0, 2.0));
        let w = View2 {
            center: (10.0, -4.0),
            ..v
        };
        assert_eq!(w.to_px((10.0, -4.0)), (400.0, 300.0));
        let back = w.to_mm(w.to_px((1.5, 2.5)));
        assert!((back.0 - 1.5).abs() < 1e-12 && (back.1 - 2.5).abs() < 1e-12);
    }

    #[test]
    fn the_wheel_zooms_around_the_pointer_and_stays_in_its_limits() {
        let mut e = editor();
        let p = (600.0, 200.0);
        let before = e.view.to_mm(p);
        e.wheel(p, 400.0);
        assert!(e.view.scale > 8.0);
        let after = e.view.to_mm(p);
        assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
        e.wheel(p, 1e9);
        assert_eq!(e.view.scale, 400.0);
        e.wheel(p, -1e9);
        assert_eq!(e.view.scale, 0.5);
        let mut e = editor();
        e.pan(80.0, -40.0);
        assert_eq!(e.view.center, (-10.0, -5.0));
    }

    #[test]
    fn a_click_lands_on_the_grid_unless_the_grid_is_off() {
        let mut e = editor();
        let snap = e.snap_at(at(&e, 3.4, 7.6));
        assert_eq!(snap.kind, SnapKind::Grid);
        assert_eq!(snap.at, (3.0, 8.0));
        e.grid_snap = false;
        let snap = e.snap_at(at(&e, 3.4, 7.6));
        assert_eq!(snap.kind, SnapKind::Free);
        assert!((snap.at.0 - 3.4).abs() < 1e-9 && (snap.at.1 - 7.6).abs() < 1e-9);
    }

    #[test]
    fn a_click_lands_on_a_point_and_then_on_a_line_before_the_grid() {
        let mut e = editor();
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 20.0, 0.0);
        // one pixel is 0.125 mm at 8 px/mm: a click 0.3 mm from the end is inside 10 px
        let near = e.snap_at(at(&e, 20.3, 0.2));
        assert_eq!(near.kind, SnapKind::Point(ElementId(2)));
        assert_eq!(near.at, (20.0, 0.0));
        let along = e.snap_at(at(&e, 10.0, 0.4));
        assert_eq!(along.kind, SnapKind::Object(ElementId(3)));
        assert!((along.at.0 - 10.0).abs() < 1e-9 && along.at.1 == 0.0);
        let far = e.snap_at(at(&e, 10.0, 5.0));
        assert_eq!(far.kind, SnapKind::Grid);
        assert_eq!(far.at, (10.0, 5.0));
    }

    #[test]
    fn a_point_clicked_on_a_line_is_tied_to_it() {
        let mut e = editor();
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 20.0, 0.0);
        e.set_tool(Tool::Point);
        click(&mut e, 10.0, 0.4);
        assert_eq!(ids(&e), 4);
        let c = e.sketch.constraints().last().unwrap();
        assert_eq!(
            c.constraint,
            Constraint::PointOnObject {
                point: ElementId(4),
                object: ElementId(3)
            }
        );
        // the solver moved the point onto the line
        assert_eq!(point(&e, 4).1, Length::ZERO);
        click(&mut e, 0.0, 0.0);
        assert_eq!(
            e.status,
            Status::Problem(Msg::AlreadyAPoint, Vec::new()),
            "a click on a point adds nothing"
        );
    }

    #[test]
    fn a_line_is_two_clicks_and_one_step_of_the_history() {
        let mut e = editor();
        e.set_tool(Tool::Line);
        click(&mut e, 3.0, 4.0);
        assert!(e.is_drawing());
        assert_eq!(ids(&e), 0);
        click(&mut e, 33.0, 24.0);
        assert!(!e.is_drawing());
        assert_eq!(ids(&e), 3);
        assert_eq!(point(&e, 1), (mm(3.0), mm(4.0)));
        assert_eq!(point(&e, 2), (mm(33.0), mm(24.0)));
        assert_eq!(
            e.sketch.constraints().len(),
            0,
            "30 by 20 is not near an axis"
        );
        assert_eq!(e.history.undo_names(), vec!["Line"]);
        assert!(e.dirty);
        e.undo();
        assert_eq!(ids(&e), 0);
        e.redo();
        assert_eq!(ids(&e), 3);
    }

    #[test]
    fn a_line_near_horizontal_or_vertical_is_made_so_and_the_window_says_it_first() {
        let mut e = editor();
        e.grid_snap = false;
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        e.pointer_move(at(&e, 20.0, 0.4));
        assert_eq!(e.proposal(), Some(Msg::HintHorizontal));
        e.pointer_move(at(&e, 20.0, 12.0));
        assert_eq!(e.proposal(), None);
        e.pointer_move(at(&e, 0.5, 20.0));
        assert_eq!(e.proposal(), Some(Msg::HintVertical));
        click(&mut e, 20.0, 0.4);
        assert_eq!(
            e.sketch.constraints()[0].constraint,
            Constraint::Horizontal(ElementId(3))
        );
        let (a, b) = (point(&e, 1), point(&e, 2));
        assert_eq!(a.1, b.1, "the ends are level after the solver");
        assert_eq!(e.report.health, Health::Under);
        assert_eq!(
            e.report.dof,
            Some(3),
            "four coordinates less one constraint"
        );
        // with the proposal off nothing is added
        let mut e = editor();
        e.grid_snap = false;
        e.auto_constraints = false;
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        assert_eq!(e.proposal(), None);
        click(&mut e, 20.0, 0.4);
        assert!(e.sketch.constraints().is_empty());
    }

    #[test]
    fn a_line_shorter_than_the_least_is_not_drawn() {
        let mut e = editor();
        e.grid_snap = false;
        e.set_tool(Tool::Line);
        click(&mut e, 5.0, 5.0);
        let p = at(&e, 5.000_000_5, 5.0);
        e.place(p);
        assert_eq!(e.status, Status::Problem(Msg::TooShort, Vec::new()));
        assert_eq!(ids(&e), 0);
        assert!(e.is_drawing());
    }

    #[test]
    fn a_polyline_shares_its_corners_and_is_one_step_that_a_click_on_the_first_corner_closes() {
        let mut e = editor();
        e.set_tool(Tool::Polyline);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 30.0, 0.0);
        click(&mut e, 30.0, 20.0);
        assert_eq!(ids(&e), 5, "three points and two lines");
        assert!(e.is_drawing());
        click(&mut e, 0.0, 0.0);
        assert_eq!(ids(&e), 6, "the last line ends on the first point");
        assert!(!e.is_drawing(), "closing the shape ends it");
        assert_eq!(e.history.undo_names(), vec!["Polyline"]);
        let lines: Vec<&Element> = e
            .sketch
            .elements()
            .iter()
            .map(|x| &x.element)
            .filter(|x| matches!(x, Element::Line { .. }))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[2],
            &Element::Line {
                a: ElementId(4),
                b: ElementId(1)
            }
        );
        // two of the three lines are horizontal or vertical
        assert_eq!(e.sketch.constraints().len(), 2);
        e.undo();
        assert_eq!(ids(&e), 0);
        // finish with the keyboard keeps what was drawn
        e.set_tool(Tool::Polyline);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 10.0, 10.0);
        e.finish();
        assert!(!e.is_drawing());
        assert_eq!(ids(&e), 3);
        assert_eq!(e.history.undo_names(), vec!["Polyline"]);
    }

    #[test]
    fn a_rectangle_is_two_corners_with_four_degrees_of_freedom() {
        let mut e = editor();
        rectangle(&mut e);
        assert_eq!(ids(&e), 8);
        assert_eq!(e.sketch.constraints().len(), 4);
        assert_eq!(point(&e, 1), (mm(0.0), mm(0.0)));
        assert_eq!(point(&e, 3), (mm(40.0), mm(25.0)));
        assert_eq!(e.report.health, Health::Under);
        assert_eq!(e.report.dof, Some(4));
        assert_eq!(e.report.tone(), Tone::Amber);
        assert_eq!(e.state_text(), "4 degrees of freedom left");
        // a corner clicked on a point that exists is tied to it
        e.set_tool(Tool::Rectangle);
        click(&mut e, 40.0, 25.0);
        click(&mut e, 60.0, 45.0);
        let last = e.sketch.constraints().last().unwrap();
        assert_eq!(
            last.constraint,
            Constraint::Coincident(ElementId(9), ElementId(3))
        );
    }

    #[test]
    fn a_circle_takes_its_radius_from_the_second_click() {
        let mut e = editor();
        e.set_tool(Tool::Circle);
        click(&mut e, 10.0, 10.0);
        e.pointer_move(at(&e, 13.0, 14.0));
        assert!(!e.scene().is_empty());
        click(&mut e, 13.0, 14.0);
        assert_eq!(ids(&e), 2);
        assert_eq!(
            e.sketch.element(ElementId(2)),
            Some(&Element::Circle {
                center: ElementId(1),
                radius: mm(5.0)
            })
        );
        assert_eq!(e.report.dof, Some(3), "a centre and a radius");
    }

    #[test]
    fn an_arc_ends_on_its_circle_in_the_direction_of_the_third_click() {
        let mut e = editor();
        e.set_tool(Tool::Arc);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 10.0, 0.0);
        click(&mut e, 0.0, 30.0);
        assert_eq!(ids(&e), 4);
        assert_eq!(
            point(&e, 3),
            (mm(0.0), mm(10.0)),
            "30 mm up is brought to the radius of 10"
        );
        assert!(matches!(
            e.sketch.element(ElementId(4)),
            Some(Element::Arc { .. })
        ));
    }

    #[test]
    fn construction_mode_draws_construction_geometry_and_the_selection_toggles_it() {
        let mut e = editor();
        e.construction_mode = true;
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 10.0, 10.0);
        assert!(e.sketch.elements()[2].construction);
        assert!(
            !e.sketch.elements()[0].construction,
            "points are not construction"
        );
        e.construction_mode = false;
        pick(&mut e, &[3]);
        e.toggle_construction();
        assert!(!e.sketch.elements()[2].construction);
        e.toggle_construction();
        assert!(e.sketch.elements()[2].construction);
        let strokes: Vec<Stroke> = e
            .scene()
            .iter()
            .filter_map(|p| match p {
                Prim::Segment { stroke, .. } => Some(*stroke),
                _ => None,
            })
            .collect();
        assert!(strokes.contains(&Stroke::Selected) || strokes.contains(&Stroke::Construction));
        e.toggle_construction();
        e.picked = Picked::default();
        e.toggle_construction();
        assert_eq!(e.status, Status::Problem(Msg::SelectFirst, Vec::new()));
    }

    #[test]
    fn a_click_picks_a_point_before_a_line_and_the_extend_key_adds_and_takes_away() {
        let mut e = editor();
        rectangle(&mut e);
        e.select_at(at(&e, 0.2, 0.1), false);
        assert_eq!(
            e.picked.elements,
            vec![ElementId(1)],
            "the corner, not the sides"
        );
        e.select_at(at(&e, 20.0, 0.1), true);
        assert_eq!(e.picked.elements, vec![ElementId(1), ElementId(5)]);
        e.select_at(at(&e, 0.0, 0.0), true);
        assert_eq!(e.picked.elements, vec![ElementId(5)]);
        e.select_at(at(&e, 20.0, 12.0), false);
        assert!(e.picked.is_empty());
        e.select_all();
        assert_eq!(e.picked.elements.len(), 8);
        e.select_constraint(ConstraintId(2), false);
        assert_eq!(e.picked.constraints, vec![ConstraintId(2)]);
        assert!(e.picked.elements.is_empty());
        e.select_element(ElementId(3), true);
        e.select_element(ElementId(3), true);
        assert_eq!(
            e.picked.elements.len(),
            0,
            "an extended pick of the same element takes it away"
        );
    }

    #[test]
    fn a_circle_is_picked_on_its_outline_and_an_arc_only_inside_its_sweep() {
        let mut e = editor();
        e.set_tool(Tool::Circle);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 10.0, 0.0);
        e.set_tool(Tool::Arc);
        click(&mut e, 40.0, 0.0);
        click(&mut e, 50.0, 0.0);
        click(&mut e, 40.0, 10.0);
        e.set_tool(Tool::Select);
        e.select_at(at(&e, 0.0, 10.2), false);
        assert_eq!(e.picked.elements, vec![ElementId(2)]);
        e.select_at(at(&e, 0.0, 3.0), false);
        assert!(e.picked.is_empty(), "inside the circle is not on it");
        // the arc runs from the east to the north: its east and north are picked, its south is not
        e.select_at(at(&e, 50.0, 0.0), false);
        assert_eq!(e.picked.elements, vec![ElementId(4)], "the start point");
        let on = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
        e.select_at(at(&e, 40.0 + on, on), false);
        assert_eq!(e.picked.elements, vec![ElementId(6)]);
        e.select_at(at(&e, 40.0, -10.2), false);
        assert!(
            e.picked.is_empty(),
            "the south of the circle of the arc is not on the arc"
        );
    }

    #[test]
    fn the_constraints_offered_are_the_ones_the_selection_allows() {
        let mut e = editor();
        rectangle(&mut e);
        // two sides: ids of the lines are 5, 6, 7, 8
        pick(&mut e, &[5, 6]);
        let offered = e.available();
        assert!(offered.contains(&ConstraintTool::Parallel));
        assert!(offered.contains(&ConstraintTool::Perpendicular));
        assert!(offered.contains(&ConstraintTool::Equal));
        assert!(offered.contains(&ConstraintTool::Horizontal));
        assert!(!offered.contains(&ConstraintTool::Tangent));
        assert!(!offered.contains(&ConstraintTool::Coincident));
        pick(&mut e, &[1, 3]);
        let offered = e.available();
        assert!(offered.contains(&ConstraintTool::Coincident));
        assert!(offered.contains(&ConstraintTool::Horizontal));
        assert!(offered.contains(&ConstraintTool::Vertical));
        assert!(offered.contains(&ConstraintTool::Fixed));
        assert!(!offered.contains(&ConstraintTool::Parallel));
        pick(&mut e, &[1, 5]);
        let offered = e.available();
        assert!(offered.contains(&ConstraintTool::PointOnObject));
        assert!(offered.contains(&ConstraintTool::Midpoint));
        pick(&mut e, &[]);
        assert!(e.available().is_empty());
        assert!(e.dimensions_available().is_empty());
        pick(&mut e, &[5]);
        assert_eq!(e.dimensions_available(), vec![DimTool::Length]);
        pick(&mut e, &[1, 3]);
        assert_eq!(
            e.dimensions_available(),
            vec![DimTool::Distance, DimTool::DistanceX, DimTool::DistanceY]
        );
        pick(&mut e, &[5, 6]);
        assert_eq!(e.dimensions_available(), vec![DimTool::Angle]);
    }

    #[test]
    fn a_geometric_constraint_is_added_to_the_selection_and_solved() {
        let mut e = editor();
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 20.0, 5.0);
        click(&mut e, 0.0, 10.0);
        click(&mut e, 20.0, 20.0);
        e.set_tool(Tool::Select);
        pick(&mut e, &[3, 6]);
        e.add_constraint(ConstraintTool::Parallel);
        assert_eq!(
            e.sketch.constraints().last().unwrap().constraint,
            Constraint::Parallel(ElementId(3), ElementId(6))
        );
        assert_eq!(e.report.health, Health::Under);
        assert_eq!(e.report.dof, Some(7), "eight coordinates less one");
        assert_eq!(e.picked.constraints.len(), 1);
        // a selection that does not fit is refused and nothing changes
        let before = e.sketch.clone();
        pick(&mut e, &[1]);
        e.add_constraint(ConstraintTool::Parallel);
        assert_eq!(e.sketch, before);
        assert_eq!(
            e.status.text(Lang::En).split(' ').next(),
            Some("PHOENIX-CAD-032")
        );
        // two points held where they are
        pick(&mut e, &[1, 2]);
        e.add_constraint(ConstraintTool::Fixed);
        assert_eq!(e.sketch.constraints().len(), 3);
        assert_eq!(e.report.dof, Some(3));
    }

    #[test]
    fn a_dimension_takes_the_value_it_has_and_is_edited_with_a_value_or_a_variable() {
        let mut e = editor();
        rectangle(&mut e);
        pick(&mut e, &[1]);
        e.add_constraint(ConstraintTool::Fixed);
        pick(&mut e, &[5]);
        e.add_dimension(DimTool::Length);
        let row = e.constraint_rows().pop().unwrap();
        assert_eq!(row.label, Msg::DiLength);
        assert_eq!(row.value.as_deref(), Some("40"));
        assert_eq!(row.edit.as_deref(), Some("40"));
        assert_eq!(
            e.report.dof,
            Some(1),
            "four, less two held and one measured"
        );
        // a number is a length in millimetres and stays a plain value
        let id = row.id;
        e.edit_dimension(id, "50");
        assert_eq!(
            point(&e, 2).0,
            mm(50.0),
            "the solver moved the right side to 50 mm"
        );
        let row = e.constraint_rows().pop().unwrap();
        assert_eq!(row.value.as_deref(), Some("50"));
        assert_eq!(row.edit.as_deref(), Some("50"));
        // a unit and a formula
        e.edit_dimension(id, "1.5 in");
        assert_eq!(
            e.constraint_rows().pop().unwrap().value.as_deref(),
            Some("38.1")
        );
        // a variable makes it follow the variable
        let mut vars = Variables::new();
        vars.set("width", mm(30.0)).unwrap();
        e.set_variables(&vars);
        e.edit_dimension(id, "width / 2 + 5 mm");
        let row = e.constraint_rows().pop().unwrap();
        assert_eq!(row.value.as_deref(), Some("20"));
        assert_eq!(row.edit.as_deref(), Some("width / 2 + 5 mm"));
        assert_eq!(point(&e, 2).0, mm(20.0));
        vars.set("width", mm(60.0)).unwrap();
        e.set_variables(&vars);
        assert_eq!(
            e.constraint_rows().pop().unwrap().value.as_deref(),
            Some("35")
        );
        assert_eq!(point(&e, 2).0, mm(35.0));
        // a text the model refuses leaves everything as it was
        let before = e.sketch.clone();
        for bad in ["nothing", "-3", "0", "width +"] {
            e.edit_dimension(id, bad);
            assert_eq!(e.sketch, before, "{bad}");
            assert!(e.status.is_problem(), "{bad}");
        }
        e.edit_dimension(id, "  ");
        assert_eq!(e.status, Status::Problem(Msg::ValueNeeded, Vec::new()));
    }

    #[test]
    fn a_dimension_can_be_typed_before_it_is_added_and_an_angle_is_in_degrees() {
        let mut e = editor();
        rectangle(&mut e);
        pick(&mut e, &[1]);
        e.add_constraint(ConstraintTool::Fixed);
        pick(&mut e, &[1, 3]);
        e.dim_input = "50".to_owned();
        e.add_dimension(DimTool::DistanceX);
        assert_eq!(point(&e, 3).0, mm(50.0));
        assert!(e.dim_input.is_empty());
        // the angle between the bottom and the right side is a quarter turn
        pick(&mut e, &[5, 6]);
        e.add_dimension(DimTool::Angle);
        let row = e.constraint_rows().pop().unwrap();
        assert_eq!(row.value.as_deref(), Some("90"));
        assert_eq!(
            e.report.health,
            Health::Over,
            "the rectangle is already square"
        );
        assert!(e.report.redundant.contains(&row.id), "{:?}", e.report);
        e.edit_dimension(row.id, "45");
        assert_eq!(e.report.tone(), Tone::Red, "now it contradicts the sides");
        assert!(matches!(e.report.health, Health::Over | Health::Failed));
        assert!(e.report.redundant.is_empty());
        // the way back is to switch the angle off
        e.set_enabled(row.id, false);
        assert_eq!(e.report.health, Health::Under);
        assert_eq!(
            e.report.dof,
            Some(1),
            "the rectangle is held at a corner and measured along one side"
        );
        assert_eq!(degrees_text(Angle::from_deg(90).unwrap()), "90");
        assert_eq!(degrees_text(Angle::new(12_345_000_000).unwrap()), "12.345");
        assert_eq!(degrees_text(Angle::new(-500_000_000).unwrap()), "-0.5");
    }

    #[test]
    fn a_rectangle_held_and_measured_is_fully_constrained() {
        let mut e = editor();
        rectangle(&mut e);
        pick(&mut e, &[1]);
        e.add_constraint(ConstraintTool::Fixed);
        assert_eq!(e.report.dof, Some(2));
        pick(&mut e, &[5]);
        e.add_dimension(DimTool::Length);
        pick(&mut e, &[6]);
        e.add_dimension(DimTool::Length);
        assert_eq!(e.report.health, Health::Full);
        assert_eq!(e.report.tone(), Tone::Green);
        assert_eq!(e.state_text(), "Fully constrained");
        assert_eq!(e.report.dof, Some(0));
    }

    #[test]
    fn constraints_that_contradict_or_repeat_are_kept_listed_and_can_be_switched_off() {
        let mut e = editor();
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 30.0, 40.0);
        e.set_tool(Tool::Select);
        pick(&mut e, &[3]);
        e.add_dimension(DimTool::Length);
        assert_eq!(
            e.constraint_rows()[0].value.as_deref(),
            Some("50"),
            "3-4-5 times ten"
        );
        let first = e.constraint_rows()[0].id;
        // the same length again: it repeats the first
        pick(&mut e, &[3]);
        e.add_dimension(DimTool::Length);
        assert_eq!(e.report.health, Health::Over);
        assert_eq!(e.report.tone(), Tone::Red);
        let second = e.constraint_rows()[1].id;
        assert_eq!(
            e.report.redundant,
            vec![first, second],
            "it names the one it repeats"
        );
        assert!(e.report.conflicting.is_empty());
        assert_eq!(e.constraint_rows()[1].mark, RowMark::Redundant);
        assert_eq!(e.problem_lines(), vec!["Redundant: c1 c2".to_owned()]);
        // a different length contradicts it
        e.edit_dimension(second, "60");
        assert_eq!(e.report.conflicting, vec![first, second]);
        assert_eq!(e.constraint_rows()[1].mark, RowMark::Conflict);
        assert_eq!(e.problem_lines(), vec!["In conflict: c1 c2".to_owned()]);
        let stroke = e.scene().into_iter().find_map(|p| match p {
            Prim::Segment {
                stroke: Stroke::Conflict,
                ..
            } => Some(()),
            _ => None,
        });
        assert!(
            stroke.is_some(),
            "what the constraint is about is drawn in the colour of a conflict"
        );
        // the sketch was not moved while it was over-constrained
        assert_eq!(point(&e, 2), (mm(30.0), mm(40.0)));
        e.disable_problems();
        assert_eq!(e.report.health, Health::Under);
        assert_eq!(e.report.dof, Some(3));
        assert!(!e.constraint_rows()[1].enabled);
        assert!(e.constraint_rows()[0].enabled);
        assert_eq!(
            e.status,
            Status::Done(Msg::ProblemsDisabled, vec!["1".into()])
        );
        e.set_enabled(second, true);
        assert_eq!(e.report.health, Health::Over);
        e.undo();
        assert_eq!(e.report.health, Health::Under);
    }

    #[test]
    fn dragging_a_corner_solves_at_every_move_and_is_one_step_that_undo_takes_back() {
        let mut e = editor();
        rectangle(&mut e);
        let drawn = e.sketch.clone();
        let corner = at(&e, 40.0, 25.0);
        assert!(e.begin_drag(corner));
        e.drag_to(at(&e, 45.0, 28.0));
        assert_eq!(point(&e, 3), (mm(45.0), mm(28.0)));
        assert_eq!(
            point(&e, 2),
            (mm(45.0), mm(0.0)),
            "the right side stays plumb"
        );
        assert_eq!(point(&e, 4), (mm(0.0), mm(28.0)), "the top stays level");
        e.drag_to(at(&e, 50.0, 30.0));
        assert!(e.is_dragging());
        e.end_drag();
        assert!(!e.is_dragging());
        assert_eq!(point(&e, 3), (mm(50.0), mm(30.0)));
        assert_eq!(e.history.undo_names(), vec!["Rectangle", "Move"]);
        e.undo();
        assert_eq!(e.sketch, drawn);
        e.redo();
        assert_eq!(point(&e, 3), (mm(50.0), mm(30.0)));
        // a click that never moved is no change
        let mut e = editor();
        rectangle(&mut e);
        assert!(e.begin_drag(at(&e, 40.0, 25.0)));
        e.end_drag();
        assert_eq!(e.history.undo_names(), vec!["Rectangle"]);
        // nothing to drag away from the drawing
        assert!(!e.begin_drag(at(&e, 20.0, 12.5)));
        e.drag_to(at(&e, 1.0, 1.0));
        e.end_drag();
    }

    #[test]
    fn a_drag_cannot_move_what_the_constraints_hold_and_dragging_a_side_moves_both_ends() {
        let mut e = editor();
        rectangle(&mut e);
        pick(&mut e, &[1]);
        e.add_constraint(ConstraintTool::Fixed);
        let held = e.sketch.clone();
        assert!(e.begin_drag(at(&e, 0.0, 0.0)));
        e.drag_to(at(&e, 5.0, 5.0));
        e.end_drag();
        assert_eq!(e.sketch, held, "a point that is held stays");
        // the top side up 10 mm: its two corners go up and the verticals follow
        e.picked = Picked::default();
        assert!(e.begin_drag(at(&e, 20.0, 25.0)));
        e.drag_to(at(&e, 20.0, 35.0));
        e.end_drag();
        assert_eq!(point(&e, 3), (mm(40.0), mm(35.0)));
        assert_eq!(point(&e, 4), (mm(0.0), mm(35.0)));
        assert_eq!(point(&e, 2), (mm(40.0), mm(0.0)));
        // Escape while dragging puts everything back
        assert!(e.begin_drag(at(&e, 40.0, 35.0)));
        e.drag_to(at(&e, 60.0, 50.0));
        e.cancel();
        assert!(!e.is_dragging());
        assert_eq!(point(&e, 3), (mm(40.0), mm(35.0)));
    }

    #[test]
    fn a_point_dropped_on_another_point_becomes_coincident_with_it() {
        let mut e = editor();
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 10.0, 20.0);
        click(&mut e, 30.0, 0.0);
        click(&mut e, 40.0, 20.0);
        e.set_tool(Tool::Select);
        assert!(e.begin_drag(at(&e, 30.0, 0.0)));
        e.drag_to(at(&e, 10.3, 20.2));
        e.end_drag();
        assert_eq!(
            point(&e, 4),
            point(&e, 2),
            "dropped exactly on the other point"
        );
        assert_eq!(
            e.sketch.constraints().last().unwrap().constraint,
            Constraint::Coincident(ElementId(4), ElementId(2))
        );
        assert_eq!(e.report.dof, Some(6), "eight coordinates less two");
        assert_eq!(e.history.undo_names(), vec!["Line", "Line", "Move"]);
        // a point does not snap to itself or to what is dragged with it
        e.undo();
        assert!(e.sketch.constraints().is_empty());
    }

    #[test]
    fn a_drag_with_the_solver_off_moves_nothing_until_it_is_dropped() {
        let mut e = editor();
        e.ui.live_drag = false;
        rectangle(&mut e);
        assert!(e.begin_drag(at(&e, 40.0, 25.0)));
        e.drag_to(at(&e, 45.0, 28.0));
        assert_eq!(point(&e, 3), (mm(40.0), mm(25.0)));
        e.end_drag();
        assert_eq!(e.history.undo_names(), vec!["Rectangle"]);
    }

    #[test]
    fn deleting_takes_what_depends_on_the_selection() {
        let mut e = editor();
        rectangle(&mut e);
        pick(&mut e, &[1]);
        e.delete_selection();
        // the corner, its two sides and their constraints go
        assert_eq!(ids(&e), 5);
        assert_eq!(e.sketch.constraints().len(), 2);
        assert_eq!(
            e.status,
            Status::Done(Msg::ElementsDeleted, vec!["5".into()])
        );
        e.undo();
        assert_eq!(ids(&e), 8);
        assert_eq!(e.sketch.constraints().len(), 4);
        // only a constraint selected: only it goes
        e.select_constraint(ConstraintId(1), false);
        e.delete_selection();
        assert_eq!(ids(&e), 8);
        assert_eq!(e.sketch.constraints().len(), 3);
        e.picked = Picked::default();
        e.delete_selection();
        assert_eq!(e.status, Status::Problem(Msg::SelectFirst, Vec::new()));
    }

    #[test]
    fn the_history_forgets_what_was_undone_when_something_new_is_done_and_keeps_its_depth() {
        let mut e = editor();
        e.ui.undo_depth = 2;
        e.history = History::new(2);
        e.set_tool(Tool::Point);
        click(&mut e, 1.0, 1.0);
        click(&mut e, 2.0, 2.0);
        click(&mut e, 3.0, 3.0);
        assert_eq!(e.history.undo_names().len(), 2);
        e.undo();
        assert!(e.can_redo());
        click(&mut e, 4.0, 4.0);
        assert!(!e.can_redo());
        e.undo();
        e.undo();
        e.undo();
        assert_eq!(e.status, Status::Done(Msg::NothingToUndo, Vec::new()));
        assert_eq!(ids(&e), 1, "the first point was past the depth");
        // what was selected and is gone is no longer selected
        e.redo();
        pick(&mut e, &[1, 2]);
        e.undo();
        assert_eq!(e.picked.elements, vec![ElementId(1)]);
    }

    #[test]
    fn the_sketch_is_saved_next_to_the_project_and_read_back_the_same() {
        let dir =
            std::env::temp_dir().join(format!("phoenix-cad-sketch-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = editor();
        rectangle(&mut e);
        pick(&mut e, &[5]);
        e.add_dimension(DimTool::Length);
        let project = dir.join("caja.phoenixproj");
        let path = file::sibling_path(&project);
        assert!(path.to_string_lossy().ends_with("caja.phoenixsketch"));
        assert!(e.save_to(&path));
        assert!(!e.dirty);
        assert_eq!(
            e.status,
            Status::Done(Msg::Saved, vec!["caja.phoenixsketch".into()])
        );
        assert!(e.save());
        let mut other = editor();
        assert!(other.open_file(&path));
        assert_eq!(other.sketch, e.sketch);
        assert_eq!(other.report, e.report);
        assert!(!other.can_undo());
        assert!(!other.dirty);
        assert_eq!(other.path.as_deref(), Some(path.as_path()));
        // a file that is not a sketch leaves the sketch that was open as it was
        let bad = dir.join("bad.phoenixsketch");
        std::fs::write(&bad, "phoenix-sketch 9\n").unwrap();
        let before = other.sketch.clone();
        assert!(!other.open_file(&bad));
        assert_eq!(other.sketch, before);
        assert_eq!(
            other.status.text(Lang::En).split(' ').next(),
            Some("PHOENIX-CAD-035")
        );
        assert!(!other.open_file(&dir.join("missing.phoenixsketch")));
        other.new_sketch();
        assert_eq!(ids(&other), 0);
        assert!(other.path.is_none());
        assert!(!other.save(), "a sketch with no file asks for a name");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_scene_has_the_grid_the_axes_the_geometry_the_signs_and_the_dimensions() {
        let mut e = editor();
        rectangle(&mut e);
        pick(&mut e, &[5]);
        e.add_dimension(DimTool::Length);
        e.picked = Picked::default();
        let scene = e.scene();
        let count = |s: Stroke| {
            scene
                .iter()
                .filter(|p| match p {
                    Prim::Segment { stroke, .. }
                    | Prim::Polyline { stroke, .. }
                    | Prim::Dot { stroke, .. }
                    | Prim::Ring { stroke, .. }
                    | Prim::Text { stroke, .. } => *stroke == s,
                })
                .count()
        };
        assert_eq!(count(Stroke::Axis), 2);
        assert!(count(Stroke::GridMinor) > 100);
        assert!(count(Stroke::GridMajor) > 4);
        assert_eq!(count(Stroke::Normal), 4 + 4, "four sides and four corners");
        assert_eq!(count(Stroke::Marker), 4, "H V H V");
        let texts: Vec<&str> = scene
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts.iter().filter(|t| **t == "H").count(), 2);
        assert!(texts.contains(&"40"));
        // an axis is where the origin is on the canvas
        let axis_x: Vec<f64> = scene
            .iter()
            .filter_map(|p| match p {
                Prim::Segment {
                    a,
                    stroke: Stroke::Axis,
                    b,
                } if a.0 == b.0 => Some(a.0),
                _ => None,
            })
            .collect();
        assert_eq!(axis_x, vec![400.0]);
        // the grid is not drawn when its lines would crowd each other
        e.view.scale = 0.5;
        let far = e.scene();
        let minor = far
            .iter()
            .filter(|p| {
                matches!(
                    p,
                    Prim::Segment {
                        stroke: Stroke::GridMinor,
                        ..
                    }
                )
            })
            .count();
        assert!(minor <= 200, "{minor}");
    }

    #[test]
    fn what_is_being_drawn_and_what_a_click_would_snap_to_are_in_the_scene() {
        let mut e = editor();
        e.set_tool(Tool::Rectangle);
        click(&mut e, 0.0, 0.0);
        e.pointer_move(at(&e, 20.0, 10.0));
        let preview = e
            .scene()
            .into_iter()
            .filter(|p| {
                matches!(
                    p,
                    Prim::Polyline {
                        stroke: Stroke::Preview,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(preview, 1);
        e.set_tool(Tool::Line);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 10.0, 10.0);
        e.set_tool(Tool::Line);
        click(&mut e, 5.0, 5.0);
        e.pointer_move(at(&e, 10.0, 10.1));
        let scene = e.scene();
        assert!(scene.iter().any(|p| matches!(p, Prim::Ring { .. })));
        assert!(scene.iter().any(|p| matches!(
            p,
            Prim::Segment {
                stroke: Stroke::Preview,
                ..
            }
        )));
        e.pointer_leave();
        assert!(e.hover.is_none());
        // Escape drops the drawing, then the selection
        e.cancel();
        assert!(!e.is_drawing());
    }

    #[test]
    fn the_view_is_fitted_to_the_drawing() {
        let mut e = editor();
        e.fit();
        assert_eq!((e.view.center, e.view.scale), ((0.0, 0.0), 8.0));
        e.set_tool(Tool::Rectangle);
        click(&mut e, 0.0, 0.0);
        click(&mut e, 40.0, 25.0);
        e.fit();
        assert_eq!(e.view.center, (20.0, 12.5));
        // 800 by 600 with 40 px of margin: 720 / 40 = 18 and 520 / 25 = 20.8
        assert!((e.view.scale - 18.0).abs() < 1e-9, "{}", e.view.scale);
    }

    #[test]
    fn the_lists_say_what_is_there() {
        let mut e = editor();
        e.set_tool(Tool::Circle);
        click(&mut e, 2.0, 3.0);
        click(&mut e, 7.0, 3.0);
        pick(&mut e, &[2]);
        let rows = e.element_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].label, rows[0].detail.as_str()),
            (Msg::ElPoint, "(2, 3)")
        );
        assert_eq!(
            (rows[1].label, rows[1].detail.as_str()),
            (Msg::ElCircle, "e1, R 5")
        );
        assert!(rows[1].selected && !rows[0].selected);
        e.add_dimension(DimTool::Diameter);
        let row = e.constraint_rows().pop().unwrap();
        assert_eq!(
            (row.label, row.value.as_deref()),
            (Msg::DiDiameter, Some("10"))
        );
        assert_eq!(row.elements, "e2");
        let en = Msg::DiDiameter.tr(Lang::En);
        let es = Msg::DiDiameter.tr(Lang::Es);
        assert_ne!(en, es);
    }

    #[test]
    fn every_tool_and_kind_has_words_in_the_eight_languages() {
        for t in Tool::ALL {
            for l in Lang::ALL {
                assert!(!t.msg().tr(l).is_empty() && !t.hint().tr(l).is_empty());
            }
        }
        for t in ConstraintTool::ALL {
            for l in Lang::ALL {
                assert!(!t.msg().tr(l).is_empty());
            }
        }
        for t in DimTool::ALL {
            for l in Lang::ALL {
                assert!(!t.msg().tr(l).is_empty());
            }
        }
        assert_eq!(
            Msg::StateUnder.tr(Lang::Pt),
            "{0} graus de liberdade por fixar"
        );
    }

    #[test]
    fn the_example_sketch_opens_fully_constrained_and_is_written_back_the_same() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("box.phoenixsketch");
        let mut e = editor();
        assert!(e.open_file(&path), "{:?}", e.status);
        assert_eq!(e.sketch.elements().len(), 12);
        assert_eq!(e.sketch.constraints().len(), 13);
        assert_eq!(e.report.health, Health::Full);
        assert_eq!(e.report.tone(), Tone::Green);
        assert_eq!(e.state_text(), "Fully constrained");
        // the plate is 100 by 60 mm and the holes sit 10 mm from its corners
        assert_eq!(point(&e, 3), (mm(100.0), mm(60.0)));
        assert_eq!(point(&e, 9), (mm(10.0), mm(10.0)));
        assert_eq!(point(&e, 11), (mm(90.0), mm(50.0)));
        assert_eq!(
            e.sketch.element(ElementId(12)),
            Some(&Element::Circle {
                center: ElementId(11),
                radius: mm(2.5)
            })
        );
        // the file is the text the program writes
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(file::to_text(&e.sketch), text);
        // dragging a hole is held by its dimensions; the plate does not move either
        let before = e.sketch.clone();
        assert!(e.begin_drag(at(&e, 10.0, 10.0)));
        e.drag_to(at(&e, 30.0, 30.0));
        e.end_drag();
        assert_eq!(e.sketch, before);
        // freeing a dimension lets the hole move and the state goes amber
        e.set_enabled(ConstraintId(9), false);
        assert_eq!(e.report.health, Health::Under);
        assert_eq!(e.report.dof, Some(1));
        let window = SketchEditor::with_defaults(Lang::Pt);
        assert_eq!(window.state_text(), "Esboço vazio");
    }

    #[test]
    fn the_distance_helpers_are_exact_on_a_three_four_five() {
        assert_eq!(dist((0.0, 0.0), (3.0, 4.0)), 5.0);
        let (d, near) = segment_distance((5.0, 3.0), (0.0, 0.0), (10.0, 0.0));
        assert_eq!((d, near), (3.0, (5.0, 0.0)));
        let (d, near) = segment_distance((-4.0, 3.0), (0.0, 0.0), (10.0, 0.0));
        assert_eq!((d, near), (5.0, (0.0, 0.0)));
        let (d, near) = circle_distance((0.0, 13.0), (0.0, 0.0), 5.0).unwrap();
        assert_eq!((d, near), (8.0, (0.0, 5.0)));
        assert!(circle_distance((1.0, 1.0), (1.0, 1.0), 5.0).is_none());
        assert!(on_arc((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)));
        assert!(!on_arc((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, -1.0)));
        assert_eq!(
            near_axis((0.0, 0.0), (10.0, 0.4), 3.0),
            Some(Axis::Horizontal)
        );
        assert_eq!(near_axis((0.0, 0.0), (10.0, 10.0), 3.0), None);
        assert_eq!(
            near_axis((0.0, 0.0), (-0.2, -9.0), 3.0),
            Some(Axis::Vertical)
        );
        assert!(to_len(f64::NAN).is_err());
        assert!(to_len(1e12).is_err());
        assert_eq!(to_len(1.5).unwrap().get(), 1_500_000);
    }
}
