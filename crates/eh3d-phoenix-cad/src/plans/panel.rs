// SPDX-License-Identifier: GPL-3.0-or-later
//! The state and the lines of the panel «Planos» of the window, with no window in it: the project
//! file is read, the sheet is evaluated against its history and its sketches, and the panel shows
//! lines of text (in the language of the window) and the buttons that change the sheet. The window
//! draws these lines and calls these methods; everything that decides is here and is tested.

use std::fs;
use std::path::{Path, PathBuf};

use eh3d_container::IdGenerator;

use super::drag::{pick_balloon, pick_note, snap, BalloonPart};
use super::measure::{view_axes, Issue, Model};
use super::model::{
    next_label, revision_label, BreakCut, Dim, DimKind, Note, NoteAnchor, Plan, Ref, SectionCut,
    Side, Source, View, ViewKind,
};
use super::out;
use super::scene::{evaluate, Evaluation};
use super::settings::SheetSettings;
use crate::config::Settings;
use crate::enclosure::Id;
use crate::export::write_bytes;
use crate::featuredag::{FeatureDag, HistorySettings, Kind};
use crate::lang::{fill, Lang, Msg};
use crate::messages;
use crate::project::Document;
use crate::sketch::settings::SketchSettings;
use crate::sketch::Sketch;
use crate::store;
use crate::units::parse_length;
use crate::CadError;

/// How a line of the panel is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// As the rest.
    Plain,
    /// Something that is as it should be.
    Good,
    /// A problem.
    Bad,
}

/// A line of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The text.
    pub text: String,
    /// How it is shown.
    pub tone: Tone,
}

fn line(text: String, tone: Tone) -> Line {
    Line { text, tone }
}

/// A layer of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerLine {
    /// The name.
    pub name: String,
    /// Shown or not.
    pub visible: bool,
    /// Protected or not.
    pub locked: bool,
    /// The text.
    pub text: String,
}

/// Everything the panel shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PanelLines {
    /// The line of the sheet, or the reason there is none.
    pub head: Option<Line>,
    /// The views.
    pub views: Vec<Line>,
    /// The dimensions.
    pub dims: Vec<Line>,
    /// The balloons.
    pub balloons: Vec<Line>,
    /// The notes with a leader.
    pub note_items: Vec<Line>,
    /// The bill.
    pub bom: Vec<Line>,
    /// The revisions of the table, oldest first.
    pub revisions: Vec<Line>,
    /// Notes that are not problems (the table of ISO 286 has not been checked).
    pub notes: Vec<Line>,
    /// The layers.
    pub layers: Vec<LayerLine>,
    /// The problems.
    pub issues: Vec<Line>,
}

/// The text of a problem in a language: its subject, its code with the title of the code, and the
/// reason.
pub fn issue_text(issue: &Issue, lang: Lang) -> String {
    let title = messages::message(issue.code, lang.code())
        .or_else(|| messages::message(issue.code, "en"))
        .unwrap_or("");
    format!(
        "{}: {} {} ({})",
        issue.subject, issue.code, title, issue.detail
    )
}

