//! The state of the window and everything it decides without a screen: the project that is open,
//! the tree of the model, the fields of the properties, what an edit does, the history of the
//! changes, the views and the texts of the status bar. The sketches of the project (kept in its file
//! and used by the extrusions and pockets) are part of the recipe it edits.
//!
//! The window (`gui`) only draws this state and passes on what the person does. An edit is a
//! transaction: the new recipe is built first and the change is kept only when the model accepts
//! it, so a refused value leaves the project exactly as it was.

use crate::assembly::{Dim, Edge, Mate, MateKind, Report};
use crate::body::{Built, Step, StepState};
use crate::catalog::{self, Axis, Kind as PartKind, Library, Orientation, PartInstance, Shape};
use crate::config::Settings;
use crate::drag::{self, Drawn, Plane};
use crate::enclosure::{
    Edging, Enclosure, EnclosureParams, Face, Hole, HoleStyle, Id, Operation, Treatment,
};
use crate::export::{self, Format};
use crate::expr::{parse, Constants, Names, Value};
use crate::fins::{FinAxis, FinSet};
use crate::fit::{check_fit, Envelope, FitReport};
use crate::history::History;
use crate::lang::{fill, Lang, Msg};
use crate::mass::mass_micrograms;
use crate::measure::{self, Geometry, Label, Measurement, Quantity, Reference};
use crate::mechanical::{Mechanical, MECHANICAL_DOMAIN};
use crate::messages;
use crate::mount::{Lid, Post};
use crate::parametric::{ParamModel, Parameter};
use crate::pattern::{Layout, PatternOp, Seed};
use crate::profile::ProfileLimits;
use crate::project::{Document, PARAMETERS_DOMAIN};
use crate::scad::mm_text;
use crate::scene::{
    face_plane, floor_plane, mm, outer_size, post_centre, project_with, references, scene_with,
    BoardBox, Camera, Fit, Frame, Highlight, Kind, Line2, Mapping, Point3, Segment,
};
use crate::sketch::file as sketch_file;
use crate::sketch::settings::SketchSettings;
use crate::sketch::{check_sketch_name, Sketch};
use crate::store;
use crate::uiconfig::UiSettings;
use crate::units::{format_angle, format_length, parse_angle, parse_length, Count};
use crate::vars::{Param, Variables};
use crate::{CadError, Length};
use std::path::{Path, PathBuf};

/// The extensions of the project files that the window opens and writes.
pub const PROJECT_EXTENSIONS: [&str; 1] = ["phoenixproj"];

/// What is selected in the tree of the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Nothing.
    Nothing,
    /// The dimensions of the box.
    Body,
    /// A hole of a face: the index of its step.
    Hole(usize),
    /// A post: the index of its step.
    Post(usize),
    /// The treatment of the edges: the index of its step.
    Edging(usize),
    /// The fixing of the lid: the index of its step.
    Lid(usize),
    /// An extrusion or a pocket: the index of its step.
    Feature(usize),
    /// A pattern of holes or posts: the index of its step.
    Pattern(usize),
    /// A set of cooling fins: the index of its step.
    Fins(usize),
    /// A part of the catalog placed in the model: the index of its step.
    Part(usize),
    /// A mate between two placed parts: the index of its step.
    Mate(usize),
    /// A part of the open library, which is not in the model: its place in the library.
    CatalogPart(usize),
    /// A step that has no editor of its own: its index.
    Other(usize),
    /// A screw hole of the lid, by its position in the list the lid derives.
    Screw(usize),
    /// The envelope that is checked against the box.
    Fit,
    /// The sheet of variables.
    Sheet,
}

/// A step that the person can add from the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewStep {
    /// A hole in a face.
    Hole,
    /// A post on the floor.
    Post,
    /// The treatment of the edges.
    Edging,
    /// An extrusion of a sketch of the project.
    Extrude,
    /// A pocket of a sketch of the project.
    Pocket,
    /// A pattern of holes in a face.
    HolePattern,
    /// A pattern of posts on the floor.
    PostPattern,
    /// A set of cooling fins on a face.
    Fins,
    /// The part of the open library that is chosen, placed in the model.
    Part,
    /// A mate between the selected part and the one before it, of the kind the settings say.
    Mate,
}

/// A row of the sheet of variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRow {
    /// The name of the variable.
    pub name: String,
    /// The value as text, in the unit of the settings.
    pub text: String,
    /// The value that was calculated.
    pub value: Length,
    /// The parameters of the box that depend on the variable, directly or through other
    /// parameters, in the order of the fields.
    pub used_by: Vec<Param>,
}

/// The names an expression of a variable sees: the other variables, then the constants.
struct VariableNames<'a> {
    vars: &'a Variables,
    constants: &'a Constants,
}

impl Names for VariableNames<'_> {
    fn lookup(&self, name: &str) -> Option<Value> {
        self.vars
            .get(name)
            .map(Value::from_length)
            .or_else(|| self.constants.get(name))
    }
}

impl Selection {
    /// The index of the step that this selection is, when it is a step.
    pub fn step(self) -> Option<usize> {
        match self {
            Selection::Hole(i)
            | Selection::Post(i)
            | Selection::Edging(i)
            | Selection::Lid(i)
            | Selection::Feature(i)
            | Selection::Pattern(i)
            | Selection::Fins(i)
            | Selection::Part(i)
            | Selection::Mate(i)
            | Selection::Other(i) => Some(i),
            Selection::Nothing
            | Selection::Body
            | Selection::CatalogPart(_)
            | Selection::Screw(_)
            | Selection::Fit
            | Selection::Sheet => None,
        }
    }
}

/// What a field of the properties edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A parameter of the box.
    Param(Param),
    /// The name of a hole or a post.
    Name,
    /// The face of a hole.
    Face,
    /// The position `u` of a hole.
    U,
    /// The position `v` of a hole.
    V,
    /// The diameter of a hole.
    Diameter,
    /// The position `x` of a post.
    PostX,
    /// The position `y` of a post.
    PostY,
    /// The diameter of a post.
    PostOuter,
    /// The height of a post.
    PostHeight,
    /// The diameter of the bore of a post.
    BoreDiameter,
    /// The depth of the bore of a post.
    BoreDepth,
    /// The treatment of one group of edges (0 vertical, 1 lid, 2 floor).
    EdgeKind(usize),
    /// The size of the treatment of one group of edges.
    EdgeSize(usize),
    /// The diameter of the screw holes of the lid.
    LidHole,
    /// The clearance of the lip of the lid.
    LidClearance,
    /// The thickness of the lip.
    LipThickness,
    /// The depth of the lip.
    LipDepth,
    /// One side of the envelope that is checked (0 length, 1 width, 2 height).
    Fit(usize),
    /// The sketch an extrusion or a pocket is made from.
    Sketch,
    /// The height of an extrusion.
    Height,
    /// The depth of a pocket, or that it goes through.
    PocketDepth,
    /// How a pattern is laid out: along a line or round a centre.
    PatternLayout,
    /// How many positions a pattern has.
    PatternCount,
    /// The step of a line of positions along `u` (or `x`).
    StepU,
    /// The step of a line of positions along `v` (or `y`).
    StepV,
    /// The centre of a circle of positions, along `u` (or `x`).
    CenterU,
    /// The centre of a circle of positions, along `v` (or `y`).
    CenterV,
    /// The angle between two positions of a circle.
    StepAngle,
    /// The direction the fins of a set run.
    FinDirection,
    /// How many fins a set has.
    FinCount,
    /// The thickness of a fin.
    FinThickness,
    /// The height of a fin.
    FinHeight,
    /// The distance from the middle of one fin to the middle of the next.
    FinPitch,
    /// Where the middle of the first fin is, across the direction the fins run.
    FinFirst,
    /// Where the fins start, along the direction they run.
    FinStart,
    /// The length of a fin.
    FinLength,
    /// The surface of the fins and the face between them (read only).
    FinArea,
    /// The estimate of the heat the fins give to still air (read only).
    FinPower,
    /// The library and the part a placed part came from (read only).
    PartLibrary,
    /// The kind of a part (read only).
    PartKind,
    /// The material of a part (read only).
    PartMaterial,
    /// The position `x` of a placed part.
    PartX,
    /// The position `y` of a placed part.
    PartY,
    /// The position `z` of a placed part.
    PartZ,
    /// The direction the length of a placed part runs along.
    PartDirection,
    /// The roll of a placed part about its length, in degrees.
    PartRoll,
    /// The length a placed profile is cut to.
    PartCut,
    /// The size of a part in the model (read only).
    PartSize,
    /// The volume of the material of a part (read only).
    PartVolume,
    /// The mass of a part (read only).
    PartMass,
    /// The part a mate is relative to (typed as its name).
    MateA,
    /// The part a mate places (typed as its name).
    MateB,
    /// The kind of a mate.
    MateKind,
    /// The side of the fixed part that a gap puts the placed part on.
    MateDirection,
    /// The air between the two faces of a gap.
    MateDistance,
    /// The axis across the plane of an alignment.
    MateAxis,
    /// The faces an alignment puts on one plane.
    MateEdge,
    /// Where a part really is once its mates are applied (read only).
    PartPlaced,
    /// The parts that share volume with a part (read only).
    PartConflicts,
}

/// How a field is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Typed text.
    Text,
    /// One of a list: the value that is kept and the words that name it.
    Choice(Vec<(&'static str, Msg)>),
}

/// A field of the panel of properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// What it edits.
    pub key: Key,
    /// The words that name it.
    pub label: Msg,
    /// The value as text.
    pub text: String,
    /// What the value calculates to, when it is a formula (empty otherwise).
    pub note: String,
    /// How it is edited.
    pub input: Input,
    /// Whether it can be changed.
    pub editable: bool,
}

/// How a step of the tree stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// It was applied.
    Normal,
    /// It is suppressed.
    Suppressed,
    /// The model refused it.
    Failed,
    /// It is after the tip.
    AfterTip,
}

/// A line of the tree of the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeItem {
    /// What it selects.
    pub selection: Selection,
    /// The name shown.
    pub label: String,
    /// How it stands.
    pub mark: Mark,
}

/// A group of the tree of the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeGroup {
    /// The words that name the group.
    pub title: Msg,
    /// Its lines.
    pub items: Vec<TreeItem>,
}

/// What the status bar says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Nothing happened yet.
    Ready,
    /// Something was done: the sentence and its values.
    Done(Msg, Vec<String>),
    /// Something was refused: the sentence and its values.
    Problem(Msg, Vec<String>),
    /// The library refused something: its error.
    Error(CadError),
}

/// The text of an error in a language: the code, the title in that language and, when the
/// library gave one, the detail it wrote (in English).
pub fn error_text(error: &CadError, lang: Lang) -> String {
    let code = error.code();
    let title = messages::message(code, lang.code()).unwrap_or("");
    let full = error.to_string();
    match full.split_once(": ") {
        Some((_, detail)) if !detail.is_empty() => format!("{code} {title} ({detail})"),
        _ => format!("{code} {title}"),
    }
}

impl Status {
    /// The text in a language.
    pub fn text(&self, lang: Lang) -> String {
        match self {
            Status::Ready => Msg::Ready.tr(lang).to_string(),
            Status::Done(message, values) | Status::Problem(message, values) => {
                let values: Vec<&str> = values.iter().map(String::as_str).collect();
                fill(message.tr(lang), &values)
            }
            Status::Error(error) => error_text(error, lang),
        }
    }

    /// Whether it is a refusal.
    pub fn is_problem(&self) -> bool {
        matches!(self, Status::Problem(..) | Status::Error(_))
    }
}

/// What the status bar measures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// The outer size of the box.
    pub outer: [Length; 3],
    /// The inner size.
    pub inner: [Length; 3],
    /// The volume of the material of the empty box, in cubic nanometres, plus the extrusions and
    /// minus the pockets.
    pub material_nm3: i128,
    /// How many steps the model refused.
    pub failed: usize,
    /// Whether the envelope fits, and the room left.
    pub fit: FitReport,
    /// Whether the envelope is the board of the project (otherwise it is the one typed).
    pub fit_from_board: bool,
    /// Which placed parts share volume (`None` when the check could not be made).
    pub interference: Option<Report>,
}

/// The two views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// The 3D view.
    Iso,
    /// The plan.
    Plan,
}

/// The point of view of one view and what the mouse does to it.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewControl {
    /// The point of view.
    pub camera: Camera,
    /// The zoom over the size that fits the pane.
    pub zoom: f64,
    /// The movement of the drawing, in pixels.
    pub pan: (f64, f64),
    home: Camera,
    zoom_min: f64,
    zoom_max: f64,
}

impl ViewControl {
    /// A view that starts at `camera` and zooms between the two limits.
    pub fn new(camera: Camera, zoom_min: f64, zoom_max: f64) -> ViewControl {
        ViewControl {
            camera,
            zoom: 1.0,
            pan: (0.0, 0.0),
            home: camera,
            zoom_min,
            zoom_max,
        }
    }

    /// Turns the view for a drag of `dx` and `dy` pixels at `speed` degrees for each pixel.
    pub fn rotate(&mut self, dx: f64, dy: f64, speed: f64) {
        let yaw = self.camera.yaw + dx * speed;
        self.camera.yaw = (yaw + 180.0).rem_euclid(360.0) - 180.0;
        self.camera.pitch = (self.camera.pitch + dy * speed).clamp(0.0, 90.0);
    }

    /// Multiplies the zoom by `factor`, inside the limits.
    pub fn zoom_by(&mut self, factor: f64) {
        self.zoom = (self.zoom * factor).clamp(self.zoom_min, self.zoom_max);
    }

    /// Moves the drawing by `dx` and `dy` pixels.
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        self.pan = (self.pan.0 + dx, self.pan.1 + dy);
    }

    /// Goes back to the point of view the view started at.
    pub fn reset(&mut self) {
        self.camera = self.home;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
    }
}

/// What a click in the views does besides turning and moving them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureTool {
    /// Nothing is measured: a press on a hole or a post drags it.
    Off,
    /// The distance between two points or edges.
    Distance,
    /// The angle between two edges or between three points.
    Angle,
}

/// What a pick may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    Any,
    Points,
    Edges,
}

/// A hole or a post that is being dragged.
#[derive(Debug, Clone)]
struct Drag {
    /// The view the pointer is in.
    view: ViewKind,
    /// The index of its step.
    step: usize,
    /// Its name.
    name: String,
    /// The recipe when the drag started, which a cancel goes back to.
    before: Mechanical,
    /// Whether the project had unsaved changes when the drag started.
    was_dirty: bool,
    /// The plane its position is a point of.
    plane: Plane,
    /// How far its centre was from the pointer when it was picked, on the plane.
    offset: (f64, f64),
    /// Whether it has moved from where it was picked.
    moved: bool,
}

/// A hole or a post that can be picked.
struct Candidate {
    step: usize,
    name: String,
    plane: Plane,
    /// The position on the plane.
    centre: (f64, f64),
    /// Where its centre is drawn.
    at: Point3,
    is_hole: bool,
}

/// The window: the project and everything the person sees of it.
#[derive(Debug, Clone)]
pub struct Window {
    /// The language of the texts.
    pub lang: Lang,
    /// The settings of the library.
    pub settings: Settings,
    /// The settings of the window.
    pub ui: UiSettings,
    /// The file the project was opened from or saved to.
    pub path: Option<PathBuf>,
    /// True when the project has a part this program does not understand: it is not edited.
    pub read_only: bool,
    /// True when the project has changes that are not saved.
    pub dirty: bool,
    /// The recipe of the box.
    pub mech: Mechanical,
    /// What is selected in the tree.
    pub selection: Selection,
    /// The fields of the properties of the selection.
    pub fields: Vec<Field>,
    /// What the status bar says.
    pub status: Status,
    /// The 3D view.
    pub view3d: ViewControl,
    /// The plan.
    pub plan: ViewControl,
    /// What the status bar measures.
    pub info: Info,
    /// The envelope checked when the project has no board.
    pub manual: [Length; 3],
    /// The settings of the sketches: the limits of their outlines and of the domain of the file.
    pub sketch_settings: SketchSettings,
    /// The sketch the next extrusion or pocket is made from (the first one when it is none).
    pub active_sketch: Option<String>,
    /// Whether a dragged hole or post snaps to the grid of the settings.
    pub snap_to_grid: bool,
    /// The measurement tool that is on.
    pub measure_tool: MeasureTool,
    /// The format the export dialog is on.
    pub export_format: Format,
    /// Whether the box to write over an existing file is ticked.
    pub export_overwrite: bool,
    /// The library of parts that is open, when one is.
    pub library: Option<Library>,
    /// The part of the library the next part added is made from (its place in the library).
    pub catalog_choice: usize,
    measurements: Vec<Measurement>,
    pending: Vec<Reference>,
    drag: Option<Drag>,
    document: Document,
    history: History<Mechanical>,
    built: Built,
}

fn param_msg(param: Param) -> Msg {
    match param {
        Param::InnerLength => Msg::FieldInnerLength,
        Param::InnerWidth => Msg::FieldInnerWidth,
        Param::InnerHeight => Msg::FieldInnerHeight,
        Param::Wall => Msg::FieldWall,
        Param::Floor => Msg::FieldFloor,
        Param::Lid => Msg::FieldLid,
        Param::EdgeMargin => Msg::FieldEdgeMargin,
        Param::MinWeb => Msg::FieldMinWeb,
    }
}

/// The words that name a format of export.
pub fn format_msg(format: Format) -> Msg {
    match format {
        Format::Script => Msg::ExportScript,
        Format::Step => Msg::ExportStep,
        Format::Report => Msg::ExportReport,
        Format::Bill => Msg::ExportBill,
    }
}

/// The word that names a kind of part.
pub fn kind_msg(kind: PartKind) -> Msg {
    match kind {
        PartKind::Profile => Msg::KindProfile,
        PartKind::Screw => Msg::KindScrew,
        PartKind::Bracket => Msg::KindBracket,
    }
}

/// The words that name a face.
pub fn face_msg(face: Face) -> Msg {
    match face {
        Face::Front => Msg::FaceFront,
        Face::Back => Msg::FaceBack,
        Face::Left => Msg::FaceLeft,
        Face::Right => Msg::FaceRight,
        Face::Top => Msg::FaceTop,
        Face::Bottom => Msg::FaceBottom,
    }
}

fn face_choices() -> Vec<(&'static str, Msg)> {
    Face::ALL.iter().map(|f| (f.name(), face_msg(*f))).collect()
}

fn edge_choices() -> Vec<(&'static str, Msg)> {
    vec![
        ("sharp", Msg::EdgeSharp),
        ("round", Msg::EdgeRound),
        ("chamfer", Msg::EdgeChamfer),
    ]
}

fn treatment_name(t: Treatment) -> &'static str {
    match t {
        Treatment::Sharp => "sharp",
        Treatment::Round(_) => "round",
        Treatment::Chamfer(_) => "chamfer",
    }
}

fn edge_at(edging: &Edging, index: usize) -> Treatment {
    match index {
        0 => edging.vertical,
        1 => edging.top,
        _ => edging.bottom,
    }
}

fn set_edge(edging: &mut Edging, index: usize, treatment: Treatment) {
    match index {
        0 => edging.vertical = treatment,
        1 => edging.top = treatment,
        _ => edging.bottom = treatment,
    }
}

fn edge_msg(index: usize) -> Msg {
    match index {
        0 => Msg::FieldVertical,
        1 => Msg::FieldTopEdges,
        _ => Msg::FieldBottomEdges,
    }
}

/// The name a step is shown with: its own, or the one of what it adds.
pub fn step_label(step: &Step) -> String {
    if !step.name.is_empty() {
        return step.name.clone();
    }
    match &step.op {
        Operation::AddHole { hole, .. } | Operation::ReplaceHole { hole, .. } => hole.name.clone(),
        Operation::AddPost { post, .. } | Operation::ReplacePost { post, .. } => post.name.clone(),
        Operation::AddPattern { pattern, .. } => pattern.name().to_string(),
        Operation::AddFins { fins, .. } => fins.name.clone(),
        Operation::AddPart { part, .. } => part.name.clone(),
        _ => "-".to_string(),
    }
}

fn the_selection_of(index: usize, step: &Step) -> Selection {
    match &step.op {
        Operation::AddHole { .. } => Selection::Hole(index),
        Operation::AddPost { .. } => Selection::Post(index),
        Operation::SetEdging(_) => Selection::Edging(index),
        Operation::SetLid(Some(_)) => Selection::Lid(index),
        Operation::Extrude { .. } | Operation::Pocket { .. } => Selection::Feature(index),
        Operation::AddPattern { .. } => Selection::Pattern(index),
        Operation::AddFins { .. } => Selection::Fins(index),
        Operation::AddPart { .. } => Selection::Part(index),
        Operation::AddMate { .. } => Selection::Mate(index),
        _ => Selection::Other(index),
    }
}

fn mark_of(state: Option<&StepState>) -> Mark {
    match state {
        Some(StepState::Suppressed) => Mark::Suppressed,
        Some(StepState::Failed(_)) => Mark::Failed,
        Some(StepState::AfterTip) => Mark::AfterTip,
        Some(StepState::Valid) | None => Mark::Normal,
    }
}

fn refusal() -> CadError {
    CadError::ValueOutOfRange("field")
}

impl Window {
    /// A window with a new project: the box of the settings, no steps.
    pub fn new(lang: Lang, settings: Settings, ui: UiSettings) -> Result<Window, CadError> {
        let mech = Mechanical::new(ParamModel::new(ui.new_box)?, Variables::new());
        let built = mech.build(&settings.constants, &settings.expr)?;
        let manual = ui.fit;
        let mut window = Window {
            lang,
            view3d: ViewControl::new(
                Camera {
                    yaw: ui.yaw,
                    pitch: ui.pitch,
                },
                ui.zoom_min,
                ui.zoom_max,
            ),
            plan: ViewControl::new(Camera::TOP, ui.zoom_min, ui.zoom_max),
            snap_to_grid: ui.drag.snap,
            measure_tool: MeasureTool::Off,
            export_format: ui.export.format,
            export_overwrite: ui.export.overwrite,
            library: None,
            catalog_choice: 0,
            measurements: Vec::new(),
            pending: Vec::new(),
            drag: None,
            history: History::new(settings.undo_depth),
            info: Info {
                outer: [Length::ZERO; 3],
                inner: [Length::ZERO; 3],
                material_nm3: 0,
                failed: 0,
                fit: check_fit(
                    &built.enclosure,
                    &Envelope {
                        length: manual[0],
                        width: manual[1],
                        height: manual[2],
                    },
                    Length::ZERO,
                ),
                fit_from_board: false,
                interference: None,
            },
            settings,
            ui,
            path: None,
            read_only: false,
            dirty: false,
            mech,
            selection: Selection::Body,
            fields: Vec::new(),
            status: Status::Ready,
            manual,
            sketch_settings: SketchSettings::defaults(),
            active_sketch: None,
            document: Document::new(),
            built,
        };
        window.refresh();
        Ok(window)
    }

