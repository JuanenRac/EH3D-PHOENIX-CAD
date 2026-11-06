//! The state of the window and everything it decides without a screen: the project that is open,
//! the tree of the model, the fields of the properties, what an edit does, the history of the
//! changes, the views and the texts of the status bar.
//!
//! The window (`gui`) only draws this state and passes on what the person does. An edit is a
//! transaction: the new recipe is built first and the change is kept only when the model accepts
//! it, so a refused value leaves the project exactly as it was.

use crate::body::{Built, Step, StepState};
use crate::config::Settings;
use crate::enclosure::{Edging, Enclosure, EnclosureParams, Face, Hole, Operation, Treatment};
use crate::fit::{check_fit, Envelope, FitReport};
use crate::history::History;
use crate::lang::{fill, Lang, Msg};
use crate::mechanical::{Mechanical, MECHANICAL_DOMAIN};
use crate::messages;
use crate::mount::{Lid, Post};
use crate::parametric::{ParamModel, Parameter};
use crate::project::{Document, PARAMETERS_DOMAIN};
use crate::scad::mm_text;
use crate::scene::{
    mm, outer_size, project, scene_of, BoardBox, Camera, Fit, Frame, Highlight, Line2,
};
use crate::store;
use crate::uiconfig::UiSettings;
use crate::units::{format_length, parse_length};
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
    /// A step that has no editor of its own: its index.
    Other(usize),
    /// A screw hole of the lid, by its position in the list the lid derives.
    Screw(usize),
    /// The envelope that is checked against the box.
    Fit,
}

impl Selection {
    /// The index of the step that this selection is, when it is a step.
    pub fn step(self) -> Option<usize> {
        match self {
            Selection::Hole(i)
            | Selection::Post(i)
            | Selection::Edging(i)
            | Selection::Lid(i)
            | Selection::Other(i) => Some(i),
            Selection::Nothing | Selection::Body | Selection::Screw(_) | Selection::Fit => None,
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
    /// The volume of the material of the empty box, in cubic nanometres.
    pub material_nm3: i128,
    /// How many steps the model refused.
    pub failed: usize,
    /// Whether the envelope fits, and the room left.
    pub fit: FitReport,
    /// Whether the envelope is the board of the project (otherwise it is the one typed).
    pub fit_from_board: bool,
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
        _ => "-".to_string(),
    }
}

fn the_selection_of(index: usize, step: &Step) -> Selection {
    match &step.op {
        Operation::AddHole { .. } => Selection::Hole(index),
        Operation::AddPost { .. } => Selection::Post(index),
        Operation::SetEdging(_) => Selection::Edging(index),
        Operation::SetLid(Some(_)) => Selection::Lid(index),
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

    // -----------------------------------------------------------------------------------------
    // The tree and the selection
    // -----------------------------------------------------------------------------------------

    /// The tree of the model.
    pub fn tree(&self) -> Vec<TreeGroup> {
        let lang = self.lang;
        let mut holes = Vec::new();
        let mut posts = Vec::new();
        let mut edges = Vec::new();
        let mut lids = Vec::new();
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
                Selection::Edging(_) => edges.push(item),
                Selection::Lid(_) => lids.push(item),
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
        let mut groups = vec![TreeGroup {
            title: Msg::GroupBox,
            items: vec![TreeItem {
                selection: Selection::Body,
                label: Msg::Dimensions.tr(lang).to_string(),
                mark: Mark::Normal,
            }],
        }];
        for (title, items) in [
            (Msg::GroupHoles, holes),
            (Msg::GroupPosts, posts),
            (Msg::GroupEdges, edges),
            (Msg::GroupLid, lids),
            (Msg::GroupScrews, screws),
            (Msg::GroupOther, others),
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
            | Selection::Other(i) => i < self.mech.steps.len(),
            Selection::Screw(k) => k < self.built.enclosure.lid_holes().len(),
            Selection::Nothing | Selection::Body | Selection::Fit => true,
        };
        self.selection = if valid { selection } else { Selection::Body };
        self.refresh_fields();
    }

    /// What is drawn in the colour of the selection.
    pub fn highlight(&self) -> Highlight {
        let id_of = |i: usize| -> Highlight {
            match self.mech.steps.get(i).map(|s| &s.op) {
                Some(Operation::AddHole { id, .. }) | Some(Operation::AddPost { id, .. }) => {
                    Highlight::Object(*id)
                }
                _ => Highlight::Nothing,
            }
        };
        match self.selection {
            Selection::Hole(i) | Selection::Post(i) => id_of(i),
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
            Operation::SetEdging(edging) => {
                Ok(Operation::SetEdging(self.edited_edging(edging, key, text)?))
            }
            Operation::SetLid(Some(lid)) => {
                Ok(Operation::SetLid(Some(self.edited_lid(lid, key, text)?)))
            }
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
        let built = next.build(&self.settings.constants, &self.settings.expr)?;
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
    // History
    // -----------------------------------------------------------------------------------------

    /// Goes back one change.
    pub fn undo(&mut self) {
        match self.history.undo(self.mech.clone()) {
            Some((name, previous)) => self.restore(previous, Msg::Undone, name),
            None => self.status = Status::Done(Msg::NothingToUndo, Vec::new()),
        }
    }

    /// Goes forward one change that was undone.
    pub fn redo(&mut self) {
        match self.history.redo(self.mech.clone()) {
            Some((name, next)) => self.restore(next, Msg::Redone, name),
            None => self.status = Status::Done(Msg::NothingToRedo, Vec::new()),
        }
    }

    fn restore(&mut self, mech: Mechanical, message: Msg, name: String) {
        match mech.build(&self.settings.constants, &self.settings.expr) {
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
        let mech = opened
            .document
            .mechanical(&self.settings.expr, &self.settings.mechanical)
            .map_err(Status::Error)?
            .ok_or_else(|| Status::Problem(Msg::NoMechanical, Vec::new()))?;
        let built = mech
            .build(&self.settings.constants, &self.settings.expr)
            .map_err(Status::Error)?;
        self.mech = mech;
        self.built = built;
        self.document = opened.document;
        self.read_only = opened.read_only;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.history.clear();
        self.selection = Selection::Body;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.status = if self.read_only {
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
            material_nm3: e.material_volume_nm3(),
            failed: self
                .built
                .states
                .iter()
                .filter(|s| matches!(s, StepState::Failed(_)))
                .count(),
            fit: check_fit(e, &envelope, self.settings.printer.fit_clearance),
            fit_from_board: from_board,
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

    /// The lines of a view put on a pane of `pane` pixels.
    pub fn lines(&self, view: ViewKind, pane: (f64, f64)) -> Vec<Line2> {
        let segments = scene_of(
            &self.built.enclosure,
            self.board_box(),
            self.highlight(),
            self.ui.circle_segments,
        );
        let (control, fit) = match view {
            ViewKind::Iso => (&self.view3d, Fit::Sphere),
            ViewKind::Plan => (&self.plan, Fit::Bounds),
        };
        project(
            &segments,
            outer_size(&self.built.enclosure),
            &Frame {
                camera: control.camera,
                zoom: control.zoom,
                pan: control.pan,
                pane,
                margin: self.ui.margin,
                fit,
            },
        )
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
        assert_eq!(w.tree()[1].items[1].mark, Mark::Suppressed);
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
}