/// The lines of a sheet that has been evaluated.
pub fn lines_of(plan: &Plan, eval: &Evaluation, s: &SheetSettings, lang: Lang) -> PanelLines {
    let mut out = PanelLines::default();
    let orientation = if plan.portrait {
        Msg::PlPortrait
    } else {
        Msg::PlLandscape
    };
    out.head = Some(line(
        fill(
            Msg::PlHead.tr(lang),
            &[
                &plan.format,
                orientation.tr(lang),
                &format!("{}:{}", eval.scale.0, eval.scale.1),
                &plan.views.len().to_string(),
                &plan.dims.len().to_string(),
                &plan.balloons.len().to_string(),
            ],
        ),
        Tone::Plain,
    ));
    for v in &eval.views {
        let mut text = fill(
            Msg::PlView.tr(lang),
            &[
                &v.name,
                v.kind.name(),
                &v.visible.to_string(),
                &v.hidden.to_string(),
            ],
        );
        if let Some(b) = plan.view(v.id).and_then(|p| p.brk) {
            let removed = (b.to_nm - b.from_nm - s.brk.gap_nm).max(0);
            text.push_str("; ");
            text.push_str(&fill(
                Msg::PlBroken.tr(lang),
                &[&super::prim::nm_text(removed, 3)],
            ));
        }
        out.views.push(line(text, Tone::Plain));
    }
    for n in &eval.notes {
        out.note_items.push(line(
            fill(Msg::PlNoteRow.tr(lang), &[&n.id.to_string(), &n.text]),
            Tone::Plain,
        ));
    }
    for d in &eval.dims {
        out.dims.push(line(
            fill(Msg::PlDim.tr(lang), &[&d.name, &d.texts.join(" ")]),
            Tone::Plain,
        ));
    }
    for b in &eval.balloons {
        if let Some(n) = b.number {
            out.balloons.push(line(
                fill(Msg::PlBalloon.tr(lang), &[&b.name, &n.to_string()]),
                Tone::Plain,
            ));
        }
    }
    for l in &eval.bom {
        out.bom.push(line(
            fill(
                Msg::PlBom.tr(lang),
                &[
                    &l.number.to_string(),
                    &l.quantity.to_string(),
                    &l.description,
                    &l.material,
                ],
            ),
            Tone::Plain,
        ));
    }
    for r in &plan.revisions {
        out.revisions.push(line(
            fill(
                Msg::PlRevRow.tr(lang),
                &[
                    &revision_label(r.number, s.revisions.numbering),
                    &r.date,
                    &r.description,
                    &r.author,
                ],
            ),
            Tone::Plain,
        ));
    }
    if eval.unverified_table {
        out.notes
            .push(line(Msg::IsoUnverified.tr(lang).to_string(), Tone::Plain));
    }
    for (name, _) in &s.layers {
        let visible = plan.layer_visible(name, s);
        let locked = plan.layer_locked(name, s);
        let mut state = if visible { Msg::PlShown } else { Msg::PlHidden }
            .tr(lang)
            .to_string();
        if locked {
            state.push_str(", ");
            state.push_str(Msg::PlLocked.tr(lang));
        }
        out.layers.push(LayerLine {
            name: name.clone(),
            visible,
            locked,
            text: fill(Msg::PlLayer.tr(lang), &[name, &state]),
        });
    }
    for i in &eval.issues {
        out.issues.push(line(issue_text(i, lang), Tone::Bad));
    }
    out
}

/// The only feature that no other one takes, which is the result of a history with no tip chosen.
fn sole_sink(dag: &FeatureDag) -> Option<Id> {
    let used: std::collections::BTreeSet<Id> = dag
        .nodes()
        .values()
        .flat_map(|n| match &n.kind {
            Kind::Feature { inputs, .. } => inputs.clone(),
            Kind::Parameter { .. } => Vec::new(),
        })
        .collect();
    let mut sinks = dag
        .nodes()
        .values()
        .filter(|n| matches!(n.kind, Kind::Feature { .. }) && !used.contains(&n.id));
    match (sinks.next(), sinks.next()) {
        (Some(only), None) => Some(only.id),
        _ => None,
    }
}

/// What is being dragged with the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragTarget {
    /// The line of a dimension.
    Dim(Id),
    /// A balloon: the circle or the end of its leader.
    Balloon(Id, BalloonPart),
    /// The line of the cut of a section view (the id is the one of the section view).
    Cut(Id),
    /// The text of a note.
    Note(Id),
}

/// A length typed in millimetres: a bare number is millimetres, a number with a unit is read with it.
pub fn parse_mm(text: &str) -> Result<i64, CadError> {
    let t = text.trim();
    let l = if t.chars().any(char::is_alphabetic) {
        parse_length(t)?
    } else {
        parse_length(&format!("{t} mm"))?
    };
    Ok(l.get())
}

/// The kinds of section a view can be the line of the cut of: the views that look along an axis
/// other than the one the parent shows across it (a top view shows `x` and `y`, so it can carry the
/// cut of a front view, across `y`, or of a right view, across `x`).
pub fn section_kinds_for(parent: ViewKind) -> Vec<ViewKind> {
    let Some((u, v)) = view_axes(parent) else {
        return Vec::new();
    };
    [ViewKind::Front, ViewKind::Top, ViewKind::Right]
        .into_iter()
        .filter(|k| k.depth_axis().is_some_and(|a| a == u || a == v) && *k != parent)
        .collect()
}