    /// Starts a new project with the box of the settings.
    pub fn new_project(&mut self) {
        let fresh = Mechanical::new(
            match ParamModel::new(self.ui.new_box) {
                Ok(model) => model,
                Err(e) => {
                    self.status = Status::Error(e);
                    return;
                }
            },
            Variables::new(),
        );
        let built = match fresh.build(&self.settings.constants, &self.settings.expr) {
            Ok(built) => built,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        self.mech = fresh;
        self.built = built;
        self.document = Document::new();
        self.path = None;
        self.read_only = false;
        self.dirty = false;
        self.history.clear();
        self.selection = Selection::Body;
        self.manual = self.ui.fit;
        self.status = Status::Done(Msg::NewProject, Vec::new());
        self.refresh();
    }

    /// The built model.
    pub fn enclosure(&self) -> &Enclosure {
        &self.built.enclosure
    }

    /// What the model did with each step.
    pub fn states(&self) -> &[StepState] {
        &self.built.states
    }

    /// The parameters of the box as calculated.
    pub fn params(&self) -> EnclosureParams {
        *self.built.enclosure.params()
    }

    /// Whether a change can be undone.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether a change can be redone.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The names of the changes that can be undone, the oldest first.
    pub fn undo_names(&self) -> Vec<&str> {
        self.history.undo_names()
    }

    /// The names of the changes that can be redone, the next one last.
    pub fn redo_names(&self) -> Vec<&str> {
        self.history.redo_names()
    }

    fn unit_text(&self, length: Length) -> String {
        format_length(length, self.settings.unit, self.settings.decimals)
    }

    /// A length as typed: with its unit, or without one, in the unit of the settings.
    pub fn parse_text(&self, text: &str) -> Result<Length, CadError> {
        let t = text.trim();
        if t.chars().any(char::is_alphabetic) {
            parse_length(t)
        } else {
            parse_length(&format!("{t} {}", self.settings.unit.suffix()))
        }
    }

    /// Builds a recipe with the settings of the window, the limits of the profiles of the sketches
    /// among them.
    fn rebuild(&self, mech: &Mechanical) -> Result<Built, CadError> {
        mech.build_with_limits(
            &self.settings.constants,
            &self.settings.expr,
            Some(ProfileLimits::from_settings(&self.sketch_settings)),
            Some(self.settings.design),
        )
    }

    /// Changes the settings of the sketches and builds the project again with them.
    pub fn set_sketch_settings(&mut self, settings: SketchSettings) {
        self.sketch_settings = settings;
        if let Ok(built) = self.rebuild(&self.mech) {
            self.built = built;
            self.refresh();
        }
    }

    // -----------------------------------------------------------------------------------------
    // The tree and the selection
    // -----------------------------------------------------------------------------------------

    /// The tree of the model.
    pub fn tree(&self) -> Vec<TreeGroup> {
        let lang = self.lang;
        let mut holes = Vec::new();
        let mut posts = Vec::new();
        let mut patterns = Vec::new();
        let mut fins = Vec::new();
        let mut parts = Vec::new();
        let mut mates = Vec::new();
        let mut edges = Vec::new();
        let mut lids = Vec::new();
        let mut features = Vec::new();
        let mut others = Vec::new();
        for (i, step) in self.mech.steps.iter().enumerate() {
            let selection = the_selection_of(i, step);
            let item = TreeItem {
                selection,
                label: step_label(step),
                mark: mark_of(self.built.states.get(i)),
            };
            match selection {
                Selection::Hole(_) => holes.push(item),
                Selection::Post(_) => posts.push(item),
                Selection::Pattern(_) => patterns.push(item),
                Selection::Fins(_) => fins.push(item),
                Selection::Part(_) => parts.push(item),
                Selection::Mate(_) => mates.push(item),
                Selection::Edging(_) => edges.push(item),
                Selection::Lid(_) => lids.push(item),
                Selection::Feature(_) => features.push(item),
                _ => others.push(item),
            }
        }
        let screws: Vec<TreeItem> = self
            .built
            .enclosure
            .lid_holes()
            .iter()
            .enumerate()
            .map(|(k, hole)| TreeItem {
                selection: Selection::Screw(k),
                label: hole.name.clone(),
                mark: Mark::Normal,
            })
            .collect();
        let library: Vec<TreeItem> = self
            .library
            .as_ref()
            .map(|l| {
                l.parts
                    .iter()
                    .enumerate()
                    .map(|(k, p)| TreeItem {
                        selection: Selection::CatalogPart(k),
                        label: p.name.clone(),
                        mark: Mark::Normal,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut groups = vec![TreeGroup {
            title: Msg::GroupBox,
            items: vec![TreeItem {
                selection: Selection::Body,
                label: Msg::Dimensions.tr(lang).to_string(),
                mark: Mark::Normal,
            }],
        }];
        groups.push(TreeGroup {
            title: Msg::GroupVariables,
            items: vec![TreeItem {
                selection: Selection::Sheet,
                label: format!("{} ({})", Msg::SheetTitle.tr(lang), self.mech.vars.len()),
                mark: Mark::Normal,
            }],
        });
        for (title, items) in [
            (Msg::GroupHoles, holes),
            (Msg::GroupPosts, posts),
            (Msg::GroupPatterns, patterns),
            (Msg::GroupFins, fins),
            (Msg::GroupParts, parts),
            (Msg::GroupMates, mates),
            (Msg::GroupEdges, edges),
            (Msg::GroupLid, lids),
            (Msg::GroupScrews, screws),
            (Msg::GroupFeatures, features),
            (Msg::GroupOther, others),
            (Msg::GroupLibrary, library),
        ] {
            if !items.is_empty() {
                groups.push(TreeGroup { title, items });
            }
        }
        groups.push(TreeGroup {
            title: Msg::GroupFit,
            items: vec![TreeItem {
                selection: Selection::Fit,
                label: Msg::Envelope.tr(lang).to_string(),
                mark: Mark::Normal,
            }],
        });
        groups
    }

    /// Selects an item of the tree. A step that does not exist selects the box.
    pub fn select(&mut self, selection: Selection) {
        let valid = match selection {
            Selection::Hole(i)
            | Selection::Post(i)
            | Selection::Edging(i)
            | Selection::Lid(i)
            | Selection::Feature(i)
            | Selection::Pattern(i)
            | Selection::Fins(i)
            | Selection::Part(i)
            | Selection::Mate(i)
            | Selection::Other(i) => i < self.mech.steps.len(),
            Selection::CatalogPart(k) => k < self.library.as_ref().map_or(0, |l| l.parts.len()),
            Selection::Screw(k) => k < self.built.enclosure.lid_holes().len(),
            Selection::Nothing | Selection::Body | Selection::Fit | Selection::Sheet => true,
        };
        self.selection = if valid { selection } else { Selection::Body };
        if let (true, Selection::CatalogPart(k)) = (valid, selection) {
            self.catalog_choice = k;
        }
        self.refresh_fields();
    }

    /// What is drawn in the colour of the selection.
    pub fn highlight(&self) -> Highlight {
        let id_of = |i: usize| -> Highlight {
            match self.mech.steps.get(i).map(|s| &s.op) {
                Some(Operation::AddHole { id, .. })
                | Some(Operation::AddPost { id, .. })
                | Some(Operation::Extrude { id, .. })
                | Some(Operation::Pocket { id, .. })
                | Some(Operation::AddFins { id, .. })
                | Some(Operation::AddPart { id, .. }) => Highlight::Object(*id),
                Some(Operation::AddMate { mate, .. }) => Highlight::Object(mate.b),
                Some(Operation::AddPattern { id, .. }) => Highlight::Pattern(*id),
                _ => Highlight::Nothing,
            }
        };
        match self.selection {
            Selection::Hole(i)
            | Selection::Post(i)
            | Selection::Feature(i)
            | Selection::Pattern(i)
            | Selection::Fins(i)
            | Selection::Part(i)
            | Selection::Mate(i) => id_of(i),
            Selection::Screw(k) => Highlight::Screw(k),
            _ => Highlight::Nothing,
        }
    }

    /// How the model treated the selected step.
    pub fn selected_state(&self) -> Option<&StepState> {
        self.selection.step().and_then(|i| self.built.states.get(i))
    }

    /// Whether the selected step is suppressed.
    pub fn selected_suppressed(&self) -> bool {
        self.selection
            .step()
            .and_then(|i| self.mech.steps.get(i))
            .is_some_and(|s| s.suppressed)
    }

    // -----------------------------------------------------------------------------------------
    // Fields
    // -----------------------------------------------------------------------------------------

    fn length_field(&self, key: Key, label: Msg, value: Length, editable: bool) -> Field {
        Field {
            key,
            label,
            text: self.unit_text(value),
            note: String::new(),
            input: Input::Text,
            editable,
        }
    }

    fn sketch_field(&self, sketch: &str, editable: bool) -> Field {
        Field {
            key: Key::Sketch,
            label: Msg::FieldSketch,
            text: sketch.to_string(),
            note: String::new(),
            input: Input::Text,
            editable,
        }
    }

    /// The depth of a pocket as typed: a length, or empty or the word for through.
    fn parse_depth(&self, text: &str) -> Result<Option<Length>, CadError> {
        let t = text.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("through") || t == Msg::Through.tr(self.lang) {
            Ok(None)
        } else {
            self.parse_text(t).map(Some)
        }
    }

    fn make_fields(&self) -> Vec<Field> {
        let writable = !self.read_only;
        let step = self
            .selection
            .step()
            .and_then(|i| self.mech.steps.get(i).map(|s| &s.op));
        match (self.selection, step) {
            (Selection::Body, _) => Param::ALL
                .iter()
                .map(|param| {
                    let resolved = self.unit_text(param.get(self.built.enclosure.params()));
                    let (text, note) = match self.mech.model.get(*param) {
                        Some(Parameter::Fixed(length)) => (self.unit_text(*length), String::new()),
                        Some(Parameter::Variable(name)) => (name.clone(), resolved),
                        Some(Parameter::Expr(expr)) => (expr.to_text(), resolved),
                        None => (
                            self.unit_text(param.get(self.mech.model.base())),
                            String::new(),
                        ),
                    };
                    Field {
                        key: Key::Param(*param),
                        label: param_msg(*param),
                        text,
                        note,
                        input: Input::Text,
                        editable: writable,
                    }
                })
                .collect(),
            (Selection::Hole(_), Some(Operation::AddHole { hole, .. })) => vec![
                Field {
                    key: Key::Name,
                    label: Msg::FieldName,
                    text: hole.name.clone(),
                    note: String::new(),
                    input: Input::Text,
                    editable: writable,
                },
                Field {
                    key: Key::Face,
                    label: Msg::FieldFace,
                    text: hole.face.name().to_string(),
                    note: String::new(),
                    input: Input::Choice(face_choices()),
                    editable: writable,
                },
                self.length_field(Key::U, Msg::FieldU, hole.u, writable),
                self.length_field(Key::V, Msg::FieldV, hole.v, writable),
                self.length_field(Key::Diameter, Msg::FieldDiameter, hole.diameter, writable),
            ],
            (Selection::Post(_), Some(Operation::AddPost { post, .. })) => vec![
                Field {
                    key: Key::Name,
                    label: Msg::FieldName,
                    text: post.name.clone(),
                    note: String::new(),
                    input: Input::Text,
                    editable: writable,
                },
                self.length_field(Key::PostX, Msg::FieldPostX, post.x, writable),
                self.length_field(Key::PostY, Msg::FieldPostY, post.y, writable),
                self.length_field(
                    Key::PostOuter,
                    Msg::FieldPostOuter,
                    post.outer_diameter,
                    writable,
                ),
                self.length_field(Key::PostHeight, Msg::FieldPostHeight, post.height, writable),
                self.length_field(
                    Key::BoreDiameter,
                    Msg::FieldBoreDiameter,
                    post.bore_diameter,
                    writable,
                ),
                self.length_field(
                    Key::BoreDepth,
                    Msg::FieldBoreDepth,
                    post.bore_depth,
                    writable,
                ),
            ],
            (Selection::Feature(_), Some(Operation::Extrude { sketch, height, .. })) => vec![
                self.sketch_field(sketch, writable),
                self.length_field(Key::Height, Msg::FieldHeight, *height, writable),
            ],
            (Selection::Feature(_), Some(Operation::Pocket { sketch, depth, .. })) => vec![
                self.sketch_field(sketch, writable),
                match depth {
                    Some(d) => {
                        self.length_field(Key::PocketDepth, Msg::FieldPocketDepth, *d, writable)
                    }
                    None => Field {
                        key: Key::PocketDepth,
                        label: Msg::FieldPocketDepth,
                        text: Msg::Through.tr(self.lang).to_string(),
                        note: String::new(),
                        input: Input::Text,
                        editable: writable,
                    },
                },
            ],
            (Selection::Pattern(_), Some(Operation::AddPattern { pattern, .. })) => {
                self.pattern_fields(pattern, writable)
            }
            (Selection::Fins(_), Some(Operation::AddFins { fins, .. })) => {
                self.fins_fields(fins, writable)
            }
            (Selection::Part(_), Some(Operation::AddPart { id, part })) => {
                self.part_fields(*id, part, writable)
            }
            (Selection::Mate(_), Some(Operation::AddMate { mate, .. })) => {
                self.mate_fields(mate, writable)
            }
            (Selection::CatalogPart(k), _) => self.catalog_fields(k),
            (Selection::Edging(_), Some(Operation::SetEdging(edging))) => {
                let mut out = Vec::new();
                for index in 0..3 {
                    let t = edge_at(edging, index);
                    out.push(Field {
                        key: Key::EdgeKind(index),
                        label: edge_msg(index),
                        text: treatment_name(t).to_string(),
                        note: String::new(),
                        input: Input::Choice(edge_choices()),
                        editable: writable,
                    });
                    out.push(self.length_field(
                        Key::EdgeSize(index),
                        Msg::FieldEdgeSize,
                        if t.is_sharp() {
                            self.ui.edge_size
                        } else {
                            t.size()
                        },
                        writable && !t.is_sharp(),
                    ));
                }
                out
            }
            (Selection::Lid(_), Some(Operation::SetLid(Some(lid)))) => {
                let mut out = vec![
                    self.length_field(Key::LidHole, Msg::FieldLidHole, lid.hole_diameter, writable),
                    self.length_field(
                        Key::LidClearance,
                        Msg::FieldClearance,
                        lid.clearance,
                        writable,
                    ),
                ];
                if let Some(lip) = &lid.lip {
                    out.push(self.length_field(
                        Key::LipThickness,
                        Msg::FieldLipThickness,
                        lip.thickness,
                        writable,
                    ));
                    out.push(self.length_field(
                        Key::LipDepth,
                        Msg::FieldLipDepth,
                        lip.depth,
                        writable,
                    ));
                }
                out
            }
            (Selection::Screw(k), _) => match self.built.enclosure.lid_holes().get(k) {
                Some(hole) => vec![
                    self.length_field(Key::U, Msg::FieldU, hole.u, false),
                    self.length_field(Key::V, Msg::FieldV, hole.v, false),
                    self.length_field(Key::Diameter, Msg::FieldDiameter, hole.diameter, false),
                ],
                None => Vec::new(),
            },
            (Selection::Fit, _) => {
                let (envelope, from_board) = self.envelope();
                [
                    (0, Msg::FieldFitLength, envelope.length),
                    (1, Msg::FieldFitWidth, envelope.width),
                    (2, Msg::FieldFitHeight, envelope.height),
                ]
                .into_iter()
                .map(|(k, label, value)| {
                    self.length_field(Key::Fit(k), label, value, !from_board && !self.read_only)
                })
                .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The fields of a pattern: the seed (the first hole or post), how the positions follow from
    /// it and how many there are.
    fn pattern_fields(&self, pattern: &PatternOp, writable: bool) -> Vec<Field> {
        let text_field = |key: Key, label: Msg, text: String| Field {
            key,
            label,
            text,
            note: String::new(),
            input: Input::Text,
            editable: writable,
        };
        let mut out = vec![text_field(
            Key::Name,
            Msg::FieldName,
            pattern.name().to_string(),
        )];
        match &pattern.seed {
            Seed::Hole(hole) => {
                out.push(Field {
                    key: Key::Face,
                    label: Msg::FieldFace,
                    text: hole.face.name().to_string(),
                    note: String::new(),
                    input: Input::Choice(face_choices()),
                    editable: writable,
                });
                out.push(self.length_field(Key::U, Msg::FieldU, hole.u, writable));
                out.push(self.length_field(Key::V, Msg::FieldV, hole.v, writable));
                out.push(self.length_field(
                    Key::Diameter,
                    Msg::FieldDiameter,
                    hole.diameter,
                    writable,
                ));
            }
            Seed::Post(post) => {
                for (key, label, value) in [
                    (Key::PostX, Msg::FieldPostX, post.x),
                    (Key::PostY, Msg::FieldPostY, post.y),
                    (Key::PostOuter, Msg::FieldPostOuter, post.outer_diameter),
                    (Key::PostHeight, Msg::FieldPostHeight, post.height),
                    (
                        Key::BoreDiameter,
                        Msg::FieldBoreDiameter,
                        post.bore_diameter,
                    ),
                    (Key::BoreDepth, Msg::FieldBoreDepth, post.bore_depth),
                ] {
                    out.push(self.length_field(key, label, value, writable));
                }
            }
        }
        let word = match pattern.layout {
            Layout::Linear { .. } => "linear",
            Layout::Circular { .. } => "circular",
        };
        out.push(Field {
            key: Key::PatternLayout,
            label: Msg::FieldLayout,
            text: word.to_string(),
            note: String::new(),
            input: Input::Choice(vec![
                ("linear", Msg::LayoutLinear),
                ("circular", Msg::LayoutCircular),
            ]),
            editable: writable,
        });
        out.push(text_field(
            Key::PatternCount,
            Msg::FieldCount,
            pattern.layout.count().get().to_string(),
        ));
        match pattern.layout {
            Layout::Linear { step, .. } => {
                out.push(self.length_field(Key::StepU, Msg::FieldStepU, step.0, writable));
                out.push(self.length_field(Key::StepV, Msg::FieldStepV, step.1, writable));
            }
            Layout::Circular { center, step, .. } => {
                out.push(self.length_field(Key::CenterU, Msg::FieldCenterU, center.0, writable));
                out.push(self.length_field(Key::CenterV, Msg::FieldCenterV, center.1, writable));
                out.push(text_field(
                    Key::StepAngle,
                    Msg::FieldStepAngle,
                    format_angle(step),
                ));
            }
        }
        out
    }

    /// The fields of a set of cooling fins, and the surface and the estimate of the heat, which
    /// are read only.
    fn fins_fields(&self, fins: &FinSet, writable: bool) -> Vec<Field> {
        let lang = self.lang;
        let plain = |key: Key, label: Msg, text: String, editable: bool, note: String| Field {
            key,
            label,
            text,
            note,
            input: Input::Text,
            editable,
        };
        let mut out = vec![
            plain(
                Key::Name,
                Msg::FieldName,
                fins.name.clone(),
                writable,
                String::new(),
            ),
            Field {
                key: Key::Face,
                label: Msg::FieldFace,
                text: fins.face.name().to_string(),
                note: String::new(),
                input: Input::Choice(face_choices()),
                editable: writable,
            },
            Field {
                key: Key::FinDirection,
                label: Msg::FieldFinDirection,
                text: fins.along.name().to_string(),
                note: String::new(),
                input: Input::Choice(vec![("u", Msg::FinRunsU), ("v", Msg::FinRunsV)]),
                editable: writable,
            },
            plain(
                Key::FinCount,
                Msg::FieldFinCount,
                fins.count.get().to_string(),
                writable,
                String::new(),
            ),
        ];
        for (key, label, value) in [
            (Key::FinThickness, Msg::FieldFinThickness, fins.thickness),
            (Key::FinHeight, Msg::FieldHeight, fins.height),
            (Key::FinPitch, Msg::FieldFinPitch, fins.pitch),
            (Key::FinFirst, Msg::FieldFinFirst, fins.first),
            (Key::FinStart, Msg::FieldFinStart, fins.start),
            (Key::FinLength, Msg::FieldFinLength, fins.length),
        ] {
            out.push(self.length_field(key, label, value, writable));
        }
        let thermal = fins.thermal(&self.settings.thermal);
        let temperature = self.settings.thermal.delta_t_k();
        out.push(plain(
            Key::FinArea,
            Msg::FieldFinArea,
            fill(
                Msg::FinAreaText.tr(lang),
                &[&format!("{:.1}", thermal.area_mm2 / 100.0)],
            ),
            false,
            String::new(),
        ));
        out.push(plain(
            Key::FinPower,
            Msg::FieldFinPower,
            fill(
                Msg::FinPowerText.tr(lang),
                &[
                    &format!("{:.2}", thermal.power_w),
                    &format!("{temperature}"),
                    &format!("{:.1}", thermal.power_w / thermal.flat_power_w),
                ],
            ),
            false,
            Msg::FinEstimateNote.tr(lang).to_string(),
        ));
        out
    }

    /// A read only field of text.
    fn info_field(key: Key, label: Msg, text: String) -> Field {
        Field {
            key,
            label,
            text,
            note: String::new(),
            input: Input::Text,
            editable: false,
        }
    }

    /// The size of a part in the model as text.
    fn size_text(&self, size: [i64; 3]) -> String {
        size.map(|v| self.unit_text(Length::new(v).unwrap_or(Length::ZERO)))
            .join(" x ")
    }

    /// The volume and the mass of a part as read only fields. The mass needs the density of the
    /// material in the settings; without it the field says so with a dash.
    fn material_fields(&self, volume: Result<i128, CadError>, material: &str) -> Vec<Field> {
        let volume = volume.ok();
        let volume_text = volume.map_or("-".to_string(), |v| {
            format!("{} mm3", catalog::volume_mm3_text(v))
        });
        let mass_text = match (volume, self.settings.materials.get(material)) {
            (Some(v), Some(density)) => mass_micrograms(v, *density).map_or("-".to_string(), |m| {
                format!("{} g", catalog::mass_g_text(m))
            }),
            _ => "-".to_string(),
        };
        vec![
            Self::info_field(Key::PartVolume, Msg::FieldPartVolume, volume_text),
            Self::info_field(Key::PartMass, Msg::FieldPartMass, mass_text),
        ]
    }

    /// The name of a placed part, or its id when it is not in the model.
    fn part_name(&self, id: &Id) -> String {
        self.built
            .enclosure
            .parts()
            .get(id)
            .map_or_else(|| id.to_string(), |p| p.name.clone())
    }

    /// The placed part with a name, refused when there is none or more than one.
    fn part_named(&self, name: &str) -> Result<Id, CadError> {
        let name = name.trim();
        let mut found = self
            .built
            .enclosure
            .parts()
            .iter()
            .filter(|(_, p)| p.name == name)
            .map(|(id, _)| *id);
        match (found.next(), found.next()) {
            (Some(id), None) => Ok(id),
            (Some(_), Some(_)) => Err(CadError::Duplicate(name.to_string())),
            _ => Err(CadError::UnknownName(name.to_string())),
        }
    }

    /// A position as text, in the unit of the window.
    fn position_text(&self, at: [Length; 3]) -> String {
        at.map(|v| self.unit_text(v)).join(", ")
    }

    /// The names of the parts that share volume with a part.
    fn partners_of(&self, id: Id) -> Vec<String> {
        let Some(report) = &self.info.interference else {
            return Vec::new();
        };
        report
            .conflicts
            .iter()
            .filter_map(|c| {
                if c.a == id {
                    Some(self.part_name(&c.b))
                } else if c.b == id {
                    Some(self.part_name(&c.a))
                } else {
                    None
                }
            })
            .collect()
    }

    /// The fields of a mate: the two parts, the kind and the values of the kind, and where the
    /// placed part is.
    fn mate_fields(&self, mate: &Mate, writable: bool) -> Vec<Field> {
        let text = |key: Key, label: Msg, text: String| Field {
            key,
            label,
            text,
            note: String::new(),
            input: Input::Text,
            editable: writable,
        };
        let choice = |key: Key, label: Msg, text: &str, list: Vec<(&'static str, Msg)>| Field {
            key,
            label,
            text: text.to_string(),
            note: String::new(),
            input: Input::Choice(list),
            editable: writable,
        };
        let mut out = vec![
            text(Key::MateA, Msg::FieldMateA, self.part_name(&mate.a)),
            text(Key::MateB, Msg::FieldMateB, self.part_name(&mate.b)),
            choice(
                Key::MateKind,
                Msg::FieldMateKind,
                mate.kind.name(),
                vec![
                    ("gap", Msg::MateGap),
                    ("align", Msg::MateAlign),
                    ("stack", Msg::MateStack),
                ],
            ),
        ];
        match mate.kind {
            MateKind::Gap {
                direction,
                distance,
            } => {
                out.push(text(
                    Key::MateDirection,
                    Msg::FieldMateDirection,
                    direction.name().to_string(),
                ));
                out.push(self.length_field(
                    Key::MateDistance,
                    Msg::FieldMateDistance,
                    distance,
                    writable,
                ));
            }
            MateKind::Align { axis, edge } => {
                out.push(text(
                    Key::MateAxis,
                    Msg::FieldMateAxis,
                    axis.name().to_string(),
                ));
                out.push(choice(
                    Key::MateEdge,
                    Msg::FieldMateEdge,
                    edge.name(),
                    vec![
                        ("min", Msg::MateEdgeMin),
                        ("max", Msg::MateEdgeMax),
                        ("center", Msg::MateEdgeCenter),
                    ],
                ));
            }
            MateKind::Stack => {}
        }
        if let Some(p) = self.built.enclosure.placed_parts().get(&mate.b) {
            out.push(Self::info_field(
                Key::PartPlaced,
                Msg::FieldPartPlaced,
                self.position_text(p.at),
            ));
        }
        out
    }

    /// A mate after an edit of the field `key`.
    fn edited_mate(&self, mate: &Mate, key: Key, text: &str) -> Result<Mate, CadError> {
        let mut m = *mate;
        let word = text.trim();
        match (key, &mut m.kind) {
            (Key::MateA, _) => m.a = self.part_named(text)?,
            (Key::MateB, _) => m.b = self.part_named(text)?,
            (Key::MateKind, kind) => {
                *kind = self
                    .ui
                    .new_step
                    .mate_of(word)
                    .ok_or_else(|| CadError::Reference(word.to_string()))?;
            }
            (Key::MateDirection, MateKind::Gap { direction, .. }) => {
                *direction =
                    Axis::from_name(word).ok_or_else(|| CadError::Reference(word.to_string()))?;
            }
            (Key::MateDistance, MateKind::Gap { distance, .. }) => {
                *distance = self.parse_text(text)?;
            }
            (Key::MateAxis, MateKind::Align { axis, .. }) => {
                *axis =
                    Dim::from_name(word).ok_or_else(|| CadError::Reference(word.to_string()))?;
            }
            (Key::MateEdge, MateKind::Align { edge, .. }) => {
                *edge =
                    Edge::from_name(word).ok_or_else(|| CadError::Reference(word.to_string()))?;
            }
            _ => return Err(refusal()),
        }
        Ok(m)
    }

    /// The fields of a part placed in the model.
    fn part_fields(&self, id: Id, part: &PartInstance, writable: bool) -> Vec<Field> {
        let lang = self.lang;
        let text = |key: Key, label: Msg, text: String| Field {
            key,
            label,
            text,
            note: String::new(),
            input: Input::Text,
            editable: writable,
        };
        let mut out = vec![
            text(Key::Name, Msg::FieldName, part.name.clone()),
            Self::info_field(
                Key::PartLibrary,
                Msg::FieldPartLibrary,
                format!("{} / {}", part.library, part.part.name),
            ),
            Self::info_field(
                Key::PartKind,
                Msg::FieldPartKind,
                kind_msg(part.part.kind()).tr(lang).to_string(),
            ),
            Self::info_field(
                Key::PartMaterial,
                Msg::FieldPartMaterial,
                part.part.material.clone(),
            ),
            self.length_field(Key::PartX, Msg::FieldPartX, part.at[0], writable),
            self.length_field(Key::PartY, Msg::FieldPartY, part.at[1], writable),
            self.length_field(Key::PartZ, Msg::FieldPartZ, part.at[2], writable),
            text(
                Key::PartDirection,
                Msg::FieldPartDirection,
                part.orientation.axis.name().to_string(),
            ),
            text(
                Key::PartRoll,
                Msg::FieldPartRoll,
                (u32::from(part.orientation.roll) * 90).to_string(),
            ),
        ];
        if let Some(cut) = part.cut {
            out.push(self.length_field(Key::PartCut, Msg::FieldPartCut, cut, writable));
        }
        out.push(Self::info_field(
            Key::PartSize,
            Msg::FieldPartSize,
            self.size_text(part.extent().0),
        ));
        // where the mates put it, when that is not where it was typed
        let placed = self.built.enclosure.placed_parts();
        if let Some(p) = placed.get(&id).filter(|p| p.at != part.at) {
            out.push(Self::info_field(
                Key::PartPlaced,
                Msg::FieldPartPlaced,
                self.position_text(p.at),
            ));
        }
        let partners = self.partners_of(id);
        if !partners.is_empty() {
            out.push(Self::info_field(
                Key::PartConflicts,
                Msg::FieldPartConflicts,
                partners.join(", "),
            ));
        }
        out.extend(self.material_fields(part.volume_nm3(), &part.part.material));
        out
    }

    /// The fields of a part of the open library, which are all read only: what it is made of and
    /// how big it is at its usual cut.
    fn catalog_fields(&self, index: usize) -> Vec<Field> {
        let Some(part) = self.library.as_ref().and_then(|l| l.parts.get(index)) else {
            return Vec::new();
        };
        let mut out = vec![
            Self::info_field(Key::Name, Msg::FieldName, part.name.clone()),
            Self::info_field(
                Key::PartKind,
                Msg::FieldPartKind,
                kind_msg(part.kind()).tr(self.lang).to_string(),
            ),
            Self::info_field(
                Key::PartMaterial,
                Msg::FieldPartMaterial,
                part.material.clone(),
            ),
            Self::info_field(
                Key::PartSize,
                Msg::FieldPartSize,
                self.size_text(part.size(None)),
            ),
        ];
        out.extend(self.material_fields(part.volume_nm3(None), &part.material));
        out
    }

    /// A part placed in the model after an edit of the field `key`.
    fn edited_part(
        &self,
        part: &PartInstance,
        key: Key,
        text: &str,
    ) -> Result<PartInstance, CadError> {
        let mut p = part.clone();
        match key {
            Key::Name => p.name = text.trim().to_string(),
            Key::PartX => p.at[0] = self.parse_text(text)?,
            Key::PartY => p.at[1] = self.parse_text(text)?,
            Key::PartZ => p.at[2] = self.parse_text(text)?,
            Key::PartDirection => {
                p.orientation.axis = Axis::from_name(text.trim())
                    .ok_or_else(|| CadError::Reference(text.trim().to_string()))?;
            }
            Key::PartRoll => {
                let degrees = text
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| CadError::ValueOutOfRange("roll"))?;
                if degrees % 90 != 0 || degrees > 270 {
                    return Err(CadError::ValueOutOfRange("roll"));
                }
                p.orientation.roll = u8::try_from(degrees / 90).unwrap_or(0);
            }
            Key::PartCut if p.cut.is_some() => p.cut = Some(self.parse_text(text)?),
            _ => return Err(refusal()),
        }
        Ok(p)
    }

    /// Opens a library of parts from a file. The materials of its parts have to be in the
    /// settings. When it cannot be opened the one that was open stays and the status bar says why.
    pub fn open_library(&mut self, path: &Path) -> bool {
        let opened = catalog::read_file(path, &self.settings.design.catalog).and_then(|library| {
            library
                .check_materials(&self.settings.materials)
                .map(|()| library)
        });
        match opened {
            Ok(library) => {
                self.status = Status::Done(
                    Msg::LibraryLoaded,
                    vec![library.name.clone(), library.parts.len().to_string()],
                );
                self.library = Some(library);
                self.catalog_choice = 0;
                self.refresh_fields();
                true
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// The bill of materials of the parts placed in the model.
    pub fn bill(&self) -> Result<catalog::Bill, CadError> {
        catalog::bill(
            self.built.enclosure.parts().values(),
            &self.settings.materials,
        )
    }

    /// The bill of materials as comma separated values.
    pub fn bill_csv_text(&self) -> Result<String, CadError> {
        self.bill().map(|bill| catalog::bill_csv(&bill))
    }

    fn refresh_fields(&mut self) {
        self.fields = self.make_fields();
    }

    fn refresh(&mut self) {
        self.info = self.measure();
        self.refresh_fields();
    }

    /// Sets the text of a field while it is typed. Nothing changes in the model until
    /// [`Window::commit`].
    pub fn type_in(&mut self, index: usize, text: String) {
        if let Some(field) = self.fields.get_mut(index) {
            field.text = text;
        }
    }

    /// Sets a field to one of its choices and commits it.
    pub fn choose(&mut self, index: usize, value: &str) {
        self.type_in(index, value.to_string());
        self.commit(index);
    }

    /// Commits the text of a field: the model is built again with it and the change is kept only
    /// when the model accepts it. A refused value is told in the status bar and the field goes
    /// back to the value it had.
    pub fn commit(&mut self, index: usize) {
        let Some(field) = self.fields.get(index).cloned() else {
            return;
        };
        if !field.editable || self.read_only {
            self.refresh_fields();
            return;
        }
        let label = field.label.tr(self.lang).to_string();
        let result = if let Key::Fit(k) = field.key {
            self.set_manual(k, &field.text)
        } else {
            let name = fill(Msg::ChangeOf.tr(self.lang), &[&label]);
            let done = Status::Done(Msg::ChangeOf, vec![label.clone()]);
            self.edited(field.key, &field.text)
                .and_then(|(next, step)| self.change(&name, done, next, step))
        };
        if let Err(e) = result {
            self.status = Status::Problem(Msg::Refused, vec![error_text(&e, self.lang)]);
        }
        self.refresh_fields();
    }

    fn set_manual(&mut self, index: usize, text: &str) -> Result<(), CadError> {
        let length = self.parse_text(text)?;
        if length.get() <= 0 {
            return Err(CadError::NotPositive("envelope"));
        }
        if let Some(slot) = self.manual.get_mut(index) {
            *slot = length;
        }
        self.refresh();
        Ok(())
    }

    /// The recipe with one field changed, and the step to check when the change is to a step.
    fn edited(&self, key: Key, text: &str) -> Result<(Mechanical, Option<usize>), CadError> {
        let mut next = self.mech.clone();
        if let (Selection::Body, Key::Param(param)) = (self.selection, key) {
            let t = text.trim();
            match self.parse_text(t) {
                Ok(length) => next.model.set(param, Parameter::Fixed(length))?,
                Err(_) => next.model.set_expr(param, t, &self.settings.expr)?,
            }
            return Ok((next, None));
        }
        let index = self.selection.step().ok_or_else(refusal)?;
        let step = self.mech.steps.get(index).ok_or_else(refusal)?;
        let op = self.edited_operation(&step.op, key, text)?;
        match next.steps.get_mut(index) {
            Some(slot) => slot.op = op,
            None => return Err(refusal()),
        }
        Ok((next, Some(index)))
    }

    fn edited_operation(
        &self,
        op: &Operation,
        key: Key,
        text: &str,
    ) -> Result<Operation, CadError> {
        match op {
            Operation::AddHole { id, hole } => Ok(Operation::AddHole {
                id: *id,
                hole: self.edited_hole(hole, key, text)?,
            }),
            Operation::AddPost { id, post } => Ok(Operation::AddPost {
                id: *id,
                post: self.edited_post(post, key, text)?,
            }),
            Operation::AddPattern { id, pattern } => Ok(Operation::AddPattern {
                id: *id,
                pattern: self.edited_pattern(pattern, key, text)?,
            }),
            Operation::AddFins { id, fins } => Ok(Operation::AddFins {
                id: *id,
                fins: self.edited_fins(fins, key, text)?,
            }),
            Operation::AddPart { id, part } => Ok(Operation::AddPart {
                id: *id,
                part: self.edited_part(part, key, text)?,
            }),
            Operation::AddMate { id, mate } => Ok(Operation::AddMate {
                id: *id,
                mate: self.edited_mate(mate, key, text)?,
            }),
            Operation::SetEdging(edging) => {
                Ok(Operation::SetEdging(self.edited_edging(edging, key, text)?))
            }
            Operation::SetLid(Some(lid)) => {
                Ok(Operation::SetLid(Some(self.edited_lid(lid, key, text)?)))
            }
            Operation::Extrude { id, sketch, height } => Ok(Operation::Extrude {
                id: *id,
                sketch: self.edited_sketch(sketch, key, text)?,
                height: if key == Key::Height {
                    self.parse_text(text)?
                } else {
                    *height
                },
            }),
            Operation::Pocket { id, sketch, depth } => Ok(Operation::Pocket {
                id: *id,
                sketch: self.edited_sketch(sketch, key, text)?,
                depth: if key == Key::PocketDepth {
                    self.parse_depth(text)?
                } else {
                    *depth
                },
            }),
            _ => Err(refusal()),
        }
    }

    /// A pattern after an edit of the field `key`: a field of the seed, or of the layout.
    fn edited_pattern(
        &self,
        pattern: &PatternOp,
        key: Key,
        text: &str,
    ) -> Result<PatternOp, CadError> {
        let mut p = pattern.clone();
        match key {
            Key::PatternLayout => p.layout = self.switched_layout(pattern, text.trim())?,
            Key::PatternCount => {
                let n = text
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| CadError::ValueOutOfRange("count"))?;
                let count = Count::new(n)?;
                match &mut p.layout {
                    Layout::Linear { count: c, .. } | Layout::Circular { count: c, .. } => {
                        *c = count;
                    }
                }
            }
            Key::StepU | Key::StepV => match &mut p.layout {
                Layout::Linear { step, .. } => {
                    let length = self.parse_text(text)?;
                    if key == Key::StepU {
                        step.0 = length;
                    } else {
                        step.1 = length;
                    }
                }
                Layout::Circular { .. } => return Err(refusal()),
            },
            Key::CenterU | Key::CenterV => match &mut p.layout {
                Layout::Circular { center, .. } => {
                    let length = self.parse_text(text)?;
                    if key == Key::CenterU {
                        center.0 = length;
                    } else {
                        center.1 = length;
                    }
                }
                Layout::Linear { .. } => return Err(refusal()),
            },
            Key::StepAngle => match &mut p.layout {
                Layout::Circular { step, .. } => *step = parse_angle(text)?,
                Layout::Linear { .. } => return Err(refusal()),
            },
            _ => {
                p.seed = match &pattern.seed {
                    Seed::Hole(hole) => Seed::Hole(self.edited_hole(hole, key, text)?),
                    Seed::Post(post) => Seed::Post(self.edited_post(post, key, text)?),
                }
            }
        }
        Ok(p)
    }

    /// A set of cooling fins after an edit of the field `key`.
    fn edited_fins(&self, fins: &FinSet, key: Key, text: &str) -> Result<FinSet, CadError> {
        let mut s = fins.clone();
        match key {
            Key::Name => s.name = text.trim().to_string(),
            Key::Face => {
                s.face = Face::from_name(text.trim())
                    .ok_or_else(|| CadError::Reference(text.trim().to_string()))?;
            }
            Key::FinDirection => {
                s.along = FinAxis::from_name(text.trim())
                    .ok_or_else(|| CadError::Reference(text.trim().to_string()))?;
            }
            Key::FinCount => {
                let n = text
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| CadError::ValueOutOfRange("count"))?;
                s.count = Count::new(n)?;
            }
            Key::FinThickness => s.thickness = self.parse_text(text)?,
            Key::FinHeight => s.height = self.parse_text(text)?,
            Key::FinPitch => s.pitch = self.parse_text(text)?,
            Key::FinFirst => s.first = self.parse_text(text)?,
            Key::FinStart => s.start = self.parse_text(text)?,
            Key::FinLength => s.length = self.parse_text(text)?,
            _ => return Err(refusal()),
        }
        Ok(s)
    }

    /// The layout a pattern gets when it is changed to `kind` (`linear` or `circular`): a line
    /// starts with the step of the settings, a circle with its centre the radius of the settings
    /// away from the seed along both axes and the angle of the settings. The count stays.
    fn switched_layout(&self, pattern: &PatternOp, kind: &str) -> Result<Layout, CadError> {
        let count = pattern.layout.count();
        let d = &self.ui.new_step;
        match (kind, pattern.layout) {
            ("linear", Layout::Linear { .. }) | ("circular", Layout::Circular { .. }) => {
                Ok(pattern.layout)
            }
            ("linear", Layout::Circular { .. }) => Ok(Layout::Linear {
                step: (d.pattern_step, Length::ZERO),
                count,
            }),
            ("circular", Layout::Linear { .. }) => {
                let (a, b) = pattern.seed_position();
                Ok(Layout::Circular {
                    center: (
                        a.checked_add(d.pattern_radius)?,
                        b.checked_add(d.pattern_radius)?,
                    ),
                    step: d.pattern_angle,
                    count,
                })
            }
            _ => Err(CadError::Reference(kind.to_string())),
        }
    }

    /// The name of the sketch of a feature after an edit of the field `key`.
    fn edited_sketch(&self, sketch: &str, key: Key, text: &str) -> Result<String, CadError> {
        match key {
            Key::Sketch => {
                let name = text.trim();
                check_sketch_name(name)?;
                Ok(name.to_string())
            }
            Key::Height | Key::PocketDepth => Ok(sketch.to_string()),
            _ => Err(refusal()),
        }
    }

    fn edited_hole(&self, hole: &Hole, key: Key, text: &str) -> Result<Hole, CadError> {
        let mut h = hole.clone();
        match key {
            Key::Name => h.name = text.trim().to_string(),
            Key::Face => {
                h.face = Face::from_name(text.trim())
                    .ok_or_else(|| CadError::Reference(text.trim().to_string()))?;
            }
            Key::U => h.u = self.parse_text(text)?,
            Key::V => h.v = self.parse_text(text)?,
            Key::Diameter => h.diameter = self.parse_text(text)?,
            _ => return Err(refusal()),
        }
        Ok(h)
    }

    fn edited_post(&self, post: &Post, key: Key, text: &str) -> Result<Post, CadError> {
        let mut p = post.clone();
        match key {
            Key::Name => p.name = text.trim().to_string(),
            Key::PostX => p.x = self.parse_text(text)?,
            Key::PostY => p.y = self.parse_text(text)?,
            Key::PostOuter => p.outer_diameter = self.parse_text(text)?,
            Key::PostHeight => p.height = self.parse_text(text)?,
            Key::BoreDiameter => p.bore_diameter = self.parse_text(text)?,
            Key::BoreDepth => p.bore_depth = self.parse_text(text)?,
            _ => return Err(refusal()),
        }
        Ok(p)
    }

    fn edited_edging(&self, edging: &Edging, key: Key, text: &str) -> Result<Edging, CadError> {
        let mut e = *edging;
        match key {
            Key::EdgeKind(index) => {
                let old = edge_at(&e, index);
                let size = if old.is_sharp() {
                    self.ui.edge_size
                } else {
                    old.size()
                };
                let next = match text.trim() {
                    "sharp" => Treatment::Sharp,
                    "round" => Treatment::Round(size),
                    "chamfer" => Treatment::Chamfer(size),
                    other => return Err(CadError::Reference(other.to_string())),
                };
                set_edge(&mut e, index, next);
            }
            Key::EdgeSize(index) => {
                let size = self.parse_text(text)?;
                let next = match edge_at(&e, index) {
                    Treatment::Sharp => Treatment::Sharp,
                    Treatment::Round(_) => Treatment::Round(size),
                    Treatment::Chamfer(_) => Treatment::Chamfer(size),
                };
                set_edge(&mut e, index, next);
            }
            _ => return Err(refusal()),
        }
        Ok(e)
    }

    fn edited_lid(&self, lid: &Lid, key: Key, text: &str) -> Result<Lid, CadError> {
        let mut l = lid.clone();
        match key {
            Key::LidHole => l.hole_diameter = self.parse_text(text)?,
            Key::LidClearance => l.clearance = self.parse_text(text)?,
            Key::LipThickness | Key::LipDepth => {
                let length = self.parse_text(text)?;
                let lip = l.lip.as_mut().ok_or_else(refusal)?;
                if key == Key::LipThickness {
                    lip.thickness = length;
                } else {
                    lip.depth = length;
                }
            }
            _ => return Err(refusal()),
        }
        Ok(l)
    }

    /// Keeps a changed recipe when the model accepts it: it must build, and the step that was
    /// edited must be applied (or be suppressed or after the tip). The change is recorded in the
    /// history under `name` and `done` is what the status bar says afterwards.
    fn change(
        &mut self,
        name: &str,
        done: Status,
        next: Mechanical,
        check_step: Option<usize>,
    ) -> Result<(), CadError> {
        if next == self.mech {
            return Ok(());
        }
        let built = self.rebuild(&next)?;
        if let Some(index) = check_step {
            if let Some(StepState::Failed(e)) = built.states.get(index) {
                return Err(e.clone());
            }
        }
        let before = std::mem::replace(&mut self.mech, next);
        self.history.record(name, before);
        self.built = built;
        self.dirty = true;
        self.status = done;
        self.refresh();
        Ok(())
    }

    /// Suppresses the selected step or brings it back.
    pub fn toggle_suppressed(&mut self) {
        let Some(index) = self.selection.step() else {
            return;
        };
        if self.read_only {
            self.status = Status::Problem(Msg::ReadOnlyBadge, Vec::new());
            return;
        }
        let Some(step) = self.mech.steps.get(index) else {
            return;
        };
        let label = step_label(step);
        let mut next = self.mech.clone();
        if let Some(slot) = next.steps.get_mut(index) {
            slot.suppressed = !slot.suppressed;
        }
        let name = fill(Msg::ToggleStep.tr(self.lang), &[&label]);
        let done = Status::Done(Msg::ToggleStep, vec![label]);
        if let Err(e) = self.change(&name, done, next, None) {
            self.status = Status::Problem(Msg::Refused, vec![error_text(&e, self.lang)]);
        }
        self.refresh_fields();
    }

    // -----------------------------------------------------------------------------------------
    // The sheet of variables
    // -----------------------------------------------------------------------------------------

    /// The rows of the sheet of variables, in the order of the names.
    pub fn sheet(&self) -> Vec<SheetRow> {
        self.mech
            .vars
            .names()
            .into_iter()
            .filter_map(|name| {
                let value = self.mech.vars.get(name)?;
                Some(SheetRow {
                    name: name.to_string(),
                    text: self.unit_text(value),
                    value,
                    used_by: self.dependents_of(name),
                })
            })
            .collect()
    }

    /// The parameters whose value depends on a variable: the ones that name it and, going on, the
    /// ones whose formula names one of those.
    fn dependents_of(&self, variable: &str) -> Vec<Param> {
        let shadowed = Param::from_name(variable).is_some();
        let mut found: Vec<Param> = Vec::new();
        loop {
            let before = found.len();
            for (param, definition) in self.mech.model.entries() {
                if found.contains(&param) {
                    continue;
                }
                let uses = match definition {
                    Parameter::Fixed(_) => false,
                    Parameter::Variable(name) => name == variable,
                    Parameter::Expr(expr) => expr.names().iter().any(|(name, _)| {
                        (name == variable && !shadowed)
                            || Param::from_name(name).is_some_and(|p| found.contains(&p))
                    }),
                };
                if uses {
                    found.push(param);
                }
            }
            if found.len() == before {
                break;
            }
        }
        found.sort();
        found
    }

    /// The words that name the parameters a row is used by, or that it is not used.
    pub fn used_by_text(&self, row: &SheetRow) -> String {
        if row.used_by.is_empty() {
            return Msg::VariableUnused.tr(self.lang).to_string();
        }
        let labels: Vec<&str> = row
            .used_by
            .iter()
            .map(|p| param_msg(*p).tr(self.lang))
            .collect();
        labels.join(", ")
    }

    /// What a text typed for a variable calculates to: a length with or without its unit, or a
    /// formula over the variables and the constants of the settings.
    pub fn variable_value(&self, text: &str) -> Result<Length, CadError> {
        let t = text.trim();
        if let Ok(length) = self.parse_text(t) {
            return Ok(length);
        }
        let formula = if t.chars().any(char::is_alphabetic) {
            t.to_string()
        } else {
            format!("({t}) * 1 {}", self.settings.unit.suffix())
        };
        let names = VariableNames {
            vars: &self.mech.vars,
            constants: &self.settings.constants,
        };
        parse(&formula, &self.settings.expr)?.eval_length(&names, &self.settings.expr)
    }

    /// Gives a variable a value or a formula, creating the variable when there is none. The
    /// parameters that use it are calculated again; when the model does not accept the new value
    /// nothing changes.
    pub fn set_variable(&mut self, name: &str, text: &str) {
        if self.read_only_refusal() {
            return;
        }
        let result = self.variable_change(name.trim(), text, false);
        self.finish(result);
    }

    /// Adds a variable that does not exist yet.
    pub fn add_variable(&mut self, name: &str, text: &str) {
        if self.read_only_refusal() {
            return;
        }
        let result = self.variable_change(name.trim(), text, true);
        self.finish(result);
    }

    fn variable_change(&mut self, name: &str, text: &str, new: bool) -> Result<(), CadError> {
        if new && self.mech.vars.get(name).is_some() {
            return Err(CadError::Duplicate(name.to_string()));
        }
        let value = self.variable_value(text)?;
        let mut next = self.mech.clone();
        next.vars.set(name, value)?;
        let shown = vec![name.to_string(), self.unit_text(value)];
        let title = fill(Msg::VariableChanged.tr(self.lang), &[&shown[0], &shown[1]]);
        self.change(
            &title,
            Status::Done(Msg::VariableChanged, shown),
            next,
            None,
        )
    }

    /// Takes a variable away. A variable that a parameter still uses is refused, and the box
    /// stays as it was.
    pub fn remove_variable(&mut self, name: &str) {
        if self.read_only_refusal() {
            return;
        }
        let result = self.variable_removal(name);
        self.finish(result);
    }

    fn variable_removal(&mut self, name: &str) -> Result<(), CadError> {
        let mut next = self.mech.clone();
        next.vars.remove(name)?;
        if !self.dependents_of(name).is_empty() {
            return Err(CadError::InUse(name.to_string()));
        }
        let title = fill(Msg::VariableRemoved.tr(self.lang), &[name]);
        let done = Status::Done(Msg::VariableRemoved, vec![name.to_string()]);
        self.change(&title, done, next, None)
    }

    /// Says in the status bar that the project is read-only and answers true when it is.
    fn read_only_refusal(&mut self) -> bool {
        if self.read_only {
            self.status = Status::Problem(Msg::ReadOnlyBadge, Vec::new());
        }
        self.read_only
    }

    /// Tells a refusal in the status bar and brings the fields up to date.
    fn finish(&mut self, result: Result<(), CadError>) {
        if let Err(e) = result {
            self.status = Status::Problem(Msg::Refused, vec![error_text(&e, self.lang)]);
        }
        self.refresh_fields();
    }

    // -----------------------------------------------------------------------------------------
    // Adding and removing steps
    // -----------------------------------------------------------------------------------------

    /// Every id the recipe uses, for a step, a hole of the board or a post made for one.
    fn used_ids(&self) -> Vec<Id> {
        let mut ids = Vec::new();
        for step in &self.mech.steps {
            match &step.op {
                Operation::AddHole { id, .. }
                | Operation::ReplaceHole { id, .. }
                | Operation::RemoveHole { id }
                | Operation::RemovePost { id }
                | Operation::Extrude { id, .. }
                | Operation::Pocket { id, .. }
                | Operation::RemoveFeature { id }
                | Operation::RemovePattern { id }
                | Operation::AddFins { id, .. }
                | Operation::RemoveFins { id }
                | Operation::AddPart { id, .. }
                | Operation::RemovePart { id }
                | Operation::AddMate { id, .. }
                | Operation::RemoveMate { id } => ids.push(*id),
                Operation::AddPattern { id, pattern } => ids.extend(pattern.instance_ids(*id)),
                Operation::AddPost { id, post } | Operation::ReplacePost { id, post } => {
                    ids.push(*id);
                    ids.extend(post.board_hole);
                }
                _ => {}
            }
        }
        if let Some(board) = &self.mech.board {
            ids.extend(board.holes.iter().map(|h| h.id));
        }
        ids
    }

    /// An id that nothing in the recipe uses, the same for the same recipe.
    fn fresh_id(&self) -> Result<Id, CadError> {
        let used = self.used_ids();
        let start = 0x1000 + self.mech.steps.len() as u64;
        for n in start..start + 0x1000 {
            let id = Id::from_parts(1_700_000_000_000, (n & 0x0fff) as u16, n)
                .map_err(|_| CadError::ValueOutOfRange("id"))?;
            if !used.contains(&id) {
                return Ok(id);
            }
        }
        Err(CadError::ValueOutOfRange("id"))
    }

    /// The operation of a new step and its name, for the `attempt`-th position tried.
    fn new_operation(&self, kind: NewStep, attempt: u32) -> Result<(Operation, String), CadError> {
        let d = &self.ui.new_step;
        let moved = d
            .pitch
            .get()
            .checked_mul(i64::from(attempt))
            .ok_or(CadError::Overflow("pitch"))?;
        let shift = Length::new(moved)?;
        match kind {
            NewStep::Hole => {
                let count = self
                    .mech
                    .steps
                    .iter()
                    .filter(|s| matches!(s.op, Operation::AddHole { .. }))
                    .count();
                let name = fill(Msg::NameHole.tr(self.lang), &[&(count + 1).to_string()]);
                let hole = Hole {
                    name: name.clone(),
                    face: d.hole_face,
                    u: d.hole_u.checked_add(shift)?,
                    v: d.hole_v,
                    diameter: d.hole_diameter,
                    style: HoleStyle::Plain,
                };
                Ok((
                    Operation::AddHole {
                        id: self.fresh_id()?,
                        hole,
                    },
                    name,
                ))
            }
            NewStep::Post => {
                let count = self
                    .mech
                    .steps
                    .iter()
                    .filter(|s| matches!(s.op, Operation::AddPost { .. }))
                    .count();
                let name = fill(Msg::NamePost.tr(self.lang), &[&(count + 1).to_string()]);
                let post = Post {
                    name: name.clone(),
                    board_hole: None,
                    x: d.post_x.checked_add(shift)?,
                    y: d.post_y,
                    outer_diameter: d.post_outer,
                    height: d.post_height,
                    kind: d.post_kind,
                    bore_diameter: d.post_bore_diameter,
                    bore_depth: d.post_bore_depth,
                    base: None,
                };
                Ok((
                    Operation::AddPost {
                        id: self.fresh_id()?,
                        post,
                    },
                    name,
                ))
            }
            NewStep::HolePattern | NewStep::PostPattern => {
                let is_hole = kind == NewStep::HolePattern;
                let count = self
                    .mech
                    .steps
                    .iter()
                    .filter(|s| match &s.op {
                        Operation::AddPattern { pattern, .. } => {
                            matches!(pattern.seed, Seed::Hole(_)) == is_hole
                        }
                        _ => false,
                    })
                    .count();
                let n = (count + 1).to_string();
                let (msg, seed) = if is_hole {
                    let name = fill(Msg::NameHolePattern.tr(self.lang), &[&n]);
                    (
                        name.clone(),
                        Seed::Hole(Hole {
                            name,
                            face: d.hole_face,
                            u: d.hole_u.checked_add(shift)?,
                            v: d.hole_v,
                            diameter: d.hole_diameter,
                            style: HoleStyle::Plain,
                        }),
                    )
                } else {
                    let name = fill(Msg::NamePostPattern.tr(self.lang), &[&n]);
                    (
                        name.clone(),
                        Seed::Post(Post {
                            name,
                            board_hole: None,
                            x: d.post_x.checked_add(shift)?,
                            y: d.post_y,
                            outer_diameter: d.post_outer,
                            height: d.post_height,
                            kind: d.post_kind,
                            bore_diameter: d.post_bore_diameter,
                            bore_depth: d.post_bore_depth,
                            base: None,
                        }),
                    )
                };
                let pattern = PatternOp {
                    seed,
                    layout: Layout::Linear {
                        step: (d.pattern_step, Length::ZERO),
                        count: Count::new(i64::from(d.pattern_count))?,
                    },
                };
                Ok((
                    Operation::AddPattern {
                        id: self.fresh_id()?,
                        pattern,
                    },
                    msg,
                ))
            }
            NewStep::Fins => {
                let count = self
                    .mech
                    .steps
                    .iter()
                    .filter(|s| matches!(s.op, Operation::AddFins { .. }))
                    .count();
                let name = fill(Msg::NameFins.tr(self.lang), &[&(count + 1).to_string()]);
                let fins = FinSet {
                    name: name.clone(),
                    face: d.fin_face,
                    along: d.fin_along,
                    count: Count::new(i64::from(d.fin_count))?,
                    thickness: d.fin_thickness,
                    height: d.fin_height,
                    pitch: d.fin_pitch,
                    first: d.fin_first.checked_add(shift)?,
                    start: d.fin_start,
                    length: d.fin_length,
                };
                Ok((
                    Operation::AddFins {
                        id: self.fresh_id()?,
                        fins,
                    },
                    name,
                ))
            }
            NewStep::Part => {
                let library = self
                    .library
                    .as_ref()
                    .ok_or_else(|| CadError::Part("no library is open".to_string()))?;
                let chosen = library
                    .parts
                    .get(self.catalog_choice)
                    .or_else(|| library.parts.first())
                    .ok_or_else(|| CadError::Part("the library has no part".to_string()))?;
                let count = self
                    .mech
                    .steps
                    .iter()
                    .filter(|s| matches!(s.op, Operation::AddPart { .. }))
                    .count();
                let name = fill(Msg::NamePart.tr(self.lang), &[&(count + 1).to_string()]);
                let cut = match chosen.shape {
                    Shape::Profile { default_cut, .. } => Some(default_cut),
                    _ => None,
                };
                let part = PartInstance {
                    name: name.clone(),
                    library: library.name.clone(),
                    part: chosen.clone(),
                    at: [d.part_x.checked_add(shift)?, d.part_y, d.part_z],
                    orientation: Orientation {
                        axis: d.part_axis,
                        roll: d.part_roll,
                    },
                    cut,
                };
                Ok((
                    Operation::AddPart {
                        id: self.fresh_id()?,
                        part,
                    },
                    name,
                ))
            }
            NewStep::Mate => {
                // the parts that are there, in the order of their steps
                let parts = self.built.enclosure.parts();
                let ids: Vec<Id> = self
                    .mech
                    .steps
                    .iter()
                    .take(self.mech.tip)
                    .filter_map(|s| match &s.op {
                        Operation::AddPart { id, .. } if parts.contains_key(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                let selected = match self.selection {
                    Selection::Part(i) => match self.mech.steps.get(i).map(|s| &s.op) {
                        Some(Operation::AddPart { id, .. }) => Some(*id),
                        _ => None,
                    },
                    _ => None,
                };
                let placed = selected
                    .filter(|id| ids.contains(id))
                    .or_else(|| ids.last().copied())
                    .ok_or_else(|| CadError::Mate("the model has no part".to_string()))?;
                let at = ids.iter().position(|id| *id == placed).unwrap_or(0);
                let fixed = if at > 0 {
                    ids[at - 1]
                } else {
                    *ids.get(1).ok_or_else(|| {
                        CadError::Mate("a mate needs two parts in the model".to_string())
                    })?
                };
                let kind = d.mate_of(&d.mate_kind).ok_or_else(refusal)?;
                let count = self
                    .mech
                    .steps
                    .iter()
                    .filter(|s| matches!(s.op, Operation::AddMate { .. }))
                    .count();
                let name = fill(Msg::NameMate.tr(self.lang), &[&(count + 1).to_string()]);
                Ok((
                    Operation::AddMate {
                        id: self.fresh_id()?,
                        mate: Mate {
                            a: fixed,
                            b: placed,
                            kind,
                        },
                    },
                    name,
                ))
            }
            NewStep::Extrude | NewStep::Pocket => {
                let sketch = self
                    .active_sketch
                    .as_ref()
                    .filter(|name| self.mech.sketch(name).is_some())
                    .cloned()
                    .or_else(|| self.mech.sketches.first().map(|s| s.name().to_string()))
                    .ok_or_else(|| CadError::Feature("the project has no sketch".to_string()))?;
                let count = self
                    .mech
                    .steps
                    .iter()
                    .filter(|s| {
                        matches!(s.op, Operation::Extrude { .. } | Operation::Pocket { .. })
                    })
                    .count();
                let id = self.fresh_id()?;
                let n = (count + 1).to_string();
                if kind == NewStep::Extrude {
                    Ok((
                        Operation::Extrude {
                            id,
                            sketch,
                            height: d.extrude_height,
                        },
                        fill(Msg::NameExtrude.tr(self.lang), &[&n]),
                    ))
                } else {
                    let depth = (d.pocket_depth.get() > 0).then_some(d.pocket_depth);
                    Ok((
                        Operation::Pocket { id, sketch, depth },
                        fill(Msg::NamePocket.tr(self.lang), &[&n]),
                    ))
                }
            }
            NewStep::Edging => {
                if self
                    .mech
                    .steps
                    .iter()
                    .any(|s| matches!(s.op, Operation::SetEdging(_)))
                {
                    return Err(CadError::Duplicate("edges".to_string()));
                }
                let edging = Edging {
                    vertical: Treatment::Round(self.ui.edge_size),
                    top: Treatment::Sharp,
                    bottom: Treatment::Sharp,
                };
                Ok((
                    Operation::SetEdging(edging),
                    Msg::NameEdging.tr(self.lang).to_string(),
                ))
            }
        }
    }

    /// Adds a step at the tip of the history and selects it. A hole or a post starts where the
    /// settings say and moves along until the model accepts it; when no position is accepted
    /// nothing changes. The addition is one entry of the history.
    pub fn add_step(&mut self, kind: NewStep) {
        if self.read_only_refusal() {
            return;
        }
        let result = self.step_addition(kind);
        self.finish(result);
    }

    fn step_addition(&mut self, kind: NewStep) -> Result<(), CadError> {
        let tries = match kind {
            NewStep::Edging | NewStep::Extrude | NewStep::Pocket | NewStep::Mate => 1,
            NewStep::Hole
            | NewStep::Post
            | NewStep::HolePattern
            | NewStep::PostPattern
            | NewStep::Fins
            | NewStep::Part => self.ui.new_step.tries,
        };
        let mut last = refusal();
        for attempt in 0..tries {
            let (op, label) = self.new_operation(kind, attempt)?;
            let mut next = self.mech.clone();
            let index = next.tip.min(next.steps.len());
            next.steps.insert(
                index,
                Step {
                    name: label.clone(),
                    op,
                    suppressed: false,
                },
            );
            next.tip = index + 1;
            let title = fill(Msg::StepAdded.tr(self.lang), &[&label]);
            let done = Status::Done(Msg::StepAdded, vec![label]);
            match self.change(&title, done, next, Some(index)) {
                Ok(()) => {
                    if let Some(step) = self.mech.steps.get(index) {
                        self.selection = the_selection_of(index, step);
                    }
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// The names of the sketches of the project.
    pub fn sketch_names(&self) -> Vec<String> {
        self.mech
            .sketches
            .iter()
            .map(|s| s.name().to_string())
            .collect()
    }

    /// Puts a sketch in the project (replacing the one of the same name) and makes it the one
    /// the next extrusion or pocket uses. One entry of the history; the steps that use the sketch
    /// are built again from it.
    pub fn keep_sketch(&mut self, sketch: Sketch) {
        if self.read_only_refusal() {
            return;
        }
        let name = sketch.name().to_string();
        let mut next = self.mech.clone();
        if next.sketches.len() >= self.sketch_settings.max_sketches && next.sketch(&name).is_none()
        {
            self.status = Status::Error(CadError::ValueOutOfRange("sketches"));
            return;
        }
        next.put_sketch(sketch);
        let title = fill(Msg::SketchKept.tr(self.lang), &[&name]);
        let done = Status::Done(Msg::SketchKept, vec![name.clone()]);
        let result = self.change(&title, done, next, None);
        if result.is_ok() {
            self.active_sketch = Some(name);
        }
        self.finish(result);
    }

    /// Reads a sketch file (`.phoenixsketch`) and puts it in the project: the way in for the
    /// sketches that were kept next to a project before they could be kept inside it.
    pub fn import_sketch_file(&mut self, path: &Path) -> bool {
        match sketch_file::read_file(path, &self.sketch_settings) {
            Ok(sketch) => {
                self.keep_sketch(sketch);
                !self.status.is_problem()
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// Writes a sketch of the project to a sketch file: the way out.
    pub fn export_sketch_file(&mut self, name: &str, path: &Path) -> bool {
        let Some(sketch) = self.mech.sketch(name) else {
            self.status = Status::Error(CadError::UnknownName(name.to_string()));
            return false;
        };
        match sketch_file::write_file(path, sketch, self.settings.backups) {
            Ok(()) => {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.status = Status::Done(Msg::Saved, vec![file]);
                true
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// Removes a sketch from the project. A sketch that a step uses is refused.
    pub fn remove_sketch(&mut self, name: &str) {
        if self.read_only_refusal() {
            return;
        }
        let used = self.mech.steps.iter().any(|s| match &s.op {
            Operation::Extrude { sketch, .. } | Operation::Pocket { sketch, .. } => sketch == name,
            _ => false,
        });
        let result = if used {
            Err(CadError::InUse(name.to_string()))
        } else if self.mech.sketch(name).is_none() {
            Err(CadError::UnknownName(name.to_string()))
        } else {
            let mut next = self.mech.clone();
            next.sketches.retain(|s| s.name() != name);
            let title = fill(Msg::StepRemoved.tr(self.lang), &[name]);
            let done = Status::Done(Msg::StepRemoved, vec![name.to_string()]);
            self.change(&title, done, next, None)
        };
        self.finish(result);
    }

    /// Removes the selected step. A step that others need (a hole that a later step replaces, for
    /// one) is refused and the recipe stays as it was.
    pub fn remove_step(&mut self) {
        if self.read_only_refusal() {
            return;
        }
        let result = self.step_removal();
        self.finish(result);
    }

    fn step_removal(&mut self) -> Result<(), CadError> {
        let index = self.selection.step().ok_or_else(refusal)?;
        let step = self.mech.steps.get(index).ok_or_else(refusal)?;
        let label = step_label(step);
        let mut next = self.mech.clone();
        next.steps.remove(index);
        if index < next.tip {
            next.tip -= 1;
        }
        let built = self.rebuild(&next)?;
        let failed = built
            .states
            .iter()
            .filter(|s| matches!(s, StepState::Failed(_)))
            .count();
        if failed > self.info.failed {
            return Err(CadError::InUse(label));
        }
        let title = fill(Msg::StepRemoved.tr(self.lang), &[&label]);
        let done = Status::Done(Msg::StepRemoved, vec![label]);
        self.change(&title, done, next, None)?;
        self.selection = Selection::Body;
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // History
    // -----------------------------------------------------------------------------------------

    /// Goes back one change.
    pub fn undo(&mut self) {
        if self.drag.is_some() {
            return;
        }
        match self.history.undo(self.mech.clone()) {
            Some((name, previous)) => self.restore(previous, Msg::Undone, name),
            None => self.status = Status::Done(Msg::NothingToUndo, Vec::new()),
        }
    }

    /// Goes forward one change that was undone.
    pub fn redo(&mut self) {
        if self.drag.is_some() {
            return;
        }
        match self.history.redo(self.mech.clone()) {
            Some((name, next)) => self.restore(next, Msg::Redone, name),
            None => self.status = Status::Done(Msg::NothingToRedo, Vec::new()),
        }
    }

    fn restore(&mut self, mech: Mechanical, message: Msg, name: String) {
        match self.rebuild(&mech) {
            Ok(built) => {
                self.mech = mech;
                self.built = built;
                self.dirty = true;
                self.status = Status::Done(message, vec![name]);
                if let Some(i) = self.selection.step() {
                    if i >= self.mech.steps.len() {
                        self.selection = Selection::Body;
                    }
                }
                self.refresh();
            }
            Err(e) => self.status = Status::Error(e),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Files
    // -----------------------------------------------------------------------------------------

    /// Opens a project. The answer is false (and the status says why) when it could not be read;
    /// the project that was open stays as it was.
    pub fn open(&mut self, path: &Path) -> bool {
        match self.read_project(path) {
            Ok(()) => true,
            Err(status) => {
                self.status = status;
                false
            }
        }
    }

    fn read_project(&mut self, path: &Path) -> Result<(), Status> {
        let limit = self.settings.mechanical.max_bytes.saturating_add(1 << 20);
        let bytes = store::load_limited(path, limit as u64).map_err(Status::Error)?;
        let opened = Document::open(&bytes, limit, &[MECHANICAL_DOMAIN, PARAMETERS_DOMAIN])
            .map_err(Status::Error)?;
        let mut mech = opened
            .document
            .mechanical(&self.settings.expr, &self.settings.mechanical)
            .map_err(Status::Error)?
            .ok_or_else(|| Status::Problem(Msg::NoMechanical, Vec::new()))?;
        // sketches that cannot be read (damaged, or of a newer version) leave the project open
        // but not editable, so that they are kept as they are
        let damaged = match opened.document.sketches(&self.sketch_settings) {
            Ok(list) => {
                mech.sketches = list;
                None
            }
            Err(e) => Some(e),
        };
        let built = self.rebuild(&mech).map_err(Status::Error)?;
        self.mech = mech;
        self.built = built;
        self.document = opened.document;
        self.read_only = opened.read_only || damaged.is_some();
        self.active_sketch = None;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.history.clear();
        self.selection = Selection::Body;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.status = if let Some(e) = damaged {
            Status::Error(e)
        } else if self.read_only {
            Status::Problem(Msg::ReadOnlyOpened, Vec::new())
        } else {
            Status::Done(Msg::Opened, vec![name])
        };
        self.refresh();
        Ok(())
    }

    /// Writes the project to `path` (atomically, keeping the copies of the settings). The answer
    /// is false when it could not be written.
    pub fn save_to(&mut self, path: &Path) -> bool {
        if self.read_only {
            self.status = Status::Problem(Msg::ReadOnlyBadge, Vec::new());
            return false;
        }
        let mut document = self.document.clone();
        let written = document
            .set_mechanical(&self.mech, &self.settings.mechanical)
            .and_then(|()| document.set_sketches(&self.mech.sketches, &self.sketch_settings))
            .and_then(|()| store::save_atomic(path, &document.to_bytes(), self.settings.backups));
        match written {
            Ok(()) => {
                self.document = document;
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.status = Status::Done(Msg::Saved, vec![name]);
                true
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// Writes the project to the file it came from. False when it has none yet.
    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(path) => self.save_to(&path),
            None => false,
        }
    }

    // -----------------------------------------------------------------------------------------
    // What the status bar measures
    // -----------------------------------------------------------------------------------------

    /// The envelope that is checked and whether it is the board of the project.
    pub fn envelope(&self) -> (Envelope, bool) {
        if let Some(board) = &self.mech.board {
            if let Some(((x0, y0), (x1, y1))) = board.bounds() {
                let height = board.thickness.get() + board.tallest_component.map_or(0, Length::get);
                if let (Ok(length), Ok(width), Ok(height)) = (
                    Length::new(x1 - x0),
                    Length::new(y1 - y0),
                    Length::new(height),
                ) {
                    return (
                        Envelope {
                            length,
                            width,
                            height,
                        },
                        true,
                    );
                }
            }
        }
        (
            Envelope {
                length: self.manual[0],
                width: self.manual[1],
                height: self.manual[2],
            },
            false,
        )
    }

    fn measure(&self) -> Info {
        let e = &self.built.enclosure;
        let p = e.params();
        let (ox, oy, oz) = p.outer();
        let (envelope, from_board) = self.envelope();
        Info {
            outer: [ox, oy, oz],
            inner: [p.inner_length, p.inner_width, p.inner_height],
            material_nm3: {
                let (added, removed) = e.features_volume_nm3();
                e.material_volume_nm3() + added - removed
            },
            failed: self
                .built
                .states
                .iter()
                .filter(|s| matches!(s, StepState::Failed(_)))
                .count(),
            fit: check_fit(e, &envelope, self.settings.printer.fit_clearance),
            fit_from_board: from_board,
            interference: e.interference().ok(),
        }
    }

    /// The sizes and the material, as the status bar shows them.
    pub fn measures_text(&self) -> String {
        let outer: Vec<String> = self.info.outer.iter().map(|l| mm_text(l.get())).collect();
        let inner: Vec<String> = self.info.inner.iter().map(|l| mm_text(l.get())).collect();
        let material = format!("{:.1}", self.info.material_nm3 as f64 / 1e21);
        fill(
            Msg::Measures.tr(self.lang),
            &[
                &outer[0], &outer[1], &outer[2], &inner[0], &inner[1], &inner[2], &material,
            ],
        )
    }

    /// Whether the envelope fits, as the status bar says it.
    pub fn fit_text(&self) -> String {
        let r = &self.info.fit;
        let spare = [
            mm_text(r.spare_length),
            mm_text(r.spare_width),
            mm_text(r.spare_height),
        ];
        let message = if r.fits {
            Msg::FitOk
        } else if r.fits_turned {
            Msg::FitTurned
        } else {
            Msg::FitNo
        };
        fill(message.tr(self.lang), &[&spare[0], &spare[1], &spare[2]])
    }

    /// How many steps the model refused, when it refused any.
    pub fn failed_text(&self) -> Option<String> {
        (self.info.failed > 0).then(|| {
            fill(
                Msg::StepsFailed.tr(self.lang),
                &[&self.info.failed.to_string()],
            )
        })
    }

    /// The fit report as text that can be copied or written to a file: the measures, whether the
    /// board fits and a line for each step with its state, in the language of the window. It is
    /// what the `check` command prints.
    pub fn report_text(&self) -> String {
        let lang = self.lang;
        let mut lines = vec![self.measures_text(), self.fit_text()];
        for (i, step) in self.mech.steps.iter().enumerate() {
            let state = match self.states().get(i) {
                Some(StepState::Valid) | None => Msg::StateValid.tr(lang).to_owned(),
                Some(StepState::Suppressed) => Msg::StateSuppressed.tr(lang).to_owned(),
                Some(StepState::AfterTip) => Msg::StateAfterTip.tr(lang).to_owned(),
                Some(StepState::Failed(e)) => {
                    format!("{}: {}", Msg::StateFailed.tr(lang), error_text(e, lang))
                }
            };
            lines.push(format!("{:>3}  {}  {state}", i + 1, step_label(step)));
        }
        // the estimate of each set of fins, said to be one
        let temperature = format!("{}", self.settings.thermal.delta_t_k());
        for set in self.built.enclosure.fins().values() {
            let t = set.thermal(&self.settings.thermal);
            lines.push(fill(
                Msg::ReportFins.tr(lang),
                &[
                    &set.name,
                    &set.count.get().to_string(),
                    &format!("{:.1}", t.area_mm2 / 100.0),
                    &format!("{:.2}", t.power_w),
                    &temperature,
                ],
            ));
        }
        lines.extend(self.parts_report_lines());
        lines.extend(self.assembly_report_lines(false));
        lines.join(
            "
",
        )
    }

    /// The text of the check of the assembly, in the language of the window: how many mates there
    /// are and which pairs of parts share volume. It is what the `interference` command prints.
    pub fn assembly_text(&self) -> String {
        self.assembly_report_lines(true).join("\n")
    }

    /// Whether no pair of placed parts shares volume.
    pub fn assembly_clear(&self) -> bool {
        self.info
            .interference
            .as_ref()
            .is_some_and(|r| r.conflicts.is_empty())
    }

    /// The lines of the report about the assembly: how many mates there are and which pairs of
    /// parts share volume (with the parts in the places the mates give them). The check is part of
    /// the report when there are two parts or more, or when `always` asks for it.
    fn assembly_report_lines(&self, always: bool) -> Vec<String> {
        let lang = self.lang;
        let mut lines = Vec::new();
        let mates = self.built.enclosure.mates().len();
        if mates > 0 {
            lines.push(fill(Msg::ReportMates.tr(lang), &[&mates.to_string()]));
        }
        if self.built.enclosure.parts().len() < 2 && !always {
            return lines;
        }
        let Some(report) = &self.info.interference else {
            return lines;
        };
        if report.conflicts.is_empty() {
            lines.push(fill(
                Msg::ReportInterferenceNone.tr(lang),
                &[
                    &report.boxes_overlapping.to_string(),
                    &report.passes.to_string(),
                ],
            ));
            return lines;
        }
        lines.push(fill(
            Msg::ReportInterference.tr(lang),
            &[
                &report.conflicts.len().to_string(),
                &report.boxes_overlapping.to_string(),
                &report.passes.to_string(),
            ],
        ));
        let shown = self.settings.design.assembly.max_report_pairs;
        for c in report.conflicts.iter().take(shown) {
            lines.push(fill(
                Msg::ReportConflict.tr(lang),
                &[
                    &self.part_name(&c.a),
                    &self.part_name(&c.b),
                    &catalog::volume_mm3_text(c.shared_nm3),
                ],
            ));
        }
        if report.conflicts.len() > shown {
            lines.push(fill(
                Msg::ReportConflictMore.tr(lang),
                &[&(report.conflicts.len() - shown).to_string()],
            ));
        }
        lines
    }

    /// The lines of the report about the parts placed in the model: the totals, a line for each
    /// part and cut, and the cut list of each profile (lengths in millimetres).
    fn parts_report_lines(&self) -> Vec<String> {
        let lang = self.lang;
        if self.built.enclosure.parts().is_empty() {
            return Vec::new();
        }
        let bill = match self.bill() {
            Ok(bill) => bill,
            Err(e) => return vec![error_text(&e, lang)],
        };
        let mm = |nm: i64| format!("{} mm", mm_text(nm));
        let mut lines = vec![fill(
            Msg::ReportParts.tr(lang),
            &[
                &bill.pieces.to_string(),
                &catalog::volume_mm3_text(bill.volume_nm3),
                &catalog::mass_g_text(bill.mass_ug),
            ],
        )];
        for l in &bill.lines {
            lines.push(fill(
                Msg::ReportPartLine.tr(lang),
                &[
                    kind_msg(l.kind).tr(lang),
                    &l.part,
                    &l.material,
                    &l.count.to_string(),
                    &l.cut.map_or("-".to_string(), |c| mm(c.get())),
                    &catalog::mass_g_text(l.mass_ug),
                ],
            ));
        }
        for cut in catalog::cut_list(&bill) {
            let pieces: Vec<String> = cut
                .cuts
                .iter()
                .map(|(len, n)| format!("{n} x {}", mm(len.get())))
                .collect();
            lines.push(fill(
                Msg::ReportCut.tr(lang),
                &[
                    &cut.part,
                    &pieces.join(" + "),
                    &mm(i64::try_from(cut.total_nm).unwrap_or(i64::MAX)),
                ],
            ));
        }
        lines
    }

    /// Says in the status bar that the report was put on the clipboard.
    pub fn report_copied(&mut self) {
        self.status = Status::Done(Msg::ReportCopied, Vec::new());
    }

    // -----------------------------------------------------------------------------------------
    // Export
    // -----------------------------------------------------------------------------------------

    /// The text of a format for the model as it is now. The STEP file is refused with
    /// `PHOENIX-CAD-022` when the model has a shape it does not draw (the extrusions and pockets
    /// of the sketches among them).
    pub fn export_text(&self, format: Format) -> Result<String, CadError> {
        let text = match format {
            Format::Bill => self.bill_csv_text()?,
            _ => self.report_text(),
        };
        export::render(format, &self.built.enclosure, &text)
    }

    /// The name suggested for a file of a format, from the name of the project.
    pub fn export_name(&self, format: Format) -> String {
        export::suggested_name(self.path.as_deref(), format)
    }

    /// Writes the model in a format to `path`. Nothing is written over unless `overwrite` says
    /// so, nor over the project that is open. The status bar says what was written, or the code
    /// of the reason it was not; the project and its history are not touched. The answer is false
    /// when nothing was written.
    pub fn export_to(&mut self, format: Format, path: &Path, overwrite: bool) -> bool {
        let written = self.export_text(format).and_then(|text| {
            export::write_file(
                path,
                &text,
                overwrite,
                self.path.as_deref(),
                self.ui.export.max_kib,
            )
        });
        match written {
            Ok(bytes) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.status = Status::Done(Msg::Exported, vec![name, bytes.to_string()]);
                true
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // The views
    // -----------------------------------------------------------------------------------------

    /// The box that holds the board, or the envelope checked, inside the box.
    pub fn board_box(&self) -> Option<BoardBox> {
        let p = self.built.enclosure.params();
        let (envelope, from_board) = self.envelope();
        let (bw, bh) = (envelope.length.get(), envelope.width.get());
        let (il, iw) = (p.inner_length.get(), p.inner_width.get());
        let placement = match (&self.mech.board, from_board) {
            (Some(board), true) => board.placement.map(|(x, y)| (x.get(), y.get())),
            _ => None,
        };
        let (px, py) = placement.unwrap_or(((il - bw) / 2, (iw - bh) / 2));
        let rest = if from_board {
            self.built
                .enclosure
                .posts()
                .values()
                .map(|post| post.height.get())
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        let min = [
            mm(p.wall.get() + px),
            mm(p.wall.get() + py),
            mm(p.floor.get() + rest),
        ];
        Some(BoardBox {
            min,
            max: [
                min[0] + mm(bw),
                min[1] + mm(bh),
                min[2] + mm(envelope.height.get()),
            ],
        })
    }

    /// How a view puts the model on a pane of `pane` pixels.
    fn frame_of(&self, view: ViewKind, pane: (f64, f64)) -> Frame {
        let (control, fit) = match view {
            ViewKind::Iso => (&self.view3d, Fit::Sphere),
            ViewKind::Plan => (&self.plan, Fit::Bounds),
        };
        Frame {
            camera: control.camera,
            zoom: control.zoom,
            pan: control.pan,
            pane,
            margin: self.ui.margin,
            fit,
        }
    }

    /// The lines of the model (without the marks of the measurements).
    fn model_segments(&self) -> Vec<Segment> {
        let conflicts = self
            .info
            .interference
            .as_ref()
            .map(Report::conflicting)
            .unwrap_or_default();
        scene_with(
            &self.built.enclosure,
            self.board_box(),
            self.highlight(),
            self.ui.circle_segments,
            &conflicts,
        )
    }

    /// How a view puts the model on a pane, which is also the way back from a point of the pane.
    pub fn view_mapping(&self, view: ViewKind, pane: (f64, f64)) -> Mapping {
        Mapping::of(
            &self.model_segments(),
            outer_size(&self.built.enclosure),
            &self.frame_of(view, pane),
        )
    }

    /// The lines of a view put on a pane of `pane` pixels: the model and the marks of the
    /// measurements. The size of the drawing depends on the model only.
    pub fn lines(&self, view: ViewKind, pane: (f64, f64)) -> Vec<Line2> {
        let mut segments = self.model_segments();
        let mapping = Mapping::of(
            &segments,
            outer_size(&self.built.enclosure),
            &self.frame_of(view, pane),
        );
        segments.extend(self.measure_segments());
        project_with(&mapping, &segments)
    }

    /// The control of a view.
    pub fn control_mut(&mut self, view: ViewKind) -> &mut ViewControl {
        match view {
            ViewKind::Iso => &mut self.view3d,
            ViewKind::Plan => &mut self.plan,
        }
    }

    /// Sets the zoom for a turn of the mouse wheel of `scroll` units (a positive turn zooms in).
    pub fn wheel(&mut self, view: ViewKind, scroll: f64) {
        let factor = (scroll * self.ui.wheel_speed).exp();
        self.control_mut(view).zoom_by(factor);
    }

    /// Turns the 3D view for a drag of `dx` and `dy` pixels.
    pub fn rotate_3d(&mut self, dx: f64, dy: f64) {
        let speed = self.ui.rotate_speed;
        self.view3d.rotate(dx, dy, speed);
    }
}

// ---------------------------------------------------------------------------------------------
// Dragging and measuring in the views
// ---------------------------------------------------------------------------------------------

/// The reference under a point of the pane: a point within `radius` pixels (the nearest to the
/// pointer, and of two at the same distance the nearer to the viewer) or, when `want` allows
/// edges and there is no point, the edge nearest to it.
fn pick_reference(
    refs: &[Reference],
    mapping: &Mapping,
    px: (f64, f64),
    radius: f64,
    want: Want,
) -> Option<usize> {
    if want != Want::Edges {
        let mut indexes = Vec::new();
        let mut drawn: Vec<Drawn> = Vec::new();
        for (k, r) in refs.iter().enumerate() {
            if let Geometry::Point(p) = r.geometry {
                indexes.push(k);
                drawn.push(mapping.to_pane(p));
            }
        }
        if let Some(i) = drag::nearest(px, &drawn, radius) {
            return indexes.get(i).copied();
        }
    }
    if want == Want::Points {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for (k, r) in refs.iter().enumerate() {
        if let Geometry::Edge(a, b) = r.geometry {
            let d = drag::distance_to_segment(px, mapping.to_pane(a).0, mapping.to_pane(b).0);
            if d <= radius && best.is_none_or(|(_, bd)| d < bd - 1e-9) {
                best = Some((k, d));
            }
        }
    }
    best.map(|(k, _)| k)
}

/// The refusal that is told when a view shows the plane of the object edge on.
fn edge_on() -> CadError {
    CadError::Drag("this view shows the plane of the object edge on".into())
}

impl Window {
    /// Whether a hole or a post is being dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The holes and the posts that were applied, with where each is on its plane and drawn.
    fn candidates(&self) -> Vec<Candidate> {
        let enclosure = &self.built.enclosure;
        let mut out = Vec::new();
        for (i, step) in self.mech.steps.iter().enumerate() {
            if !matches!(self.built.states.get(i), Some(StepState::Valid)) {
                continue;
            }
            match &step.op {
                Operation::AddHole { id, hole } if enclosure.holes().contains_key(id) => {
                    let plane = face_plane(enclosure, hole.face);
                    let centre = (mm(hole.u.get()), mm(hole.v.get()));
                    out.push(Candidate {
                        step: i,
                        name: hole.name.clone(),
                        plane,
                        centre,
                        at: plane.at(centre.0, centre.1),
                        is_hole: true,
                    });
                }
                Operation::AddPost { id, post } if enclosure.posts().contains_key(id) => {
                    out.push(Candidate {
                        step: i,
                        name: post.name.clone(),
                        plane: floor_plane(enclosure.params()),
                        centre: (mm(post.x.get()), mm(post.y.get())),
                        at: post_centre(enclosure.params(), post),
                        is_hole: false,
                    });
                }
                _ => {}
            }
        }
        out
    }

    /// Picks the hole or the post drawn within the radius of the settings of the point `px` of a
    /// view and starts dragging it; the thing is selected in the tree. False when there is none
    /// (the view is turned or moved instead), when the project is read-only, a tool of
    /// measurement is on, or the view does not show where on the plane of the thing the pointer is
    /// (a face seen edge on), which the status bar says.
    pub fn drag_begin(&mut self, view: ViewKind, pane: (f64, f64), px: (f64, f64)) -> bool {
        if self.read_only || self.drag.is_some() || self.measure_tool != MeasureTool::Off {
            return false;
        }
        let mapping = self.view_mapping(view, pane);
        let candidates = self.candidates();
        let drawn: Vec<Drawn> = candidates.iter().map(|c| mapping.to_pane(c.at)).collect();
        let Some(c) =
            drag::nearest(px, &drawn, self.ui.drag.hit_radius).and_then(|k| candidates.get(k))
        else {
            return false;
        };
        let Some(pointer) = mapping.on_plane(px, c.plane.origin, c.plane.a, c.plane.b) else {
            self.status = Status::Error(edge_on());
            return false;
        };
        self.select(if c.is_hole {
            Selection::Hole(c.step)
        } else {
            Selection::Post(c.step)
        });
        self.drag = Some(Drag {
            view,
            step: c.step,
            name: c.name.clone(),
            before: self.mech.clone(),
            was_dirty: self.dirty,
            plane: c.plane,
            offset: drag::grab_offset(c.centre, pointer),
            moved: false,
        });
        true
    }

    /// Moves the thing that is dragged to where the pointer `px` is, snapped to the grid when
    /// that is on. The new position is kept only when the model accepts it (the margins, the
    /// distances to the other holes and posts, the lid and every other check of the model): a
    /// refusal is told in the status bar and the thing stays where it last was accepted. Nothing
    /// is recorded in the history until [`Window::drag_end`].
    pub fn drag_move(&mut self, pane: (f64, f64), px: (f64, f64)) {
        let Some(drag) = self.drag.as_ref() else {
            return;
        };
        let (view, plane, offset, step) = (drag.view, drag.plane, drag.offset, drag.step);
        let name = drag.name.clone();
        let mapping = self.view_mapping(view, pane);
        let Some(pointer) = mapping.on_plane(px, plane.origin, plane.a, plane.b) else {
            self.status = Status::Error(edge_on());
            return;
        };
        let grid = if self.snap_to_grid {
            self.ui.drag.grid.get()
        } else {
            0
        };
        let (un, vn) = drag::dragged(pointer, offset, grid);
        let (u, v) = match (Length::new(un), Length::new(vn)) {
            (Ok(u), Ok(v)) => (u, v),
            (Err(e), _) | (_, Err(e)) => {
                self.status = Status::Problem(Msg::Refused, vec![error_text(&e, self.lang)]);
                return;
            }
        };
        let mut next = self.mech.clone();
        match next.steps.get_mut(step).map(|s| &mut s.op) {
            Some(Operation::AddHole { hole, .. }) => {
                hole.u = u;
                hole.v = v;
            }
            Some(Operation::AddPost { post, .. }) => {
                post.x = u;
                post.y = v;
            }
            _ => return,
        }
        if next == self.mech {
            return;
        }
        let built = match self.rebuild(&next) {
            Ok(built) => built,
            Err(e) => {
                self.status = Status::Problem(Msg::Refused, vec![error_text(&e, self.lang)]);
                return;
            }
        };
        // the thing and every other step must be as valid as they were
        for (k, state) in built.states.iter().enumerate() {
            if let StepState::Failed(e) = state {
                if !matches!(self.built.states.get(k), Some(StepState::Failed(_))) {
                    self.status = Status::Problem(Msg::Refused, vec![error_text(e, self.lang)]);
                    return;
                }
            }
        }
        if let Some(d) = self.drag.as_mut() {
            d.moved = true;
        }
        self.status = Status::Done(
            Msg::DragDone,
            vec![name, self.unit_text(u), self.unit_text(v)],
        );
        self.mech = next;
        self.built = built;
        self.dirty = true;
        self.refresh();
    }

    /// Lets go of what is dragged: if it moved, the move is one entry of the history, which undo
    /// takes back to where it was picked.
    pub fn drag_end(&mut self) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if !drag.moved || self.mech == drag.before {
            self.dirty = drag.was_dirty;
            return;
        }
        let name = fill(Msg::DragName.tr(self.lang), &[&drag.name]);
        self.history.record(&name, drag.before);
        self.dirty = true;
    }

    /// Gives up the drag: what was dragged goes back to where it was picked and nothing is
    /// recorded.
    pub fn drag_cancel(&mut self) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.moved {
            if let Ok(built) = self.rebuild(&drag.before) {
                self.mech = drag.before;
                self.built = built;
            }
            self.dirty = drag.was_dirty;
            self.refresh();
        }
        self.status = Status::Done(Msg::DragCancelled, Vec::new());
    }

    // -----------------------------------------------------------------------------------------
    // Measurements
    // -----------------------------------------------------------------------------------------

    /// Turns a measurement tool on or off; what was half picked is forgotten.
    pub fn set_measure_tool(&mut self, tool: MeasureTool) {
        self.pending.clear();
        self.measure_tool = tool;
        if tool != MeasureTool::Off {
            self.status = Status::Done(Msg::MeasurePickFirst, Vec::new());
        }
    }

    /// The readings kept, the oldest first.
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// How many references of the measurement in progress are picked.
    pub fn picked(&self) -> usize {
        self.pending.len()
    }

    /// Forgets every reading and what was half picked.
    pub fn clear_measurements(&mut self) {
        self.measurements.clear();
        self.pending.clear();
    }

    /// Forgets one reading.
    pub fn remove_measurement(&mut self, index: usize) {
        if index < self.measurements.len() {
            self.measurements.remove(index);
        }
    }

    /// What the next pick may be: a point, an edge or either.
    fn wanted(&self) -> Want {
        match (self.measure_tool, self.pending.first()) {
            (MeasureTool::Angle, Some(first)) if first.is_point() => Want::Points,
            (MeasureTool::Angle, Some(_)) => Want::Edges,
            _ => Want::Any,
        }
    }

    /// A click at the point `px` of a view while a measurement tool is on: the point or the edge
    /// under it is picked (points first), and when the measurement has all it needs the reading
    /// is kept and told in the status bar. True when a tool is on, so the click is not for the
    /// view.
    pub fn measure_click(&mut self, view: ViewKind, pane: (f64, f64), px: (f64, f64)) -> bool {
        if self.measure_tool == MeasureTool::Off {
            return false;
        }
        let mapping = self.view_mapping(view, pane);
        let refs = references(&self.built.enclosure);
        let radius = self.ui.measure.pick_radius;
        let Some(picked) =
            pick_reference(&refs, &mapping, px, radius, self.wanted()).and_then(|k| refs.get(k))
        else {
            return true;
        };
        self.pending.push(picked.clone());
        let needed = match (self.measure_tool, self.pending.first()) {
            (MeasureTool::Angle, Some(first)) => measure::angle_needs(first),
            _ => 2,
        };
        if self.pending.len() < needed {
            self.status = Status::Done(
                Msg::MeasurePickNext,
                vec![(self.pending.len() + 1).to_string(), needed.to_string()],
            );
            return true;
        }
        let picks = std::mem::take(&mut self.pending);
        let result = match (self.measure_tool, picks.as_slice()) {
            (MeasureTool::Distance, [a, b]) => Ok(measure::distance(a, b)),
            _ => measure::angle(&picks),
        };
        match result {
            Ok(reading) => {
                self.status = Status::Done(Msg::Measured, vec![self.value_text(&reading)]);
                self.measurements.push(reading);
                let keep = self.ui.measure.keep as usize;
                if self.measurements.len() > keep {
                    let extra = self.measurements.len() - keep;
                    self.measurements.drain(..extra);
                }
            }
            Err(e) => self.status = Status::Error(e),
        }
        true
    }

    /// A length in millimetres as text, in the unit of the measurements.
    fn length_text(&self, millimetres: f64) -> String {
        let unit = self.ui.measure.unit.unwrap_or(self.settings.unit);
        match Length::new(drag::mm_to_nm(millimetres)) {
            Ok(length) => format_length(length, unit, self.ui.measure.decimals),
            Err(_) => format!("{millimetres} mm"),
        }
    }

    /// An angle in degrees as text.
    fn angle_text(&self, degrees: f64) -> String {
        format!(
            "{:.*}\u{b0}",
            self.ui.measure.angle_decimals as usize, degrees
        )
    }

    fn point_text(&self, p: Point3) -> String {
        format!(
            "{}, {}, {}",
            self.length_text(p[0]),
            self.length_text(p[1]),
            self.length_text(p[2])
        )
    }

    /// What a reference is, in words.
    pub fn reference_text(&self, reference: &Reference) -> String {
        let lang = self.lang;
        match (&reference.label, reference.geometry) {
            (Label::Hole(name), _) => fill(Msg::RefHole.tr(lang), &[name]),
            (Label::Post(name), _) => fill(Msg::RefPost.tr(lang), &[name]),
            (Label::OuterCorner, Geometry::Point(p)) => {
                fill(Msg::RefOuterCorner.tr(lang), &[&self.point_text(p)])
            }
            (Label::InnerCorner, Geometry::Point(p)) => {
                fill(Msg::RefInnerCorner.tr(lang), &[&self.point_text(p)])
            }
            (Label::OuterEdge, Geometry::Edge(a, b)) => fill(
                Msg::RefOuterEdge.tr(lang),
                &[&self.point_text(a), &self.point_text(b)],
            ),
            (Label::InnerEdge, Geometry::Edge(a, b)) => fill(
                Msg::RefInnerEdge.tr(lang),
                &[&self.point_text(a), &self.point_text(b)],
            ),
            _ => String::new(),
        }
    }

    /// The value of a reading with its unit.
    pub fn value_text(&self, reading: &Measurement) -> String {
        match reading.quantity {
            Quantity::Distance => self.length_text(reading.value),
            Quantity::Angle => self.angle_text(reading.value),
        }
    }

    /// A reading as a sentence for the panel.
    pub fn reading_text(&self, reading: &Measurement) -> String {
        let lang = self.lang;
        let names: Vec<String> = reading
            .refs
            .iter()
            .map(|r| self.reference_text(r))
            .collect();
        let value = self.value_text(reading);
        match (reading.quantity, names.as_slice()) {
            (Quantity::Distance, [a, b]) => {
                let [dx, dy, dz] = reading.delta;
                fill(
                    Msg::ReadDistance.tr(lang),
                    &[
                        a,
                        b,
                        &value,
                        &self.length_text(dx),
                        &self.length_text(dy),
                        &self.length_text(dz),
                    ],
                )
            }
            (Quantity::Angle, [a, b]) => fill(Msg::ReadAngleEdges.tr(lang), &[a, b, &value]),
            (Quantity::Angle, [a, v, b]) => fill(Msg::ReadAngle.tr(lang), &[a, v, b, &value]),
            _ => value,
        }
    }

    /// The marks the measurements add to the drawing: the lines of the readings, the references
    /// picked so far, and a small cross on each point.
    fn measure_segments(&self) -> Vec<Segment> {
        let half = mm(self.ui.measure.marker.get()) / 2.0;
        let cross = |out: &mut Vec<Segment>, p: Point3| {
            for d in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                out.push(Segment {
                    a: [p[0] - half * d[0], p[1] - half * d[1], p[2] - half * d[2]],
                    b: [p[0] + half * d[0], p[1] + half * d[1], p[2] + half * d[2]],
                    kind: Kind::Measure,
                });
            }
        };
        let mut out = Vec::new();
        for reading in &self.measurements {
            for (a, b) in &reading.lines {
                out.push(Segment {
                    a: *a,
                    b: *b,
                    kind: Kind::Measure,
                });
                cross(&mut out, *a);
                cross(&mut out, *b);
            }
        }
        for picked in &self.pending {
            match picked.geometry {
                Geometry::Point(p) => cross(&mut out, p),
                Geometry::Edge(a, b) => out.push(Segment {
                    a,
                    b,
                    kind: Kind::Measure,
                }),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm_len(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn example_path() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/box.phoenixproj"
        ))
    }

    fn fresh() -> Window {
        Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap()
    }

    fn example() -> Window {
        let mut w = fresh();
        assert!(w.open(&example_path()), "{:?}", w.status);
        w
    }

    /// The index of the field of a key.
    fn field(w: &Window, key: Key) -> usize {
        w.fields.iter().position(|f| f.key == key).unwrap()
    }

    fn set(w: &mut Window, key: Key, text: &str) {
        let i = field(w, key);
        w.type_in(i, text.to_string());
        w.commit(i);
    }

    /// A fresh folder for one test, removed at the end of it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("phoenix-cad-window-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_new_project_is_the_box_of_the_settings_and_measures_it() {
        let w = fresh();
        assert_eq!(w.info.outer, [mm_len(104), mm_len(64), mm_len(35)]);
        assert_eq!(w.info.inner, [mm_len(100), mm_len(60), mm_len(30)]);
        // 104 * 64 * 35 - 100 * 60 * 30 = 52 960 mm3
        assert_eq!(
            w.measures_text(),
            "Outside 104 x 64 x 35 mm - inside 100 x 60 x 30 mm - material 53.0 cm3"
        );
        // a box 100 x 60 x 30 holds 90 x 50 x 20 with 0.2 mm all round and 9.6 mm to spare
        assert_eq!(w.fit_text(), "It fits, with 9.6 / 9.6 / 9.6 mm to spare");
        assert_eq!(w.failed_text(), None);
        assert!(!w.dirty && !w.can_undo() && !w.can_redo());
        assert_eq!(w.selection, Selection::Body);
        assert_eq!(w.fields.len(), 8);
        assert_eq!(w.fields[0].text, "100.000 mm");
    }

    #[test]
    fn the_example_opens_and_every_step_is_applied() {
        let w = example();
        assert_eq!(w.mech.steps.len(), 8);
        assert!(
            w.states().iter().all(|s| *s == StepState::Valid),
            "{:?}",
            w.states()
        );
        assert!(!w.read_only && !w.dirty);
        let groups = w.tree();
        let sizes: Vec<(Msg, usize)> = groups.iter().map(|g| (g.title, g.items.len())).collect();
        assert_eq!(
            sizes,
            [
                (Msg::GroupBox, 1),
                (Msg::GroupVariables, 1),
                (Msg::GroupHoles, 2),
                (Msg::GroupPosts, 4),
                (Msg::GroupEdges, 1),
                (Msg::GroupLid, 1),
                (Msg::GroupScrews, 4),
                (Msg::GroupFit, 1),
            ]
        );
        // the board of the example is 60 x 40 mm and 1.6 + 5 mm high: 100 - 60 - 0.4 = 39.6
        assert!(w.info.fit_from_board);
        assert_eq!(w.fit_text(), "It fits, with 39.6 / 19.6 / 23 mm to spare");
    }

    #[test]
    fn the_length_of_the_box_comes_from_a_variable_in_the_example() {
        let mut w = example();
        assert_eq!(w.fields[0].text, "length");
        assert_eq!(w.fields[0].note, "100.000 mm");
        // a value typed over a variable makes the length a plain value
        set(&mut w, Key::Param(Param::InnerLength), "120");
        assert_eq!(w.info.inner[0], mm_len(120));
        assert_eq!(w.fields[0].text, "120.000 mm");
        assert!(w.dirty && w.can_undo());
    }

    #[test]
    fn a_wall_that_is_changed_regenerates_the_box_and_the_fit() {
        let mut w = example();
        set(&mut w, Key::Param(Param::Wall), "3 mm");
        assert_eq!(w.info.outer, [mm_len(106), mm_len(66), mm_len(35)]);
        assert_eq!(
            w.status,
            Status::Done(Msg::ChangeOf, vec!["Wall".to_string()])
        );
        assert_eq!(w.undo_names(), ["Change of Wall"]);
        // the fit is checked again with the new box (the inner space did not change)
        assert_eq!(w.fit_text(), "It fits, with 39.6 / 19.6 / 23 mm to spare");
        // a box too small for the board says so
        set(&mut w, Key::Param(Param::InnerLength), "50 mm");
        assert!(!w.info.fit.fits);
        assert!(w.fit_text().starts_with("It does not fit"));
    }

    #[test]
    fn a_formula_follows_what_it_names() {
        let mut w = example();
        set(&mut w, Key::Param(Param::Floor), "wall");
        assert_eq!(w.fields[field(&w, Key::Param(Param::Floor))].text, "wall");
        assert_eq!(
            w.fields[field(&w, Key::Param(Param::Floor))].note,
            "2.000 mm"
        );
        set(&mut w, Key::Param(Param::Wall), "3 mm");
        // the floor is the wall: 30 + 3 + 3 high
        assert_eq!(w.info.outer[2], mm_len(36));
        let note = &w.fields[field(&w, Key::Param(Param::Floor))].note;
        assert_eq!(note, "3.000 mm");
    }

    #[test]
    fn a_value_the_model_refuses_leaves_the_project_as_it_was() {
        let mut w = example();
        let before = w.mech.clone();
        set(&mut w, Key::Param(Param::Wall), "0 mm");
        assert!(w.status.is_problem());
        assert_eq!(w.mech, before);
        assert!(!w.dirty && !w.can_undo());
        // the field shows the value it had
        assert_eq!(
            w.fields[field(&w, Key::Param(Param::Wall))].text,
            "2.000 mm"
        );
        // nonsense is refused with the code of the library, in the language of the window
        set(&mut w, Key::Param(Param::Wall), "2 +");
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-023"));
        w.lang = Lang::Es;
        assert!(w
            .status
            .text(Lang::Es)
            .starts_with("Sin cambios: PHOENIX-CAD-023"));
        assert_eq!(w.mech, before);
    }

    #[test]
    fn a_hole_is_edited_and_moved_to_where_the_model_refuses_it() {
        let mut w = example();
        w.select(Selection::Hole(0));
        assert_eq!(w.fields.len(), 5);
        assert_eq!(w.fields[field(&w, Key::U)].text, "30.000 mm");
        set(&mut w, Key::U, "40");
        assert_eq!(w.fields[field(&w, Key::U)].text, "40.000 mm");
        assert_eq!(w.states()[0], StepState::Valid);
        // 1 mm from the edge is closer than the margin of 3 mm
        let kept = w.mech.clone();
        set(&mut w, Key::U, "1 mm");
        assert!(w
            .status
            .text(Lang::En)
            .starts_with("Not changed: PHOENIX-CAD-003"));
        assert_eq!(w.mech, kept);
        // the hole goes to another face
        let face = field(&w, Key::Face);
        w.choose(face, "top");
        assert_eq!(w.fields[field(&w, Key::Face)].text, "top");
        assert_eq!(w.states()[0], StepState::Valid);
    }

    #[test]
    fn a_post_and_the_edges_and_the_lid_have_their_own_fields() {
        let mut w = example();
        w.select(Selection::Post(3));
        assert_eq!(w.fields.len(), 7);
        set(&mut w, Key::PostHeight, "8 mm");
        assert_eq!(w.states()[3], StepState::Valid);
        w.select(Selection::Edging(2));
        assert_eq!(w.fields.len(), 6);
        assert_eq!(w.fields[0].text, "round");
        assert_eq!(w.fields[1].text, "1.500 mm");
        assert!(w.fields[1].editable && !w.fields[3].editable);
        // sharp to chamfered takes the size of the settings
        let kind = field(&w, Key::EdgeKind(1));
        w.choose(kind, "chamfer");
        assert_eq!(w.fields[field(&w, Key::EdgeKind(1))].text, "chamfer");
        assert_eq!(w.fields[field(&w, Key::EdgeSize(1))].text, "1.000 mm");
        w.select(Selection::Lid(7));
        assert_eq!(w.fields.len(), 4);
        set(&mut w, Key::LipDepth, "4 mm");
        assert_eq!(w.states()[7], StepState::Valid);
    }

    #[test]
    fn the_screws_of_the_lid_are_derived_and_cannot_be_edited() {
        let mut w = example();
        w.select(Selection::Screw(0));
        assert_eq!(w.fields.len(), 3);
        assert!(w.fields.iter().all(|f| !f.editable));
        // over the first post, 2 mm of wall and 25 mm along, 2 mm of wall and 15 mm across
        assert_eq!(w.fields[0].text, "27.000 mm");
        assert_eq!(w.fields[1].text, "17.000 mm");
        assert_eq!(w.highlight(), Highlight::Screw(0));
        w.select(Selection::Screw(9));
        assert_eq!(w.selection, Selection::Body);
    }

    #[test]
    fn a_step_can_be_suppressed_and_brought_back() {
        let mut w = example();
        w.select(Selection::Hole(1));
        assert!(!w.selected_suppressed());
        w.toggle_suppressed();
        assert!(w.selected_suppressed());
        assert_eq!(w.states()[1], StepState::Suppressed);
        assert_eq!(w.tree()[2].items[1].mark, Mark::Suppressed);
        assert_eq!(w.enclosure().holes().len(), 1);
        w.undo();
        assert_eq!(w.enclosure().holes().len(), 2);
        assert_eq!(w.states()[1], StepState::Valid);
    }

    #[test]
    fn the_history_goes_back_and_forward_and_forgets_on_a_new_change() {
        let mut w = example();
        set(&mut w, Key::Param(Param::Wall), "3 mm");
        set(&mut w, Key::Param(Param::Lid), "4 mm");
        assert_eq!(w.info.outer, [mm_len(106), mm_len(66), mm_len(36)]);
        w.undo();
        assert_eq!(w.info.outer, [mm_len(106), mm_len(66), mm_len(35)]);
        assert_eq!(
            w.status,
            Status::Done(Msg::Undone, vec!["Change of Lid thickness".to_string()])
        );
        w.undo();
        assert_eq!(w.info.outer, [mm_len(104), mm_len(64), mm_len(35)]);
        assert!(!w.can_undo() && w.can_redo());
        w.undo();
        assert_eq!(w.status, Status::Done(Msg::NothingToUndo, Vec::new()));
        w.redo();
        assert_eq!(w.info.outer, [mm_len(106), mm_len(66), mm_len(35)]);
        // a new change forgets the one that could have been redone
        set(&mut w, Key::Param(Param::Floor), "3 mm");
        assert!(!w.can_redo());
        w.redo();
        assert_eq!(w.status, Status::Done(Msg::NothingToRedo, Vec::new()));
        assert_eq!(w.info.outer[2], mm_len(36));
    }

    #[test]
    fn the_envelope_is_typed_when_the_project_has_no_board() {
        let mut w = fresh();
        w.select(Selection::Fit);
        assert!(w.fields.iter().all(|f| f.editable));
        set(&mut w, Key::Fit(0), "99 mm");
        assert_eq!(w.manual[0], mm_len(99));
        // 100 - 99 - 0.4 = 0.6 mm
        assert_eq!(w.fit_text(), "It fits, with 0.6 / 9.6 / 9.6 mm to spare");
        set(&mut w, Key::Fit(0), "100 mm");
        assert!(w.fit_text().starts_with("It does not fit"));
        set(&mut w, Key::Fit(1), "0 mm");
        assert!(w.status.is_problem());
        assert_eq!(w.manual[1], mm_len(50));
        // it is not a change of the project
        assert!(!w.dirty && !w.can_undo());
        // with a board the envelope is the board's and cannot be typed
        let mut with_board = example();
        with_board.select(Selection::Fit);
        assert!(with_board.fields.iter().all(|f| !f.editable));
    }

    #[test]
    fn a_project_is_saved_and_read_back_the_same() {
        let dir = Scratch::new("save");
        let mut w = example();
        set(&mut w, Key::Param(Param::Wall), "3 mm");
        let path = dir.0.join("changed.phoenixproj");
        assert!(w.save_to(&path), "{:?}", w.status);
        assert!(!w.dirty);
        assert_eq!(w.path.as_deref(), Some(path.as_path()));
        let mut other = fresh();
        assert!(other.open(&path), "{:?}", other.status);
        assert_eq!(other.mech, w.mech);
        assert_eq!(other.info.outer, [mm_len(106), mm_len(66), mm_len(35)]);
        // saving again keeps the copy of the settings
        assert!(w.save());
        assert!(crate::store::backup_path(&path, 1).exists());
    }

    #[test]
    fn a_file_that_is_not_a_project_is_refused_and_the_project_stays() {
        let dir = Scratch::new("bad");
        let path = dir.0.join("bad.phoenixproj");
        std::fs::write(&path, b"this is not a project").unwrap();
        let mut w = example();
        let before = w.mech.clone();
        assert!(!w.open(&path));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-018"));
        assert_eq!(w.mech, before);
        assert!(!w.open(&dir.0.join("missing.phoenixproj")));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-012"));
    }

    #[test]
    fn a_project_with_a_part_that_is_not_understood_is_read_only() {
        let dir = Scratch::new("readonly");
        let mut doc = Document::new();
        doc.set_mechanical(&fresh().mech, &Settings::defaults().mechanical)
            .unwrap();
        doc.set("thermal", true, b"something newer".to_vec())
            .unwrap();
        let path = dir.0.join("newer.phoenixproj");
        std::fs::write(&path, doc.to_bytes()).unwrap();
        let mut w = fresh();
        assert!(w.open(&path));
        assert!(w.read_only);
        assert_eq!(w.status, Status::Problem(Msg::ReadOnlyOpened, Vec::new()));
        assert!(w.fields.iter().all(|f| !f.editable));
        let before = w.mech.clone();
        set(&mut w, Key::Param(Param::Wall), "3 mm");
        assert_eq!(w.mech, before);
        assert!(!w.save_to(&dir.0.join("copy.phoenixproj")));
    }

    #[test]
    fn a_project_without_a_mechanical_part_is_refused() {
        let dir = Scratch::new("empty");
        let path = dir.0.join("empty.phoenixproj");
        std::fs::write(&path, Document::new().to_bytes()).unwrap();
        let mut w = fresh();
        assert!(!w.open(&path));
        assert_eq!(w.status, Status::Problem(Msg::NoMechanical, Vec::new()));
    }

    #[test]
    fn a_new_project_replaces_the_one_that_was_open() {
        let mut w = example();
        set(&mut w, Key::Param(Param::Wall), "3 mm");
        w.new_project();
        assert_eq!(w.mech.steps.len(), 0);
        assert!(w.path.is_none() && !w.dirty && !w.can_undo());
        assert_eq!(w.info.outer, [mm_len(104), mm_len(64), mm_len(35)]);
        assert_eq!(w.status, Status::Done(Msg::NewProject, Vec::new()));
    }

    #[test]
    fn the_views_turn_zoom_and_come_back() {
        let mut w = fresh();
        let start = w.view3d.camera;
        assert_eq!(
            start,
            Camera {
                yaw: 35.0,
                pitch: 28.0
            }
        );
        // 100 px to the right at 0.4 degrees for each pixel
        w.rotate_3d(100.0, -50.0);
        assert!((w.view3d.camera.yaw - 75.0).abs() < 1e-9);
        assert!((w.view3d.camera.pitch - 8.0).abs() < 1e-9);
        w.rotate_3d(0.0, -1000.0);
        assert_eq!(w.view3d.camera.pitch, 0.0);
        w.rotate_3d(0.0, 100_000.0);
        assert_eq!(w.view3d.camera.pitch, 90.0);
        // the zoom stays inside its limits
        w.wheel(ViewKind::Iso, 1.0e6);
        assert_eq!(w.view3d.zoom, 12.0);
        w.wheel(ViewKind::Plan, -1.0e6);
        assert_eq!(w.plan.zoom, 0.2);
        w.control_mut(ViewKind::Plan).pan_by(5.0, 6.0);
        assert_eq!(w.plan.pan, (5.0, 6.0));
        w.control_mut(ViewKind::Iso).reset();
        w.control_mut(ViewKind::Plan).reset();
        assert_eq!(w.view3d.camera, start);
        assert_eq!(
            (w.view3d.zoom, w.plan.zoom, w.plan.pan),
            (1.0, 1.0, (0.0, 0.0))
        );
        // the yaw wraps round
        w.rotate_3d(1000.0, 0.0);
        assert!(w.view3d.camera.yaw > -180.0 && w.view3d.camera.yaw <= 180.0);
    }

    #[test]
    fn the_views_have_lines_inside_their_pane() {
        let w = example();
        for view in [ViewKind::Iso, ViewKind::Plan] {
            let lines = w.lines(view, (600.0, 400.0));
            assert!(lines.len() > 100);
            for l in &lines {
                for p in [l.a, l.b] {
                    assert!(
                        p.0 > -1.0 && p.0 < 601.0 && p.1 > -1.0 && p.1 < 401.0,
                        "{p:?}"
                    );
                }
            }
        }
        // the plan shows the length to the right: the outer box spans 104 mm in x
        let plan = w.lines(ViewKind::Plan, (600.0, 400.0));
        let widest = plan.iter().map(|l| l.a.0.max(l.b.0)).fold(0.0f64, f64::max);
        let narrowest = plan
            .iter()
            .map(|l| l.a.0.min(l.b.0))
            .fold(600.0f64, f64::min);
        // the pane is 600 px wide with a margin of 24 on each side: the 104 mm of the length
        // take 552 px (and the 64 mm of the width take 340 px, which fit in the 352 left)
        assert!(
            (widest - narrowest - 552.0).abs() < 1e-6,
            "{}",
            widest - narrowest
        );
    }

    #[test]
    fn the_board_box_sits_on_the_tallest_post() {
        let w = example();
        let b = w.board_box().unwrap();
        // centred: (100 - 60) / 2 = 20 mm and (60 - 40) / 2 = 10 mm from the walls of 2 mm; on
        // the posts, 2 mm of floor and 6 mm of post
        assert_eq!(b.min, [22.0, 12.0, 8.0]);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(b.max[0], 82.0) && close(b.max[1], 52.0) && close(b.max[2], 14.6));
    }

    #[test]
    fn errors_are_told_with_their_code_and_the_title_of_the_language() {
        let e = CadError::HoleOutsideFace("usb".into());
        assert_eq!(
            error_text(&e, Lang::En),
            "PHOENIX-CAD-003 Hole outside its face (hole usb)"
        );
        assert!(error_text(&e, Lang::De).starts_with("PHOENIX-CAD-003 "));
        assert_eq!(
            error_text(&CadError::ExprOverflow { pos: 3 }, Lang::En),
            "PHOENIX-CAD-026 Expression arithmetic overflows"
        );
    }

    /// The recipe of a window put straight into it, built again (a test sets up what the
    /// window never makes by itself).
    fn rebuild(w: &mut Window) {
        w.built = w
            .mech
            .build(&w.settings.constants, &w.settings.expr)
            .unwrap();
        w.refresh();
    }

    fn hole_of(w: &Window, index: usize) -> Hole {
        match &w.mech.steps[index].op {
            Operation::AddHole { hole, .. } => hole.clone(),
            other => panic!("not a hole: {other:?}"),
        }
    }

    fn post_of(w: &Window, index: usize) -> Post {
        match &w.mech.steps[index].op {
            Operation::AddPost { post, .. } => post.clone(),
            other => panic!("not a post: {other:?}"),
        }
    }

    fn problem(w: &Window) -> bool {
        w.status.is_problem()
    }

    #[test]
    fn the_sheet_lists_the_variables_and_the_parameters_that_use_them() {
        let w = example();
        let rows = w.sheet();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["length", "width"]);
        assert_eq!(rows[0].text, "100.000 mm");
        assert_eq!(rows[0].value, mm_len(100));
        assert_eq!(rows[0].used_by, [Param::InnerLength]);
        assert_eq!(rows[1].used_by, [Param::InnerWidth]);
        assert_eq!(w.used_by_text(&rows[0]), "Inner length");
        let groups = w.tree();
        assert_eq!(groups[1].title, Msg::GroupVariables);
        assert_eq!(groups[1].items[0].selection, Selection::Sheet);
        assert_eq!(groups[1].items[0].label, "Sheet of variables (2)");
    }

    #[test]
    fn the_sheet_is_selected_without_fields_or_a_step() {
        let mut w = example();
        w.select(Selection::Sheet);
        assert_eq!(w.selection, Selection::Sheet);
        assert_eq!(w.selection.step(), None);
        assert!(w.fields.is_empty());
    }

    #[test]
    fn a_variable_that_is_changed_recalculates_what_depends_on_it() {
        let mut w = example();
        w.set_variable("length", "120 mm");
        // the inner length 120 and two walls of 2 mm
        assert_eq!(w.info.inner[0], mm_len(120));
        assert_eq!(w.info.outer[0], mm_len(124));
        assert_eq!(
            w.status,
            Status::Done(
                Msg::VariableChanged,
                vec!["length".to_string(), "120.000 mm".to_string()]
            )
        );
        assert_eq!(w.sheet()[0].value, mm_len(120));
        assert!(w.dirty && w.can_undo());
        w.undo();
        assert_eq!(w.info.inner[0], mm_len(100));
        assert_eq!(w.sheet()[0].value, mm_len(100));
        assert_eq!(
            w.status,
            Status::Done(
                Msg::Undone,
                vec!["Variable length = 120.000 mm".to_string()]
            )
        );
        w.redo();
        assert_eq!(w.info.inner[0], mm_len(120));
    }

    #[test]
    fn a_variable_can_be_a_formula_over_the_others_and_over_numbers() {
        let mut w = example();
        // 60 * 2 + 10 = 130
        w.set_variable("length", "width * 2 + 10 mm");
        assert_eq!(w.sheet()[0].value, mm_len(130));
        assert_eq!(w.info.inner[0], mm_len(130));
        // a formula of numbers only is in the unit of the settings: 100 / 2 + 70 = 120
        w.set_variable("length", "100 / 2 + 70");
        assert_eq!(w.sheet()[0].value, mm_len(120));
        assert_eq!(w.sheet()[0].text, "120.000 mm");
        // a value in another unit: 5 in = 127 mm
        w.set_variable("length", "5 in");
        assert_eq!(w.sheet()[0].value, mm_len(127));
    }

    #[test]
    fn a_value_the_model_refuses_leaves_the_variable_as_it_was() {
        let mut w = example();
        let before = w.mech.clone();
        for text in ["0 mm", "nothing + 1 mm", "width +", "2 +* 3"] {
            w.set_variable("length", text);
            assert!(problem(&w), "{text}");
            assert_eq!(w.mech, before, "{text}");
        }
        assert!(!w.dirty && !w.can_undo());
        // a name that is not a word
        w.add_variable("2fast", "5 mm");
        assert!(problem(&w));
        assert_eq!(w.mech, before);
    }

    #[test]
    fn a_variable_is_added_used_and_removed_only_when_nothing_needs_it() {
        let mut w = example();
        w.add_variable("gap", "5 mm");
        let names: Vec<String> = w.sheet().into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["gap", "length", "width"]);
        assert_eq!(w.used_by_text(&w.sheet()[0]), "not used");
        // adding it again is refused and the value stays
        w.add_variable("gap", "9 mm");
        assert!(problem(&w));
        assert_eq!(w.sheet()[0].value, mm_len(5));
        // the wall takes its value from the variable, the lid from the wall
        w.select(Selection::Body);
        set(&mut w, Key::Param(Param::Wall), "gap");
        set(&mut w, Key::Param(Param::Lid), "wall + 1 mm");
        assert_eq!(w.info.outer, [mm_len(110), mm_len(70), mm_len(30 + 2 + 6)]);
        assert_eq!(w.sheet()[0].used_by, [Param::Wall, Param::Lid]);
        w.remove_variable("gap");
        assert!(problem(&w));
        assert_eq!(w.sheet()[0].name, "gap");
        // changing the variable moves the wall and the lid: 3 and 4 mm (the floor stays 2 mm)
        w.set_variable("gap", "3 mm");
        assert_eq!(w.info.outer, [mm_len(106), mm_len(66), mm_len(30 + 2 + 4)]);
        // a variable nothing uses goes, and comes back with undo
        w.add_variable("spare", "1 mm");
        assert_eq!(w.sheet().len(), 4);
        w.remove_variable("spare");
        assert_eq!(w.sheet().len(), 3);
        w.undo();
        assert_eq!(w.sheet().len(), 4);
        // one that does not exist is refused
        w.remove_variable("ghost");
        assert!(problem(&w));
    }

    #[test]
    fn a_hole_is_added_where_the_settings_say_and_moves_along_when_taken() {
        let mut w = fresh();
        w.add_step(NewStep::Hole);
        assert_eq!(w.mech.steps.len(), 1);
        assert_eq!(w.mech.tip, 1);
        assert_eq!(w.selection, Selection::Hole(0));
        assert_eq!(
            w.status,
            Status::Done(Msg::StepAdded, vec!["Hole 1".to_string()])
        );
        let first = hole_of(&w, 0);
        assert_eq!(first.name, "Hole 1");
        assert_eq!(first.face, Face::Front);
        assert_eq!(
            (first.u, first.v, first.diameter),
            (mm_len(14), mm_len(15), mm_len(6))
        );
        assert_eq!(w.fields.len(), 5);
        assert!(w.dirty && w.can_undo());
        // the same place is taken: the next try is one pitch (12 mm) along
        w.add_step(NewStep::Hole);
        assert_eq!(w.mech.steps.len(), 2);
        assert_eq!(hole_of(&w, 1).u, mm_len(26));
        assert_eq!(w.selection, Selection::Hole(1));
        assert_eq!(w.enclosure().holes().len(), 2);
        let ids: Vec<Id> = w
            .mech
            .steps
            .iter()
            .filter_map(|s| match &s.op {
                Operation::AddHole { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_ne!(ids[0], ids[1]);
        // one undo takes back one addition, and redo brings it again
        w.undo();
        assert_eq!(w.mech.steps.len(), 1);
        assert_eq!(w.selection, Selection::Body);
        assert_eq!(w.enclosure().holes().len(), 1);
        w.redo();
        assert_eq!(w.mech.steps.len(), 2);
        assert_eq!(w.enclosure().holes().len(), 2);
    }

    #[test]
    fn a_post_is_added_and_the_next_one_moves_along() {
        let mut w = fresh();
        w.add_step(NewStep::Post);
        let first = post_of(&w, 0);
        assert_eq!(first.name, "Post 1");
        assert_eq!((first.x, first.y), (mm_len(10), mm_len(10)));
        assert_eq!(first.board_hole, None);
        assert_eq!(w.selection, Selection::Post(0));
        w.add_step(NewStep::Post);
        assert_eq!(post_of(&w, 1).x, mm_len(22));
        assert_eq!(w.enclosure().posts().len(), 2);
        assert_eq!(w.tree()[2].title, Msg::GroupPosts);
        assert_eq!(w.tree()[2].items.len(), 2);
    }

    #[test]
    fn the_edge_treatment_is_added_once() {
        let mut w = fresh();
        w.add_step(NewStep::Edging);
        assert_eq!(w.selection, Selection::Edging(0));
        assert_eq!(
            w.status,
            Status::Done(Msg::StepAdded, vec!["Edge treatment".to_string()])
        );
        assert_eq!(w.enclosure().edging().vertical, Treatment::Round(mm_len(1)));
        assert!(w.enclosure().edging().top.is_sharp());
        w.add_step(NewStep::Edging);
        assert!(problem(&w));
        assert_eq!(w.mech.steps.len(), 1);
    }

    #[test]
    fn a_step_added_behind_the_tip_goes_at_the_tip() {
        let mut w = example();
        w.mech.tip = 3;
        rebuild(&mut w);
        assert_eq!(w.states()[3], StepState::AfterTip);
        w.add_step(NewStep::Hole);
        assert_eq!(w.mech.steps.len(), 9);
        assert_eq!(w.mech.tip, 4);
        assert_eq!(hole_of(&w, 3).name, "Hole 3");
        assert_eq!(w.states()[3], StepState::Valid);
        assert_eq!(w.states()[4], StepState::AfterTip);
        assert_eq!(w.selection, Selection::Hole(3));
        w.undo();
        assert_eq!(w.mech.tip, 3);
        assert_eq!(w.mech.steps.len(), 8);
    }

    #[test]
    fn a_step_is_removed_with_one_undo_to_bring_it_back() {
        let mut w = fresh();
        w.add_step(NewStep::Hole);
        w.add_step(NewStep::Post);
        w.select(Selection::Hole(0));
        w.remove_step();
        assert_eq!(w.mech.steps.len(), 1);
        assert_eq!(w.mech.tip, 1);
        assert_eq!(w.selection, Selection::Body);
        assert_eq!(w.enclosure().holes().len(), 0);
        assert_eq!(w.enclosure().posts().len(), 1);
        assert_eq!(
            w.status,
            Status::Done(Msg::StepRemoved, vec!["Hole 1".to_string()])
        );
        w.undo();
        assert_eq!(w.mech.steps.len(), 2);
        assert_eq!(w.mech.tip, 2);
        assert_eq!(w.enclosure().holes().len(), 1);
        // nothing selected that is a step: refused
        w.select(Selection::Body);
        let before = w.mech.clone();
        w.remove_step();
        assert!(problem(&w));
        assert_eq!(w.mech, before);
    }

    #[test]
    fn a_step_that_a_later_step_needs_is_not_removed() {
        let mut w = fresh();
        w.add_step(NewStep::Hole);
        let (id, hole) = match &w.mech.steps[0].op {
            Operation::AddHole { id, hole } => (*id, hole.clone()),
            other => panic!("not a hole: {other:?}"),
        };
        w.mech.steps.push(Step {
            name: "wider".to_string(),
            op: Operation::ReplaceHole { id, hole },
            suppressed: false,
        });
        w.mech.tip = 2;
        rebuild(&mut w);
        assert!(w.states().iter().all(|s| *s == StepState::Valid));
        w.select(Selection::Hole(0));
        let before = w.mech.clone();
        w.remove_step();
        assert!(problem(&w));
        assert_eq!(w.mech, before);
    }

    #[test]
    fn a_read_only_project_gets_no_new_steps_or_variables() {
        let mut w = fresh();
        w.read_only = true;
        w.add_step(NewStep::Hole);
        assert_eq!(w.status, Status::Problem(Msg::ReadOnlyBadge, Vec::new()));
        w.add_variable("gap", "5 mm");
        w.set_variable("gap", "5 mm");
        w.remove_variable("gap");
        w.remove_step();
        assert_eq!(w.status, Status::Problem(Msg::ReadOnlyBadge, Vec::new()));
        assert!(w.mech.steps.is_empty() && w.mech.vars.is_empty());
        assert!(!w.dirty);
    }

    #[test]
    fn the_positions_tried_come_from_the_settings() {
        let ui = UiSettings::with_user_config(
            "[new_step]\ntries = 1\nhole_face = left\nhole_u = 20 mm\nhole_diameter = 4 mm\n",
        )
        .unwrap();
        let mut w = Window::new(Lang::En, Settings::defaults(), ui).unwrap();
        w.add_step(NewStep::Hole);
        let hole = hole_of(&w, 0);
        assert_eq!(
            (hole.face, hole.u, hole.diameter),
            (Face::Left, mm_len(20), mm_len(4))
        );
        // one try only: the same place is refused
        w.add_step(NewStep::Hole);
        assert!(problem(&w));
        assert_eq!(w.mech.steps.len(), 1);
    }

    #[test]
    fn a_project_with_added_steps_and_variables_is_saved_and_read_again() {
        let dir = Scratch::new("added");
        let path = dir.0.join("added.phoenixproj");
        let mut w = fresh();
        w.add_variable("gap", "5 mm");
        w.add_step(NewStep::Hole);
        w.add_step(NewStep::Post);
        w.add_step(NewStep::Edging);
        assert!(w.save_to(&path), "{:?}", w.status);
        let mut again = fresh();
        assert!(again.open(&path), "{:?}", again.status);
        assert_eq!(again.mech, w.mech);
        assert_eq!(again.sheet()[0].value, mm_len(5));
        assert!(again.states().iter().all(|s| *s == StepState::Valid));
    }

    /// A sketch on the lid: a rectangle of 40 by 25 mm and a hole of diameter 6 mm in it, with
    /// its origin at (10, 10) mm of the face.
    fn plate() -> Sketch {
        let plane = crate::sketch::SketchPlane {
            face: Face::Top,
            u0: mm_len(10),
            v0: mm_len(10),
            offset: Length::ZERO,
        };
        let mut k = Sketch::new("plate", plane).unwrap();
        k.add_rectangle((mm_len(0), mm_len(0)), (mm_len(40), mm_len(25)))
            .unwrap();
        let c = k.add_point(mm_len(20), mm_len(12)).unwrap();
        k.add_circle(c, mm_len(3)).unwrap();
        k
    }

    /// The material of the empty box (52 960 mm3) with `extra` mm3 more, in cubic nanometres.
    fn material(extra_mm3: f64) -> f64 {
        (52_960.0 + extra_mm3) * 1e18
    }

    fn area_mm2() -> f64 {
        1000.0 - 9.0 * std::f64::consts::PI
    }

    fn close(a: i128, b: f64) -> bool {
        (a as f64 - b).abs() < 1e8
    }

    #[test]
    fn an_extrusion_needs_a_sketch_and_then_changes_the_tree_the_volume_and_the_history() {
        let mut w = fresh();
        w.add_step(NewStep::Extrude);
        assert!(problem(&w));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-037"));
        assert!(w.mech.steps.is_empty() && !w.can_undo());
        w.keep_sketch(plate());
        assert_eq!(w.sketch_names(), ["plate"]);
        assert_eq!(
            w.status,
            Status::Done(Msg::SketchKept, vec!["plate".into()])
        );
        w.add_step(NewStep::Extrude);
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(w.selection, Selection::Feature(0));
        let groups = w.tree();
        let features = groups
            .iter()
            .find(|g| g.title == Msg::GroupFeatures)
            .unwrap();
        assert_eq!(features.items.len(), 1);
        assert_eq!(features.items[0].label, "Extrusion 1");
        let keys: Vec<Key> = w.fields.iter().map(|f| f.key).collect();
        assert_eq!(keys, [Key::Sketch, Key::Height]);
        // the box and 5 mm of the plate: 52 960 + 5 * (1000 - 9 pi) mm3
        assert!(close(w.info.material_nm3, material(5.0 * area_mm2())));
        set(&mut w, Key::Height, "10 mm");
        assert!(
            close(w.info.material_nm3, material(10.0 * area_mm2())),
            "{}",
            w.info.material_nm3
        );
        assert!(
            w.measures_text().contains("material 62.7 cm3"),
            "{}",
            w.measures_text()
        );
        // the lines of the views have the outline and its height
        let with = w.lines(ViewKind::Iso, (400.0, 300.0)).len();
        w.undo();
        w.undo();
        w.undo();
        assert!(w.mech.steps.is_empty() && w.mech.sketches.is_empty());
        assert!(w.lines(ViewKind::Iso, (400.0, 300.0)).len() < with);
        w.redo();
        w.redo();
        w.redo();
        assert!(close(w.info.material_nm3, material(10.0 * area_mm2())));
    }

    #[test]
    fn a_pocket_goes_through_or_to_a_depth_and_takes_its_volume_away() {
        let mut w = fresh();
        w.keep_sketch(plate());
        w.add_step(NewStep::Pocket);
        assert!(!problem(&w), "{:?}", w.status);
        let depth = field(&w, Key::PocketDepth);
        assert_eq!(w.fields[depth].text, "through");
        // the lid is 3 mm thick
        assert!(close(w.info.material_nm3, material(-3.0 * area_mm2())));
        set(&mut w, Key::PocketDepth, "2 mm");
        assert!(close(w.info.material_nm3, material(-2.0 * area_mm2())));
        // deeper than the lid is refused and the pocket stays as it was
        set(&mut w, Key::PocketDepth, "4 mm");
        assert!(problem(&w));
        assert!(close(w.info.material_nm3, material(-2.0 * area_mm2())));
        set(&mut w, Key::PocketDepth, "through");
        assert!(matches!(
            w.mech.steps[0].op,
            Operation::Pocket { depth: None, .. }
        ));
        // a name that is not a name of a sketch is refused too
        set(&mut w, Key::Sketch, "no such sketch");
        assert!(problem(&w));
        set(&mut w, Key::Sketch, "other");
        assert!(problem(&w));
        assert_eq!(w.sketch_names(), ["plate"]);
    }

    #[test]
    fn a_change_of_the_sketch_changes_the_steps_that_use_it_and_a_used_sketch_cannot_be_removed() {
        let mut w = fresh();
        w.keep_sketch(plate());
        w.add_step(NewStep::Extrude);
        let mut small = plate();
        small.set_plane(crate::sketch::SketchPlane {
            face: Face::Top,
            u0: mm_len(10),
            v0: mm_len(10),
            offset: Length::ZERO,
        });
        let c = small.add_point(mm_len(5), mm_len(5)).unwrap();
        small.add_circle(c, mm_len(2)).unwrap();
        w.keep_sketch(small);
        assert_eq!(w.sketch_names(), ["plate"]);
        // a second hole of diameter 4 mm: 5 * pi * 2^2 less
        let expect = material(5.0 * (area_mm2() - 4.0 * std::f64::consts::PI));
        assert!(close(w.info.material_nm3, expect));
        w.remove_sketch("plate");
        assert!(problem(&w));
        assert_eq!(w.sketch_names(), ["plate"]);
        w.select(Selection::Feature(0));
        w.remove_step();
        w.remove_sketch("plate");
        assert!(!problem(&w), "{:?}", w.status);
        assert!(w.sketch_names().is_empty());
    }

    #[test]
    fn the_sketches_and_the_steps_go_to_the_project_file_and_come_back() {
        let dir = Scratch::new("sketches");
        let path = dir.0.join("with.phoenixproj");
        let mut w = fresh();
        w.keep_sketch(plate());
        w.add_step(NewStep::Extrude);
        w.add_step(NewStep::Pocket);
        assert!(w.save_to(&path), "{:?}", w.status);
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes).to_string();
        assert!(text.contains("domain sketch optional "));
        assert!(text.contains("mechanical 2\n"));
        let mut again = fresh();
        assert!(again.open(&path), "{:?}", again.status);
        assert_eq!(again.mech, w.mech);
        assert!(again.states().iter().all(|s| *s == StepState::Valid));
        assert_eq!(again.info.material_nm3, w.info.material_nm3);
        // saving it again gives the same file
        assert!(again.save_to(&path));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        // a project with no sketch has no such domain and stays in version 1
        let plain = dir.0.join("plain.phoenixproj");
        let mut p = fresh();
        p.add_step(NewStep::Hole);
        assert!(p.save_to(&plain));
        let plain_text = String::from_utf8_lossy(&std::fs::read(&plain).unwrap()).to_string();
        assert!(!plain_text.contains("domain sketch") && plain_text.contains("mechanical 1\n"));
        // the example, written before the sketches existed, opens as it did
        let old = example();
        assert!(old.mech.sketches.is_empty() && !old.read_only);
    }

    #[test]
    fn sketches_that_cannot_be_read_open_the_project_read_only_and_are_kept_as_they_are() {
        let dir = Scratch::new("damaged");
        let path = dir.0.join("with.phoenixproj");
        let mut w = fresh();
        w.keep_sketch(plate());
        w.add_step(NewStep::Extrude);
        assert!(w.save_to(&path));
        let good = Document::from_bytes(&std::fs::read(&path).unwrap(), 1 << 24).unwrap();
        for (what, data) in [
            ("damaged", b"hello\n".to_vec()),
            ("newer", b"sketch 2\nwhat comes later \xff\n".to_vec()),
        ] {
            let mut doc = good.clone();
            doc.set("sketch", false, data.clone()).unwrap();
            let broken = dir.0.join(format!("{what}.phoenixproj"));
            std::fs::write(&broken, doc.to_bytes()).unwrap();
            let before = std::fs::read(&broken).unwrap();
            let mut opened = fresh();
            assert!(opened.open(&broken), "{what}: {:?}", opened.status);
            assert!(opened.read_only, "{what}");
            match &opened.status {
                Status::Error(e) => assert_eq!(e.code(), "PHOENIX-CAD-035", "{what}"),
                other => panic!("{what}: {other:?}"),
            }
            // the steps that need the sketch say they failed, and nothing can be written over it
            assert!(matches!(opened.states()[0], StepState::Failed(_)), "{what}");
            assert!(!opened.save_to(&broken));
            assert_eq!(std::fs::read(&broken).unwrap(), before, "{what}");
        }
    }

    #[test]
    fn a_sketch_file_is_imported_into_the_project_and_a_sketch_is_exported_to_one() {
        let dir = Scratch::new("files");
        let file = dir.0.join("plate.phoenixsketch");
        let mut w = fresh();
        w.keep_sketch(plate());
        assert!(w.export_sketch_file("plate", &file), "{:?}", w.status);
        assert!(!w.export_sketch_file("nothing", &file));
        let mut other = fresh();
        assert!(other.import_sketch_file(&file), "{:?}", other.status);
        assert_eq!(other.mech.sketches, w.mech.sketches);
        assert!(other.dirty && other.can_undo());
        // a file that is not a sketch is refused and the project is as it was
        std::fs::write(dir.0.join("bad.phoenixsketch"), "nothing\n").unwrap();
        let mut third = fresh();
        assert!(!third.import_sketch_file(&dir.0.join("bad.phoenixsketch")));
        assert!(third.mech.sketches.is_empty() && !third.dirty);
        assert!(!third.import_sketch_file(&dir.0.join("missing.phoenixsketch")));
    }

    // ---------------------------------------------------------------------------------------------
    // Dragging and measuring
    // ---------------------------------------------------------------------------------------------

    fn pane() -> (f64, f64) {
        (800.0, 600.0)
    }

    /// Where a point of the model is drawn in a view of the example.
    fn drawn(w: &Window, view: ViewKind, p: Point3) -> (f64, f64) {
        w.view_mapping(view, pane()).to_pane(p).0
    }

    /// The position of the post of a step, in millimetres.
    fn post_at(w: &Window, step: usize) -> (f64, f64) {
        match &w.mech.steps[step].op {
            Operation::AddPost { post, .. } => (mm(post.x.get()), mm(post.y.get())),
            other => panic!("not a post: {other:?}"),
        }
    }

    /// The position of the hole of a step, in millimetres.
    fn hole_at(w: &Window, step: usize) -> (f64, f64) {
        match &w.mech.steps[step].op {
            Operation::AddHole { hole, .. } => (mm(hole.u.get()), mm(hole.v.get())),
            other => panic!("not a hole: {other:?}"),
        }
    }

    /// The top centre of the post of a step, as drawn in a view.
    fn post_px(w: &Window, view: ViewKind, step: usize) -> (f64, f64) {
        match &w.mech.steps[step].op {
            Operation::AddPost { post, .. } => {
                drawn(w, view, post_centre(w.enclosure().params(), post))
            }
            other => panic!("not a post: {other:?}"),
        }
    }

    // the steps of the example: 0 and 1 are the holes, 2 the edges, 3 to 6 the posts H1 to H4, 7 the lid

    #[test]
    fn a_post_dragged_in_the_plan_lands_on_the_grid_and_is_one_entry_of_the_history() {
        let mut w = example();
        assert_eq!(post_at(&w, 3), (25.0, 15.0));
        let start = post_px(&w, ViewKind::Plan, 3);
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        assert!(w.dragging());
        assert_eq!(w.selection, Selection::Post(3));
        // the pointer goes to 30.3 and 19.8 mm from the inner corner: the floor is at 2 mm
        // and so are the walls, so the point of the model is (32.3, 21.8)
        let to = drawn(&w, ViewKind::Plan, [32.3, 21.8, 2.0]);
        w.drag_move(pane(), to);
        // on the grid of 0.5 mm: 30.3 goes up to 30.5 and 19.8 up to 20
        assert_eq!(post_at(&w, 3), (30.5, 20.0));
        assert_eq!(
            w.status,
            Status::Done(
                Msg::DragDone,
                vec![
                    "post H1".to_string(),
                    "30.500 mm".to_string(),
                    "20.000 mm".to_string()
                ]
            )
        );
        // nothing is in the history while the pointer is down
        assert!(!w.can_undo());
        assert!(w.states().iter().all(|s| *s == StepState::Valid));
        w.drag_end();
        assert!(!w.dragging());
        assert!(w.dirty);
        assert_eq!(w.undo_names(), ["Move post H1"]);
        w.undo();
        assert_eq!(post_at(&w, 3), (25.0, 15.0));
        w.redo();
        assert_eq!(post_at(&w, 3), (30.5, 20.0));
    }

    #[test]
    fn without_the_grid_a_drag_is_exact_to_the_nanometre() {
        let mut w = example();
        w.snap_to_grid = false;
        let start = post_px(&w, ViewKind::Plan, 3);
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        w.drag_move(pane(), drawn(&w, ViewKind::Plan, [32.3, 21.8, 2.0]));
        match &w.mech.steps[3].op {
            Operation::AddPost { post, .. } => {
                assert_eq!(post.x.get(), 30_300_000);
                assert_eq!(post.y.get(), 19_800_000);
            }
            other => panic!("{other:?}"),
        }
        w.drag_end();
    }

    #[test]
    fn a_post_keeps_its_place_under_the_pointer_when_it_is_picked_off_centre() {
        let mut w = example();
        w.snap_to_grid = false;
        // picked 1 mm to the right and half a millimetre up of the centre of the post (27, 17)
        let start = drawn(&w, ViewKind::Plan, [28.0, 17.5, 2.0]);
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        // the pointer goes 10 mm to the right and 5 mm up: so does the post
        w.drag_move(pane(), drawn(&w, ViewKind::Plan, [38.0, 22.5, 2.0]));
        let (x, y) = post_at(&w, 3);
        assert!(
            (x - 35.0).abs() < 1e-6 && (y - 20.0).abs() < 1e-6,
            "{x} {y}"
        );
        w.drag_end();
    }

    #[test]
    fn a_position_the_model_refuses_is_told_and_the_post_stays_where_it_was_accepted() {
        let mut w = example();
        let start = post_px(&w, ViewKind::Plan, 3);
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        w.drag_move(pane(), drawn(&w, ViewKind::Plan, [32.3, 21.8, 2.0]));
        assert_eq!(post_at(&w, 3), (30.5, 20.0));
        // on top of the post H2 (at 75, 15 from the inner corner)
        w.drag_move(pane(), drawn(&w, ViewKind::Plan, [77.0, 17.0, 2.0]));
        assert!(w.status.is_problem(), "{:?}", w.status);
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-028"));
        assert_eq!(post_at(&w, 3), (30.5, 20.0));
        // outside the walls
        w.drag_move(pane(), drawn(&w, ViewKind::Plan, [-20.0, 17.0, 2.0]));
        assert!(w.status.is_problem(), "{:?}", w.status);
        assert_eq!(post_at(&w, 3), (30.5, 20.0));
        assert!(w.states().iter().all(|s| *s == StepState::Valid));
        w.drag_end();
        assert_eq!(w.undo_names(), ["Move post H1"]);
    }

    #[test]
    fn a_drag_that_is_cancelled_or_never_moved_leaves_nothing_behind() {
        let mut w = example();
        let start = post_px(&w, ViewKind::Plan, 3);
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        w.drag_move(pane(), drawn(&w, ViewKind::Plan, [32.3, 21.8, 2.0]));
        assert!(w.dirty);
        w.drag_cancel();
        assert!(!w.dragging() && !w.dirty && !w.can_undo());
        assert_eq!(post_at(&w, 3), (25.0, 15.0));
        assert_eq!(w.status, Status::Done(Msg::DragCancelled, Vec::new()));
        // a press and a release without a move
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        w.drag_end();
        assert!(!w.dirty && !w.can_undo());
        // undo is not taken while the pointer is down
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        w.drag_move(pane(), drawn(&w, ViewKind::Plan, [32.3, 21.8, 2.0]));
        w.undo();
        assert_eq!(post_at(&w, 3), (30.5, 20.0));
        w.drag_end();
    }

    #[test]
    fn a_hole_is_dragged_over_its_face_in_the_3d_view_but_not_in_the_plan() {
        let mut w = example();
        assert_eq!(hole_at(&w, 0), (30.0, 17.0));
        // the front face is seen edge on from above: the status bar says so
        let in_plan = drawn(&w, ViewKind::Plan, [30.0, 0.0, 17.0]);
        assert!(!w.drag_begin(ViewKind::Plan, pane(), in_plan));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-039"));
        // in the 3D view u runs along the length and v up: (40.3, 17.2) lands on (40.5, 17)
        let start = drawn(&w, ViewKind::Iso, [30.0, 0.0, 17.0]);
        assert!(w.drag_begin(ViewKind::Iso, pane(), start));
        assert_eq!(w.selection, Selection::Hole(0));
        w.drag_move(pane(), drawn(&w, ViewKind::Iso, [40.3, 0.0, 17.2]));
        assert_eq!(hole_at(&w, 0), (40.5, 17.0));
        w.drag_end();
        assert_eq!(w.undo_names(), ["Move usb"]);
    }

    #[test]
    fn nothing_is_dragged_when_the_project_is_read_only_or_a_tool_of_measurement_is_on() {
        let mut w = example();
        let start = post_px(&w, ViewKind::Plan, 3);
        // far from every hole and post
        assert!(!w.drag_begin(ViewKind::Plan, pane(), (5.0, 5.0)));
        w.read_only = true;
        assert!(!w.drag_begin(ViewKind::Plan, pane(), start));
        w.read_only = false;
        w.set_measure_tool(MeasureTool::Distance);
        assert!(!w.drag_begin(ViewKind::Plan, pane(), start));
        w.set_measure_tool(MeasureTool::Off);
        assert!(w.drag_begin(ViewKind::Plan, pane(), start));
        w.drag_cancel();
    }

    #[test]
    fn the_distance_between_two_holes_is_read_in_the_unit_of_the_measurements() {
        let mut w = example();
        assert!(!w.measure_click(ViewKind::Plan, pane(), (0.0, 0.0)));
        w.set_measure_tool(MeasureTool::Distance);
        assert_eq!(w.status, Status::Done(Msg::MeasurePickFirst, Vec::new()));
        // the hole usb is at (30, 0, 17) and the hole vent, on the back, at (52, 64, 17)
        let usb = drawn(&w, ViewKind::Plan, [30.0, 0.0, 17.0]);
        let vent = drawn(&w, ViewKind::Plan, [52.0, 64.0, 17.0]);
        assert!(w.measure_click(ViewKind::Plan, pane(), usb));
        assert_eq!(w.picked(), 1);
        assert_eq!(
            w.status,
            Status::Done(Msg::MeasurePickNext, vec!["2".to_string(), "2".to_string()])
        );
        assert!(w.measure_click(ViewKind::Plan, pane(), vent));
        assert_eq!(w.picked(), 0);
        let reading = w.measurements()[0].clone();
        // sqrt(22^2 + 64^2 + 0^2) = sqrt(4580)
        assert!((reading.value - 4580f64.sqrt()).abs() < 1e-9);
        assert_eq!(w.value_text(&reading), "67.676 mm");
        assert_eq!(
            w.reading_text(&reading),
            "Distance from hole usb to hole vent: 67.676 mm (x 22.000 mm, y 64.000 mm, z 0.000 mm)"
        );
        assert_eq!(
            w.status,
            Status::Done(Msg::Measured, vec!["67.676 mm".to_string()])
        );
        // the line and a cross at each end are drawn: 1 line and 6 lines of crosses
        let marks = w
            .lines(ViewKind::Plan, pane())
            .iter()
            .filter(|l| l.kind == Kind::Measure)
            .count();
        assert_eq!(marks, 7);
        // in inches, with four decimals: 67.6757 mm is 2.6644 in
        w.ui.measure.unit = Some(crate::units::Unit::Inch);
        w.ui.measure.decimals = 4;
        assert_eq!(w.value_text(&reading), "2.6644 in");
    }

    #[test]
    fn the_angle_at_a_post_is_read_between_the_lines_to_two_others() {
        let mut w = example();
        w.set_measure_tool(MeasureTool::Angle);
        // the posts H2, H1 and H3 at (77, 17), (27, 17) and (27, 47), 8 mm up: the lines to
        // the other two run along x and along y from H1, a right angle
        for p in [[77.0, 17.0, 8.0], [27.0, 17.0, 8.0], [27.0, 47.0, 8.0]] {
            let px = drawn(&w, ViewKind::Plan, p);
            assert!(w.measure_click(ViewKind::Plan, pane(), px));
        }
        let reading = w.measurements()[0].clone();
        assert!((reading.value - 90.0).abs() < 1e-9);
        assert_eq!(
            w.reading_text(&reading),
            "Angle at post post H1 between post post H2 and post post H3: 90.00\u{b0}"
        );
    }

    #[test]
    fn the_angle_between_two_edges_is_read_when_the_first_pick_is_an_edge() {
        let mut w = example();
        w.set_measure_tool(MeasureTool::Angle);
        // the middle of the front edge of the outer box and of its left edge
        for p in [[52.0, 0.0, 0.0], [0.0, 32.0, 0.0]] {
            let px = drawn(&w, ViewKind::Plan, p);
            assert!(w.measure_click(ViewKind::Plan, pane(), px));
        }
        let reading = w.measurements()[0].clone();
        assert!((reading.value - 90.0).abs() < 1e-9);
        assert_eq!(
            w.reading_text(&reading),
            "Angle between outer edge (0.000 mm, 0.000 mm, 0.000 mm) to (104.000 mm, 0.000 mm, 0.000 mm) and outer edge (0.000 mm, 0.000 mm, 0.000 mm) to (0.000 mm, 64.000 mm, 0.000 mm): 90.00\u{b0}"
        );
    }

    #[test]
    fn the_readings_kept_are_the_newest_and_can_be_cleared() {
        let mut w = example();
        w.ui.measure.keep = 2;
        w.set_measure_tool(MeasureTool::Distance);
        let usb = drawn(&w, ViewKind::Plan, [30.0, 0.0, 17.0]);
        let vent = drawn(&w, ViewKind::Plan, [52.0, 64.0, 17.0]);
        let h1 = post_px(&w, ViewKind::Plan, 3);
        for (a, b) in [(usb, vent), (usb, h1), (vent, h1)] {
            w.measure_click(ViewKind::Plan, pane(), a);
            w.measure_click(ViewKind::Plan, pane(), b);
        }
        assert_eq!(w.measurements().len(), 2);
        // the first one kept is usb to the post H1: (27, 17, 8) from (30, 0, 17) is 3, 17, -9
        let first = &w.measurements()[0];
        assert_eq!(first.delta, [-3.0, 17.0, -9.0]);
        w.remove_measurement(0);
        assert_eq!(w.measurements().len(), 1);
        w.measure_click(ViewKind::Plan, pane(), usb);
        assert_eq!(w.picked(), 1);
        w.clear_measurements();
        assert!(w.measurements().is_empty() && w.picked() == 0);
        // a click on nothing picks nothing
        assert!(w.measure_click(ViewKind::Plan, pane(), (1.0, 1.0)));
        assert_eq!(w.picked(), 0);
        w.set_measure_tool(MeasureTool::Off);
        assert!(!w.measure_click(ViewKind::Plan, pane(), usb));
    }

    // -----------------------------------------------------------------------------------------
    // The fit report and the export
    // -----------------------------------------------------------------------------------------

    /// A window on a copy of the example in a fresh folder, the copy being named `name`.
    fn example_copy(dir: &Scratch, name: &str) -> (Window, PathBuf) {
        let copy = dir.0.join(name);
        std::fs::copy(example_path(), &copy).unwrap();
        let mut w = fresh();
        assert!(w.open(&copy), "{:?}", w.status);
        (w, copy)
    }

    #[test]
    fn the_report_of_a_new_project_is_its_measures_and_its_fit() {
        let mut w = fresh();
        // 70 and 41 characters, joined by a line break
        assert_eq!(
            w.report_text(),
            "Outside 104 x 64 x 35 mm - inside 100 x 60 x 30 mm - material 53.0 cm3\n\
             It fits, with 9.6 / 9.6 / 9.6 mm to spare"
        );
        assert_eq!(w.report_text().len(), 112);
        w.lang = Lang::Es;
        assert_eq!(
            w.report_text(),
            "Exterior 104 x 64 x 35 mm - interior 100 x 60 x 30 mm - material 53.0 cm3\n\
             Cabe, con 9.6 / 9.6 / 9.6 mm de sobra"
        );
        assert_eq!(w.report_text().len(), 111);
    }

    #[test]
    fn the_report_has_a_line_for_each_step_with_its_state() {
        let mut w = example();
        let text = w.report_text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2 + 8, "{text}");
        assert_eq!(lines[1], "It fits, with 39.6 / 19.6 / 23 mm to spare");
        assert_eq!(lines[2], "  1  usb port  valid");
        assert_eq!(lines[9], "  8  lid  valid");
        // a suppressed step says so, and nothing else in the report changes its place
        w.select(Selection::Hole(0));
        w.toggle_suppressed();
        let text = w.report_text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 10, "{text}");
        assert_eq!(lines[2], "  1  usb port  suppressed");
        assert_eq!(lines[3], "  2  vent  valid");
    }

    #[test]
    fn the_report_is_made_of_the_measures_the_fit_and_the_steps() {
        // the check command prints exactly this text
        let w = example();
        let text = w.report_text();
        assert!(text.starts_with(&w.measures_text()));
        assert!(text.contains(&w.fit_text()));
        assert_eq!(text.lines().count(), 2 + w.mech.steps.len());
    }

    #[test]
    fn the_export_starts_as_the_settings_say() {
        let w = fresh();
        assert_eq!(w.export_format, Format::Script);
        assert!(!w.export_overwrite);
        let ui =
            UiSettings::with_user_config("[export]\nformat = report\noverwrite = 1\n").unwrap();
        let w = Window::new(Lang::En, Settings::defaults(), ui).unwrap();
        assert_eq!(w.export_format, Format::Report);
        assert!(w.export_overwrite);
    }

    #[test]
    fn the_names_suggested_follow_the_project() {
        let dir = Scratch::new("names");
        let (w, _) = example_copy(&dir, "mybox.phoenixproj");
        assert_eq!(w.export_name(Format::Script), "mybox.scad");
        assert_eq!(w.export_name(Format::Step), "mybox.step");
        assert_eq!(w.export_name(Format::Report), "mybox.txt");
        assert_eq!(fresh().export_name(Format::Step), "box.step");
    }

    #[test]
    fn the_script_is_written_and_the_status_says_how_many_bytes() {
        let dir = Scratch::new("script");
        let mut w = fresh();
        let target = dir.0.join("box.scad");
        assert!(
            w.export_to(Format::Script, &target, false),
            "{:?}",
            w.status
        );
        let text = std::fs::read_to_string(&target).unwrap();
        assert_eq!(text, crate::scad::to_scad(w.enclosure()));
        assert_eq!(
            w.status,
            Status::Done(
                Msg::Exported,
                vec!["box.scad".into(), text.len().to_string()]
            )
        );
        assert!(w.status.text(Lang::En).starts_with("Exported box.scad ("));
        // the project is not touched by an export
        assert!(!w.dirty && !w.can_undo());
    }

    #[test]
    fn the_report_is_written_in_the_language_of_the_window() {
        let dir = Scratch::new("report");
        let mut w = fresh();
        let target = dir.0.join("fit.txt");
        assert!(w.export_to(Format::Report, &target, false));
        // 112 bytes in English: 70 + 1 + 41
        assert_eq!(std::fs::read(&target).unwrap().len(), 112);
        w.lang = Lang::Es;
        assert!(w.export_to(Format::Report, &target, true));
        let spanish = std::fs::read_to_string(&target).unwrap();
        assert_eq!(spanish.len(), 111);
        assert!(
            spanish.starts_with("Exterior 104 x 64 x 35 mm"),
            "{spanish}"
        );
        assert_eq!(spanish, w.report_text());
    }

    #[test]
    fn a_new_box_is_written_as_a_step_file() {
        let dir = Scratch::new("step");
        let mut w = fresh();
        let target = dir.0.join("box.step");
        assert!(w.export_to(Format::Step, &target, false), "{:?}", w.status);
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.starts_with("ISO-10303-21;"), "{text}");
        assert_eq!(text, crate::step::to_step(w.enclosure()));
    }

    #[test]
    fn a_shape_the_step_file_does_not_draw_gives_the_code_022_and_writes_nothing() {
        let dir = Scratch::new("shape");
        // the example has rounded corners and a lid with a lip
        let mut w = example();
        let target = dir.0.join("box.step");
        assert!(!w.export_to(Format::Step, &target, true));
        assert!(!target.exists());
        assert!(w.status.is_problem());
        let text = w.status.text(Lang::En);
        assert!(
            text.starts_with("PHOENIX-CAD-022 Shape not drawn by this export"),
            "{text}"
        );
        // the script of the same model is written
        assert!(w.export_to(Format::Script, &dir.0.join("box.scad"), false));
    }

    #[test]
    fn a_file_that_exists_is_kept_until_the_box_to_write_over_it_is_ticked() {
        let dir = Scratch::new("exists");
        let mut w = fresh();
        let target = dir.0.join("fit.txt");
        std::fs::write(&target, "mine").unwrap();
        assert!(!w.export_to(Format::Report, &target, false));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "mine");
        let text = w.status.text(Lang::En);
        assert!(
            text.starts_with("PHOENIX-CAD-040 File exists and was not written over (fit.txt)"),
            "{text}"
        );
        let text = w.status.text(Lang::Pt);
        assert!(
            text.starts_with("PHOENIX-CAD-040 O ficheiro existe e não foi substituído"),
            "{text}"
        );
        assert!(w.export_to(Format::Report, &target, true));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), w.report_text());
    }

    #[test]
    fn the_project_that_is_open_is_never_written_over() {
        let dir = Scratch::new("project");
        // a project opened from a file with an ordinary name
        let (mut w, copy) = example_copy(&dir, "keep.dat");
        let before = std::fs::read(&copy).unwrap();
        assert!(!w.export_to(Format::Script, &copy, true));
        assert_eq!(std::fs::read(&copy).unwrap(), before);
        let text = w.status.text(Lang::En);
        assert!(
            text.starts_with("PHOENIX-CAD-041 Export not possible"),
            "{text}"
        );
        assert!(text.contains("the project that is open"), "{text}");
        // and a name that ends in the extension of a project is refused too
        let other = dir.0.join("other.phoenixproj");
        assert!(!w.export_to(Format::Script, &other, true));
        assert!(!other.exists());
        assert!(!w.dirty);
    }

    #[test]
    fn a_text_larger_than_the_setting_is_refused_with_the_code_041() {
        let dir = Scratch::new("large");
        let ui = UiSettings::with_user_config("[export]\nmax_kib = 1\n").unwrap();
        let mut w = Window::new(Lang::En, Settings::defaults(), ui).unwrap();
        // the report is 112 bytes and fits in 1 KiB
        assert!(w.export_to(Format::Report, &dir.0.join("fit.txt"), false));
        // the script of the example is larger than 1024 bytes
        assert!(w.open(&example_path()));
        let big = crate::scad::to_scad(w.enclosure()).len();
        assert!(big > 1024, "{big}");
        assert!(!w.export_to(Format::Script, &dir.0.join("big.scad"), false));
        assert!(!dir.0.join("big.scad").exists());
        let text = w.status.text(Lang::En);
        assert!(
            text.contains("PHOENIX-CAD-041") && text.contains(&format!("{big} bytes")),
            "{text}"
        );
    }

    #[test]
    fn the_report_is_marked_as_copied() {
        let mut w = fresh();
        w.report_copied();
        assert_eq!(w.status.text(Lang::En), "Report copied to the clipboard");
        assert!(!w.status.is_problem());
    }

    fn pattern_of(w: &Window, index: usize) -> PatternOp {
        match &w.mech.steps[index].op {
            Operation::AddPattern { pattern, .. } => pattern.clone(),
            other => panic!("not a pattern: {other:?}"),
        }
    }

    /// The positions of the holes of the enclosure, in millimetres, in the order of their names.
    fn hole_places(w: &Window) -> Vec<(String, f64, f64)> {
        let mut out: Vec<(String, f64, f64)> = w
            .enclosure()
            .holes()
            .values()
            .map(|h| {
                (
                    h.name.clone(),
                    h.u.get() as f64 / 1e6,
                    h.v.get() as f64 / 1e6,
                )
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    #[test]
    fn a_pattern_of_holes_is_added_from_the_settings_and_shows_in_its_own_group_of_the_tree() {
        let mut w = fresh();
        w.add_step(NewStep::HolePattern);
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(w.mech.steps.len(), 1);
        assert_eq!(w.selection, Selection::Pattern(0));
        assert_eq!(w.mech.steps[0].name, "Hole pattern 1");
        // four holes on the front face from (14, 15) mm, 10 mm apart
        assert_eq!(
            hole_places(&w),
            vec![
                ("Hole pattern 1 1".to_string(), 14.0, 15.0),
                ("Hole pattern 1 2".to_string(), 24.0, 15.0),
                ("Hole pattern 1 3".to_string(), 34.0, 15.0),
                ("Hole pattern 1 4".to_string(), 44.0, 15.0),
            ]
        );
        let groups = w.tree();
        let group = groups
            .iter()
            .find(|g| g.title == Msg::GroupPatterns)
            .unwrap();
        assert_eq!(group.items.len(), 1);
        assert_eq!(group.items[0].label, "Hole pattern 1");
        assert_eq!(group.items[0].selection, Selection::Pattern(0));
        // no group of single holes: the positions are not steps
        assert!(groups.iter().all(|g| g.title != Msg::GroupHoles));
        // the fields: name, face, u, v, diameter, layout, count and the two steps of a line
        let keys: Vec<Key> = w.fields.iter().map(|f| f.key).collect();
        assert_eq!(
            keys,
            vec![
                Key::Name,
                Key::Face,
                Key::U,
                Key::V,
                Key::Diameter,
                Key::PatternLayout,
                Key::PatternCount,
                Key::StepU,
                Key::StepV
            ]
        );
        assert_eq!(w.fields[field(&w, Key::PatternCount)].text, "4");
        assert_eq!(w.fields[field(&w, Key::StepU)].text, "10.000 mm");
        // the whole pattern is the selection that is drawn in its colour
        let id = match &w.mech.steps[0].op {
            Operation::AddPattern { id, .. } => *id,
            _ => unreachable!(),
        };
        assert_eq!(w.highlight(), Highlight::Pattern(id));
    }

    #[test]
    fn editing_a_pattern_builds_all_its_positions_again_and_a_refusal_changes_nothing() {
        let mut w = fresh();
        w.add_step(NewStep::HolePattern);
        set(&mut w, Key::PatternCount, "6");
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(w.enclosure().holes().len(), 6);
        // the seed moves and the whole row follows: from 20 to 70 mm
        set(&mut w, Key::U, "20 mm");
        let places = hole_places(&w);
        assert_eq!((places[0].1, places[5].1), (20.0, 70.0));
        // 5 mm apart leaves 6 - 5 = -1 mm of web between holes of 6 mm: refused, and nothing moved
        let before = w.mech.clone();
        set(&mut w, Key::StepU, "5 mm");
        assert!(problem(&w));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-004"));
        assert_eq!(w.mech, before);
        // 8 mm apart leaves exactly the least web of 2 mm
        set(&mut w, Key::StepU, "8 mm");
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(hole_places(&w)[5].1, 60.0);
        // one position is not a pattern, and a count that is not a number is refused
        for text in ["1", "0", "-3", "many", "99999999999"] {
            let before = w.mech.clone();
            set(&mut w, Key::PatternCount, text);
            assert!(problem(&w), "{text}");
            assert_eq!(w.mech, before, "{text}");
        }
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-007"));
        set(&mut w, Key::PatternCount, "1");
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-042"));
        // the face changes for every position
        set(&mut w, Key::Face, "top");
        assert!(w.enclosure().holes().values().all(|h| h.face == Face::Top));
        // more than the settings allow is refused with the same code
        let mut small = Settings::defaults();
        small.design.pattern_max = 6;
        let mut limited = Window::new(Lang::En, small, UiSettings::defaults()).unwrap();
        limited.add_step(NewStep::HolePattern);
        set(&mut limited, Key::PatternCount, "6");
        assert!(!problem(&limited), "{:?}", limited.status);
        set(&mut limited, Key::PatternCount, "7");
        assert!(problem(&limited));
        assert!(limited.status.text(Lang::En).contains("PHOENIX-CAD-042"));
        assert_eq!(limited.enclosure().holes().len(), 6);
    }

    #[test]
    fn a_line_becomes_a_circle_of_exact_positions_and_back() {
        let mut w = fresh();
        w.add_step(NewStep::HolePattern);
        set(&mut w, Key::U, "20 mm");
        // the circle of the settings: centre 5 mm from the seed along both axes, 90 degrees
        set(&mut w, Key::PatternLayout, "circular");
        assert!(!problem(&w), "{:?}", w.status);
        let keys: Vec<Key> = w.fields.iter().map(|f| f.key).collect();
        assert_eq!(
            &keys[5..],
            &[
                Key::PatternLayout,
                Key::PatternCount,
                Key::CenterU,
                Key::CenterV,
                Key::StepAngle
            ]
        );
        assert_eq!(w.fields[field(&w, Key::CenterU)].text, "25.000 mm");
        assert_eq!(w.fields[field(&w, Key::StepAngle)].text, "90");
        let places: Vec<(f64, f64)> = hole_places(&w).iter().map(|p| (p.1, p.2)).collect();
        assert_eq!(
            places,
            vec![(20.0, 15.0), (30.0, 15.0), (30.0, 25.0), (20.0, 25.0)]
        );
        // 45 degrees puts the holes 5.4 mm apart: refused
        let before = w.mech.clone();
        set(&mut w, Key::StepAngle, "45");
        assert!(problem(&w));
        assert_eq!(w.mech, before);
        // an angle that is not an angle is refused, and so is a field of the other layout
        set(&mut w, Key::StepAngle, "round");
        assert!(problem(&w));
        assert_eq!(w.mech, before);
        // the angle can be a fraction of a degree: 360 / 4 = 90 written as 360/4
        set(&mut w, Key::StepAngle, "360/4");
        assert_eq!(w.mech, before);
        set(&mut w, Key::PatternLayout, "linear");
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(w.fields[field(&w, Key::StepU)].text, "10.000 mm");
        assert_eq!(w.fields[field(&w, Key::StepV)].text, "0.000 mm");
        let one = pattern_of(&w, 0);
        assert!(matches!(one.layout, Layout::Linear { .. }));
        // and a layout that is neither is not one
        set(&mut w, Key::PatternLayout, "spiral");
        assert!(problem(&w));
    }

    #[test]
    fn a_pattern_goes_back_with_undo_and_a_removal_takes_every_position() {
        let mut w = fresh();
        w.add_step(NewStep::HolePattern);
        w.add_step(NewStep::PostPattern);
        assert_eq!(w.selection, Selection::Pattern(1));
        assert_eq!(w.enclosure().holes().len(), 4);
        assert_eq!(w.enclosure().posts().len(), 4);
        let names: Vec<String> = w
            .enclosure()
            .posts()
            .values()
            .map(|p| p.name.clone())
            .collect();
        assert!(names.contains(&"Post pattern 1 4".to_string()), "{names:?}");
        // editing a post pattern: a different pillar and a longer row
        set(&mut w, Key::PostOuter, "9 mm");
        assert!(!problem(&w), "{:?}", w.status);
        set(&mut w, Key::PatternCount, "5");
        assert_eq!(w.enclosure().posts().len(), 5);
        w.undo();
        assert_eq!(w.enclosure().posts().len(), 4);
        w.redo();
        assert_eq!(w.enclosure().posts().len(), 5);
        // removing the selected pattern takes its five posts and leaves the holes
        w.remove_step();
        assert!(!problem(&w), "{:?}", w.status);
        assert!(w.enclosure().posts().is_empty());
        assert_eq!(w.enclosure().holes().len(), 4);
        assert_eq!(w.mech.steps.len(), 1);
        w.undo();
        assert_eq!(w.enclosure().posts().len(), 5);
        // a second pattern of holes is named after the first
        w.add_step(NewStep::HolePattern);
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(w.mech.steps[2].name, "Hole pattern 2");
        // the new pattern moved along by the pitch until nothing was in the way: 14, 26, 38 and 50
        // are within the web of a hole of the first one (14, 24, 34 and 44), 62 is not
        assert_eq!(pattern_of(&w, 2).seed_position().0, mm_len(62));
        assert_eq!(w.enclosure().holes().len(), 8);
    }

    #[test]
    fn a_single_hole_added_after_a_pattern_does_not_take_an_id_of_a_position() {
        let mut w = fresh();
        w.add_step(NewStep::PostPattern);
        let before: Vec<Id> = w.enclosure().posts().keys().copied().collect();
        // the row of the first four posts is at 10, 20, 30 and 40 mm: the next places that fit are
        // 58, 70 and 82 mm
        for _ in 0..3 {
            w.add_step(NewStep::Post);
        }
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(w.enclosure().posts().len(), before.len() + 3);
        for id in &before {
            assert!(w.enclosure().posts().contains_key(id));
        }
        // and one position of a pattern cannot be removed on its own through the model
        let any = *before.last().unwrap();
        let mut e = w.enclosure().clone();
        assert_eq!(
            e.apply(Operation::RemovePost { id: any })
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-042"
        );
    }

    #[test]
    fn a_project_with_patterns_is_written_as_version_three_and_opens_the_same() {
        let dir = Scratch::new("patterns");
        let path = dir.0.join("vents.phoenixproj");
        let mut w = fresh();
        w.add_step(NewStep::HolePattern);
        w.add_step(NewStep::PostPattern);
        set(&mut w, Key::PatternLayout, "circular");
        assert!(!problem(&w), "{:?}", w.status);
        assert!(w.save_to(&path), "{:?}", w.status);
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("mechanical 3\n"), "{text}");
        assert!(text.contains(" add-pattern "));
        let mut again = fresh();
        assert!(again.open(&path), "{:?}", again.status);
        assert_eq!(again.mech, w.mech);
        assert_eq!(hole_places(&again), hole_places(&w));
        assert_eq!(again.enclosure().posts().len(), 4);
        // the same bytes when it is written again
        let second = dir.0.join("again.phoenixproj");
        assert!(again.save_to(&second));
        assert_eq!(std::fs::read(&second).unwrap(), bytes);
    }

    fn fins_of(w: &Window, index: usize) -> FinSet {
        match &w.mech.steps[index].op {
            Operation::AddFins { fins, .. } => fins.clone(),
            other => panic!("not fins: {other:?}"),
        }
    }

    fn text_of(w: &Window, key: Key) -> String {
        w.fields[field(w, key)].text.clone()
    }

    #[test]
    fn a_set_of_cooling_fins_is_added_from_the_settings_with_its_surface_and_an_estimate() {
        let mut w = fresh();
        let bare = w.info.material_nm3;
        w.add_step(NewStep::Fins);
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(w.selection, Selection::Fins(0));
        assert_eq!(w.mech.steps[0].name, "Fins 1");
        let set = fins_of(&w, 0);
        assert_eq!(
            (set.face, set.along, set.count.get()),
            (Face::Top, FinAxis::U, 5)
        );
        // 5 fins of 2 x 15 x 80 mm add 12000 mm3 of material
        assert_eq!(
            w.info.material_nm3 - bare,
            12_000i128 * 1_000_000_000_000_000_000
        );
        let groups = w.tree();
        let group = groups.iter().find(|g| g.title == Msg::GroupFins).unwrap();
        assert_eq!(group.items.len(), 1);
        assert_eq!(group.items[0].selection, Selection::Fins(0));
        let keys: Vec<Key> = w.fields.iter().map(|f| f.key).collect();
        assert_eq!(
            keys,
            vec![
                Key::Name,
                Key::Face,
                Key::FinDirection,
                Key::FinCount,
                Key::FinThickness,
                Key::FinHeight,
                Key::FinPitch,
                Key::FinFirst,
                Key::FinStart,
                Key::FinLength,
                Key::FinArea,
                Key::FinPower
            ]
        );
        // worked out by hand: a fin of 2 x 15 x 80 mm has 2620 mm2, five of them 13100 mm2, and the
        // bare face between them 34 * 80 - 5 * 2 * 80 = 1920 mm2: 15020 mm2 = 150.2 cm2
        assert_eq!(text_of(&w, Key::FinArea), "150.2 cm2");
        assert_eq!(
            text_of(&w, Key::FinPower),
            "4.79 W at 40 K over the air, 5.5 times the bare face"
        );
        // both are read only, and the second says that it is an estimate
        for key in [Key::FinArea, Key::FinPower] {
            assert!(!w.fields[field(&w, key)].editable);
        }
        assert!(w.fields[field(&w, Key::FinPower)].note.contains("estimate"));
        assert_eq!(text_of(&w, Key::FinThickness), "2.000 mm");
        let id = match &w.mech.steps[0].op {
            Operation::AddFins { id, .. } => *id,
            _ => unreachable!(),
        };
        assert_eq!(w.highlight(), Highlight::Object(id));
        // the report says the numbers are an estimate
        assert!(w
            .report_text()
            .ends_with("Fins 1: 5 fins, 150.2 cm2 of surface, about 4.79 W at 40 K over the air (estimate for still air)"));
    }

    #[test]
    fn editing_the_fins_gives_new_numbers_and_every_limit_refuses_with_its_code() {
        let mut w = fresh();
        w.add_step(NewStep::Fins);
        set(&mut w, Key::FinCount, "7");
        assert!(!problem(&w), "{:?}", w.status);
        // 7 fins: 212.2 cm2 and 6.77 W (worked out by hand and by a second calculation)
        assert_eq!(text_of(&w, Key::FinArea), "212.2 cm2");
        assert_eq!(
            text_of(&w, Key::FinPower),
            "6.77 W at 40 K over the air, 5.3 times the bare face"
        );
        // each refusal leaves the set as it was and says why
        for (key, text) in [
            (Key::FinPitch, "4 mm"),
            (Key::FinThickness, "0.5 mm"),
            (Key::FinHeight, "40 mm"),
            (Key::FinFirst, "1 mm"),
            (Key::FinLength, "120 mm"),
            (Key::FinCount, "129"),
            (Key::Face, "front"),
            (Key::FinDirection, "v"),
        ] {
            let before = w.mech.clone();
            set(&mut w, key, text);
            assert!(problem(&w), "{key:?} {text}");
            assert!(
                w.status.text(Lang::En).contains("PHOENIX-CAD-043"),
                "{key:?} {text}: {}",
                w.status.text(Lang::En)
            );
            assert_eq!(w.mech, before, "{key:?} {text}");
        }
        // a count that is not a number is another error
        set(&mut w, Key::FinCount, "lots");
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-007"));
        // the floor takes the same fins, and the name is free text
        set(&mut w, Key::Face, "bottom");
        assert!(!problem(&w), "{:?}", w.status);
        assert_eq!(fins_of(&w, 0).face, Face::Bottom);
        set(&mut w, Key::Name, "radiator");
        assert_eq!(w.mech.steps[0].name, "Fins 1");
        assert_eq!(fins_of(&w, 0).name, "radiator");
        assert!(w
            .tree()
            .iter()
            .any(|g| g.items.iter().any(|i| i.label == "Fins 1")));
        // undo goes back one change at a time
        w.undo();
        assert_eq!(fins_of(&w, 0).name, "Fins 1");
        w.undo();
        assert_eq!(fins_of(&w, 0).face, Face::Top);
    }

    #[test]
    fn the_estimate_follows_the_settings_and_a_second_set_that_has_no_room_is_refused() {
        // fins printed in plastic (0.2 W/mK): the estimate drops from 4.79 W to 1.92 W
        let plastic = Settings::with_user_config("[thermal]\nconductivity_w_mk = 0.2\n").unwrap();
        let mut w = Window::new(Lang::En, plastic, UiSettings::defaults()).unwrap();
        w.add_step(NewStep::Fins);
        assert_eq!(
            text_of(&w, Key::FinPower),
            "1.92 W at 40 K over the air, 2.2 times the bare face"
        );
        let warm =
            Settings::with_user_config("[thermal]\ndelta_t_k = 25.5\nconvection_w_m2k = 10\n")
                .unwrap();
        let mut v = Window::new(Lang::Es, warm, UiSettings::defaults()).unwrap();
        v.add_step(NewStep::Fins);
        assert!(
            text_of(&v, Key::FinPower).contains("con 25.5 K sobre el aire"),
            "{}",
            text_of(&v, Key::FinPower)
        );
        // the face has no room for a second set of five fins: every try is refused
        let mut x = fresh();
        x.add_step(NewStep::Fins);
        x.add_step(NewStep::Fins);
        assert!(problem(&x));
        assert!(x.status.text(Lang::En).contains("PHOENIX-CAD-043"));
        assert_eq!(x.mech.steps.len(), 1);
        // but a short first set leaves room: 2 fins up to 21 mm, then 5 fins from 36 mm would
        // end at 69 mm, past the face, so the second set is made smaller first
        set(&mut x, Key::FinCount, "2");
        x.add_step(NewStep::Fins);
        assert!(problem(&x));
        x.ui.new_step.fin_count = 2;
        x.add_step(NewStep::Fins);
        assert!(!problem(&x), "{:?}", x.status);
        assert_eq!(x.mech.steps.len(), 2);
        assert_eq!(x.mech.steps[1].name, "Fins 2");
        assert_eq!(x.enclosure().fins().len(), 2);
        // removing the first takes its fins, and undo brings them back
        x.select(Selection::Fins(0));
        x.remove_step();
        assert_eq!(x.enclosure().fins().len(), 1);
        x.undo();
        assert_eq!(x.enclosure().fins().len(), 2);
    }

    #[test]
    fn a_project_with_fins_is_written_as_version_three_and_the_script_has_them() {
        let dir = Scratch::new("fins");
        let path = dir.0.join("sink.phoenixproj");
        let mut w = fresh();
        w.add_step(NewStep::Fins);
        assert!(w.save_to(&path), "{:?}", w.status);
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("mechanical 3\n"), "{text}");
        assert!(text.contains(" add-fins "));
        let mut again = fresh();
        assert!(again.open(&path), "{:?}", again.status);
        assert_eq!(again.mech, w.mech);
        assert_eq!(again.enclosure().fins().len(), 1);
        let script = again.export_text(Format::Script).unwrap();
        assert!(script.contains("// fins Fins 1, top face"));
        // the STEP file does not draw them and says so
        let err = again.export_text(Format::Step).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-022");
    }

    fn library_path() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/chassis.phoenixlib"
        ))
    }

    fn frame_path() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/frame.phoenixproj"
        ))
    }

    fn with_library() -> Window {
        let mut w = fresh();
        assert!(w.open_library(&library_path()), "{:?}", w.status);
        w
    }

    /// Chooses a part of the library by clicking it in the tree.
    fn choose_part(w: &mut Window, name: &str) {
        let k = w
            .library
            .as_ref()
            .unwrap()
            .parts
            .iter()
            .position(|p| p.name == name)
            .unwrap();
        w.select(Selection::CatalogPart(k));
    }

    fn part_of(w: &Window, index: usize) -> PartInstance {
        match &w.mech.steps[index].op {
            Operation::AddPart { part, .. } => part.clone(),
            other => panic!("not a part: {other:?}"),
        }
    }

    #[test]
    fn the_library_is_opened_listed_in_the_tree_and_each_part_shows_its_numbers() {
        let mut w = with_library();
        assert_eq!(
            w.status.text(Lang::En),
            "Library chassis-basic opened: 8 parts"
        );
        let tree = w.tree();
        let group = tree.iter().find(|g| g.title == Msg::GroupLibrary).unwrap();
        let names: Vec<&str> = group.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            names,
            [
                "tube-20x20",
                "tube-40x20",
                "bar-10x10",
                "bolt-m3x10",
                "flat-m3x8",
                "hex-m4x16",
                "angle-30",
                "angle-20"
            ]
        );
        assert_eq!(group.items[3].selection, Selection::CatalogPart(3));
        // no library, no group
        assert!(!fresh().tree().iter().any(|g| g.title == Msg::GroupLibrary));
        // 20 x 20 with a wall of 1.5 mm is 111 mm2: 33300 mm3 over the usual cut of 300 mm, 89.91 g of aluminium
        w.select(Selection::CatalogPart(0));
        assert_eq!(w.catalog_choice, 0);
        assert_eq!(text_of(&w, Key::Name), "tube-20x20");
        assert_eq!(text_of(&w, Key::PartKind), "profile");
        assert_eq!(text_of(&w, Key::PartMaterial), "aluminium");
        assert_eq!(
            text_of(&w, Key::PartSize),
            "20.000 mm x 20.000 mm x 300.000 mm"
        );
        assert_eq!(text_of(&w, Key::PartVolume), "33300.000 mm3");
        assert_eq!(text_of(&w, Key::PartMass), "89.910 g");
        assert!(w.fields.iter().all(|f| !f.editable));
        // the screw M3 x 10: head 5.5 x 3, so 5.5 x 5.5 x 13 mm and 1.114 g of steel
        choose_part(&mut w, "bolt-m3x10");
        assert_eq!(w.catalog_choice, 3);
        assert_eq!(
            text_of(&w, Key::PartSize),
            "5.500 mm x 5.500 mm x 13.000 mm"
        );
        assert_eq!(text_of(&w, Key::PartMass), "1.114 g");
        // a part that is not there selects the box
        w.select(Selection::CatalogPart(99));
        assert_eq!(w.selection, Selection::Body);
    }

    #[test]
    fn a_library_that_cannot_be_opened_leaves_the_one_that_is_open() {
        let mut w = with_library();
        assert!(!w.open_library(&library_path().with_file_name("none.phoenixlib")));
        assert!(w.status.is_problem());
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-012"));
        assert_eq!(w.library.as_ref().unwrap().parts.len(), 8);
        // a project is not a library
        assert!(!w.open_library(&example_path()));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-044"));
        // a material that the settings do not have
        let dir = Scratch::new("library-material");
        let file = dir.0.join("odd.phoenixlib");
        std::fs::write(
            &file,
            "phoenix-library 1\nname odd\npart bar profile unobtainium 10000000 10000000 0 100000000 1000000000\n",
        )
        .unwrap();
        assert!(!w.open_library(&file));
        let text = w.status.text(Lang::En);
        assert!(
            text.contains("PHOENIX-CAD-044") && text.contains("unobtainium"),
            "{text}"
        );
        assert_eq!(w.library.as_ref().unwrap().name, "chassis-basic");
        // and the same library in Portuguese of Portugal
        w.lang = Lang::Pt;
        assert!(w
            .status
            .text(Lang::Pt)
            .contains("Biblioteca de peças inválida"));
    }

    #[test]
    fn a_part_is_added_from_the_library_where_the_settings_say_and_moves_along_to_a_free_place() {
        let mut w = with_library();
        choose_part(&mut w, "tube-40x20");
        w.add_step(NewStep::Part);
        assert!(!w.status.is_problem(), "{:?}", w.status);
        assert_eq!(w.selection, Selection::Part(0));
        assert_eq!(w.mech.steps[0].name, "Part 1");
        let first = part_of(&w, 0);
        assert_eq!(first.library, "chassis-basic");
        assert_eq!(first.part.name, "tube-40x20");
        assert_eq!(first.cut, Some(mm_len(300)));
        assert_eq!(first.at, [mm_len(0), mm_len(0), mm_len(0)]);
        let group = w
            .tree()
            .into_iter()
            .find(|g| g.title == Msg::GroupParts)
            .unwrap();
        assert_eq!(group.items.len(), 1);
        assert_eq!(group.items[0].selection, Selection::Part(0));
        // the second one would share volume with the first (40 mm wide along x): the tries move
        // it by 12 mm each, and the fourth try (48 mm) is the first that is clear of it
        w.add_step(NewStep::Part);
        assert_eq!(part_of(&w, 1).at, [mm_len(48), mm_len(0), mm_len(0)]);
        assert_eq!(w.enclosure().parts().len(), 2);
        // the part is highlighted and drawn
        assert!(matches!(w.highlight(), Highlight::Object(_)));
        // the kinds of parts that have no cut have none
        choose_part(&mut w, "angle-30");
        w.add_step(NewStep::Part);
        assert_eq!(part_of(&w, 2).cut, None);
        assert!(!w.fields.iter().any(|f| f.key == Key::PartCut));
        // removing the step takes the part away and undo brings it back
        w.remove_step();
        assert_eq!(w.enclosure().parts().len(), 2);
        w.undo();
        assert_eq!(w.enclosure().parts().len(), 3);
        w.redo();
        assert_eq!(w.enclosure().parts().len(), 2);
    }

    #[test]
    fn a_part_cannot_be_added_without_a_library() {
        let mut w = fresh();
        w.add_step(NewStep::Part);
        assert!(w.status.is_problem());
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-045"));
        assert!(w.mech.steps.is_empty());
        assert!(!w.can_undo());
    }

    #[test]
    fn editing_a_part_gives_new_numbers_and_every_limit_refuses_with_its_code() {
        let mut w = with_library();
        choose_part(&mut w, "tube-40x20");
        w.add_step(NewStep::Part);
        let keys: Vec<Key> = w.fields.iter().map(|f| f.key).collect();
        assert_eq!(
            keys,
            [
                Key::Name,
                Key::PartLibrary,
                Key::PartKind,
                Key::PartMaterial,
                Key::PartX,
                Key::PartY,
                Key::PartZ,
                Key::PartDirection,
                Key::PartRoll,
                Key::PartCut,
                Key::PartSize,
                Key::PartVolume,
                Key::PartMass
            ]
        );
        assert_eq!(text_of(&w, Key::PartLibrary), "chassis-basic / tube-40x20");
        // the usual cut: 40 x 20 with a wall of 2 mm is 224 mm2, 67200 mm3 over 300 mm, 181.44 g
        assert_eq!(text_of(&w, Key::PartVolume), "67200.000 mm3");
        assert_eq!(text_of(&w, Key::PartMass), "181.440 g");
        // a length along +x with a quarter turn of roll: 250 mm along x, 20 along y, 40 along z
        set(&mut w, Key::PartCut, "250");
        set(&mut w, Key::PartDirection, "+x");
        set(&mut w, Key::PartRoll, "90");
        set(&mut w, Key::PartX, "100");
        set(&mut w, Key::PartY, "-5 mm");
        set(&mut w, Key::PartZ, "0.5 in");
        assert!(!w.status.is_problem(), "{:?}", w.status);
        let part = part_of(&w, 0);
        assert_eq!(part.cut, Some(mm_len(250)));
        assert_eq!(part.orientation.roll, 1);
        assert_eq!(
            part.at,
            [mm_len(100), mm_len(-5), Length::new(12_700_000).unwrap()]
        );
        assert_eq!(
            text_of(&w, Key::PartSize),
            "250.000 mm x 20.000 mm x 40.000 mm"
        );
        // 224 mm2 over 250 mm is 56000 mm3 and 151.2 g
        assert_eq!(text_of(&w, Key::PartVolume), "56000.000 mm3");
        assert_eq!(text_of(&w, Key::PartMass), "151.200 g");
        assert_eq!(text_of(&w, Key::PartRoll), "90");
        // the refusals leave the part as it was
        let before = w.mech.clone();
        for (key, text, code) in [
            (Key::PartCut, "2001", "PHOENIX-CAD-045"),
            (Key::PartCut, "0.9 mm", "PHOENIX-CAD-045"),
            (Key::PartCut, "long", "PHOENIX-CAD-008"),
            (Key::PartDirection, "up", "PHOENIX-CAD-010"),
            (Key::PartRoll, "45", "PHOENIX-CAD-007"),
            (Key::PartRoll, "360", "PHOENIX-CAD-007"),
            (Key::PartRoll, "-90", "PHOENIX-CAD-007"),
            (Key::PartX, "7000", "PHOENIX-CAD-045"),
            (Key::PartX, "far", "PHOENIX-CAD-008"),
        ] {
            set(&mut w, key, text);
            assert!(w.status.is_problem(), "{key:?} {text}");
            assert!(
                w.status.text(Lang::En).contains(code),
                "{key:?} {text}: {}",
                w.status.text(Lang::En)
            );
            assert_eq!(w.mech, before, "{key:?} {text}");
        }
        // the bar is 2000 mm long: exactly that is allowed
        set(&mut w, Key::PartCut, "2000");
        assert!(!w.status.is_problem());
        assert_eq!(part_of(&w, 0).cut, Some(mm_len(2000)));
        // the name is free text
        set(&mut w, Key::Name, "rail");
        assert_eq!(part_of(&w, 0).name, "rail");
        // the read only fields are not edited
        let i = field(&w, Key::PartMass);
        assert!(!w.fields[i].editable);
        w.type_in(i, "1".to_string());
        w.commit(i);
        assert_eq!(part_of(&w, 0).name, "rail");
        // every edit was one entry of the history
        w.undo();
        assert_eq!(part_of(&w, 0).name, "Part 1");
        w.redo();
        assert_eq!(part_of(&w, 0).name, "rail");
    }

    #[test]
    fn two_profiles_cannot_share_volume_in_the_window_and_the_setting_can_allow_it() {
        let mut w = with_library();
        choose_part(&mut w, "tube-20x20");
        w.add_step(NewStep::Part);
        w.add_step(NewStep::Part);
        // the second landed 24 mm along (20 mm wide, tries every 12 mm)
        assert_eq!(part_of(&w, 1).at[0], mm_len(24));
        set(&mut w, Key::PartX, "19.999999");
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-045"));
        assert!(w.status.text(Lang::En).contains("shares volume with"));
        set(&mut w, Key::PartX, "20");
        assert!(!w.status.is_problem());
        let mut loose =
            Settings::with_user_config("[catalog]\nrefuse_profile_overlap = 0\n").unwrap();
        loose.design.catalog.refuse_overlap = false;
        let mut v = Window::new(Lang::En, loose, UiSettings::defaults()).unwrap();
        assert!(v.open_library(&library_path()));
        v.add_step(NewStep::Part);
        v.add_step(NewStep::Part);
        // with the overlap allowed the first try is clear
        assert_eq!(part_of(&v, 1).at[0], mm_len(0));
    }

    #[test]
    fn the_example_frame_opens_with_every_step_valid_and_has_a_bill_of_materials() {
        let mut w = fresh();
        assert!(w.open(&frame_path()), "{:?}", w.status);
        assert_eq!(w.mech.steps.len(), 18);
        assert_eq!(w.info.failed, 0);
        assert_eq!(w.enclosure().parts().len(), 18);
        let bill = w.bill().unwrap();
        assert_eq!(bill.pieces, 18);
        assert_eq!(bill.volume_nm3, 251_025_900_595_183_194_300_000);
        assert_eq!(bill.mass_ug, 684_085_058);
        assert_eq!(
            w.bill_csv_text().unwrap(),
            "kind,part,material,count,cut_mm,volume_mm3,mass_g\n\
             profile,tube-20x20,aluminium,4,150,66600.000,179.820\n\
             profile,tube-20x20,aluminium,2,160,35520.000,95.904\n\
             profile,tube-40x20,aluminium,2,300,134400.000,362.880\n\
             screw,bolt-m3x10,steel,4,,567.843,4.458\n\
             screw,hex-m4x16,stainless,2,,639.761,5.118\n\
             bracket,angle-30,aluminium,4,,13298.296,35.905\n\
             total,,,18,,251025.901,684.085\n"
        );
        let report = w.report_text();
        for line in [
            "Parts: 18 pieces, 251025.901 mm3 of material, 684.085 g",
            "  profile: tube-20x20, aluminium, 4 x 150 mm, 179.820 g",
            "  screw: bolt-m3x10, steel, 4 x -, 4.458 g",
            "  bracket: angle-30, aluminium, 4 x -, 35.905 g",
            "Cut list tube-20x20: 2 x 160 mm + 4 x 150 mm, 920 mm in all",
            "Cut list tube-40x20: 2 x 300 mm, 600 mm in all",
        ] {
            assert!(report.contains(line), "{line}\n{report}");
        }
        // the report speaks the language of the window
        w.lang = Lang::De;
        let german = w.report_text();
        assert!(
            german.contains("Teile: 18 Stück, 251025.901 mm3 Material, 684.085 g"),
            "{german}"
        );
        assert!(
            german.contains("  Profil: tube-20x20, aluminium, 4 x 150 mm, 179.820 g"),
            "{german}"
        );
        assert!(
            german.contains("Zuschnittliste tube-40x20: 2 x 300 mm, insgesamt 600 mm"),
            "{german}"
        );
        // and a project without parts has no such lines
        let plain = example().report_text();
        assert!(!plain.contains("Parts:"));
    }

    #[test]
    fn the_bill_is_exported_to_a_file_and_the_script_has_the_parts() {
        let dir = Scratch::new("bill");
        let mut w = fresh();
        assert!(w.open(&frame_path()), "{:?}", w.status);
        assert_eq!(w.export_name(Format::Bill), "frame.csv");
        let target = dir.0.join("frame.csv");
        assert!(w.export_to(Format::Bill, &target, false), "{:?}", w.status);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            w.bill_csv_text().unwrap()
        );
        // the script has one block for each part and the STEP file says it does not draw them
        let script = w.export_text(Format::Script).unwrap();
        assert_eq!(script.matches("  // part ").count(), 18);
        assert!(script.contains("  // part front rail, tube-40x20 of the library chassis-basic\n"));
        let err = w.export_text(Format::Step).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-022");
        // a project with no parts has a bill with only the totals
        let plain = example().export_text(Format::Bill).unwrap();
        assert_eq!(
            plain,
            "kind,part,material,count,cut_mm,volume_mm3,mass_g\ntotal,,,0,,0.000,0.000\n"
        );
    }

    #[test]
    fn a_project_with_parts_is_written_as_version_four_and_read_again_the_same() {
        let dir = Scratch::new("parts-file");
        let path = dir.0.join("rail.phoenixproj");
        let mut w = with_library();
        choose_part(&mut w, "bar-10x10");
        w.add_step(NewStep::Part);
        choose_part(&mut w, "hex-m4x16");
        w.add_step(NewStep::Part);
        assert!(w.save_to(&path), "{:?}", w.status);
        let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        assert!(text.contains("mechanical 4\n"), "{text}");
        assert_eq!(text.matches(" add-part ").count(), 2);
        let mut again = fresh();
        assert!(again.open(&path), "{:?}", again.status);
        assert_eq!(again.mech, w.mech);
        assert_eq!(again.enclosure().parts().len(), 2);
        // and written again it is the same bytes
        let second = dir.0.join("again.phoenixproj");
        assert!(again.save_to(&second));
        assert_eq!(
            std::fs::read(&second).unwrap(),
            std::fs::read(&path).unwrap()
        );
        // the example written before the parts existed is still version 1 when it is saved
        let mut old = example();
        let plain = dir.0.join("plain.phoenixproj");
        assert!(old.save_to(&plain));
        let plain_text = String::from_utf8(std::fs::read(&plain).unwrap()).unwrap();
        assert!(plain_text.contains("mechanical 1\n") && !plain_text.contains("add-part"));
    }

    fn assembly_path() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/assembly.phoenixproj"
        ))
    }

    /// Two tubes of 20 x 20 x 300 mm added from the library: the second lands at x = 24 mm, the
    /// first place along that the tries find clear of the first one.
    fn two_tubes_in_the_window() -> Window {
        let mut w = with_library();
        choose_part(&mut w, "tube-20x20");
        w.add_step(NewStep::Part);
        w.add_step(NewStep::Part);
        assert!(!w.status.is_problem(), "{:?}", w.status);
        assert_eq!(w.enclosure().parts().len(), 2);
        assert_eq!(part_of(&w, 1).at, [mm_len(24), mm_len(0), mm_len(0)]);
        w
    }

    /// Where a placed part really is, in whole millimetres, once the mates are applied.
    fn placed_at(w: &Window, name: &str) -> [i64; 3] {
        let placed = w.enclosure().placed_parts();
        let p = placed.values().find(|p| p.name == name).unwrap();
        p.at.map(|l| l.get() / 1_000_000)
    }

    fn conflict_lines(w: &Window) -> usize {
        w.lines(ViewKind::Iso, (600.0, 400.0))
            .iter()
            .filter(|l| l.kind == Kind::Conflict)
            .count()
    }

    #[test]
    fn a_mate_is_added_from_the_window_and_its_fields_change_where_the_part_is() {
        let mut w = two_tubes_in_the_window();
        w.select(Selection::Part(1));
        w.add_step(NewStep::Mate);
        assert!(!w.status.is_problem(), "{:?}", w.status);
        assert_eq!(w.selection, Selection::Mate(2));
        assert_eq!(w.mech.steps[2].name, "Mate 1");
        assert_eq!(w.mech.version(), 5);
        // the settings start a mate as a stack, of the selected part on the one before it: the
        // first tube is 300 mm tall, so the second stands at z = 300
        assert_eq!(text_of(&w, Key::MateA), "Part 1");
        assert_eq!(text_of(&w, Key::MateB), "Part 2");
        assert_eq!(text_of(&w, Key::MateKind), "stack");
        assert_eq!(
            text_of(&w, Key::PartPlaced),
            "24.000 mm, 0.000 mm, 300.000 mm"
        );
        let tree = w.tree();
        let group = tree.iter().find(|g| g.title == Msg::GroupMates).unwrap();
        assert_eq!(group.items.len(), 1);
        assert_eq!(group.items[0].label, "Mate 1");
        assert_eq!(group.items[0].selection, Selection::Mate(2));
        // the part it places is the one drawn in the colour of the selection
        assert!(matches!(w.highlight(), Highlight::Object(_)));
        // a gap: the fields of the kind appear, 5 mm of air above the first tube
        let kind = field(&w, Key::MateKind);
        w.choose(kind, "gap");
        assert!(!w.status.is_problem(), "{:?}", w.status);
        assert_eq!(text_of(&w, Key::MateDirection), "+z");
        assert_eq!(text_of(&w, Key::MateDistance), "0.000 mm");
        set(&mut w, Key::MateDistance, "5 mm");
        assert_eq!(
            text_of(&w, Key::PartPlaced),
            "24.000 mm, 0.000 mm, 305.000 mm"
        );
        // on the side of larger x it is at 0 + 20 + 5 and keeps the z it was typed with
        set(&mut w, Key::MateDirection, "+x");
        assert_eq!(
            text_of(&w, Key::PartPlaced),
            "25.000 mm, 0.000 mm, 0.000 mm"
        );
        // an alignment: the upper faces in x are both at 20 mm
        let kind = field(&w, Key::MateKind);
        w.choose(kind, "align");
        set(&mut w, Key::MateAxis, "y");
        let edge = field(&w, Key::MateEdge);
        w.choose(edge, "max");
        assert!(!w.status.is_problem(), "{:?}", w.status);
        assert_eq!(text_of(&w, Key::MateEdge), "max");
        assert_eq!(
            text_of(&w, Key::PartPlaced),
            "24.000 mm, 0.000 mm, 0.000 mm"
        );
        // refusals tell why and leave the mate as it was
        set(&mut w, Key::MateAxis, "w");
        assert!(w.status.is_problem());
        assert_eq!(text_of(&w, Key::MateAxis), "y");
        set(&mut w, Key::MateB, "Part 7");
        assert!(
            w.status.text(Lang::En).contains("PHOENIX-CAD-014"),
            "{:?}",
            w.status
        );
        set(&mut w, Key::MateA, "Part 2");
        assert!(
            w.status.text(Lang::En).contains("PHOENIX-CAD-046"),
            "{:?}",
            w.status
        );
        assert_eq!(text_of(&w, Key::MateA), "Part 1");
        // every field change is an entry of the history
        w.undo();
        assert_eq!(text_of(&w, Key::MateEdge), "min");
        w.undo();
        w.undo();
        assert_eq!(text_of(&w, Key::MateKind), "gap");
        assert_eq!(text_of(&w, Key::MateDirection), "+x");
        w.redo();
        assert_eq!(text_of(&w, Key::MateKind), "align");
    }

    #[test]
    fn a_mate_that_cannot_be_added_says_why_and_leaves_the_project_as_it_was() {
        // no part at all, and only one
        let mut w = with_library();
        w.add_step(NewStep::Mate);
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-046"));
        assert!(
            w.status.text(Lang::En).contains("no part"),
            "{:?}",
            w.status
        );
        choose_part(&mut w, "tube-20x20");
        w.add_step(NewStep::Part);
        w.add_step(NewStep::Mate);
        assert!(
            w.status.text(Lang::En).contains("two parts"),
            "{:?}",
            w.status
        );
        assert_eq!(w.mech.steps.len(), 1);
        // the same part placed twice along z
        let mut w = two_tubes_in_the_window();
        w.add_step(NewStep::Mate);
        assert!(!w.status.is_problem(), "{:?}", w.status);
        w.add_step(NewStep::Mate);
        let text = w.status.text(Lang::En);
        assert!(
            text.contains("PHOENIX-CAD-046") && text.contains("two mates along z"),
            "{text}"
        );
        assert_eq!(w.mech.steps.len(), 3);
        assert_eq!(w.enclosure().mates().len(), 1);
        // a part a mate uses cannot be removed from the tree
        w.select(Selection::Part(0));
        w.remove_step();
        assert!(w.status.is_problem(), "{:?}", w.status);
        assert_eq!(w.enclosure().parts().len(), 2);
        // but the mate can, and the part goes back to where it was typed
        w.select(Selection::Mate(2));
        w.remove_step();
        assert!(!w.status.is_problem(), "{:?}", w.status);
        assert!(w.enclosure().mates().is_empty());
        assert_eq!(placed_at(&w, "Part 2"), [24, 0, 0]);
        w.undo();
        assert_eq!(placed_at(&w, "Part 2"), [24, 0, 300]);
        assert!(w.tree().iter().any(|g| g.title == Msg::GroupMates));
    }

    #[test]
    fn parts_that_share_volume_are_told_in_the_report_the_fields_and_the_views() {
        let mut w = two_tubes_in_the_window();
        assert_eq!(conflict_lines(&w), 0);
        assert!(w.info.interference.as_ref().unwrap().conflicts.is_empty());
        assert!(w.assembly_clear());
        assert!(w.report_text().contains(
            "Interference: none (0 pairs with overlapping boxes looked at, 0 screws through holes)"
        ));
        // an alignment of the lower faces in x puts the second tube over the first one
        w.select(Selection::Part(1));
        w.add_step(NewStep::Mate);
        let kind = field(&w, Key::MateKind);
        w.choose(kind, "align");
        set(&mut w, Key::MateAxis, "x");
        assert!(!w.status.is_problem(), "{:?}", w.status);
        assert!(!w.assembly_clear());
        let report = w.info.interference.as_ref().unwrap();
        assert_eq!(report.conflicts.len(), 1);
        // 20 x 20 x 300 mm of bounding box shared
        assert_eq!(
            report.conflicts[0].shared_nm3,
            120_000 * 1_000_000_000_000_000_000
        );
        let text = w.report_text();
        assert!(text.contains("Mates: 1"), "{text}");
        assert!(
            text.contains("Interference: 1 pairs share volume (1 pairs with overlapping boxes looked at, 0 screws through holes)"),
            "{text}"
        );
        assert!(
            text.contains("  Part 1 / Part 2: up to 120000.000 mm3 shared by the bounding boxes"),
            "{text}"
        );
        assert_eq!(w.assembly_text().lines().count(), 3);
        // the part says who it shares volume with, and the views draw it in the colour of a conflict
        w.select(Selection::Part(1));
        assert_eq!(text_of(&w, Key::PartConflicts), "Part 1");
        assert_eq!(text_of(&w, Key::PartPlaced), "0.000 mm, 0.000 mm, 0.000 mm");
        w.select(Selection::Part(0));
        assert_eq!(text_of(&w, Key::PartConflicts), "Part 2");
        w.select(Selection::Body);
        assert!(conflict_lines(&w) > 0);
        // the part that is selected keeps the colour of the selection
        let before = conflict_lines(&w);
        w.select(Selection::Part(1));
        assert!(conflict_lines(&w) < before);
        // a mate that takes them apart takes the conflict away
        w.select(Selection::Mate(2));
        let kind = field(&w, Key::MateKind);
        w.choose(kind, "stack");
        assert!(w.assembly_clear());
        w.select(Selection::Body);
        assert_eq!(conflict_lines(&w), 0);
        // and the report in the language of the window
        w.lang = Lang::Es;
        assert!(w.report_text().contains("Interferencias: ninguna (0 pares"));
        w.lang = Lang::Pt;
        assert!(w.report_text().contains("Interferências: nenhuma (0 pares"));
    }

    #[test]
    fn the_assembly_example_opens_with_every_mate_valid_and_no_interference() {
        let mut w = fresh();
        assert!(w.open(&assembly_path()), "{:?}", w.status);
        assert_eq!(w.mech.version(), 5);
        assert_eq!(w.info.failed, 0);
        assert_eq!(w.enclosure().parts().len(), 6);
        assert_eq!(w.enclosure().mates().len(), 11);
        let tree = w.tree();
        let group = tree.iter().find(|g| g.title == Msg::GroupMates).unwrap();
        assert_eq!(group.items.len(), 11);
        // the base rail is 300 x 40 x 20 mm; the posts stand on it, the left one flush with its
        // end and both in the middle of its width, the top rail stands on the left post, and the
        // bracket is against the post (x = 20) in the middle of its width, at the z it was typed
        assert_eq!(placed_at(&w, "base rail"), [0, 0, 0]);
        assert_eq!(placed_at(&w, "left post"), [0, 10, 20]);
        assert_eq!(placed_at(&w, "right post"), [280, 10, 20]);
        assert_eq!(placed_at(&w, "top rail"), [0, 0, 170]);
        assert_eq!(placed_at(&w, "shelf bracket"), [20, 10, 100]);
        // the bolt goes through the hole of the bracket: no conflict, one pass
        assert!(w.assembly_clear());
        let report = w.info.interference.as_ref().unwrap();
        assert_eq!(
            (report.boxes_overlapping, report.passes, report.clear),
            (1, 1, 0)
        );
        // a clearance larger than the room in the hole turns the pass into a conflict
        let mut tight = Settings::defaults();
        tight.design.assembly.hole_clearance = Length::new(2_000_000).unwrap();
        let mut w = Window::new(Lang::En, tight, UiSettings::defaults()).unwrap();
        assert!(w.open(&assembly_path()), "{:?}", w.status);
        assert!(!w.assembly_clear());
        let text = w.report_text();
        assert!(
            text.contains(
                "  shelf bracket / shelf bolt: up to 181.500 mm3 shared by the bounding boxes"
            ),
            "{text}"
        );
    }

    #[test]
    fn the_bolts_of_the_example_frame_go_through_the_holes_of_the_brackets() {
        let mut w = fresh();
        assert!(w.open(&frame_path()), "{:?}", w.status);
        assert_eq!(w.mech.version(), 4);
        assert!(w.assembly_clear());
        let report = w.info.interference.as_ref().unwrap();
        assert_eq!(
            (report.boxes_overlapping, report.passes, report.clear),
            (4, 4, 0)
        );
        assert!(w.report_text().contains(
            "Interference: none (4 pairs with overlapping boxes looked at, 4 screws through holes)"
        ));
    }
}
