// SPDX-License-Identifier: GPL-3.0-or-later
//! The state and the lines of the panel «Planos» of the window, with no window in it: the project
//! file is read, the sheet is evaluated against its history and its sketches, and the panel shows
//! lines of text (in the language of the window) and the buttons that change the sheet. The window
//! draws these lines and calls these methods; everything that decides is here and is tested.

use std::fs;
use std::path::{Path, PathBuf};

use eh3d_container::IdGenerator;

use super::measure::{Issue, Model};
use super::model::{Plan, Source, View, ViewKind};
use super::out;
use super::scene::{evaluate, Evaluation};
use super::settings::SheetSettings;
use crate::config::Settings;
use crate::enclosure::Id;
use crate::export::write_bytes;
use crate::featuredag::{FeatureDag, HistorySettings};
use crate::lang::{fill, Lang, Msg};
use crate::messages;
use crate::project::Document;
use crate::sketch::settings::SketchSettings;
use crate::sketch::Sketch;
use crate::store;
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
    /// The bill.
    pub bom: Vec<Line>,
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
        out.views.push(line(
            fill(
                Msg::PlView.tr(lang),
                &[
                    &v.name,
                    v.kind.name(),
                    &v.visible.to_string(),
                    &v.hidden.to_string(),
                ],
            ),
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
                    extra: Vec::new(),
                },
                &self.sheet,
            )?;
        }
        self.plan = Some(plan);
        self.dirty = true;
        self.evaluate(lang)
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