/// What the buttons of the panel do. There is one set of them: no mode, no second view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormAction {
    /// Add a revision to the table.
    Revision,
    /// Dimension the chosen view with its two overall sizes.
    Dims,
    /// Add a section cut from the chosen view.
    Section,
    /// Add a note to the chosen view.
    Note,
    /// Break the chosen view over a range.
    Break,
    /// Make the chosen view whole again.
    Unbreak,
}

/// The texts and choices of the form of the panel, with no window in it: the window shows these
/// and calls [`PlanForm::apply`] when a button is pressed; what the button does is decided and tested
/// here.
#[derive(Debug, Clone, Default)]
pub struct PlanForm {
    /// The date of a revision.
    pub date: String,
    /// What the revision says.
    pub description: String,
    /// Who made it.
    pub author: String,
    /// The text of a note.
    pub text: String,
    /// The position of a cut, in millimetres.
    pub position: String,
    /// Where the range of a break starts, in millimetres.
    pub from: String,
    /// Where it ends, in millimetres.
    pub to: String,
    /// The chosen view (an index into the views of the sheet).
    pub view: usize,
    /// The chosen kind of section (an index into [`PlanForm::kinds`]).
    pub kind: usize,
    /// Whether the break is along the axis across the picture.
    pub along_u: bool,
}

impl PlanForm {
    /// The view that is chosen, when the sheet has any (an index past the end is the last one).
    pub fn chosen(&self, st: &PlanState) -> Option<(Id, ViewKind)> {
        let plan = st.plan.as_ref()?;
        let v = plan.views.get(self.view).or_else(|| plan.views.last())?;
        Some((v.id, v.kind))
    }

    /// The kinds of section the chosen view can carry the line of the cut of.
    pub fn kinds(&self, st: &PlanState) -> Vec<ViewKind> {
        self.chosen(st)
            .map(|(_, k)| section_kinds_for(k))
            .unwrap_or_default()
    }

    /// Does what the button says with what the form holds, and says what was done.
    pub fn apply(
        &mut self,
        st: &mut PlanState,
        action: FormAction,
        lang: Lang,
    ) -> Result<Line, CadError> {
        let done = |what: String| line(fill(Msg::PlAdded.tr(lang), &[&what]), Tone::Good);
        let (view, _) = self
            .chosen(st)
            .ok_or_else(|| CadError::Plan(Msg::PlNone.tr(Lang::En).to_string()))?;
        let out = match action {
            FormAction::Revision => {
                let n = st.add_revision(&self.date, &self.description, &self.author, lang)?;
                self.description.clear();
                let label = revision_label(n, st.sheet.revisions.numbering);
                done(format!("{} {label}", Msg::TbRevision.tr(lang)))
            }
            FormAction::Dims => {
                let n = st.add_overall_dims(view, lang)?;
                done(format!("{} {n}", Msg::PlDims.tr(lang)))
            }
            FormAction::Section => {
                let kinds = self.kinds(st);
                let kind = kinds
                    .get(self.kind)
                    .or_else(|| kinds.first())
                    .copied()
                    .ok_or_else(|| {
                        CadError::PlanSection(
                            "the view shows no axis across which a cut can be made".to_string(),
                        )
                    })?;
                let label = st.add_section(view, kind, &self.position, lang)?;
                done(format!("{label}-{label}"))
            }
            FormAction::Note => {
                st.add_note(&self.text, view, lang)?;
                self.text.clear();
                done(Msg::PlNotes.tr(lang).to_string())
            }
            FormAction::Break => {
                st.add_break(view, self.along_u, &self.from, &self.to, lang)?;
                done(Msg::PlAddBreak.tr(lang).to_string())
            }
            FormAction::Unbreak => {
                st.remove_break(view, lang)?;
                done(Msg::PlViewOne.tr(lang).to_string())
            }
        };
        Ok(out)
    }
}

/// What the panel keeps: the settings, the project it is on and the sheet.
#[derive(Debug)]
pub struct PlanState {
    /// The settings of the drawings.
    pub sheet: SheetSettings,
    /// The settings of the library (the constants and the expressions of the history).
    pub cfg: Settings,
    /// The settings of the history.
    pub hist: HistorySettings,
    /// The settings of the sketches.
    pub sketch_settings: SketchSettings,
    /// The project file the sheet belongs to.
    pub path: Option<PathBuf>,
    /// The sheet, when the project has one.
    pub plan: Option<Plan>,
    /// The result of the last evaluation.
    pub eval: Option<Evaluation>,
    /// Whether the sheet has changes that are not in the file.
    pub dirty: bool,
    /// What the last action said, as text, and whether it is a problem.
    pub note: Option<Line>,
    /// What is being dragged.
    pub drag: Option<DragTarget>,
    dag: Option<FeatureDag>,
    sketches: Vec<Sketch>,
    ids: IdGenerator<eh3d_container::SystemClock, eh3d_container::SystemRandom>,
}

/// The largest project file the panel reads.
const MAX_PROJECT_BYTES: usize = 256 << 20;

impl PlanState {
    /// A panel that is on no project.
    pub fn new(
        sheet: SheetSettings,
        cfg: Settings,
        hist: HistorySettings,
        sketch_settings: SketchSettings,
    ) -> PlanState {
        PlanState {
            sheet,
            cfg,
            hist,
            sketch_settings,
            path: None,
            plan: None,
            eval: None,
            dirty: false,
            note: None,
            drag: None,
            dag: None,
            sketches: Vec::new(),
            ids: IdGenerator::system(),
        }
    }

    /// A panel with every setting at its default.
    pub fn with_defaults() -> PlanState {
        PlanState::new(
            SheetSettings::defaults(),
            Settings::defaults(),
            HistorySettings::defaults(),
            SketchSettings::defaults(),
        )
    }

    /// Reads the project file named: its history (built again), its sketches and its sheet.
    pub fn load(&mut self, path: &Path, lang: Lang) -> Result<(), CadError> {
        let bytes = store::load_limited(path, MAX_PROJECT_BYTES as u64)?;
        self.load_bytes(&bytes, Some(path.to_path_buf()), lang)
    }

    fn read_model(&mut self, doc: &Document) -> Result<(), CadError> {
        let mut dag = doc.history(self.hist.clone(), &self.cfg)?;
        if let Some(d) = dag.as_mut() {
            d.rebuild(&self.cfg)?;
        }
        self.dag = dag;
        self.sketches = doc.sketches(&self.sketch_settings)?;
        Ok(())
    }

    /// Reads a project from its bytes.
    pub fn load_bytes(
        &mut self,
        bytes: &[u8],
        path: Option<PathBuf>,
        lang: Lang,
    ) -> Result<(), CadError> {
        let doc = Document::from_bytes(bytes, MAX_PROJECT_BYTES)?;
        self.read_model(&doc)?;
        self.plan = doc.plan(&self.sheet)?;
        self.path = path;
        self.dirty = false;
        self.note = None;
        self.evaluate(lang)
    }

    /// Reads the history and the sketches of the file again and keeps the sheet as it is (what the
    /// button «Update from the model» does).
    pub fn refresh(&mut self, lang: Lang) -> Result<(), CadError> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        let bytes = store::load_limited(&path, MAX_PROJECT_BYTES as u64)?;
        let doc = Document::from_bytes(&bytes, MAX_PROJECT_BYTES)?;
        self.read_model(&doc)?;
        self.evaluate(lang)
    }

    /// Works out the sheet against the model.
    pub fn evaluate(&mut self, lang: Lang) -> Result<(), CadError> {
        self.eval = match &self.plan {
            Some(plan) => {
                let model = Model {
                    dag: self.dag.as_ref(),
                    sketches: &self.sketches,
                };
                Some(evaluate(plan, &model, &self.cfg, &self.sheet, lang)?)
            }
            None => None,
        };
        Ok(())
    }

    /// Makes the sheet of a project that has none: the front, top, right and isometric views of
    /// the result of the history. Refused when the project has no history.
    pub fn create_sheet(&mut self, lang: Lang) -> Result<(), CadError> {
        if self.dag.is_none() {
            return Err(CadError::Plan(Msg::FeatNone.tr(Lang::En).to_string()));
        }
        let mut plan = Plan::new(&self.sheet);
        for (kind, name) in [
            (ViewKind::Front, "front"),
            (ViewKind::Top, "top"),
            (ViewKind::Right, "right"),
            (ViewKind::Iso, "iso"),
        ] {
            let id: Id = self
                .ids
                .generate()
                .map_err(|e| CadError::Plan(e.to_string()))?;
            plan.add_view(
                View {
                    id,
                    name: name.to_string(),
                    kind,
                    source: Source::Tip,
                    place: super::model::Place::Auto,
                    scale: None,
                    hidden: self.sheet.hidden_lines,
                    section: None,
                    detail: None,
                    hatch: None,
                    brk: None,
                    extra: Vec::new(),
                },
                &self.sheet,
            )?;
        }
        self.plan = Some(plan);
        self.dirty = true;
        self.evaluate(lang)
    }

    /// The dimension whose line is under a point of the sheet (micrometres), within `tolerance`.
    /// The nearest line wins when two are under the point.
    pub fn pick_dim(&self, pointer: (i64, i64), tolerance: i64) -> Option<Id> {
        let eval = self.eval.as_ref()?;
        eval.dims
            .iter()
            .filter_map(|d| d.line.map(|l| (d.id, l)))
            .filter(|(_, l)| l.hit(pointer, tolerance))
            .min_by_key(|(_, l)| {
                let across = if l.horizontal { pointer.1 } else { pointer.0 };
                (across - l.line_at()).abs()
            })
            .map(|(id, _)| id)
    }

    /// What is under a point of the sheet and can be dragged: a balloon (its circle or the end of
    /// its leader), the text of a note, the line of a cut or the line of a dimension, in that order.
    pub fn pick(&self, pointer: (i64, i64)) -> Option<DragTarget> {
        let plan = self.plan.as_ref()?;
        let eval = self.eval.as_ref()?;
        let tolerance = self.sheet.dim.drag_snap_um.max(2000);
        if let Some((id, part)) = pick_balloon(
            &plan.balloons,
            self.sheet.balloon_radius_um,
            pointer,
            tolerance,
        ) {
            return Some(DragTarget::Balloon(id, part));
        }
        if let Some(id) = pick_note(
            &plan.notes,
            self.sheet.note.text_height_um,
            pointer,
            tolerance,
        ) {
            return Some(DragTarget::Note(id));
        }
        if let Some(c) = eval
            .cuts
            .iter()
            .filter(|c| c.draggable && c.hit(pointer, tolerance))
            .min_by_key(|c| c.distance(pointer))
        {
            return Some(DragTarget::Cut(c.view));
        }
        self.pick_dim(pointer, tolerance).map(DragTarget::Dim)
    }

    /// Starts to drag what is under the point, when there is something.
    pub fn begin_drag(&mut self, pointer: (i64, i64)) -> bool {
        self.drag = self.pick(pointer);
        self.drag.is_some()
    }

    /// Moves what is being dragged to the point. A line of a dimension lands on a multiple of
    /// `sheet.dim.drag_snap` (the result is its distance from the view, in micrometres); the line
    /// of a cut lands on a multiple of `sheet.note.drag_snap` of the sheet (the result is the
    /// position of the plane in nanometres); a balloon and a note land on the same grid (the
    /// result is 0).
    pub fn drag_to(&mut self, pointer: (i64, i64), lang: Lang) -> Result<Option<i64>, CadError> {
        let (Some(target), Some(eval)) = (self.drag, self.eval.as_ref()) else {
            return Ok(None);
        };
        let grid = self.sheet.note.drag_snap_um;
        let result = match target {
            DragTarget::Dim(id) => {
                let Some(line) = eval.dims.iter().find(|d| d.id == id).and_then(|d| d.line) else {
                    return Ok(None);
                };
                let offset = line.offset_for(pointer, self.sheet.dim.drag_snap_um);
                let Some(plan) = self.plan.as_mut() else {
                    return Ok(None);
                };
                plan.set_dim_offset(id, Some(offset), &self.sheet)?;
                offset
            }
            DragTarget::Cut(view) => {
                let Some(line) = eval.cuts.iter().find(|c| c.view == view).copied() else {
                    return Ok(None);
                };
                let at_nm = line.at_nm_for(pointer, grid);
                let Some(plan) = self.plan.as_mut() else {
                    return Ok(None);
                };
                plan.move_cut(view, at_nm, &self.sheet)?;
                at_nm
            }
            DragTarget::Balloon(id, part) => {
                let Some(plan) = self.plan.as_mut() else {
                    return Ok(None);
                };
                let Some(b) = plan.balloons.iter().find(|b| b.id == id) else {
                    return Ok(None);
                };
                let p = snap(pointer, grid);
                let (at, to) = match part {
                    BalloonPart::Circle => (p, b.to),
                    BalloonPart::Tip => (b.at, p),
                };
                plan.move_balloon(id, at, to, &self.sheet)?;
                0
            }
            DragTarget::Note(id) => {
                let Some(plan) = self.plan.as_mut() else {
                    return Ok(None);
                };
                plan.move_note(id, snap(pointer, grid), &self.sheet)?;
                0
            }
        };
        self.dirty = true;
        self.evaluate(lang)?;
        Ok(Some(result))
    }

    /// Ends the dragging.
    pub fn end_drag(&mut self) {
        self.drag = None;
    }

    fn new_id(&mut self) -> Result<Id, CadError> {
        self.ids
            .generate()
            .map_err(|e| CadError::Plan(e.to_string()))
    }

    /// The views of the sheet, for a list to choose from: the id and the name.
    pub fn view_choices(&self) -> Vec<(Id, String)> {
        self.plan
            .as_ref()
            .map(|p| p.views.iter().map(|v| (v.id, v.name.clone())).collect())
            .unwrap_or_default()
    }

    /// Dimensions a view of the front, the top or the right with the two overall sizes of the part
    /// it looks at (across the picture and up it), tied to the faces of its node of the history.
    /// Returns how many were added.
    pub fn add_overall_dims(&mut self, view: Id, lang: Lang) -> Result<usize, CadError> {
        let node = {
            let plan = self
                .plan
                .as_ref()
                .ok_or_else(|| CadError::PlanDim("there is no sheet".to_string()))?;
            let v = plan
                .view(view)
                .ok_or_else(|| CadError::UnknownName(view.to_string()))?;
            match (&v.source, self.dag.as_ref()) {
                (Source::Node(n), _) => *n,
                (Source::Tip, Some(d)) => d
                    .tip()
                    .or_else(|| sole_sink(d))
                    .ok_or_else(|| CadError::PlanDim("the history has no result".to_string()))?,
                _ => {
                    return Err(CadError::PlanDim(
                        "the view looks at no solid of the history".to_string(),
                    ))
                }
            }
        };
        let (kind, name) = {
            let v = self.plan.as_ref().and_then(|p| p.view(view));
            let v = v.ok_or_else(|| CadError::UnknownName(view.to_string()))?;
            (v.kind, v.name.clone())
        };
        let (u, w) = view_axes(kind).ok_or_else(|| {
            CadError::PlanDim("lengths are drawn in the front, top and right views".to_string())
        })?;
        let mut next = self
            .plan
            .clone()
            .ok_or_else(|| CadError::PlanDim("there is no sheet".to_string()))?;
        let mut added = 0;
        for (axis, word) in [(u, "width"), (w, "height")] {
            let id = self.new_id()?;
            next.add_dim(
                Dim {
                    id,
                    name: format!("{name} {word}"),
                    kind: DimKind::Linear,
                    refs: vec![
                        Ref::Face {
                            node,
                            axis,
                            side: Side::Min,
                        },
                        Ref::Face {
                            node,
                            axis,
                            side: Side::Max,
                        },
                    ],
                    view,
                    offset_um: None,
                    step_um: None,
                    tol: None,
                    layer: "DIMENSION".to_string(),
                    extra: Vec::new(),
                },
                &self.sheet,
            )?;
            added += 1;
        }
        self.plan = Some(next);
        self.dirty = true;
        self.evaluate(lang)?;
        Ok(added)
    }

    /// Adds a section view of the solid that `parent` looks at, of kind `kind`, cut across the axis
    /// that `kind` looks along at the position typed (millimetres; a unit may follow), with the next
    /// free letters. The line of the cut is drawn in `parent`. Returns the letters.
    pub fn add_section(
        &mut self,
        parent: Id,
        kind: ViewKind,
        position: &str,
        lang: Lang,
    ) -> Result<String, CadError> {
        let at_nm = parse_mm(position)?;
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| CadError::PlanSection("there is no sheet".to_string()))?;
        let pv = plan
            .view(parent)
            .ok_or_else(|| CadError::UnknownName(parent.to_string()))?;
        if !section_kinds_for(pv.kind).contains(&kind) {
            return Err(CadError::PlanSection(format!(
                "the view {} does not show the axis of a cut for a {} view",
                pv.name,
                kind.name()
            )));
        }
        let label = next_label(plan);
        let source = pv.source.clone();
        let id = self.new_id()?;
        let mut view = View::plain(id, &format!("section {label}"), kind, source, false);
        view.section = Some(SectionCut {
            parent,
            at_nm,
            label: label.clone(),
        });
        let plan = self
            .plan
            .as_mut()
            .ok_or_else(|| CadError::PlanSection("there is no sheet".to_string()))?;
        plan.add_view(view, &self.sheet)?;
        self.dirty = true;
        self.evaluate(lang)?;
        Ok(label)
    }

    /// Adds a note with a leader to the middle of the picture of a view. The text starts to the
    /// right of the view and above it, inside the frame of the sheet; it is dragged from there.
    pub fn add_note(&mut self, text: &str, view: Id, lang: Lang) -> Result<Id, CadError> {
        let eval = self
            .eval
            .as_ref()
            .ok_or_else(|| CadError::PlanNote("there is no sheet".to_string()))?;
        let r = eval
            .views
            .iter()
            .find(|v| v.id == view)
            .ok_or_else(|| CadError::PlanNote("the view could not be made".to_string()))?;
        let (lo, hi) = r.picture;
        let anchor = NoteAnchor::View {
            view,
            u_nm: lo.0 + (hi.0 - lo.0) / 2,
            v_nm: lo.1 + (hi.1 - lo.1) / 2,
        };
        let h = self.sheet.note.text_height_um;
        let width = text.chars().count() as i64 * h * 6 / 10;
        let max_x = eval.scene.width_um - self.sheet.margin_um - width;
        let max_y = eval.scene.height_um - self.sheet.margin_um - h;
        let at = (
            (r.origin.0 + r.size.0 + self.sheet.view_gap_um / 2)
                .min(max_x)
                .max(self.sheet.margin_um),
            (r.origin.1 + r.size.1).min(max_y).max(self.sheet.margin_um),
        );
        let id = self.new_id()?;
        let plan = self
            .plan
            .as_mut()
            .ok_or_else(|| CadError::PlanNote("there is no sheet".to_string()))?;
        plan.add_note(
            Note {
                id,
                text: text.to_string(),
                anchor,
                at,
                centre: false,
                layer: "NOTE".to_string(),
                extra: Vec::new(),
            },
            &self.sheet,
        )?;
        self.dirty = true;
        self.evaluate(lang)?;
        Ok(id)
    }

    /// Makes a view a broken view: the range typed (millimetres, in the coordinates of the picture)
    /// is left out. `along_u` says whether the range is along the axis across the picture or up it.
    pub fn add_break(
        &mut self,
        view: Id,
        along_u: bool,
        from: &str,
        to: &str,
        lang: Lang,
    ) -> Result<(), CadError> {
        let cut = BreakCut {
            along_u,
            from_nm: parse_mm(from)?,
            to_nm: parse_mm(to)?,
        };
        let plan = self
            .plan
            .as_mut()
            .ok_or_else(|| CadError::PlanBreak("there is no sheet".to_string()))?;
        plan.set_break(view, Some(cut), &self.sheet)?;
        self.dirty = true;
        self.evaluate(lang)
    }

    /// Makes a broken view an ordinary one again.
    pub fn remove_break(&mut self, view: Id, lang: Lang) -> Result<(), CadError> {
        let plan = self
            .plan
            .as_mut()
            .ok_or_else(|| CadError::PlanBreak("there is no sheet".to_string()))?;
        plan.set_break(view, None, &self.sheet)?;
        self.dirty = true;
        self.evaluate(lang)
    }

    /// Adds a revision to the table of the sheet (the date and the author are free texts).
    pub fn add_revision(
        &mut self,
        date: &str,
        description: &str,
        author: &str,
        lang: Lang,
    ) -> Result<u32, CadError> {
        let Some(plan) = self.plan.as_mut() else {
            return Err(CadError::PlanRevision("there is no sheet".to_string()));
        };
        let n = plan.add_revision(date, description, author, &self.sheet)?;
        self.dirty = true;
        self.evaluate(lang)?;
        Ok(n)
    }

    /// Shows or hides a layer.
    pub fn toggle_visible(&mut self, name: &str, lang: Lang) -> Result<(), CadError> {
        let Some(plan) = self.plan.as_mut() else {
            return Ok(());
        };
        let now = plan.layer_visible(name, &self.sheet);
        plan.set_layer_visible(name, !now, &self.sheet)?;
        self.dirty = true;
        self.evaluate(lang)
    }

    /// Protects a layer or lets it be changed again.
    pub fn toggle_locked(&mut self, name: &str, lang: Lang) -> Result<(), CadError> {
        let Some(plan) = self.plan.as_mut() else {
            return Ok(());
        };
        let now = plan.layer_locked(name, &self.sheet);
        plan.set_layer_locked(name, !now, &self.sheet)?;
        self.dirty = true;
        self.evaluate(lang)
    }

    /// Keeps the sheet in the project file (the other domains are written back as they are, and the
    /// numbers of the items of the bill are fixed).
    pub fn save(&mut self, lang: Lang) -> Result<(), CadError> {
        let (Some(path), Some(plan)) = (self.path.clone(), self.plan.clone()) else {
            return Err(CadError::Plan(
                "there is no project file or no sheet".to_string(),
            ));
        };
        let bytes = store::load_limited(&path, MAX_PROJECT_BYTES as u64)?;
        let mut doc = Document::from_bytes(&bytes, MAX_PROJECT_BYTES)?;
        let mut plan = plan;
        if let Some(d) = &self.dag {
            let parts = super::bom::part_nodes(d, &self.cfg, self.sheet.max_parts)?;
            plan.assign_numbers(&parts);
        }
        plan.validate(&self.sheet)?;
        doc.set_plan(&plan)?;
        store::save_atomic(&path, &doc.to_bytes(), 1)?;
        self.plan = Some(plan);
        self.dirty = false;
        self.note = Some(line(
            fill(Msg::PlSaved.tr(lang), &[&path.display().to_string()]),
            Tone::Good,
        ));
        self.evaluate(lang)
    }

    /// Writes the sheet as an SVG file; never over a file that exists unless `overwrite`.
    pub fn export_svg(
        &mut self,
        target: &Path,
        overwrite: bool,
        lang: Lang,
    ) -> Result<(), CadError> {
        let eval = self
            .eval
            .as_ref()
            .ok_or_else(|| CadError::Plan(Msg::PlNone.tr(Lang::En).to_string()))?;
        let title = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let svg = out::to_svg(&eval.scene, &title);
        write_bytes(
            target,
            svg.as_bytes(),
            overwrite,
            self.path.as_deref(),
            1 << 14,
        )?;
        self.note = Some(line(
            fill(
                Msg::PlWrote.tr(lang),
                &[
                    &target.display().to_string(),
                    &eval.scene.prims.len().to_string(),
                ],
            ),
            Tone::Good,
        ));
        Ok(())
    }

    /// What the panel shows now.
    pub fn lines(&self, lang: Lang) -> PanelLines {
        match (&self.plan, &self.eval) {
            (Some(p), Some(e)) => lines_of(p, e, &self.sheet, lang),
            _ => PanelLines {
                head: Some(line(
                    if self.path.is_some() {
                        Msg::PlNone
                    } else {
                        Msg::PlNoProject
                    }
                    .tr(lang)
                    .to_string(),
                    Tone::Plain,
                )),
                ..PanelLines::default()
            },
        }
    }

    /// The text of an error in a language, for the panel to show in red.
    pub fn error_line(e: &CadError, lang: Lang) -> Line {
        line(crate::window::error_text(e, lang), Tone::Bad)
    }

    /// Writes the project's sheet text for a test or a report: whether the project holds a sheet.
    pub fn has_sheet(&self) -> bool {
        self.plan.is_some()
    }

    /// Reads a file into text for the preview, when it is small (an exported SVG).
    pub fn read_text(path: &Path) -> Option<String> {
        fs::read_to_string(path)
            .ok()
            .filter(|t| t.len() < (1 << 24))
    }
}
