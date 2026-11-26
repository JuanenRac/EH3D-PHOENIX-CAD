//! The window itself (built with the `gui` feature): it draws the state of `window::Window` and
//! passes on what the person does. Everything that can be decided without a screen is decided
//! there, in code that is tested.

use crate::assembly::Dim;
use crate::body::StepState;
use crate::browse::{self, Browser};
use crate::catalog;
use crate::config::Settings;
use crate::dockui::{self, Cmd, LayoutWatch, MenuFacts};
use crate::eh3dfile::SaveSettings;
use crate::enclosure::Face;
use crate::export::Format;
use crate::lang::{fill, Lang, Msg};
use crate::panels::{default_drawing_name, Panel, PanelState, SketchFormat};
use crate::plans::panel::{FormAction, PlanForm, PlanState, Tone as PlanTone};
use crate::plans::prim::{Anchor as PlanAnchor, Shape as PlanShape};
use crate::scene::{Kind, Line2};
use crate::shot::{ShotRequest, ShotTheme, StartView, PATIENCE_FRAMES};
use crate::sketch::file as sketch_file;
use crate::sketch::settings::SketchSettings;
use crate::sketch_ui::{
    ConstraintTool, DimTool, Prim, RowMark, SketchEditor, SketchUiSettings, Stroke as SketchStroke,
    Tone, Tool,
};
use crate::sketchcheck;
use crate::suitewin::{self, BoardWork, Branches, Saved};
use crate::uiconfig::UiSettings;
use crate::window::{
    error_text, face_msg, Input, Mark, MeasureTool, NewStep, Selection, Status, ViewKind, Window,
    PROJECT_EXTENSIONS,
};
use crate::CadError;
use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, Pos2, Rect, Stroke, Vec2,
};
use eh3d_attachpanel::{AttachPanel, Program};
use eh3d_dock::about::Phoenix;
use eh3d_dock::core::AboutInfo;
use eh3d_dock::gui::Dock;
use eh3d_dock::menu::{Chosen as MenuChosen, DockAction, MenuBar, StdAction};
use eh3d_dock::palette;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

mod modelview;
mod signing;
mod texts;
mod variants;

/// The extensions of the public key files that check signatures.
const KEY_EXTENSIONS: [&str; 1] = ["pub"];

/// The extension offered for the secret key files; every file can be listed too.
const SECRET_EXTENSIONS: [&str; 1] = ["key"];

// the colours of the theme Forge, the one the other programs of the suite start with: charcoal, with
// titles in the colour of the program and amber for what asks for attention and what is selected
const BACKGROUND: Color32 = Color32::from_rgb(0x0f, 0x0d, 0x0b);
const PANEL: Color32 = Color32::from_rgb(0x17, 0x13, 0x10);
const RAISED: Color32 = Color32::from_rgb(0x22, 0x1c, 0x17);
const TEXT: Color32 = Color32::from_rgb(0xf4, 0xec, 0xe0);
const DIM: Color32 = Color32::from_rgb(0x9c, 0x8d, 0x7a);
const ACCENT: Color32 = Color32::from_rgb(0xff, 0xb0, 0x00);
const OK: Color32 = Color32::from_rgb(0x9b, 0xe3, 0x4a);
const DANGER: Color32 = Color32::from_rgb(0xff, 0x54, 0x70);
const OUTER: Color32 = Color32::from_rgb(0xc9, 0xbf, 0xb0);
const CAVITY: Color32 = Color32::from_rgb(0x6e, 0x64, 0x58);
const HOLE: Color32 = Color32::from_rgb(0x4c, 0xc9, 0xf0);
const SCREW: Color32 = Color32::from_rgb(0xb4, 0x8c, 0xf0);
const POST: Color32 = Color32::from_rgb(0xf0, 0x8a, 0x4c);
const MEASURE: Color32 = Color32::from_rgb(0xff, 0xe0, 0x66);

/// The extensions of the sketch files that the window opens and writes.
const SKETCH_EXTENSIONS: [&str; 1] = [sketch_file::EXTENSION];

/// The extensions of the library files that the window opens.
const LIBRARY_EXTENSIONS: [&str; 1] = [catalog::EXTENSION];

/// The extensions of the drawings that the window imports into a sketch.
const DXF_EXTENSIONS: [&str; 1] = ["dxf"];

enum Outcome {
    Cancel,
    Pick(PathBuf),
}

/// What a dialog of the project file of the suite asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SuiteAsk {
    /// The file to open.
    Open,
    /// The name to save the project under.
    SaveAs,
    /// The file of another program whose original is kept.
    Import,
    /// A public key file that checks the signatures.
    Key,
    /// A 3D file or a flat drawing to look at.
    ViewFile,
    /// The secret key file that signs.
    SignKey,
    /// The name of a new secret key.
    NewKey,
}

/// The window that opens a project or asks for the name of the file to write.
struct Dialog {
    /// What the dialog is for when it is one of the project file of the suite.
    suite: Option<SuiteAsk>,
    browser: Browser,
    /// The name being typed when the window asks for a file to write.
    save: Option<String>,
    /// Whether the files are sketches (otherwise they are projects).
    sketch: bool,
    /// The format of the file being exported, when the window asks for a file to export.
    export: Option<Format>,
    /// Whether the file is written over when it exists (the box of the export window).
    overwrite: bool,
    /// Whether the files are libraries of parts (otherwise they are projects or sketches).
    library: bool,
    /// Whether the files are DXF drawings to import (the choice goes to the import panel).
    dxf: bool,
}

struct App {
    win: Window,
    /// The sketch.
    sketch: SketchEditor,
    /// The part of the window that shows.
    workspace: StartView,
    /// Whether the sketch is to be fitted to the canvas as soon as the canvas has its size.
    fit_next: bool,
    /// The text being typed in the value of each dimension of the sketch, by the number of the
    /// constraint.
    dim_edits: HashMap<u32, String>,
    /// The picture mode, while the window is there only to be photographed.
    shot: Option<ShotState>,
    dialog: Option<Dialog>,
    /// The text being typed in the value of each row of the sheet of variables.
    var_edits: HashMap<String, String>,
    /// The name and the value being typed for a new variable.
    new_name: String,
    new_value: String,
    /// The sketch and the reference dimension a variable is about to follow.
    follow_sketch: String,
    follow_constraint: String,
    /// True from the moment a drag of a hole or a post is cancelled with Esc until the button
    /// is let go, so that the rest of that drag does not turn or move the view.
    ignore_drag: bool,
    /// Whether the window with the text of the fit report is open.
    report_open: bool,
    /// The panels of the Reports menu: which one is open and what is chosen in it.
    panels: PanelState,
    /// The panel of the settings of the server, made the first time the server panel opens.
    link: Option<crate::linkpane::LinkPane>,
    /// What the frame has to do with the panels, asked by the commands of the menus.
    pending_dock: Vec<DockAction>,
    /// The panels that are to be the tab on show of their group.
    pending_active: Vec<&'static str>,
    /// The files opened or written since the frame last looked, for the list of recent files.
    recent_added: Vec<String>,
    /// The files of the suite among the recent ones, as the frame keeps them.
    recent_suite: Vec<String>,
    /// The panel of the attachments of the project of the suite.
    attachments: AttachPanel,
    /// The window of the branches of the suite file, while it is open: what was read of it.
    branches: Option<Result<Branches, String>>,
    /// The window that tells what an import or the board did.
    suite_note: Option<(Msg, Vec<String>)>,
    /// The window of the variants, while it is open.
    variants: Option<variants::VariantsForm>,
    /// The window of the title and the description of the suite project, while it is open.
    texts: Option<texts::TextsForm>,
    /// The public key files chosen while the program runs, besides those of the settings.
    chosen_keys: Vec<String>,
    /// The window of a file of another program, while it is open.
    viewer: Option<modelview::ViewerState>,
    /// The window that signs the project of the suite, while it is open.
    signing: Option<signing::SignForm>,
    /// The settings file given with `--config`, where the arrangement of the panels is kept.
    config_path: Option<PathBuf>,
    /// Whether the 3D view and the sketch were drawn in this frame.
    drew_view3d: bool,
    drew_sketch: bool,
    /// The panel of the drawing sheets, and the project it was last read from.
    plans: PlanState,
    plans_form: PlanForm,
    plans_seen: Option<PathBuf>,
}

impl App {
    fn new(win: Window, sketch: SketchEditor, workspace: StartView) -> App {
        let mut panels = PanelState::new(&win.ui.panels, win.ui.export.overwrite);
        panels.mate = win.mate_draft_new();
        panels.title_enabled = win.settings.title_block.enabled;
        panels.title_fields = win.title_block_fields();
        panels.part_pattern = win.part_pattern_draft_new();
        App {
            win,
            panels,
            fit_next: !sketch.sketch.elements().is_empty(),
            sketch,
            workspace,
            dim_edits: HashMap::new(),
            shot: None,
            dialog: None,
            var_edits: HashMap::new(),
            new_name: String::new(),
            new_value: String::new(),
            follow_sketch: String::new(),
            follow_constraint: String::new(),
            ignore_drag: false,
            report_open: false,
            link: None,
            pending_dock: Vec::new(),
            pending_active: Vec::new(),
            recent_added: Vec::new(),
            recent_suite: Vec::new(),
            attachments: AttachPanel::new(Program::new(
                crate::eh3dfile::BRANCH,
                env!("CARGO_PKG_VERSION"),
            )),
            branches: None,
            suite_note: None,
            variants: None,
            texts: None,
            chosen_keys: Vec::new(),
            viewer: None,
            signing: None,
            config_path: None,
            drew_view3d: false,
            drew_sketch: false,
            plans: PlanState::with_defaults(),
            plans_form: PlanForm::default(),
            plans_seen: None,
        }
    }

    fn folder(&self) -> PathBuf {
        self.win
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(browse::start_dir)
    }

    fn open_dialog(&mut self) {
        self.dialog = Some(Dialog {
            suite: None,
            browser: Browser::new(self.folder(), &PROJECT_EXTENSIONS),
            save: None,
            sketch: false,
            export: None,
            overwrite: false,
            library: false,
            dxf: false,
        });
    }

    /// Asks for a file of the suite, a name to save one or a file to import into one.
    fn suite_dialog(&mut self, ask: SuiteAsk) {
        let extensions: &[&'static str] = match ask {
            SuiteAsk::Import | SuiteAsk::ViewFile => &suitewin::IMPORT_EXTENSIONS,
            SuiteAsk::SignKey | SuiteAsk::NewKey => &SECRET_EXTENSIONS,
            SuiteAsk::Open | SuiteAsk::SaveAs => &suitewin::SUITE_EXTENSIONS,
            SuiteAsk::Key => &KEY_EXTENSIONS,
        };
        let save = match ask {
            SuiteAsk::SaveAs => Some(suitewin::suggested_name(&self.win)),
            SuiteAsk::NewKey => Some(signing::NEW_KEY_NAME.to_owned()),
            _ => None,
        };
        self.dialog = Some(Dialog {
            suite: Some(ask),
            browser: Browser::new(self.folder(), extensions),
            save,
            sketch: false,
            export: None,
            overwrite: false,
            library: false,
            dxf: false,
        });
    }

    /// Does what was chosen in a dialog of the suite file; false leaves the dialog open.
    fn suite_pick(&mut self, ask: SuiteAsk, path: &Path, overwrite: bool) -> bool {
        let done = match ask {
            SuiteAsk::Open => suitewin::open(&mut self.win, path) != suitewin::Opened::Refused,
            SuiteAsk::SaveAs => suitewin::save_as(&mut self.win, path, overwrite),
            SuiteAsk::Import => self.suite_import(path),
            SuiteAsk::Key => {
                let key = path.display().to_string();
                if !self.chosen_keys.contains(&key) {
                    self.chosen_keys.push(key);
                }
                true
            }
            SuiteAsk::ViewFile => {
                self.open_viewer(crate::modelview::Origin::File(path.to_path_buf()));
                true
            }
            SuiteAsk::SignKey => self.take_sign_key(path),
            SuiteAsk::NewKey => self.make_sign_key(path),
        };
        self.refresh_branches();
        done
    }

    /// Keeps the original of a file in the suite file and tells what was read of it.
    fn suite_import(&mut self, foreign: &Path) -> bool {
        let lang = self.win.lang;
        let Some(target) = suitewin::current_file(&self.win) else {
            self.suite_note = Some((
                Msg::SuiteImportTitle,
                vec![Msg::SuiteNeedFile.tr(lang).into()],
            ));
            return false;
        };
        let how = SaveSettings {
            keep_backup: self.win.settings.backups > 0,
            ..SaveSettings::default()
        };
        match suitewin::import_original(&target, foreign, &self.win.settings.mesh, &how) {
            Ok(done) => {
                let mut lines = vec![fill(
                    Msg::SuiteImported.tr(lang),
                    &[
                        &done.name,
                        &done.solids.to_string(),
                        &done.triangles.to_string(),
                    ],
                )];
                lines.extend(suitewin::report_lines(&done.report, lang));
                self.suite_note = Some((Msg::SuiteImportTitle, lines));
                true
            }
            Err(e) => {
                self.suite_note = Some((Msg::SuiteImportTitle, vec![e.text(lang)]));
                false
            }
        }
    }

    /// Takes the board of the branch `pcb` into the box and tells what changed.
    fn suite_board(&mut self) {
        let lines = match suitewin::take_board(&mut self.win) {
            Ok(taken) => taken.text.lines().map(str::to_owned).collect(),
            Err(why) => vec![why],
        };
        self.suite_note = Some((Msg::SuiteMenuBoard, lines));
        self.refresh_branches();
    }

    /// Checks the fit, cuts the openings or hands the box back, and shows the answer.
    fn suite_work(&mut self, work: BoardWork, title: Msg) {
        let lines = match suitewin::board_work(&mut self.win, work) {
            Ok(done) => done.text.lines().map(str::to_owned).collect(),
            Err(why) => vec![why],
        };
        self.suite_note = Some((title, lines));
        self.refresh_branches();
    }

    /// Checks the sketch that is being drawn and shows what it found.
    fn check_sketch(&mut self) {
        let lang = self.win.lang;
        let lines = match sketchcheck::check(&self.sketch.sketch, &self.win.sketch_settings) {
            Ok(report) => report.lines(lang),
            Err(e) => vec![error_text(&e, lang)],
        };
        self.suite_note = Some((Msg::CheckMenu, lines));
    }

    /// Reads the branches of the suite file again.
    fn read_branches(&self) -> Result<Branches, String> {
        let lang = self.win.lang;
        let file = suitewin::current_file(&self.win)
            .ok_or_else(|| Msg::SuiteNeedFile.tr(lang).to_owned())?;
        let mut keys = self.win.ui.suite.trusted_keys.clone();
        keys.extend(self.chosen_keys.iter().cloned());
        suitewin::branches_with(&file, &keys).map_err(|e| error_text(&e, lang))
    }

    /// Reads the branches again when their window is open.
    fn refresh_branches(&mut self) {
        if self.branches.is_some() {
            self.branches = Some(self.read_branches());
        }
    }

    /// The window with the branches of the suite file, their state and the recent suite files.
    fn branches_window(&mut self, ctx: &egui::Context) {
        let Some(read) = &self.branches else {
            return;
        };
        let lang = self.win.lang;
        let editable = !self.win.read_only;
        let mut open = true;
        let mut again = false;
        let mut board = false;
        let mut trust = false;
        let mut pick: Option<PathBuf> = None;
        let mut original: Option<PathBuf> = None;
        egui::Window::new(Msg::SuiteBranchesTitle.tr(lang))
            .open(&mut open)
            .default_size([640.0, 360.0])
            .show(ctx, |ui| {
                match read {
                    Ok(found) => {
                        ui.label(found.file.display().to_string());
                        egui::Grid::new("suite-branches")
                            .striped(true)
                            .show(ui, |ui| {
                                for head in [
                                    Msg::SuiteColBranch,
                                    Msg::SuiteColApp,
                                    Msg::SuiteColFormat,
                                    Msg::SuiteColFiles,
                                    Msg::SuiteColState,
                                ] {
                                    ui.label(egui::RichText::new(head.tr(lang)).strong());
                                }
                                ui.end_row();
                                for line in &found.summary.branches {
                                    ui.label(line.id.as_str());
                                    ui.label(line.app.as_str());
                                    ui.label(line.format.as_str());
                                    ui.label(line.files.to_string());
                                    let colour = if line.stale.is_empty() { TEXT } else { ACCENT };
                                    let state = suitewin::state_text(line, lang);
                                    ui.colored_label(colour, state);
                                    ui.end_row();
                                }
                            });
                        let signature =
                            suitewin::signature_text(&found.summary.project_signature, lang);
                        if let Some(words) = signature {
                            ui.label(fill(Msg::SuiteProjectSignature.tr(lang), &[&words]));
                        }
                        if found.summary.any_wrong_signature() {
                            ui.colored_label(DANGER, Msg::SuiteSigInvalid.tr(lang));
                        }
                        for problem in &found.key_problems {
                            ui.colored_label(DANGER, problem.as_str());
                        }
                        if !found.summary.findings.is_empty() {
                            let count = found.summary.findings.len().to_string();
                            ui.colored_label(DANGER, fill(Msg::SuiteFindings.tr(lang), &[&count]));
                            for f in &found.summary.findings {
                                ui.label(format!("{} {}", f.code.code(), f.detail));
                            }
                        }
                        if let Some(report) = &found.original {
                            for text in suitewin::report_lines(report, lang) {
                                ui.label(text);
                            }
                            let shown = report
                                .original
                                .as_deref()
                                .is_some_and(crate::modelview::can_show);
                            if shown && ui.button(Msg::MvOriginal.tr(lang)).clicked() {
                                original = Some(found.file.clone());
                            }
                        }
                    }
                    Err(why) => {
                        ui.colored_label(DANGER, why.as_str());
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button(Msg::SuiteRefresh.tr(lang)).clicked() {
                        again = true;
                    }
                    if ui.button(Msg::SuiteTrustKey.tr(lang)).clicked() {
                        trust = true;
                    }
                    let take = egui::Button::new(Msg::SuiteMenuBoard.tr(lang));
                    if ui.add_enabled(editable && read.is_ok(), take).clicked() {
                        board = true;
                    }
                });
                if !self.recent_suite.is_empty() {
                    ui.separator();
                    ui.label(Msg::SuiteRecent.tr(lang));
                    for p in &self.recent_suite {
                        if ui.selectable_label(false, p.as_str()).clicked() {
                            pick = Some(PathBuf::from(p));
                        }
                    }
                }
            });
        if !open {
            self.branches = None;
            return;
        }
        if let Some(file) = original {
            self.view_original(&file);
        }
        if let Some(path) = pick {
            if suitewin::open_any(&mut self.win, &path) {
                self.recent_added.push(path.display().to_string());
            }
            again = true;
        }
        if trust {
            self.suite_dialog(SuiteAsk::Key);
        }
        if board {
            self.suite_board();
        } else if again {
            self.refresh_branches();
        }
    }

    /// The window that tells what an import or the board did.
    fn suite_note_window(&mut self, ctx: &egui::Context) {
        let Some((title, lines)) = &self.suite_note else {
            return;
        };
        let lang = self.win.lang;
        let mut open = true;
        egui::Window::new(title.tr(lang))
            .id(egui::Id::unique("suite-note"))
            .open(&mut open)
            .default_size([520.0, 240.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for line in lines {
                        ui.label(line.as_str());
                    }
                });
            });
        if !open {
            self.suite_note = None;
        }
    }

    /// Asks which library of parts to open.
    fn open_library_dialog(&mut self) {
        self.dialog = Some(Dialog {
            suite: None,
            browser: Browser::new(self.folder(), &LIBRARY_EXTENSIONS),
            save: None,
            sketch: false,
            export: None,
            overwrite: false,
            library: true,
            dxf: false,
        });
    }

    /// Asks which DXF drawing to import; the file chosen goes to the import panel.
    fn open_dxf_dialog(&mut self) {
        self.dialog = Some(Dialog {
            suite: None,
            browser: Browser::new(self.folder(), &DXF_EXTENSIONS),
            save: None,
            sketch: false,
            export: None,
            overwrite: false,
            library: false,
            dxf: true,
        });
    }

    /// Asks where to write the model in a format. The box to write over a file that exists
    /// starts as the settings say, and the name suggested follows the project.
    fn export_dialog(&mut self, format: Format) {
        self.win.export_format = format;
        self.dialog = Some(Dialog {
            suite: None,
            browser: Browser::new(self.folder(), &[format.extension()]),
            save: Some(self.win.export_name(format)),
            sketch: false,
            export: Some(format),
            overwrite: self.win.export_overwrite,
            library: false,
            dxf: false,
        });
    }

    fn save_dialog(&mut self) {
        let name = self
            .win
            .path
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| format!("box.{}", PROJECT_EXTENSIONS[0]));
        self.dialog = Some(Dialog {
            suite: None,
            browser: Browser::new(self.folder(), &PROJECT_EXTENSIONS),
            save: Some(name),
            sketch: false,
            export: None,
            overwrite: false,
            library: false,
            dxf: false,
        });
    }

    /// Saves to the file the project has, or asks for a name when it has none.
    fn save_or_ask(&mut self) {
        if self.win.path.is_none() {
            self.save_dialog();
        } else {
            self.win.save();
        }
    }

    fn panel_model_tree(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(Msg::ModelTitle.tr(lang))
                .strong()
                .color(title_colour(ui.visuals())),
        );
        ui.separator();
        let editable = !self.win.read_only;
        let has_step = self.win.selection.step().is_some();
        let mut add = None;
        let mut remove = false;
        let mut break_now = false;
        ui.horizontal_wrapped(|ui| {
            for (kind, msg) in [
                (NewStep::Hole, Msg::AddHole),
                (NewStep::Post, Msg::AddPost),
                (NewStep::Edging, Msg::AddEdging),
                (NewStep::Extrude, Msg::AddExtrude),
                (NewStep::Pocket, Msg::AddPocket),
                (NewStep::HolePattern, Msg::AddHolePattern),
                (NewStep::PostPattern, Msg::AddPostPattern),
                (NewStep::Fins, Msg::AddFins),
                (NewStep::Part, Msg::AddPart),
                (NewStep::Mate, Msg::AddMate),
                (NewStep::Turned, Msg::AddTurned),
                (NewStep::Swept, Msg::AddSwept),
            ] {
                if ui
                    .add_enabled(editable, egui::Button::new(msg.tr(lang)))
                    .clicked()
                {
                    add = Some(kind);
                }
            }
            if ui
                .add_enabled(
                    editable && has_step,
                    egui::Button::new(Msg::RemoveStep.tr(lang)),
                )
                .clicked()
            {
                remove = true;
            }
            if matches!(self.win.selection, Selection::Pattern(_))
                && ui
                    .add_enabled(editable, egui::Button::new(Msg::PatternBreak.tr(lang)))
                    .clicked()
            {
                break_now = true;
            }
        });
        if let Some(kind) = add {
            self.win.add_step(kind);
        }
        if break_now {
            self.win.break_pattern();
        }
        if remove {
            self.win.remove_step();
        }
        ui.separator();
        let mut picked = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for group in self.win.tree() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(group.title.tr(lang)).color(DIM));
                for item in group.items {
                    let (suffix, colour) = match item.mark {
                        Mark::Normal => ("", TEXT),
                        Mark::Suppressed => ("  [off]", DIM),
                        Mark::Failed => ("  [!]", DANGER),
                        Mark::AfterTip => ("  [-]", DIM),
                    };
                    let text = egui::RichText::new(format!("{}{suffix}", item.label)).color(colour);
                    let selected = self.win.selection == item.selection;
                    if ui.selectable_label(selected, text).clicked() {
                        picked = Some(item.selection);
                    }
                }
            }
        });
        if let Some(selection) = picked {
            self.win.select(selection);
        }
    }

    fn panel_properties(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(Msg::PropertiesTitle.tr(lang))
                .strong()
                .color(title_colour(ui.visuals())),
        );
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.properties_body(ui);
        });
    }

    /// The sheet of variables: one row for each, with the value or formula to type, what it
    /// calculates to and the parameters that use it, and a line to add another.
    fn sheet_body(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        let editable = !self.win.read_only;
        ui.label(egui::RichText::new(Msg::SheetHint.tr(lang)).color(DIM));
        ui.add_space(6.0);
        let mut edited: Option<(String, String)> = None;
        let mut removed: Option<String> = None;
        let mut unfollowed: Option<String> = None;
        let links = self.win.link_rows();
        for row in self.win.sheet() {
            let used = self.win.used_by_text(&row);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(row.name.as_str())
                        .strong()
                        .color(title_colour(ui.visuals())),
                );
                ui.label(egui::RichText::new(format!("= {}", row.text)).color(TEXT));
            });
            ui.label(
                egui::RichText::new(format!("{}: {used}", Msg::VariableUsedBy.tr(lang))).color(DIM),
            );
            let link = links.iter().find(|l| l.variable == row.name);
            if let Some(link) = link {
                let words = fill(Msg::FollowRow.tr(lang), &[&link.constraint, &link.sketch]);
                ui.label(egui::RichText::new(words).color(DIM));
                if let Some(why) = &link.broken {
                    ui.colored_label(DANGER, fill(Msg::FollowBroken.tr(lang), &[why]));
                }
                let stop = egui::Button::new(Msg::FollowStopButton.tr(lang));
                if ui.add_enabled(editable, stop).clicked() {
                    unfollowed = Some(row.name.clone());
                }
            }
            let free = editable && link.is_none();
            let mut text = self
                .var_edits
                .remove(&row.name)
                .unwrap_or_else(|| row.text.clone());
            ui.horizontal(|ui| {
                let edit = egui::TextEdit::singleline(&mut text).desired_width(150.0);
                let response = ui.add_enabled(free, edit);
                if response.lost_focus() && text != row.text {
                    edited = Some((row.name.clone(), text.clone()));
                }
                if !response.has_focus() {
                    text.clone_from(&row.text);
                }
                if ui
                    .add_enabled(editable, egui::Button::new(Msg::RemoveVariable.tr(lang)))
                    .clicked()
                {
                    removed = Some(row.name.clone());
                }
            });
            self.var_edits.insert(row.name.clone(), text);
        }
        if let Some((name, text)) = edited {
            self.win.set_variable(&name, &text);
        }
        if let Some(name) = removed {
            self.win.remove_variable(&name);
            self.var_edits.remove(&name);
        }
        if let Some(name) = unfollowed {
            self.win.stop_following(&name);
            self.var_edits.remove(&name);
        }
        ui.separator();
        ui.label(egui::RichText::new(Msg::AddVariable.tr(lang)).color(DIM));
        let mut add = false;
        ui.horizontal(|ui| {
            let name = egui::TextEdit::singleline(&mut self.new_name)
                .hint_text(Msg::VariableName.tr(lang))
                .desired_width(90.0);
            ui.add_enabled(editable, name);
            let value = egui::TextEdit::singleline(&mut self.new_value)
                .hint_text(Msg::VariableValue.tr(lang))
                .desired_width(120.0);
            ui.add_enabled(editable, value);
            let ready = editable && !self.new_name.trim().is_empty();
            if ui
                .add_enabled(ready, egui::Button::new(Msg::AddVariable.tr(lang)))
                .clicked()
            {
                add = true;
            }
        });
        if add {
            self.win.add_variable(&self.new_name, &self.new_value);
            if !self.win.status.is_problem() {
                self.new_name.clear();
                self.new_value.clear();
            }
        }
        // the variable named above can follow a reference dimension of a sketch
        let sketches = self.win.sketch_names();
        if sketches.is_empty() {
            return;
        }
        ui.separator();
        ui.label(egui::RichText::new(Msg::FollowHeading.tr(lang)).color(DIM));
        let mut follow = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("follow-sketch")
                .selected_text(if self.follow_sketch.is_empty() {
                    Msg::FollowSketchField.tr(lang)
                } else {
                    self.follow_sketch.as_str()
                })
                .show_ui(ui, |ui| {
                    for name in &sketches {
                        ui.selectable_value(&mut self.follow_sketch, name.clone(), name);
                    }
                });
            let constraint = egui::TextEdit::singleline(&mut self.follow_constraint)
                .hint_text(Msg::FollowConstraintField.tr(lang))
                .desired_width(110.0);
            ui.add_enabled(editable, constraint);
            let ready = editable
                && !self.new_name.trim().is_empty()
                && !self.follow_sketch.is_empty()
                && !self.follow_constraint.trim().is_empty();
            let button = egui::Button::new(Msg::FollowButton.tr(lang));
            follow = ui.add_enabled(ready, button).clicked();
        });
        if follow
            && self.win.follow_dimension(
                &self.new_name,
                &self.follow_sketch,
                &self.follow_constraint,
            )
        {
            self.var_edits.remove(self.new_name.trim());
            self.new_name.clear();
            self.follow_constraint.clear();
        }
    }

    fn properties_body(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        if self.win.selection == Selection::Sheet {
            self.sheet_body(ui);
            return;
        }
        if self.win.fields.is_empty() && self.win.selection.step().is_none() {
            ui.label(egui::RichText::new(Msg::NothingSelected.tr(lang)).color(DIM));
            return;
        }
        let hint = match self.win.selection {
            Selection::Body => Some(Msg::EditHint),
            Selection::Screw(_) => Some(Msg::DerivedNote),
            Selection::Fit if self.win.info.fit_from_board => Some(Msg::FromBoard),
            _ => None,
        };
        if let Some(hint) = hint {
            ui.label(egui::RichText::new(hint.tr(lang)).color(DIM));
            ui.add_space(6.0);
        }
        for i in 0..self.win.fields.len() {
            let field = self.win.fields[i].clone();
            ui.label(egui::RichText::new(field.label.tr(lang)).color(DIM));
            match &field.input {
                Input::Text => {
                    let mut text = field.text.clone();
                    let edit = egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY);
                    let response = ui.add_enabled(field.editable, edit);
                    if response.changed() {
                        self.win.type_in(i, text);
                    }
                    if response.lost_focus() {
                        self.win.commit(i);
                    }
                    if !field.note.is_empty() {
                        ui.label(egui::RichText::new(format!("= {}", field.note)).color(DIM));
                    }
                }
                Input::Choice(choices) => {
                    let current = choices
                        .iter()
                        .find(|(value, _)| *value == field.text)
                        .map_or("", |(_, msg)| msg.tr(lang));
                    let mut picked: Option<&'static str> = None;
                    ui.add_enabled_ui(field.editable, |ui| {
                        egui::ComboBox::from_id_salt(("choice", i))
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for (value, msg) in choices {
                                    let chosen = *value == field.text;
                                    if ui.selectable_label(chosen, msg.tr(lang)).clicked() {
                                        picked = Some(*value);
                                    }
                                }
                            });
                    });
                    if let Some(value) = picked {
                        self.win.choose(i, value);
                    }
                }
            }
            ui.add_space(4.0);
        }
        self.step_state(ui);
    }

    /// The state of the selected step and the switch that suppresses it.
    fn step_state(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        let Some(state) = self.win.selected_state().cloned() else {
            return;
        };
        ui.separator();
        let (word, colour) = match &state {
            StepState::Valid => (Msg::StateValid, OK),
            StepState::Suppressed => (Msg::StateSuppressed, DIM),
            StepState::Failed(_) => (Msg::StateFailed, DANGER),
            StepState::AfterTip => (Msg::StateAfterTip, DIM),
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(Msg::StateLabel.tr(lang)).color(DIM));
            ui.label(egui::RichText::new(word.tr(lang)).color(colour));
        });
        if let StepState::Failed(error) = &state {
            ui.colored_label(DANGER, error_text(error, lang));
        }
        let mut suppressed = self.win.selected_suppressed();
        let editable = !self.win.read_only;
        let changed = ui
            .add_enabled_ui(editable, |ui| {
                ui.checkbox(&mut suppressed, Msg::SuppressStep.tr(lang))
                    .changed()
            })
            .inner;
        if changed {
            self.win.toggle_suppressed();
        }
    }

    fn panel_console_model(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.horizontal(|ui| {
            let colour = if self.win.status.is_problem() {
                DANGER
            } else {
                OK
            };
            ui.label(egui::RichText::new(self.win.status.text(lang)).color(colour));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(self.win.measures_text()).color(TEXT));
            ui.separator();
            let fit = &self.win.info.fit;
            let colour = if fit.fits {
                OK
            } else if fit.fits_turned {
                ACCENT
            } else {
                DANGER
            };
            ui.label(egui::RichText::new(self.win.fit_text()).color(colour));
            if let Some(failed) = self.win.failed_text() {
                ui.separator();
                ui.label(egui::RichText::new(failed).color(DANGER));
            }
        });
    }

    /// The tools of the views: the two measurements, the grid a dragged hole or post snaps to,
    /// and the readings kept.
    fn panel_tasks(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(Msg::MeasureTitle.tr(lang))
                    .strong()
                    .color(title_colour(ui.visuals())),
            );
            for (tool, msg) in [
                (MeasureTool::Distance, Msg::MeasureDistanceTool),
                (MeasureTool::Angle, Msg::MeasureAngleTool),
            ] {
                let on = self.win.measure_tool == tool;
                if ui.selectable_label(on, msg.tr(lang)).clicked() {
                    self.win
                        .set_measure_tool(if on { MeasureTool::Off } else { tool });
                }
            }
            ui.checkbox(&mut self.win.snap_to_grid, Msg::GridSnap.tr(lang));
            if ui.button(Msg::MeasureClear.tr(lang)).clicked() {
                self.win.clear_measurements();
            }
            let hint = if self.win.measure_tool == MeasureTool::Off {
                Msg::DragHint
            } else {
                Msg::MeasureHint
            };
            ui.label(egui::RichText::new(hint.tr(lang)).color(DIM));
        });
        let rows: Vec<String> = self
            .win
            .measurements()
            .iter()
            .map(|m| self.win.reading_text(m))
            .collect();
        let mut removed = None;
        egui::ScrollArea::vertical()
            .max_height(84.0)
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(egui::RichText::new(Msg::NoMeasurements.tr(lang)).color(DIM));
                }
                for (i, row) in rows.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button(Msg::DeleteSelection.tr(lang)).clicked() {
                            removed = Some(i);
                        }
                        ui.label(egui::RichText::new(row.as_str()).color(MEASURE));
                    });
                }
            });
        if let Some(i) = removed {
            self.win.remove_measurement(i);
        }
    }

    fn panel_view3d(&mut self, ui: &mut egui::Ui) {
        self.drew_view3d = true;
        let total = ui.available_size();
        let gap = 6.0;
        let first = ((total.x - gap) * 0.62).max(120.0);
        let second = (total.x - gap - first).max(120.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            self.pane(ui, ViewKind::Iso, Vec2::new(first, total.y));
            self.pane(ui, ViewKind::Plan, Vec2::new(second, total.y));
        });
    }

    fn pane(&mut self, ui: &mut egui::Ui, view: ViewKind, size: Vec2) {
        let lang = self.win.lang;
        let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, BACKGROUND);
        let pane = (f64::from(rect.width()), f64::from(rect.height()));
        let local = |p: Pos2| (f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
        // a press on a hole or a post drags it; anywhere else the drag turns or moves the view
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = response.interact_pointer_pos() {
                self.win.drag_begin(view, pane, local(p));
            }
        }
        if self.win.dragging() {
            if response.dragged_by(egui::PointerButton::Primary) {
                if let Some(p) = response.interact_pointer_pos() {
                    self.win.drag_move(pane, local(p));
                }
            }
            if response.drag_stopped() {
                self.win.drag_end();
            }
        } else if response.drag_stopped() {
            self.ignore_drag = false;
        } else if response.dragged() && !self.ignore_drag {
            let d = response.drag_delta();
            match view {
                ViewKind::Iso => self.win.rotate_3d(f64::from(d.x), f64::from(d.y)),
                ViewKind::Plan => self
                    .win
                    .control_mut(ViewKind::Plan)
                    .pan_by(f64::from(d.x), f64::from(d.y)),
            }
        }
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                self.win.measure_click(view, pane, local(p));
            }
        }
        if response.double_clicked() {
            self.win.control_mut(view).reset();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.win.wheel(view, f64::from(scroll));
            }
        }
        let painter = painter.with_clip_rect(rect);
        let mut lines = self
            .win
            .lines(view, (f64::from(rect.width()), f64::from(rect.height())));
        lines.sort_by_key(|l| rank(l.kind));
        let board = if self.win.info.fit.fits {
            OK
        } else if self.win.info.fit.fits_turned {
            ACCENT
        } else {
            DANGER
        };
        for line in &lines {
            let (colour, width) = style_of(line, board);
            let at = |p: (f64, f64)| Pos2::new(rect.min.x + p.0 as f32, rect.min.y + p.1 as f32);
            painter.line_segment([at(line.a), at(line.b)], Stroke::new(width, colour));
        }
        let (title, hint) = match view {
            ViewKind::Iso => (Msg::View3d, Msg::ViewHint3d),
            ViewKind::Plan => (Msg::ViewPlan, Msg::ViewHintPlan),
        };
        painter.text(
            Pos2::new(rect.min.x + 12.0, rect.min.y + 10.0),
            egui::Align2::LEFT_TOP,
            title.tr(lang),
            egui::FontId::proportional(14.0),
            canvas_title_colour(),
        );
        painter.text(
            Pos2::new(rect.min.x + 12.0, rect.max.y - 10.0),
            egui::Align2::LEFT_BOTTOM,
            hint.tr(lang),
            egui::FontId::proportional(12.0),
            DIM,
        );
        if view == ViewKind::Iso {
            let mut y = rect.min.y + 34.0;
            for (colour, msg) in [
                (OUTER, Msg::GroupBox),
                (HOLE, Msg::GroupHoles),
                (POST, Msg::GroupPosts),
                (SCREW, Msg::GroupScrews),
                (board, Msg::LegendBoard),
            ] {
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(rect.min.x + 12.0, y), Vec2::new(12.0, 12.0)),
                    2.0,
                    colour,
                );
                painter.text(
                    Pos2::new(rect.min.x + 30.0, y + 6.0),
                    egui::Align2::LEFT_CENTER,
                    msg.tr(lang),
                    egui::FontId::proportional(12.0),
                    TEXT,
                );
                y += 18.0;
            }
        }
    }

    /// The window with the text of the fit report, which can be selected and copied, or put in a
    /// file from the same window.
    fn report_window(&mut self, ctx: &egui::Context) {
        if !self.report_open {
            return;
        }
        let lang = self.win.lang;
        let text = self.win.report_text();
        let mut open = true;
        let mut copy = false;
        let mut write = false;
        egui::Window::new(Msg::ReportTitle.tr(lang))
            .open(&mut open)
            .default_size([620.0, 320.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(Msg::ReportCopy.tr(lang)).clicked() {
                        copy = true;
                    }
                    if ui.button(Msg::ExportReport.tr(lang)).clicked() {
                        write = true;
                    }
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // a text that is shown and can be selected but not changed
                    let mut shown: &str = text.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
            });
        if copy {
            ctx.copy_text(text);
            self.win.report_copied();
        } else if write {
            self.export_dialog(Format::Report);
        }
        self.report_open = open;
    }

    /// The window of the open panel of the Reports menu: its choices and its report as text that
    /// can be selected and copied, in the language of the window. The panel of the drawing of a
    /// sketch writes a file next to the project instead (never over one that exists unless its box
    /// is ticked).
    fn panel_window(&mut self, ctx: &egui::Context) {
        let Some(panel) = self.panels.open else {
            return;
        };
        let lang = self.win.lang;
        let names = self.win.sketch_names();
        let materials: Vec<String> = self.win.settings.materials.keys().cloned().collect();
        if self.panels.sketch >= names.len() {
            self.panels.sketch = 0;
        }
        let mut open = true;
        let mut copy = false;
        let mut write = false;
        let mut choose = false;
        let mut import = false;
        let mut look = false;
        let mut add_mate = false;
        let mut add_pattern = false;
        let mut write_all = false;
        let mut undo_now = false;
        let mut redo_now = false;
        let mut goto_now = false;
        let mut units_apply = false;
        let mut units_follow = false;
        let mut server_save = false;
        let mut server_open = false;
        let mut server_list = false;
        let mut server_check = false;
        let mut server_report = false;
        let mut title_apply = false;
        let mut compare_file = false;
        let mut compare_server = false;
        let choices: Vec<String> = self
            .win
            .mate_choices()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        let part_choices: Vec<String> = self
            .win
            .part_pattern_choices()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        let mut text = String::new();
        let title = panel.title().tr(lang).trim_end_matches("...");
        egui::Window::new(title)
            .open(&mut open)
            .default_size([620.0, 320.0])
            .show(ctx, |ui| {
                match panel {
                    Panel::Holes => {
                        ui.checkbox(&mut self.panels.holes_tools, Msg::PanelHolesTools.tr(lang));
                    }
                    Panel::Placements => {
                        ui.checkbox(&mut self.panels.explode, Msg::PanelExplode.tr(lang));
                        if self.panels.explode {
                            ui.horizontal(|ui| {
                                ui.label(Msg::PanelExplodePercent.tr(lang));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.panels.explode_text)
                                        .desired_width(60.0),
                                );
                            });
                        }
                    }
                    Panel::Bom => {
                        ui.checkbox(
                            &mut self.panels.bom_by_assembly,
                            Msg::PanelBomByAssembly.tr(lang),
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(
                                &mut self.panels.bom_group,
                                None,
                                Msg::BomGroupNone.tr(lang),
                            );
                            ui.selectable_value(
                                &mut self.panels.bom_group,
                                Some(crate::bom::GroupBy::Material),
                                Msg::BomByMaterial.tr(lang),
                            );
                            ui.selectable_value(
                                &mut self.panels.bom_group,
                                Some(crate::bom::GroupBy::Kind),
                                Msg::BomByKind.tr(lang),
                            );
                            ui.selectable_value(
                                &mut self.panels.bom_group,
                                Some(crate::bom::GroupBy::Name),
                                Msg::BomByName.tr(lang),
                            );
                        });
                    }
                    Panel::History => {
                        ui.horizontal(|ui| {
                            let undo = egui::Button::new(Msg::Undo.tr(lang));
                            if ui.add_enabled(self.win.can_undo(), undo).clicked() {
                                undo_now = true;
                            }
                            let redo = egui::Button::new(Msg::Redo.tr(lang));
                            if ui.add_enabled(self.win.can_redo(), redo).clicked() {
                                redo_now = true;
                            }
                            ui.label(Msg::HistoryGoto.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.history_goto)
                                    .desired_width(50.0),
                            );
                            if ui.button(Msg::HistoryGo.tr(lang)).clicked() {
                                goto_now = true;
                            }
                        });
                    }
                    Panel::Mates if choices.len() >= 2 => {
                        let draft = &mut self.panels.mate;
                        if draft.fixed >= choices.len() {
                            draft.fixed = 0;
                        }
                        if draft.placed >= choices.len() {
                            draft.placed = 1;
                        }
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldMateA.tr(lang));
                            egui::ComboBox::from_id_salt("panel-mate-fixed")
                                .selected_text(choices[draft.fixed].clone())
                                .show_ui(ui, |ui| {
                                    for (i, n) in choices.iter().enumerate() {
                                        ui.selectable_value(&mut draft.fixed, i, n.as_str());
                                    }
                                });
                            ui.label(Msg::FieldMateB.tr(lang));
                            egui::ComboBox::from_id_salt("panel-mate-placed")
                                .selected_text(choices[draft.placed].clone())
                                .show_ui(ui, |ui| {
                                    for (i, n) in choices.iter().enumerate() {
                                        ui.selectable_value(&mut draft.placed, i, n.as_str());
                                    }
                                });
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label(Msg::FieldMateKind.tr(lang));
                            for (word, msg) in [
                                ("gap", Msg::MateGap),
                                ("align", Msg::MateAlign),
                                ("stack", Msg::MateStack),
                                ("offset", Msg::MateOffset),
                                ("symmetric", Msg::MateSymmetric),
                                ("turn", Msg::MateTurn),
                                ("inset", Msg::MateInset),
                                ("proportion", Msg::MateProportion),
                                ("overlap", Msg::MateOverlap),
                                ("tangent", Msg::MateTangent),
                                ("angle", Msg::MateAngle),
                            ] {
                                ui.selectable_value(
                                    &mut draft.kind,
                                    word.to_string(),
                                    msg.tr(lang),
                                );
                            }
                        });
                        let kind = draft.kind.clone();
                        if kind == "gap" || kind == "overlap" || kind == "tangent" {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(Msg::FieldMateDirection.tr(lang));
                                for a in catalog::Axis::ALL {
                                    ui.selectable_value(
                                        &mut draft.direction,
                                        a.name().to_string(),
                                        a.name(),
                                    );
                                }
                            });
                        }
                        if [
                            "align",
                            "offset",
                            "symmetric",
                            "inset",
                            "proportion",
                            "angle",
                        ]
                        .contains(&kind.as_str())
                        {
                            ui.horizontal(|ui| {
                                ui.label(Msg::FieldMateAxis.tr(lang));
                                for name in ["x", "y", "z"] {
                                    ui.selectable_value(&mut draft.axis, name.to_string(), name);
                                }
                            });
                        }
                        if kind == "align" || kind == "inset" {
                            ui.horizontal(|ui| {
                                ui.label(Msg::FieldMateEdge.tr(lang));
                                for (word, msg) in [
                                    ("min", Msg::MateEdgeMin),
                                    ("max", Msg::MateEdgeMax),
                                    ("center", Msg::MateEdgeCenter),
                                ] {
                                    ui.selectable_value(
                                        &mut draft.edge,
                                        word.to_string(),
                                        msg.tr(lang),
                                    );
                                }
                            });
                        }
                        if ["gap", "offset", "inset", "overlap"].contains(&kind.as_str()) {
                            ui.horizontal(|ui| {
                                ui.label(if kind != "offset" {
                                    Msg::FieldMateDistance.tr(lang)
                                } else {
                                    Msg::FieldMateOffset.tr(lang)
                                });
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.distance)
                                        .desired_width(80.0),
                                );
                            });
                        }
                        if kind == "symmetric" {
                            ui.horizontal(|ui| {
                                ui.label(Msg::FieldMatePlane.tr(lang));
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.plane)
                                        .desired_width(80.0),
                                );
                            });
                        }
                        if kind == "proportion" {
                            ui.horizontal(|ui| {
                                ui.label(Msg::FieldMateProportion.tr(lang));
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.proportion)
                                        .desired_width(80.0),
                                );
                            });
                        }
                        if kind == "turn" || kind == "angle" {
                            ui.horizontal(|ui| {
                                ui.label(Msg::FieldMateTurn.tr(lang));
                                for degrees in ["0", "90", "180", "270"] {
                                    ui.selectable_value(
                                        &mut draft.turn,
                                        degrees.to_string(),
                                        degrees,
                                    );
                                }
                            });
                        }
                        if ui.button(Msg::MatesAddDo.tr(lang)).clicked() {
                            add_mate = true;
                        }
                    }
                    Panel::PartPattern if !part_choices.is_empty() => {
                        let draft = &mut self.panels.part_pattern;
                        if draft.part >= part_choices.len() {
                            draft.part = 0;
                        }
                        ui.horizontal(|ui| {
                            ui.label(Msg::PartPatPart.tr(lang));
                            egui::ComboBox::from_id_salt("panel-pattern-part")
                                .selected_text(part_choices[draft.part].clone())
                                .show_ui(ui, |ui| {
                                    for (i, n) in part_choices.iter().enumerate() {
                                        ui.selectable_value(&mut draft.part, i, n.as_str());
                                    }
                                });
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label(Msg::PartPatKind.tr(lang));
                            for (word, msg) in [
                                ("linear", Msg::PartPatLinear),
                                ("grid", Msg::PartPatGrid),
                                ("circular", Msg::PartPatCircular),
                            ] {
                                ui.selectable_value(
                                    &mut draft.kind,
                                    word.to_string(),
                                    msg.tr(lang),
                                );
                            }
                        });
                        let kind = draft.kind.clone();
                        ui.horizontal(|ui| {
                            ui.label(Msg::PartPatCount.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.count).desired_width(60.0),
                            );
                            if kind == "grid" {
                                ui.label(Msg::PartPatRows.tr(lang));
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.rows).desired_width(60.0),
                                );
                            }
                        });
                        if kind == "circular" {
                            ui.horizontal(|ui| {
                                ui.label(Msg::PartPatCenter.tr(lang));
                                for value in draft.center.iter_mut() {
                                    ui.add(egui::TextEdit::singleline(value).desired_width(60.0));
                                }
                                ui.label(Msg::PartPatAngle.tr(lang));
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.angle)
                                        .desired_width(60.0),
                                );
                            });
                        } else {
                            let used = if kind == "grid" { 2 } else { 3 };
                            ui.horizontal(|ui| {
                                ui.label(Msg::PartPatStep.tr(lang));
                                for value in draft.step.iter_mut().take(used) {
                                    ui.add(egui::TextEdit::singleline(value).desired_width(60.0));
                                }
                            });
                        }
                        let side = self.win.ui.panels.pattern_preview_size as f64;
                        let lines = self.win.part_pattern_lines(&self.panels.part_pattern, side);
                        if !lines.is_empty() {
                            let (response, painter) = ui.allocate_painter(
                                Vec2::new(side as f32, side as f32),
                                egui::Sense::hover(),
                            );
                            let rect = response.rect;
                            painter.rect_filled(rect, 0.0, BACKGROUND);
                            for l in &lines {
                                let at = |p: (f64, f64)| {
                                    Pos2::new(rect.min.x + p.0 as f32, rect.min.y + p.1 as f32)
                                };
                                let colour = if l.hole { ACCENT } else { OK };
                                painter.line_segment([at(l.a), at(l.b)], Stroke::new(1.5, colour));
                            }
                        }
                        if ui.button(Msg::PartPatAddDo.tr(lang)).clicked() {
                            add_pattern = true;
                        }
                    }
                    Panel::Revolve if !names.is_empty() => {
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldSketch.tr(lang));
                            egui::ComboBox::from_id_salt("panel-revolve-sketch")
                                .selected_text(names[self.panels.sketch].clone())
                                .show_ui(ui, |ui| {
                                    for (i, n) in names.iter().enumerate() {
                                        ui.selectable_value(&mut self.panels.sketch, i, n.as_str());
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelAxis.tr(lang));
                            ui.selectable_value(
                                &mut self.panels.revolve_axis,
                                crate::revolve::Axis::X,
                                "x",
                            );
                            ui.selectable_value(
                                &mut self.panels.revolve_axis,
                                crate::revolve::Axis::Y,
                                "y",
                            );
                            ui.label(Msg::PanelAngle.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.revolve_angle)
                                    .desired_width(60.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelMaterial.tr(lang));
                            let shown = if self.panels.revolve_material.is_empty() {
                                Msg::PanelNoMaterial.tr(lang).to_string()
                            } else {
                                self.panels.revolve_material.clone()
                            };
                            egui::ComboBox::from_id_salt("panel-revolve-material")
                                .selected_text(shown)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.panels.revolve_material,
                                        String::new(),
                                        Msg::PanelNoMaterial.tr(lang),
                                    );
                                    for m in &materials {
                                        ui.selectable_value(
                                            &mut self.panels.revolve_material,
                                            m.clone(),
                                            m.as_str(),
                                        );
                                    }
                                });
                        });
                    }
                    Panel::ImportDxf => {
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelImportFile.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.import_file)
                                    .desired_width(320.0),
                            );
                            if ui.button(Msg::PanelImportChoose.tr(lang)).clicked() {
                                choose = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelImportName.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.import_name)
                                    .desired_width(160.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelImportFace.tr(lang));
                            egui::ComboBox::from_id_salt("panel-import-face")
                                .selected_text(face_msg(self.panels.import_face).tr(lang))
                                .show_ui(ui, |ui| {
                                    for face in Face::ALL {
                                        ui.selectable_value(
                                            &mut self.panels.import_face,
                                            face,
                                            face_msg(face).tr(lang),
                                        );
                                    }
                                });
                            ui.label(Msg::PanelImportUnit.tr(lang));
                            for unit in crate::dxf_import::UNITS {
                                ui.selectable_value(
                                    &mut self.panels.import_unit,
                                    unit.to_string(),
                                    unit,
                                );
                            }
                            ui.selectable_value(
                                &mut self.panels.import_unit,
                                "auto".to_string(),
                                "auto",
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelImportLayers.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.import_layers)
                                    .desired_width(200.0),
                            );
                        });
                        if self.panels.import_file.trim().is_empty() {
                            ui.label(egui::RichText::new(Msg::PanelImportNone.tr(lang)).color(DIM));
                        } else {
                            ui.horizontal(|ui| {
                                if ui.button(Msg::PanelImportPreview.tr(lang)).clicked() {
                                    look = true;
                                }
                                if ui.button(Msg::PanelImportDo.tr(lang)).clicked() {
                                    import = true;
                                }
                            });
                        }
                        if !self.panels.import_look.is_empty() {
                            ui.label(
                                egui::RichText::new(self.panels.import_look.as_str()).color(DIM),
                            );
                        }
                        if !self.panels.import_lines.is_empty() {
                            let side = self.win.ui.panels.import_preview_size as f32;
                            let (response, painter) =
                                ui.allocate_painter(Vec2::new(side, side), egui::Sense::hover());
                            let rect = response.rect;
                            painter.rect_filled(rect, 0.0, BACKGROUND);
                            for l in &self.panels.import_lines {
                                let at = |p: (f64, f64)| {
                                    Pos2::new(rect.min.x + p.0 as f32, rect.min.y + p.1 as f32)
                                };
                                let colour = if l.hole { ACCENT } else { OK };
                                painter.line_segment([at(l.a), at(l.b)], Stroke::new(1.5, colour));
                            }
                        }
                    }
                    Panel::Section => {
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelSectionAxis.tr(lang));
                            for (axis, name) in [(Dim::X, "x"), (Dim::Y, "y"), (Dim::Z, "z")] {
                                ui.selectable_value(&mut self.panels.section_axis, axis, name);
                            }
                            ui.label(Msg::PanelSectionAt.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.section_at)
                                    .desired_width(80.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelSectionMore.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.section_extra)
                                    .desired_width(200.0),
                            );
                        });
                        let pictures = self.win.section_pictures(
                            self.panels.section_axis,
                            &self.panels.section_at,
                            &self.panels.section_extra,
                        );
                        let side = self.win.ui.panels.section_preview_size as f32;
                        if !pictures.is_empty() {
                            ui.label(
                                egui::RichText::new(Msg::PanelSectionPictures.tr(lang)).color(DIM),
                            );
                        }
                        ui.horizontal_wrapped(|ui| {
                            for (title, lines) in &pictures {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(title.as_str()).color(DIM));
                                    let (response, painter) = ui.allocate_painter(
                                        Vec2::new(side, side),
                                        egui::Sense::hover(),
                                    );
                                    let rect = response.rect;
                                    painter.rect_filled(rect, 0.0, BACKGROUND);
                                    for l in lines {
                                        let at = |p: (f64, f64)| {
                                            Pos2::new(
                                                rect.min.x + p.0 as f32,
                                                rect.min.y + p.1 as f32,
                                            )
                                        };
                                        let colour = if l.hole { ACCENT } else { OK };
                                        painter.line_segment(
                                            [at(l.a), at(l.b)],
                                            Stroke::new(1.5, colour),
                                        );
                                    }
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::PanelFormat.tr(lang));
                            ui.selectable_value(&mut self.panels.format, SketchFormat::Dxf, "DXF");
                            ui.selectable_value(&mut self.panels.format, SketchFormat::Svg, "SVG");
                        });
                        let suggested = self.win.section_file_name(
                            self.panels.section_axis,
                            &self.panels.section_at,
                            self.panels.format,
                        );
                        ui.horizontal(|ui| {
                            ui.label(Msg::FileName.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.file)
                                    .hint_text(suggested.as_str())
                                    .desired_width(260.0),
                            );
                        });
                        ui.checkbox(&mut self.panels.overwrite, Msg::ExportOverwrite.tr(lang));
                        ui.horizontal(|ui| {
                            if ui.button(Msg::PanelWrite.tr(lang)).clicked() {
                                write = true;
                            }
                            if ui.button(Msg::PanelSectionWriteAll.tr(lang)).clicked() {
                                write_all = true;
                            }
                        });
                    }
                    Panel::Units => {
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldUnitsUnit.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.units_unit)
                                    .desired_width(60.0),
                            );
                            ui.label(Msg::FieldUnitsDecimals.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.units_decimals)
                                    .desired_width(40.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button(Msg::UnitsApplyDo.tr(lang)).clicked() {
                                units_apply = true;
                            }
                            if ui.button(Msg::UnitsFollowDo.tr(lang)).clicked() {
                                units_follow = true;
                            }
                        });
                    }
                    Panel::TitleBlock => {
                        ui.checkbox(&mut self.panels.title_enabled, Msg::TitleBlockUse.tr(lang));
                        for (msg, value) in [
                            Msg::TbOrganisation,
                            Msg::TbTitle,
                            Msg::TbNumber,
                            Msg::TbRevision,
                            Msg::TbAuthor,
                        ]
                        .into_iter()
                        .zip(self.panels.title_fields.iter_mut())
                        {
                            ui.horizontal(|ui| {
                                ui.label(msg.tr(lang));
                                ui.add(egui::TextEdit::singleline(value).desired_width(300.0));
                            });
                        }
                        if ui.button(Msg::TitleBlockApplyDo.tr(lang)).clicked() {
                            title_apply = true;
                        }
                    }
                    Panel::Compare => {
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldCompareFile.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.compare_file)
                                    .desired_width(300.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button(Msg::CompareFileDo.tr(lang)).clicked() {
                                compare_file = true;
                            }
                            if ui.button(Msg::CompareServerDo.tr(lang)).clicked() {
                                compare_server = true;
                            }
                        });
                    }
                    Panel::Server => {
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldServerName.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.server_name)
                                    .desired_width(200.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldServerComment.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.server_comment)
                                    .desired_width(300.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button(Msg::ServerSaveDo.tr(lang)).clicked() {
                                server_save = true;
                            }
                            if ui.button(Msg::ServerOpenDo.tr(lang)).clicked() {
                                server_open = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldServerQuery.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.server_query)
                                    .desired_width(160.0),
                            );
                            if ui.button(Msg::ServerListDo.tr(lang)).clicked() {
                                server_list = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button(Msg::ServerCheckDo.tr(lang)).clicked() {
                                server_check = true;
                            }
                            if ui.button(Msg::ServerReportDo.tr(lang)).clicked() {
                                server_report = true;
                            }
                        });
                        if !self.panels.server_list.is_empty() {
                            let mut shown: &str = self.panels.server_list.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut shown)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY),
                            );
                        }
                        // the panel of the settings of the server that every program of the suite shows
                        ui.separator();
                        let pane = self.link.get_or_insert_with(|| {
                            crate::linkpane::LinkPane::new(&self.win.settings.server)
                        });
                        pane.refresh();
                        if let Some(action) = eh3d_view::server_gui::show(
                            ui,
                            &mut pane.panel,
                            crate::linkpane::view_lang(lang),
                        ) {
                            pane.handle(action, &mut self.win.settings.server);
                        }
                    }
                    _ => {}
                }
                if panel == Panel::SketchExport {
                    if names.is_empty() {
                        ui.label(egui::RichText::new(Msg::PanelNoSketch.tr(lang)).color(DIM));
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(Msg::FieldSketch.tr(lang));
                            egui::ComboBox::from_id_salt("panel-sketch")
                                .selected_text(names[self.panels.sketch].clone())
                                .show_ui(ui, |ui| {
                                    for (i, n) in names.iter().enumerate() {
                                        ui.selectable_value(&mut self.panels.sketch, i, n.as_str());
                                    }
                                });
                            ui.label(Msg::PanelFormat.tr(lang));
                            ui.selectable_value(&mut self.panels.format, SketchFormat::Dxf, "DXF");
                            ui.selectable_value(&mut self.panels.format, SketchFormat::Svg, "SVG");
                        });
                        let suggested =
                            default_drawing_name(&names[self.panels.sketch], self.panels.format);
                        ui.horizontal(|ui| {
                            ui.label(Msg::FileName.tr(lang));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.panels.file)
                                    .hint_text(suggested.as_str())
                                    .desired_width(260.0),
                            );
                        });
                        ui.checkbox(&mut self.panels.overwrite, Msg::ExportOverwrite.tr(lang));
                        if ui.button(Msg::PanelWrite.tr(lang)).clicked() {
                            write = true;
                        }
                    }
                } else {
                    text = if panel == Panel::ImportDxf {
                        self.panels.import_report.clone()
                    } else if panel == Panel::Mates {
                        self.win.mates_panel_text(&self.panels.mate)
                    } else if panel == Panel::Compare {
                        self.panels.compare_text.clone()
                    } else if panel == Panel::PartPattern {
                        self.win.part_pattern_text(&self.panels.part_pattern)
                    } else if panel == Panel::Section {
                        self.win.sections_panel_text(
                            self.panels.section_axis,
                            &self.panels.section_at,
                            &self.panels.section_extra,
                        )
                    } else if panel == Panel::Revolve && !names.is_empty() {
                        let material = (!self.panels.revolve_material.is_empty())
                            .then_some(self.panels.revolve_material.as_str());
                        self.win.revolve_panel_text(
                            &names[self.panels.sketch],
                            self.panels.revolve_axis,
                            &self.panels.revolve_angle,
                            material,
                        )
                    } else {
                        self.win.panel_text(panel, &self.panels.options())
                    };
                    if ui.button(Msg::ReportCopy.tr(lang)).clicked() {
                        copy = true;
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // a text that is shown and can be selected but not changed
                        let mut shown: &str = text.as_str();
                        ui.add(
                            egui::TextEdit::multiline(&mut shown)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                    });
                }
            });
        if undo_now {
            self.win.undo();
        } else if redo_now {
            self.win.redo();
        } else if goto_now {
            let target = self.panels.history_goto.clone();
            self.win.history_goto_text(&target);
        }
        if add_mate {
            let draft = self.panels.mate.clone();
            self.win.add_mate_from_draft(&draft);
        }
        if add_pattern {
            let draft = self.panels.part_pattern.clone();
            self.win.add_part_pattern(&draft);
        }
        if units_apply {
            let (unit, decimals) = (
                self.panels.units_unit.clone(),
                self.panels.units_decimals.clone(),
            );
            self.win.apply_units_text(&unit, &decimals);
        }
        if units_follow {
            self.win.set_project_units(None);
        }
        if server_save {
            let (name, comment) = (
                self.panels.server_name.clone(),
                self.panels.server_comment.clone(),
            );
            self.win.server_save_now(&name, &comment);
        }
        if server_open {
            let name = self.panels.server_name.clone();
            self.win.server_open_now(&name);
        }
        if compare_file {
            let file = self.panels.compare_file.clone();
            self.panels.compare_text = self.win.compare_with_file_text(&file);
        }
        if compare_server {
            let name = self.panels.server_name.clone();
            self.panels.compare_text = self.win.compare_with_server_now(&name);
        }
        if server_list {
            let query = self.panels.server_query.clone();
            self.panels.server_list = self.win.server_list_now(&query);
        }
        if server_check || server_report {
            let kind = if server_check { "check" } else { "report" };
            let name = self.panels.server_name.clone();
            self.panels.server_list = self.win.server_station_now(kind, &name);
        }
        if title_apply {
            let fields = self.panels.title_fields.clone();
            self.win
                .apply_title_block(self.panels.title_enabled, &fields);
        }
        if choose {
            self.open_dxf_dialog();
        }
        if import {
            let file = PathBuf::from(self.panels.import_file.trim());
            let name = self.panels.import_name.clone();
            let unit = self.panels.import_unit.clone();
            let layers = self.panels.import_layers.clone();
            self.panels.import_report = self
                .win
                .import_dxf_layers(&file, &name, self.panels.import_face, &unit, &layers)
                .unwrap_or_default();
        }
        if look {
            let file = PathBuf::from(self.panels.import_file.trim());
            let unit = self.panels.import_unit.clone();
            let layers = self.panels.import_layers.clone();
            let (text, lines) = self
                .win
                .dxf_preview(&file, &unit, &layers)
                .unwrap_or_default();
            self.panels.import_look = text;
            self.panels.import_lines = lines;
        }
        if copy {
            ctx.copy_text(text);
            self.win.report_copied();
        } else if write_all && panel == Panel::Section {
            self.win.write_section_set(
                self.panels.section_axis,
                &self.panels.section_at,
                &self.panels.section_extra,
                self.panels.format,
                self.panels.overwrite,
            );
        } else if write && panel == Panel::Section {
            let file = if self.panels.file.trim().is_empty() {
                self.win.section_file_name(
                    self.panels.section_axis,
                    &self.panels.section_at,
                    self.panels.format,
                )
            } else {
                self.panels.file.clone()
            };
            self.win.write_section_drawing(
                self.panels.section_axis,
                &self.panels.section_at,
                self.panels.format,
                &file,
                self.panels.overwrite,
            );
        } else if write {
            let sketch = names[self.panels.sketch].clone();
            let format = self.panels.format;
            let file = if self.panels.file.trim().is_empty() {
                default_drawing_name(&sketch, format)
            } else {
                self.panels.file.clone()
            };
            self.win
                .write_sketch_drawing(&sketch, format, &file, self.panels.overwrite);
        }
        if !open {
            self.panels.open = None;
        }
    }

    fn dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        match show_dialog(ctx, &mut dialog, self.win.lang) {
            None => self.dialog = Some(dialog),
            Some(Outcome::Cancel) => {}
            Some(Outcome::Pick(path)) => {
                // the box to write over a file that exists is ticked again only by the settings
                // or by the person: an order to write over is for one file
                let done = if let Some(ask) = dialog.suite {
                    self.suite_pick(ask, &path, dialog.overwrite)
                } else if let Some(format) = dialog.export {
                    self.win.export_to(format, &path, dialog.overwrite)
                } else if dialog.dxf {
                    self.panels.import_file = path.display().to_string();
                    self.panels.open = Some(Panel::ImportDxf);
                    true
                } else if dialog.library {
                    self.win.open_library(&path)
                } else {
                    match (dialog.sketch, dialog.save.is_some()) {
                        (false, true) => self.win.save_to(&path),
                        (false, false) => suitewin::open_any(&mut self.win, &path),
                        (true, true) => self.sketch.save_to(&path),
                        (true, false) => {
                            let opened = self.sketch.open_file(&path);
                            self.fit_next = opened;
                            opened
                        }
                    }
                };
                // a file to import and a key are not projects to open again
                let imported = matches!(
                    dialog.suite,
                    Some(
                        SuiteAsk::Import
                            | SuiteAsk::Key
                            | SuiteAsk::ViewFile
                            | SuiteAsk::SignKey
                            | SuiteAsk::NewKey
                    )
                );
                if done && dialog.export.is_none() && !dialog.dxf && !imported {
                    self.recent_added.push(path.display().to_string());
                }
                if !done {
                    // the window stays open so that another name can be tried
                    self.dialog = Some(dialog);
                }
            }
        }
    }
}

/// The order lines are drawn in: the box first and the selection on top.
fn rank(kind: Kind) -> u8 {
    match kind {
        Kind::LidPlane => 0,
        Kind::Cavity => 1,
        Kind::Outer => 2,
        Kind::Board => 3,
        Kind::Post => 4,
        Kind::Hole => 5,
        Kind::Screw => 6,
        Kind::Conflict => 7,
        Kind::Selected => 8,
        Kind::Measure => 9,
    }
}

/// The colour and the width of a line.
fn style_of(line: &Line2, board: Color32) -> (Color32, f32) {
    match line.kind {
        Kind::Outer => (OUTER, 1.4),
        Kind::Cavity => (CAVITY, 1.0),
        Kind::LidPlane => (CAVITY, 0.8),
        Kind::Hole => (HOLE, 1.3),
        Kind::Screw => (SCREW, 1.3),
        Kind::Post => (POST, 1.3),
        Kind::Board => (board, 1.2),
        Kind::Selected => (ACCENT, 2.4),
        Kind::Measure => (MEASURE, 1.8),
        Kind::Conflict => (DANGER, 2.0),
    }
}

/// Draws the window that opens a file (or asks for the name of the file to write) and says what
/// the person chose.
fn show_dialog(ctx: &egui::Context, d: &mut Dialog, lang: Lang) -> Option<Outcome> {
    let mut outcome = None;
    let mut open = true;
    let title = match (d.sketch, d.save.is_some()) {
        _ if d.suite == Some(SuiteAsk::Open) => Msg::SuiteOpenTitle,
        _ if d.suite == Some(SuiteAsk::SaveAs) => Msg::SuiteSaveTitle,
        _ if d.suite == Some(SuiteAsk::Import) => Msg::SuiteImportTitle,
        _ if d.suite == Some(SuiteAsk::Key) => Msg::SuiteTrustKey,
        _ if d.suite == Some(SuiteAsk::ViewFile) => Msg::MvMenu,
        _ if d.suite == Some(SuiteAsk::SignKey) => Msg::SignChoose,
        _ if d.suite == Some(SuiteAsk::NewKey) => Msg::SignNewKey,
        _ if d.export.is_some() => Msg::ExportTitle,
        _ if d.dxf => Msg::ImportDxfTitle,
        _ if d.library => Msg::OpenLibraryTitle,
        (false, true) => Msg::SaveTitle,
        (false, false) => Msg::OpenTitle,
        (true, true) => Msg::SaveSketchTitle,
        (true, false) => Msg::OpenSketchTitle,
    };
    let export_extension = d.export.map(Format::extension);
    let export_extensions: Vec<&str> = export_extension.into_iter().collect();
    let extensions: &[&str] = if matches!(d.suite, Some(SuiteAsk::Import | SuiteAsk::ViewFile)) {
        &suitewin::IMPORT_EXTENSIONS
    } else if matches!(d.suite, Some(SuiteAsk::SignKey | SuiteAsk::NewKey)) {
        &SECRET_EXTENSIONS
    } else if d.suite == Some(SuiteAsk::Key) {
        &KEY_EXTENSIONS
    } else if d.suite.is_some() {
        &suitewin::SUITE_EXTENSIONS
    } else if d.export.is_some() {
        &export_extensions
    } else if d.dxf {
        &DXF_EXTENSIONS
    } else if d.library {
        &LIBRARY_EXTENSIONS
    } else if d.sketch {
        &SKETCH_EXTENSIONS
    } else {
        &PROJECT_EXTENSIONS
    };
    egui::Window::new(title.tr(lang))
        .open(&mut open)
        .collapsible(false)
        .default_size([620.0, 460.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(Msg::Up.tr(lang)).clicked() {
                    d.browser.up();
                }
                let edit = egui::TextEdit::singleline(&mut d.browser.path_text)
                    .desired_width(ui.available_width());
                let response = ui.add(edit);
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Some(p) = d.browser.go_typed() {
                        if d.save.is_none() {
                            outcome = Some(Outcome::Pick(p));
                        }
                    }
                }
            });
            if let Some(e) = &d.browser.error {
                ui.colored_label(DANGER, e.as_str());
            }
            if ui
                .checkbox(&mut d.browser.show_all, Msg::AllFiles.tr(lang))
                .changed()
            {
                d.browser.refresh();
            }
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    if d.browser.entries.is_empty() {
                        ui.label(egui::RichText::new(Msg::EmptyFolder.tr(lang)).color(DIM));
                    }
                    for e in d.browser.entries.clone() {
                        let label = if e.is_dir {
                            format!("{}/", e.name)
                        } else {
                            format!("{}    ({})", e.name, browse::size_text(e.size))
                        };
                        if !ui.selectable_label(false, label).clicked() {
                            continue;
                        }
                        if e.is_dir {
                            d.browser.enter(&e.name);
                        } else if d.save.is_some() {
                            d.save = Some(e.name.clone());
                        } else if let Some(p) = d.browser.enter(&e.name) {
                            outcome = Some(Outcome::Pick(p));
                        }
                    }
                });
            ui.separator();
            if d.export.is_some() || d.suite == Some(SuiteAsk::SaveAs) {
                ui.checkbox(&mut d.overwrite, Msg::ExportOverwrite.tr(lang));
            }
            ui.horizontal(|ui| {
                if let Some(name) = d.save.as_mut() {
                    ui.label(Msg::FileName.tr(lang));
                    ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
                    if ui.button(Msg::Save.tr(lang)).clicked() && !name.trim().is_empty() {
                        outcome = Some(Outcome::Pick(
                            d.browser.dir.join(with_extension_of(name, extensions)),
                        ));
                    }
                }
                if ui.button(Msg::Cancel.tr(lang)).clicked() {
                    outcome = Some(Outcome::Cancel);
                }
            });
        });
    if !open && outcome.is_none() {
        outcome = Some(Outcome::Cancel);
    }
    outcome
}

/// A file name typed to save a file: it gets the first of the extensions when it has none of them.
fn with_extension_of(name: &str, extensions: &[&str]) -> String {
    let name = name.trim();
    if browse::matches_extension(name, extensions) {
        name.to_string()
    } else {
        format!("{name}.{}", extensions[0])
    }
}

// the colours of the sketch
const GRID_MINOR: Color32 = Color32::from_rgba_premultiplied(14, 10, 0, 14);
const GRID_MAJOR: Color32 = Color32::from_rgba_premultiplied(34, 23, 0, 34);
const AXIS: Color32 = Color32::from_rgba_premultiplied(120, 83, 0, 120);
const PREVIEW: Color32 = Color32::from_rgba_premultiplied(60, 158, 188, 200);

/// The colour and the width of a primitive of the sketch.
fn sketch_style(stroke: SketchStroke) -> (Color32, f32) {
    match stroke {
        SketchStroke::GridMinor => (GRID_MINOR, 1.0),
        SketchStroke::GridMajor => (GRID_MAJOR, 1.0),
        SketchStroke::Axis => (AXIS, 1.3),
        SketchStroke::Normal => (OUTER, 1.8),
        SketchStroke::Construction => (HOLE, 1.2),
        SketchStroke::Selected => (ACCENT, 2.8),
        SketchStroke::Related => (HOLE, 2.6),
        SketchStroke::Conflict => (DANGER, 2.6),
        SketchStroke::Redundant => (POST, 2.6),
        SketchStroke::Preview => (PREVIEW, 1.4),
        SketchStroke::Dimension => (DIM, 1.0),
        SketchStroke::Marker => (SCREW, 1.0),
    }
}

/// The colour of the indicator of the state of the sketch.
fn tone_colour(tone: Tone) -> Color32 {
    match tone {
        Tone::Green => OK,
        Tone::Amber => ACCENT,
        Tone::Red => DANGER,
    }
}

/// Draws the primitives of the sketch on a canvas whose upper left corner is `rect.min`.
fn paint_sketch(painter: &egui::Painter, rect: Rect, prims: &[Prim]) {
    let at = |p: (f64, f64)| Pos2::new(rect.min.x + p.0 as f32, rect.min.y + p.1 as f32);
    for prim in prims {
        match prim {
            Prim::Segment { a, b, stroke } => {
                let (colour, width) = sketch_style(*stroke);
                let points = [at(*a), at(*b)];
                if *stroke == SketchStroke::Construction {
                    painter.extend(egui::Shape::dashed_line(
                        &points,
                        Stroke::new(width, colour),
                        6.0,
                        4.0,
                    ));
                } else {
                    painter.line_segment(points, Stroke::new(width, colour));
                }
            }
            Prim::Polyline { points, stroke } => {
                let (colour, width) = sketch_style(*stroke);
                let points: Vec<Pos2> = points.iter().map(|p| at(*p)).collect();
                if *stroke == SketchStroke::Construction {
                    painter.extend(egui::Shape::dashed_line(
                        &points,
                        Stroke::new(width, colour),
                        6.0,
                        4.0,
                    ));
                } else {
                    painter.add(egui::Shape::line(points, Stroke::new(width, colour)));
                }
            }
            Prim::Dot { at: p, stroke } => {
                let (colour, _) = sketch_style(*stroke);
                let radius = match stroke {
                    SketchStroke::Normal => 3.0,
                    SketchStroke::Selected => 5.0,
                    _ => 4.0,
                };
                painter.circle_filled(at(*p), radius, colour);
            }
            Prim::Ring { at: p, stroke } => {
                let (colour, width) = sketch_style(*stroke);
                painter.circle_stroke(at(*p), 8.0, Stroke::new(width, colour));
            }
            Prim::Text {
                at: p,
                text,
                stroke,
            } => {
                let (colour, _) = sketch_style(*stroke);
                painter.text(
                    at(*p),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(12.0),
                    colour,
                );
            }
        }
    }
}

/// What the person chose in the menu of the canvas.
enum Chosen {
    Constraint(ConstraintTool),
    Dimension(DimTool),
}

impl App {
    fn sketch_folder(&self) -> PathBuf {
        self.sketch
            .path
            .as_ref()
            .or(self.win.path.as_ref())
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(browse::start_dir)
    }

    fn open_sketch_dialog(&mut self) {
        self.dialog = Some(Dialog {
            suite: None,
            browser: Browser::new(self.sketch_folder(), &SKETCH_EXTENSIONS),
            save: None,
            sketch: true,
            export: None,
            overwrite: false,
            library: false,
            dxf: false,
        });
    }

    fn save_sketch_dialog(&mut self) {
        let name = self
            .sketch
            .path
            .clone()
            .or_else(|| self.win.path.as_deref().map(sketch_file::sibling_path))
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| format!("sketch.{}", SKETCH_EXTENSIONS[0]));
        self.dialog = Some(Dialog {
            suite: None,
            browser: Browser::new(self.sketch_folder(), &SKETCH_EXTENSIONS),
            save: Some(name),
            sketch: true,
            export: None,
            overwrite: false,
            library: false,
            dxf: false,
        });
    }

    /// Saves to the file the sketch has, or asks for a name when it has none.
    fn save_sketch_or_ask(&mut self) {
        if self.sketch.path.is_none() {
            self.save_sketch_dialog();
        } else {
            self.sketch.save();
        }
    }

    fn panel_sketch_tools(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(Msg::ToolsTitle.tr(lang))
                    .strong()
                    .color(title_colour(ui.visuals())),
            );
            for tool in Tool::ALL {
                let on = self.sketch.tool == tool;
                if ui.selectable_label(on, tool.msg().tr(lang)).clicked() {
                    self.sketch.set_tool(tool);
                }
            }
            ui.separator();
            ui.checkbox(
                &mut self.sketch.construction_mode,
                Msg::ConstructionMode.tr(lang),
            );
            ui.checkbox(&mut self.sketch.grid_snap, Msg::GridSnap.tr(lang));
            ui.checkbox(
                &mut self.sketch.auto_constraints,
                Msg::AutoConstraints.tr(lang),
            );
            ui.separator();
            let has_elements = !self.sketch.picked.elements.is_empty();
            if ui
                .add_enabled(
                    has_elements,
                    egui::Button::new(Msg::ToggleConstruction.tr(lang)),
                )
                .clicked()
            {
                self.sketch.toggle_construction();
            }
            let has_any = !self.sketch.picked.is_empty();
            if ui
                .add_enabled(has_any, egui::Button::new(Msg::DeleteSelection.tr(lang)))
                .clicked()
            {
                self.sketch.delete_selection();
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.sketch.name_input).desired_width(110.0));
            if ui
                .add_enabled(
                    !self.win.read_only,
                    egui::Button::new(Msg::KeepSketch.tr(lang)),
                )
                .clicked()
            {
                self.keep_sketch();
            }
        });
    }

    /// The constraints that tie the geometry, the buttons that add one to the selection and the
    /// elements of the sketch.
    fn panel_constraints(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(Msg::ConstraintsTitle.tr(lang))
                .strong()
                .color(title_colour(ui.visuals())),
        );
        ui.label(egui::RichText::new(Msg::AddConstraintHint.tr(lang)).color(DIM));
        let available = self.sketch.available();
        let mut add = None;
        ui.horizontal_wrapped(|ui| {
            for tool in ConstraintTool::ALL {
                let enabled = available.contains(&tool);
                if ui
                    .add_enabled(enabled, egui::Button::new(tool.msg().tr(lang)))
                    .clicked()
                {
                    add = Some(tool);
                }
            }
        });
        if let Some(tool) = add {
            self.sketch.add_constraint(tool);
        }
        ui.separator();
        let extend = ui.input(|i| i.modifiers.command);
        let rows: Vec<_> = self
            .sketch
            .constraint_rows()
            .into_iter()
            .filter(|r| r.value.is_none())
            .collect();
        let mut toggled = None;
        let mut picked = None;
        let mut removed = None;
        egui::ScrollArea::vertical()
            .id_salt("sketch_constraint_rows")
            .max_height(ui.available_height() * 0.55)
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(egui::RichText::new(Msg::NoConstraints.tr(lang)).color(DIM));
                }
                for row in &rows {
                    ui.horizontal(|ui| {
                        let mut on = row.enabled;
                        if ui
                            .checkbox(&mut on, "")
                            .on_hover_text(Msg::ConstraintEnabled.tr(lang))
                            .changed()
                        {
                            toggled = Some((row.id, on));
                        }
                        let colour = match (row.enabled, row.mark) {
                            (false, _) => DIM,
                            (true, RowMark::Ok) => TEXT,
                            (true, RowMark::Redundant) => POST,
                            (true, RowMark::Conflict) => DANGER,
                        };
                        let text = egui::RichText::new(format!(
                            "{}  {}  {}",
                            row.id,
                            row.label.tr(lang),
                            row.elements
                        ))
                        .color(colour);
                        if ui.selectable_label(row.selected, text).clicked() {
                            picked = Some(row.id);
                        }
                        if ui.small_button("x").clicked() {
                            removed = Some(row.id);
                        }
                    });
                }
            });
        if let Some((id, on)) = toggled {
            self.sketch.set_enabled(id, on);
        }
        if let Some(id) = picked {
            self.sketch.select_constraint(id, extend);
        }
        if let Some(id) = removed {
            self.sketch.select_constraint(id, false);
            self.sketch.delete_selection();
        }
        ui.separator();
        ui.label(
            egui::RichText::new(Msg::ElementsTitle.tr(lang))
                .strong()
                .color(title_colour(ui.visuals())),
        );
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("sketch_element_rows")
            .show(ui, |ui| {
                for row in self.sketch.element_rows() {
                    let marker = if row.construction { " *" } else { "" };
                    let text = egui::RichText::new(format!(
                        "{}  {}  {}{marker}",
                        row.id,
                        row.label.tr(lang),
                        row.detail
                    ))
                    .color(if row.construction { HOLE } else { TEXT });
                    if ui.selectable_label(row.selected, text).clicked() {
                        chosen = Some(row.id);
                    }
                }
            });
        if let Some(id) = chosen {
            self.sketch.select_element(id, extend);
        }
    }

    /// The dimensions: the value (or variable) to type, the buttons that add one to the selection
    /// and the dimensions of the sketch with their values to edit.
    fn panel_dimensions(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(Msg::DimensionsTitle.tr(lang))
                .strong()
                .color(title_colour(ui.visuals())),
        );
        ui.label(egui::RichText::new(Msg::DimensionsHint.tr(lang)).color(DIM));
        ui.add(
            egui::TextEdit::singleline(&mut self.sketch.dim_input)
                .hint_text(Msg::DimValueHint.tr(lang))
                .desired_width(f32::INFINITY),
        );
        let names: Vec<String> = self
            .win
            .mech
            .vars
            .names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        if !names.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "{}: {}",
                    Msg::GroupVariables.tr(lang),
                    names.join(", ")
                ))
                .color(DIM),
            );
        }
        let available = self.sketch.dimensions_available();
        let mut add = None;
        ui.horizontal_wrapped(|ui| {
            for tool in DimTool::ALL {
                let enabled = available.contains(&tool);
                if ui
                    .add_enabled(enabled, egui::Button::new(tool.msg().tr(lang)))
                    .clicked()
                {
                    add = Some(tool);
                }
            }
        });
        if let Some(tool) = add {
            self.sketch.add_dimension(tool);
        }
        let can_round = self.sketch.can_round_corner();
        if ui
            .add_enabled(can_round, egui::Button::new(Msg::RoundCorner.tr(lang)))
            .on_hover_text(Msg::RoundCornerHint.tr(lang))
            .clicked()
        {
            self.sketch.round_corner();
        }
        let can_mirror = self.sketch.can_mirror();
        if ui
            .add_enabled(can_mirror, egui::Button::new(Msg::MirrorSelection.tr(lang)))
            .on_hover_text(Msg::MirrorHint.tr(lang))
            .clicked()
        {
            self.sketch.mirror_selection();
        }
        let can_chamfer_corner = self.sketch.can_chamfer_corner();
        if ui
            .add_enabled(
                can_chamfer_corner,
                egui::Button::new(Msg::ChamferCorner.tr(lang)),
            )
            .on_hover_text(Msg::ChamferHint.tr(lang))
            .clicked()
        {
            self.sketch.chamfer_corner();
        }
        let can_trim_line = self.sketch.can_trim_line();
        if ui
            .add_enabled(can_trim_line, egui::Button::new(Msg::TrimLine.tr(lang)))
            .on_hover_text(Msg::TrimHint.tr(lang))
            .clicked()
        {
            self.sketch.trim_selected_line();
        }
        let can_extend_line = self.sketch.can_extend_line();
        if ui
            .add_enabled(can_extend_line, egui::Button::new(Msg::ExtendLine.tr(lang)))
            .on_hover_text(Msg::ExtendHint.tr(lang))
            .clicked()
        {
            self.sketch.extend_selected_line();
        }
        let can_linear_pattern = self.sketch.can_linear_pattern();
        if ui
            .add_enabled(
                can_linear_pattern,
                egui::Button::new(Msg::LinearPattern.tr(lang)),
            )
            .on_hover_text(Msg::LinearPatternHint.tr(lang))
            .clicked()
        {
            self.sketch.linear_pattern_selection();
        }
        let can_circular_pattern = self.sketch.can_circular_pattern();
        if ui
            .add_enabled(
                can_circular_pattern,
                egui::Button::new(Msg::CircularPattern.tr(lang)),
            )
            .on_hover_text(Msg::CircularPatternHint.tr(lang))
            .clicked()
        {
            self.sketch.circular_pattern_selection();
        }
        let can_offset_outline = self.sketch.can_offset_outline();
        if ui
            .add_enabled(
                can_offset_outline,
                egui::Button::new(Msg::OffsetOutline.tr(lang)),
            )
            .on_hover_text(Msg::OffsetHint.tr(lang))
            .clicked()
        {
            self.sketch.offset_selection();
        }
        let extend = ui.input(|i| i.modifiers.command);
        let rows: Vec<_> = self
            .sketch
            .constraint_rows()
            .into_iter()
            .filter(|r| r.value.is_some())
            .collect();
        let mut edited = None;
        let mut toggled = None;
        let mut referred = None;
        let mut picked = None;
        let mut removed = None;
        egui::ScrollArea::vertical()
            .id_salt("sketch_dimension_rows")
            .show(ui, |ui| {
                for row in &rows {
                    ui.separator();
                    ui.horizontal(|ui| {
                        let mut on = row.enabled;
                        if ui
                            .checkbox(&mut on, "")
                            .on_hover_text(Msg::ConstraintEnabled.tr(lang))
                            .changed()
                        {
                            toggled = Some((row.id, on));
                        }
                        let colour = match (row.enabled, row.mark) {
                            (false, _) => DIM,
                            (true, RowMark::Ok) => TEXT,
                            (true, RowMark::Redundant) => POST,
                            (true, RowMark::Conflict) => DANGER,
                        };
                        let text = egui::RichText::new(format!(
                            "{}  {}  {}",
                            row.id,
                            row.label.tr(lang),
                            row.elements
                        ))
                        .color(colour);
                        if ui.selectable_label(row.selected, text).clicked() {
                            picked = Some(row.id);
                        }
                        if ui.small_button("x").clicked() {
                            removed = Some(row.id);
                        }
                    });
                    let shown = row.edit.clone().unwrap_or_default();
                    let mut text = self
                        .dim_edits
                        .remove(&row.id.0)
                        .unwrap_or_else(|| shown.clone());
                    ui.horizontal(|ui| {
                        let edit = egui::TextEdit::singleline(&mut text).desired_width(170.0);
                        let response = ui.add_enabled(row.enabled && !row.reference, edit);
                        if response.lost_focus() && text != shown {
                            edited = Some((row.id, text.clone()));
                        }
                        if !response.has_focus() {
                            text.clone_from(&shown);
                        }
                        let value = row.value.clone().unwrap_or_default();
                        ui.label(egui::RichText::new(format!("= {value}")).color(DIM));
                    });
                    let mut reference = row.reference;
                    if ui
                        .checkbox(&mut reference, Msg::ReferenceDimension.tr(lang))
                        .changed()
                    {
                        referred = Some((row.id, reference));
                    }
                    self.dim_edits.insert(row.id.0, text);
                }
            });
        if let Some((id, on)) = toggled {
            self.sketch.set_enabled(id, on);
        }
        if let Some((id, reference)) = referred {
            self.dim_edits.remove(&id.0);
            self.sketch.set_reference(id, reference);
        }
        if let Some(id) = picked {
            self.sketch.select_constraint(id, extend);
        }
        if let Some(id) = removed {
            self.sketch.select_constraint(id, false);
            self.sketch.delete_selection();
        }
        if let Some((id, text)) = edited {
            self.sketch.edit_dimension(id, &text);
            self.dim_edits.remove(&id.0);
        }
    }

    /// The indicator of the state of the sketch, the constraints that repeat or contradict the
    /// others with the button that switches them off, and the status line.
    fn panel_sketch_status(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.horizontal(|ui| {
            let colour = tone_colour(self.sketch.report.tone());
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 6.0, colour);
            ui.label(egui::RichText::new(self.sketch.state_text()).color(colour));
            ui.separator();
            let status = if self.sketch.status.is_problem() {
                DANGER
            } else {
                OK
            };
            ui.label(egui::RichText::new(self.sketch.status.text(lang)).color(status));
        });
        let lines = self.sketch.problem_lines();
        if !lines.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for line in &lines {
                    ui.colored_label(DANGER, line.as_str());
                }
                if !self.sketch.report.problems().is_empty()
                    && ui.button(Msg::DisableProblems.tr(lang)).clicked()
                {
                    self.sketch.disable_problems();
                }
            });
        }
        ui.label(egui::RichText::new(self.sketch.tool.hint().tr(lang)).color(DIM));
        // the entry line: a point typed as text is placed as a click would place it
        ui.horizontal(|ui| {
            ui.label(Msg::PointEntryLabel.tr(lang));
            let line = ui.add(
                egui::TextEdit::singleline(&mut self.sketch.point_input)
                    .hint_text(Msg::PointEntryHint.tr(lang))
                    .desired_width(f32::INFINITY),
            );
            if line.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.sketch.type_point();
                line.request_focus();
            }
        });
    }

    /// The canvas: the grid, the axes and the geometry, and what the pointer does on them.
    fn panel_sketch(&mut self, ui: &mut egui::Ui) {
        self.drew_sketch = true;
        let lang = self.win.lang;
        let size = ui.available_size();
        let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, BACKGROUND);
        self.sketch
            .set_canvas((f64::from(rect.width()), f64::from(rect.height())));
        if self.fit_next {
            self.fit_next = false;
            self.sketch.fit();
        }
        let local = |p: Pos2| (f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
        let (space, extend) = ui.input(|i| (i.key_down(egui::Key::Space), i.modifiers.command));
        let pointer = response.hover_pos().or(response.interact_pointer_pos());
        if let Some(p) = pointer {
            self.sketch.pointer_move(local(p));
        } else if !self.sketch.is_dragging() {
            self.sketch.pointer_leave();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(p) = pointer {
                    self.sketch.wheel(local(p), f64::from(scroll));
                }
            }
        }
        let panning = response.dragged_by(egui::PointerButton::Middle)
            || (space && response.dragged_by(egui::PointerButton::Primary));
        if panning {
            let d = response.drag_delta();
            self.sketch.pan(f64::from(d.x), f64::from(d.y));
        } else if self.sketch.tool == Tool::Select {
            if response.drag_started_by(egui::PointerButton::Primary) {
                if let Some(p) = response.interact_pointer_pos() {
                    self.sketch.begin_drag(local(p));
                }
            }
            if response.dragged_by(egui::PointerButton::Primary) && self.sketch.is_dragging() {
                if let Some(p) = pointer {
                    self.sketch.drag_to(local(p));
                }
            }
            if response.drag_stopped() {
                self.sketch.end_drag();
            }
            if response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    self.sketch.select_at(local(p), extend);
                }
            }
            if response.secondary_clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    let under = self.sketch.hit(local(p));
                    if under.is_some_and(|id| !self.sketch.picked.elements.contains(&id)) {
                        self.sketch.select_at(local(p), false);
                    }
                }
            }
        } else {
            if response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    self.sketch.place(local(p));
                }
            }
            if response.secondary_clicked() {
                self.sketch.finish();
            }
        }
        if response.double_clicked() {
            if self.sketch.tool == Tool::Polyline {
                self.sketch.finish();
            } else if self.sketch.tool == Tool::Select {
                self.sketch.fit();
            }
        }
        let mut chosen = None;
        if self.sketch.tool == Tool::Select {
            response.context_menu(|ui| {
                let constraints = self.sketch.available();
                let dimensions = self.sketch.dimensions_available();
                if constraints.is_empty() && dimensions.is_empty() {
                    ui.label(egui::RichText::new(Msg::AddConstraintHint.tr(lang)).color(DIM));
                }
                for tool in constraints {
                    if ui.button(tool.msg().tr(lang)).clicked() {
                        chosen = Some(Chosen::Constraint(tool));
                        ui.close();
                    }
                }
                if !dimensions.is_empty() {
                    ui.separator();
                }
                for tool in dimensions {
                    if ui.button(tool.msg().tr(lang)).clicked() {
                        chosen = Some(Chosen::Dimension(tool));
                        ui.close();
                    }
                }
            });
        }
        match chosen {
            Some(Chosen::Constraint(tool)) => self.sketch.add_constraint(tool),
            Some(Chosen::Dimension(tool)) => self.sketch.add_dimension(tool),
            None => {}
        }
        let painter = painter.with_clip_rect(rect);
        paint_sketch(&painter, rect, &self.sketch.scene());
        painter.text(
            Pos2::new(rect.min.x + 12.0, rect.min.y + 10.0),
            egui::Align2::LEFT_TOP,
            Msg::WorkspaceSketch.tr(lang),
            egui::FontId::proportional(14.0),
            canvas_title_colour(),
        );
        if let Some(hint) = self.sketch.proposal() {
            painter.text(
                Pos2::new(rect.min.x + 12.0, rect.min.y + 30.0),
                egui::Align2::LEFT_TOP,
                hint.tr(lang),
                egui::FontId::proportional(12.0),
                HOLE,
            );
        }
        painter.text(
            Pos2::new(rect.min.x + 12.0, rect.max.y - 10.0),
            egui::Align2::LEFT_BOTTOM,
            Msg::CanvasHint.tr(lang),
            egui::FontId::proportional(12.0),
            DIM,
        );
    }
}

impl App {
    /// The keys of the sketch: select all, Escape, Enter and Delete (open, save, undo and redo are
    /// the shortcuts of the menus of the frame). They are ignored while a text is being typed.
    fn sketch_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let keys = ctx.input(|i| {
            [
                i.modifiers.command && i.key_pressed(egui::Key::A),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            ]
        });
        let [all, escape, enter, delete] = keys;
        if all {
            self.sketch.select_all();
        }
        if escape {
            self.sketch.cancel();
        }
        if enter {
            self.sketch.finish();
        }
        if delete && !self.sketch.picked.is_empty() {
            self.sketch.delete_selection();
        }
    }
}

impl App {
    /// What is done at the start of every frame: a file dropped on the window is opened, the keys
    /// of the model or of the sketch are read.
    fn begin_frame(&mut self, ctx: &egui::Context) {
        self.drew_view3d = false;
        self.drew_sketch = false;
        // the limits of the attachments follow the settings; a file dropped on their panel is
        // attached there
        self.attachments.set_limits(self.win.ui.attachments.clone());
        let dropped = self.attachments.take_drop(ctx, suitewin::opens_file);
        if let Some(p) = dropped.first() {
            let opened = if crate::shot::is_sketch_file(p) {
                self.fit_next = self.sketch.open_file(p);
                self.workspace = StartView::Sketch;
                self.pending_active.push(dockui::id::SKETCH);
                self.fit_next
            } else {
                suitewin::open_any(&mut self.win, p)
            };
            if opened {
                self.recent_added.push(p.display().to_string());
            }
            self.refresh_branches();
        }
        if self.workspace == StartView::Sketch {
            self.sketch.lang = self.win.lang;
            self.sketch.set_variables(&self.win.mech.vars);
            self.sketch_keys(ctx);
        } else if !ctx.egui_wants_keyboard_input()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            if self.win.dragging() {
                self.win.drag_cancel();
                self.ignore_drag = true;
            } else if self.win.measure_tool != MeasureTool::Off {
                self.win.set_measure_tool(MeasureTool::Off);
            }
        }
    }

    /// What is done at the end of every frame: the mode follows the panel on show, and the windows
    /// that float over the frame are drawn.
    fn finish_frame(&mut self, ctx: &egui::Context) {
        self.workspace =
            dockui::mode_after_frame(self.workspace, self.drew_view3d, self.drew_sketch);
        self.report_window(ctx);
        self.panel_window(ctx);
        self.branches_window(ctx);
        self.suite_note_window(ctx);
        self.variants_window(ctx);
        self.texts_window(ctx);
        self.viewer_window(ctx);
        self.signing_window(ctx);
        self.dialog_window(ctx);
        self.shot_tick(ctx);
    }

    /// What the window knows when it decides which entries of the menus can be chosen.
    fn menu_facts(&self) -> MenuFacts {
        let sketch = self.workspace == StartView::Sketch;
        MenuFacts {
            sketch_mode: sketch,
            can_undo: if sketch {
                self.sketch.can_undo()
            } else {
                self.win.can_undo()
            },
            can_redo: if sketch {
                self.sketch.can_redo()
            } else {
                self.win.can_redo()
            },
            can_save: !self.win.read_only,
            suite_file: suitewin::current_file(&self.win).is_some(),
        }
    }

    /// The name of the file in use, with what is said about it.
    fn title_strip(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::top("title").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("EH3D-PHOENIX-CAD")
                        .strong()
                        .color(title_colour(ui.visuals())),
                );
                ui.separator();
                let (path, dirty) = if self.workspace == StartView::Sketch {
                    (self.sketch.path.as_ref(), self.sketch.dirty)
                } else {
                    (self.win.path.as_ref(), self.win.dirty)
                };
                let name = path
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                ui.label(egui::RichText::new(name).color(TEXT));
                if dirty {
                    ui.label(egui::RichText::new(Msg::Unsaved.tr(lang)).color(ACCENT));
                }
                if self.win.read_only {
                    ui.label(egui::RichText::new(Msg::ReadOnlyBadge.tr(lang)).color(DANGER));
                }
            });
        });
    }

    /// The attachments of the project of the suite that the window works on; after a write the
    /// list of the branches is read again.
    fn panel_attachments(&mut self, ui: &mut egui::Ui) {
        let file = suitewin::current_file(&self.win);
        self.attachment_view_button(ui);
        if self
            .attachments
            .show(ui, file.as_deref(), self.win.lang.code())
        {
            self.refresh_branches();
        }
    }

    /// The panel of the status and the measures: the one of the model or the one of the sketch.
    fn panel_console(&mut self, ui: &mut egui::Ui) {
        if self.workspace == StartView::Sketch {
            self.panel_sketch_status(ui);
        } else {
            self.panel_console_model(ui);
        }
    }

    /// The drawing sheets: the lines of the sheet of the project (views, dimensions, balloons, the
    /// bill, the layers and the problems), the buttons that make it, update it, keep it and write it
    /// as SVG, and a small picture of the sheet. What it does is in `crate::plans::panel`.
    fn panel_drawing(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        // the project that is open is read when it changes
        if self.plans_seen != self.win.path {
            self.plans_seen = self.win.path.clone();
            self.plans = PlanState::with_defaults();
            if let Some(path) = self.win.path.clone() {
                if let Err(e) = self.plans.load(&path, lang) {
                    self.plans.note = Some(PlanState::error_line(&e, lang));
                }
            }
        }
        let mut action: Option<&'static str> = None;
        let mut form_action: Option<FormAction> = None;
        let choices = self.plans.view_choices();
        let kinds = self.plans_form.kinds(&self.plans);
        let mut toggles: Vec<(String, bool)> = Vec::new();
        // the pointer on the picture of the sheet, in micrometres: 0 pressed, 1 moved, 2 released
        let mut pointer_events: Vec<(u8, (i64, i64))> = Vec::new();
        let have_file = self.win.path.is_some();
        let has_sheet = self.plans.has_sheet();
        ui.horizontal_wrapped(|ui| {
            if !has_sheet
                && ui
                    .add_enabled(have_file, egui::Button::new(Msg::PlCreate.tr(lang)))
                    .clicked()
            {
                action = Some("create");
            }
            if has_sheet {
                if ui.button(Msg::PlRefresh.tr(lang)).clicked() {
                    action = Some("refresh");
                }
                if ui
                    .add_enabled(self.plans.dirty, egui::Button::new(Msg::PlSave.tr(lang)))
                    .clicked()
                {
                    action = Some("save");
                }
                if ui.button(Msg::PlExport.tr(lang)).clicked() {
                    action = Some("svg");
                }
            }
        });
        let lines = self.plans.lines(lang);
        let tone = |t: PlanTone| match t {
            PlanTone::Plain => TEXT,
            PlanTone::Good => OK,
            PlanTone::Bad => DANGER,
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(h) = &lines.head {
                ui.label(egui::RichText::new(&h.text).color(tone(h.tone)).strong());
            }
            if let Some(n) = &self.plans.note {
                ui.label(egui::RichText::new(&n.text).color(tone(n.tone)));
            }
            if has_sheet && !choices.is_empty() {
                // one set of buttons, with no modes: the sheet is changed from here and by dragging
                let form = &mut self.plans_form;
                egui::CollapsingHeader::new(Msg::PlEditSheet.tr(lang))
                    .default_open(true)
                    .show(ui, |ui| {
                        let name_of = |i: usize| {
                            choices
                                .get(i)
                                .or_else(|| choices.last())
                                .map(|(_, n)| n.clone())
                                .unwrap_or_default()
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(Msg::PlViewOne.tr(lang));
                            egui::ComboBox::from_id_salt("plans_view")
                                .selected_text(name_of(form.view))
                                .show_ui(ui, |ui| {
                                    for (i, (_, n)) in choices.iter().enumerate() {
                                        ui.selectable_value(&mut form.view, i, n);
                                    }
                                });
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut form.date)
                                    .hint_text(Msg::TbDate.tr(lang))
                                    .desired_width(70.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut form.description)
                                    .hint_text(Msg::PlDescription.tr(lang))
                                    .desired_width(130.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut form.author)
                                    .hint_text(Msg::TbAuthor.tr(lang))
                                    .desired_width(70.0),
                            );
                            if ui.button(Msg::PlAddRevision.tr(lang)).clicked() {
                                form_action = Some(FormAction::Revision);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(Msg::PlAddDims.tr(lang)).clicked() {
                                form_action = Some(FormAction::Dims);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            let shown = kinds
                                .get(form.kind)
                                .or_else(|| kinds.first())
                                .map_or("-", |k| k.name());
                            egui::ComboBox::from_id_salt("plans_kind")
                                .selected_text(shown)
                                .show_ui(ui, |ui| {
                                    for (i, k) in kinds.iter().enumerate() {
                                        ui.selectable_value(&mut form.kind, i, k.name());
                                    }
                                });
                            ui.add(
                                egui::TextEdit::singleline(&mut form.position)
                                    .hint_text(Msg::PlPosition.tr(lang))
                                    .desired_width(80.0),
                            );
                            if ui
                                .add_enabled(
                                    !kinds.is_empty(),
                                    egui::Button::new(Msg::PlAddSection.tr(lang)),
                                )
                                .clicked()
                            {
                                form_action = Some(FormAction::Section);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut form.text)
                                    .hint_text(Msg::PlText.tr(lang))
                                    .desired_width(200.0),
                            );
                            if ui.button(Msg::PlAddNote.tr(lang)).clicked() {
                                form_action = Some(FormAction::Note);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut form.along_u, "u");
                            ui.add(
                                egui::TextEdit::singleline(&mut form.from)
                                    .hint_text(Msg::PlFrom.tr(lang))
                                    .desired_width(70.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut form.to)
                                    .hint_text(Msg::PlTo.tr(lang))
                                    .desired_width(70.0),
                            );
                            if ui.button(Msg::PlAddBreak.tr(lang)).clicked() {
                                form_action = Some(FormAction::Break);
                            }
                            if ui.button("×").clicked() {
                                form_action = Some(FormAction::Unbreak);
                            }
                        });
                    });
            }
            for (title, list) in [
                (Msg::PlViews, &lines.views),
                (Msg::PlDims, &lines.dims),
                (Msg::PlBalloons, &lines.balloons),
                (Msg::PlNotes, &lines.note_items),
                (Msg::PlBomHead, &lines.bom),
                (Msg::TbRevision, &lines.revisions),
            ] {
                if list.is_empty() {
                    continue;
                }
                let name = if title == Msg::PlBomHead {
                    crate::lang::fill(title.tr(lang), &[&list.len().to_string()])
                } else {
                    title.tr(lang).to_string()
                };
                ui.label(egui::RichText::new(name).color(title_colour(ui.visuals())));
                for l in list {
                    ui.label(egui::RichText::new(&l.text).color(tone(l.tone)));
                }
            }
            for l in &lines.notes {
                ui.label(egui::RichText::new(&l.text).color(tone(l.tone)));
            }
            if !lines.layers.is_empty() {
                ui.label(
                    egui::RichText::new(Msg::PlLayers.tr(lang)).color(title_colour(ui.visuals())),
                );
                for l in &lines.layers {
                    let mut visible = l.visible;
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut visible, &l.text).changed() {
                            toggles.push((l.name.clone(), false));
                        }
                        let mut locked = l.locked;
                        if ui.checkbox(&mut locked, Msg::PlLocked.tr(lang)).changed() {
                            toggles.push((l.name.clone(), true));
                        }
                    });
                }
            }
            if !lines.issues.is_empty() {
                ui.label(egui::RichText::new(Msg::PlIssues.tr(lang)).color(ACCENT));
                for l in &lines.issues {
                    ui.label(egui::RichText::new(&l.text).color(tone(l.tone)));
                }
            } else if has_sheet {
                ui.label(egui::RichText::new(Msg::PlClean.tr(lang)).color(OK));
            }
            if let Some(eval) = &self.plans.eval {
                let width = ui.available_width().max(120.0);
                let scale = width / eval.scene.width_um as f32;
                let size = Vec2::new(width, eval.scene.height_um as f32 * scale);
                let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
                let rect = response.rect;
                // a dimension line is taken with the pointer and dragged; the distance is worked
                // out by `PlanState`
                let to_sheet = |p: Pos2| -> (i64, i64) {
                    (
                        ((p.x - rect.left()) / scale) as i64,
                        ((rect.bottom() - p.y) / scale) as i64,
                    )
                };
                if let Some(p) = response.interact_pointer_pos() {
                    if response.drag_started() {
                        pointer_events.push((0, to_sheet(p)));
                    } else if response.dragged() {
                        pointer_events.push((1, to_sheet(p)));
                    }
                }
                if response.drag_stopped() {
                    pointer_events.push((2, (0, 0)));
                }
                painter.rect_filled(rect, 0.0, Color32::WHITE);
                let at = |p: (i64, i64)| {
                    Pos2::new(
                        rect.left() + p.0 as f32 * scale,
                        rect.bottom() - p.1 as f32 * scale,
                    )
                };
                for prim in &eval.scene.prims {
                    let Some((hex, _, _)) = eval.scene.layers.get(&prim.layer) else {
                        continue;
                    };
                    let rgb = u32::from_str_radix(hex, 16).unwrap_or(0);
                    let colour = Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
                    let stroke = Stroke::new(1.0, colour);
                    match &prim.shape {
                        PlanShape::Line { a, b } => {
                            painter.line_segment([at(*a), at(*b)], stroke);
                        }
                        PlanShape::Circle { c, r } => {
                            painter.circle_stroke(at(*c), *r as f32 * scale, stroke);
                        }
                        PlanShape::Arc { c, r, a0, a1 } => {
                            let steps = 24;
                            let points: Vec<Pos2> = (0..=steps)
                                .map(|k| {
                                    let a = (a0 + (a1 - a0) * f64::from(k) / f64::from(steps))
                                        .to_radians();
                                    at((
                                        c.0 + (*r as f64 * a.cos()) as i64,
                                        c.1 + (*r as f64 * a.sin()) as i64,
                                    ))
                                })
                                .collect();
                            painter.add(egui::Shape::line(points, stroke));
                        }
                        PlanShape::Text {
                            at: p,
                            h,
                            text,
                            anchor,
                            ..
                        } => {
                            let align = if *anchor == PlanAnchor::Middle {
                                egui::Align2::CENTER_BOTTOM
                            } else {
                                egui::Align2::LEFT_BOTTOM
                            };
                            let size = (*h as f32 * scale).max(5.0);
                            painter.text(
                                at(*p),
                                align,
                                text,
                                egui::FontId::proportional(size),
                                colour,
                            );
                        }
                    }
                }
            }
        });
        // what was asked is done after the lines are drawn
        for (kind, point) in pointer_events {
            let r = match kind {
                0 => {
                    self.plans.begin_drag(point);
                    Ok(None)
                }
                1 => self.plans.drag_to(point, lang),
                _ => {
                    self.plans.end_drag();
                    Ok(None)
                }
            };
            if let Err(e) = r {
                self.plans.note = Some(PlanState::error_line(&e, lang));
            }
        }
        for (name, lock) in toggles {
            let r = if lock {
                self.plans.toggle_locked(&name, lang)
            } else {
                self.plans.toggle_visible(&name, lang)
            };
            if let Err(e) = r {
                self.plans.note = Some(PlanState::error_line(&e, lang));
            }
        }
        if let Some(fa) = form_action {
            match self.plans_form.apply(&mut self.plans, fa, lang) {
                Ok(l) => self.plans.note = Some(l),
                Err(e) => self.plans.note = Some(PlanState::error_line(&e, lang)),
            }
        }
        let result = match action {
            Some("create") => self.plans.create_sheet(lang),
            Some("refresh") => self.plans.refresh(lang),
            Some("save") => self.plans.save(lang),
            Some("svg") => match self.win.path.clone() {
                Some(path) => self
                    .plans
                    .export_svg(&path.with_extension("svg"), false, lang),
                None => Ok(()),
            },
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.plans.note = Some(PlanState::error_line(&e, lang));
        }
    }

    /// The sheet of variables as a panel of its own.
    fn panel_variables(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.sheet_body(ui);
        });
    }

    /// The fit report as a panel: the text can be selected and copied.
    fn panel_report(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        let text = self.win.report_text();
        let mut copy = false;
        let mut write = false;
        ui.horizontal(|ui| {
            if ui.button(Msg::ReportCopy.tr(lang)).clicked() {
                copy = true;
            }
            if ui.button(Msg::ExportReport.tr(lang)).clicked() {
                write = true;
            }
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            // a text that is shown and can be selected but not changed
            let mut shown: &str = text.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut shown)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
        });
        if copy {
            ui.ctx().copy_text(text);
            self.win.report_copied();
        } else if write {
            self.export_dialog(Format::Report);
        }
    }

    /// Does what a command of the menus says.
    fn run_cmd(&mut self, cmd: Cmd, ctx: &egui::Context) {
        let sketch = self.workspace == StartView::Sketch;
        match cmd {
            Cmd::Std(StdAction::New) => {
                if sketch {
                    self.sketch.new_sketch();
                } else {
                    self.win.new_project();
                }
            }
            Cmd::Std(StdAction::Open) => {
                if sketch {
                    self.open_sketch_dialog();
                } else {
                    self.open_dialog();
                }
            }
            Cmd::Std(StdAction::Save) => {
                if sketch {
                    self.save_sketch_or_ask();
                } else if !self.win.read_only {
                    self.save_or_ask();
                }
            }
            Cmd::Std(StdAction::SaveAs) => {
                if sketch {
                    self.save_sketch_dialog();
                } else if !self.win.read_only {
                    self.save_dialog();
                }
            }
            Cmd::Std(StdAction::Undo) => {
                if sketch {
                    self.sketch.undo();
                } else {
                    self.win.undo();
                }
            }
            Cmd::Std(StdAction::Redo) => {
                if sketch {
                    self.sketch.redo();
                } else {
                    self.win.redo();
                }
            }
            Cmd::Std(StdAction::Quit) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Cmd::Std(_) => {}
            Cmd::OpenLibrary => self.open_library_dialog(),
            Cmd::ImportDxf => self.panels.open = Some(Panel::ImportDxf),
            Cmd::SuiteOpen => self.suite_dialog(SuiteAsk::Open),
            Cmd::SuiteSave => match suitewin::save(&mut self.win) {
                Saved::NeedsName => self.suite_dialog(SuiteAsk::SaveAs),
                Saved::Written(written) => {
                    if let Some(path) = self.win.path.as_ref().filter(|_| written) {
                        self.recent_added.push(path.display().to_string());
                    }
                    self.refresh_branches();
                }
            },
            Cmd::SuiteSaveAs => self.suite_dialog(SuiteAsk::SaveAs),
            Cmd::SuiteImport => self.suite_dialog(SuiteAsk::Import),
            Cmd::SuiteBranches => self.branches = Some(self.read_branches()),
            Cmd::SuiteTexts => self.open_texts(),
            Cmd::SuiteSign => self.open_signing(),
            Cmd::ViewFile => self.suite_dialog(SuiteAsk::ViewFile),
            Cmd::SuiteBoard => self.suite_board(),
            Cmd::SuiteCheck => self.suite_work(BoardWork::CheckFit, Msg::SuiteMenuCheck),
            Cmd::SuiteOpenings => {
                self.suite_work(BoardWork::CutOpenings, Msg::SuiteMenuOpenings);
            }
            Cmd::SuiteHandBack => {
                self.suite_work(BoardWork::HandBack, Msg::SuiteMenuHandBack);
            }
            Cmd::CheckSketch => self.check_sketch(),
            Cmd::Variants => self.open_variants(),
            Cmd::Export(format) => self.export_dialog(format),
            Cmd::FitReport => self.report_open = true,
            Cmd::EditModel => {
                self.workspace = StartView::Model;
                for p in [
                    dockui::id::VIEW3D,
                    dockui::id::MODEL_TREE,
                    dockui::id::PROPERTIES,
                ] {
                    self.pending_dock.push(DockAction::ShowPanel(p.to_string()));
                    self.pending_active.push(p);
                }
            }
            Cmd::EditSketch => {
                self.workspace = StartView::Sketch;
                for p in [
                    dockui::id::SKETCH,
                    dockui::id::SKETCH_TOOLS,
                    dockui::id::CONSTRAINTS,
                    dockui::id::DIMENSIONS,
                ] {
                    self.pending_dock.push(DockAction::ShowPanel(p.to_string()));
                    self.pending_active.push(p);
                }
            }
            Cmd::KeepSketch => self.keep_sketch(),
            Cmd::ShowDrawings => {
                self.pending_dock
                    .push(DockAction::ShowPanel(dockui::id::DRAWING.to_string()));
                self.pending_active.push(dockui::id::DRAWING);
            }
            Cmd::FitView => {
                if sketch {
                    self.sketch.fit();
                } else {
                    self.win.control_mut(ViewKind::Iso).reset();
                    self.win.control_mut(ViewKind::Plan).reset();
                }
            }
            Cmd::Report(panel) => self.panels.open = Some(panel),
            Cmd::Language(lang) => self.win.lang = lang,
        }
    }

    /// Keeps the sketch in the project under the name typed.
    fn keep_sketch(&mut self) {
        if self.win.read_only {
            return;
        }
        if let Some(kept) = self.sketch.sketch_for_project() {
            self.win.keep_sketch(kept);
            self.sketch.mark_kept();
        }
    }

    /// Keeps the arrangement of the panels in the settings file, when the person gave one (a
    /// picture keeps nothing).
    fn save_layout(&mut self, pairs: &[(String, String)]) {
        if self.shot.is_some() {
            return;
        }
        let Some(path) = self.config_path.clone() else {
            return;
        };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let merged = dockui::with_dock_section(&text, pairs);
        if let Err(e) = std::fs::write(&path, merged) {
            self.win.status = Status::Error(CadError::Io(format!("{}: {e}", path.display())));
        }
    }
}

/// The window: the frame of the suite with the panels of the program in it and the menu bar above.
struct Shell {
    app: Rc<RefCell<App>>,
    dock: Dock,
    bar: MenuBar<Cmd>,
    watch: LayoutWatch,
    /// How many frames were drawn, so the arrangement is looked at a few times a second and not at
    /// every frame.
    ticks: u32,
}

/// How a panel of the window is drawn: with the program and the room the frame gives it.
type PanelDraw = fn(&mut App, &mut egui::Ui);

/// How often the arrangement of the panels is compared with what was written, in frames.
const LAYOUT_CHECK_EVERY: u32 = 20;

impl Shell {
    /// `window`: the workspace the window opens in (`--window`), `settings`: the text of the
    /// settings file that holds the arrangement.
    fn new(mut app: App, window: Option<&str>, settings: &str, config: Option<PathBuf>) -> Shell {
        let lang = app.win.lang;
        let start_sketch = app.workspace == StartView::Sketch;
        app.config_path = config;
        let app = Rc::new(RefCell::new(app));
        let mut dock = Dock::new(dockui::defs(), dockui::workspaces());
        let draw: [(&str, PanelDraw); 13] = [
            (dockui::id::VIEW3D, App::panel_view3d),
            (dockui::id::MODEL_TREE, App::panel_model_tree),
            (dockui::id::PROPERTIES, App::panel_properties),
            (dockui::id::TASKS, App::panel_tasks),
            (dockui::id::REPORT, App::panel_report),
            (dockui::id::CONSOLE, App::panel_console),
            (dockui::id::VARIABLES, App::panel_variables),
            (dockui::id::SKETCH, App::panel_sketch),
            (dockui::id::SKETCH_TOOLS, App::panel_sketch_tools),
            (dockui::id::CONSTRAINTS, App::panel_constraints),
            (dockui::id::DIMENSIONS, App::panel_dimensions),
            (dockui::id::DRAWING, App::panel_drawing),
            (dockui::id::ATTACHMENTS, App::panel_attachments),
        ];
        for (name, f) in draw {
            let a = app.clone();
            dock.register(name, move |ui| f(&mut a.borrow_mut(), ui));
        }
        dock.load_config(&dockui::dock_text_of(settings));
        dock.set_language(dockui::view_lang(lang));
        if let Some(w) = window {
            let _ = dock.apply(DockAction::SwitchWorkspace(w.to_string()));
        }
        if start_sketch {
            for p in [dockui::id::SKETCH, dockui::id::SKETCH_TOOLS] {
                let _ = dock.apply(DockAction::ShowPanel(p.to_string()));
                let _ = dock.core_mut().layout_mut().set_active(p);
            }
        }
        let watch = LayoutWatch::new(&dock.core().config_pairs());
        Shell {
            app,
            dock,
            bar: dockui::menu_bar(),
            watch,
            ticks: 0,
        }
    }

    /// What the program says about itself in the window «About».
    fn about() -> AboutInfo {
        AboutInfo {
            app_name: "EH3D-PHOENIX-CAD".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            license: "GPL-3.0-or-later".to_string(),
            extra: Vec::new(),
        }
    }

    /// Does what was chosen in the menus and what the menus asked of the frame, and keeps the
    /// list of recent files.
    fn after(&mut self, chosen: Option<MenuChosen<Cmd>>, ctx: &egui::Context) {
        let mut app = self.app.borrow_mut();
        match chosen {
            Some(MenuChosen::App(cmd)) => app.run_cmd(cmd, ctx),
            Some(MenuChosen::Recent(path)) => {
                let path = PathBuf::from(path);
                let opened = if crate::shot::is_sketch_file(&path) {
                    app.fit_next = app.sketch.open_file(&path);
                    app.workspace = StartView::Sketch;
                    app.pending_active.push(dockui::id::SKETCH);
                    app.fit_next
                } else {
                    suitewin::open_any(&mut app.win, &path)
                };
                if opened {
                    app.recent_added.push(path.display().to_string());
                } else if !path.exists() {
                    // a file that is gone is taken out of the list
                    let gone = path.display().to_string();
                    self.dock.core_mut().recent_mut().remove(&gone);
                }
                app.refresh_branches();
            }
            Some(MenuChosen::Dock(_)) | None => {}
        }
        for action in std::mem::take(&mut app.pending_dock) {
            let _ = self.dock.apply(action);
        }
        for id in std::mem::take(&mut app.pending_active) {
            let _ = self.dock.core_mut().layout_mut().set_active(id);
        }
        for path in std::mem::take(&mut app.recent_added) {
            self.dock.core_mut().recent_mut().push(&path);
        }
        app.recent_suite = suitewin::recent_projects(self.dock.core().recent().items());
    }
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (lang, facts) = {
            let mut app = self.app.borrow_mut();
            app.begin_frame(&ctx);
            (app.win.lang, app.menu_facts())
        };
        self.dock.set_language(dockui::view_lang(lang));
        self.dock.set_about(Shell::about());
        self.dock.set_brand(dockui::about_brand());
        self.dock
            .set_menu_state(dockui::disabled_entries(&facts), dockui::checks(lang));
        // the menu bar, then the name of the file, then the panels
        let mut chosen = None;
        egui::Panel::top("menu").show(ui, |ui| {
            chosen = self.dock.menu_bar(ui, &self.bar);
        });
        self.app.borrow_mut().title_strip(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.dock.show(ui));
        self.after(chosen, &ctx);
        self.app.borrow_mut().finish_frame(&ctx);
        // the arrangement is kept in the settings file when it changed
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks.is_multiple_of(LAYOUT_CHECK_EVERY) {
            let pairs = self.dock.core().config_pairs();
            if self.watch.changed(&pairs) {
                self.app.borrow_mut().save_layout(&pairs);
            }
        }
    }
}

/// The icon of the program for the window, from the file of the brand.
fn window_icon() -> Option<egui::IconData> {
    let bytes: &[u8] = include_bytes!("../../../images/brand/icon-256.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

/// The fonts of the system that draw Japanese and Chinese are added to the ones of egui, which
/// have no such letters.
fn install_fonts(ctx: &egui::Context) {
    let candidates = [
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/YuGothM.ttc",
        "C:/Windows/Fonts/meiryo.ttc",
        "C:/Windows/Fonts/simsun.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/PingFang.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = FontDefinitions::default();
            fonts
                .font_data
                .insert("cjk".into(), Arc::new(FontData::from_owned(bytes)));
            for family in [FontFamily::Monospace, FontFamily::Proportional] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

/// The colours of the theme Forge for the controls of egui.
fn apply_theme(ctx: &egui::Context, theme: ShotTheme) {
    ctx.set_visuals(theme_visuals(theme));
}

/// The colour of the titles and highlighted names of the window: the one of the program (ice
/// blue), in the tone that reads on every ground of these visuals, light or dark. Amber is left to
/// warnings.
fn title_colour(visuals: &egui::Visuals) -> Color32 {
    let [r, g, b] = eh3d_dock::gui::accents_of(Phoenix::Cad, visuals).link;
    Color32::from_rgb(r, g, b)
}

/// [`title_colour`] for the titles painted on the canvas of the drawings, which keeps its dark
/// ground in every theme.
fn canvas_title_colour() -> Color32 {
    let ground = [BACKGROUND.r(), BACKGROUND.g(), BACKGROUND.b()];
    let [r, g, b] = palette::readable(Phoenix::Cad.tones().main, ground);
    Color32::from_rgb(r, g, b)
}

fn theme_visuals(theme: ShotTheme) -> egui::Visuals {
    match theme {
        ShotTheme::Forge => {}
        ShotTheme::Dark => return egui::Visuals::dark(),
        ShotTheme::Light => return egui::Visuals::light(),
    }
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = RAISED;
    v.extreme_bg_color = Color32::from_rgb(0x14, 0x11, 0x0e);
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(0xff, 0xb0, 0x00, 110);
    v.selection.stroke = Stroke::new(1.5, ACCENT);
    v.widgets.inactive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xff, 0xb0, 0x00, 110));
    v.widgets.inactive.weak_bg_fill = RAISED;
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, ACCENT);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x33, 0x28, 0x1c);
    v.widgets.active.bg_stroke = Stroke::new(2.0, ACCENT);
    v.widgets.inactive.corner_radius = 6.0.into();
    v.widgets.hovered.corner_radius = 6.0.into();
    v.widgets.active.corner_radius = 6.0.into();
    v
}

/// What the window needs to start.
pub struct Start {
    /// The language of the texts.
    pub lang: Lang,
    /// The settings of the library.
    pub settings: Settings,
    /// The settings of the window.
    pub ui: UiSettings,
    /// The settings of the solver of the sketch.
    pub solver: SketchSettings,
    /// The settings of the sketch window.
    pub sketch_ui: SketchUiSettings,
    /// A project to open at once.
    pub project: Option<PathBuf>,
    /// A sketch file to open at once.
    pub sketch: Option<PathBuf>,
    /// The part of the window that shows first.
    pub view: StartView,
    /// The workspace of the frame that the window opens in; none takes the one of the settings
    /// file.
    pub window: Option<&'static str>,
    /// The settings file given with `--config`, where the arrangement of the panels is kept.
    pub config: Option<PathBuf>,
    /// A 3D file or a flat drawing of another program to look at as the window opens.
    pub viewer: Option<PathBuf>,
}

fn make_window(start: Start) -> Result<(Window, SketchEditor, StartView), String> {
    let mut sketch = SketchEditor::new(
        start.lang,
        start.settings.clone(),
        start.solver.clone(),
        start.sketch_ui,
    );
    let mut win = Window::new(start.lang, start.settings, start.ui).map_err(|e| e.to_string())?;
    win.sketch_settings = start.solver;
    if let Some(path) = start.project {
        win.open(&path);
    }
    sketch.set_variables(&win.mech.vars);
    if let Some(path) = &start.sketch {
        sketch.open_file(path);
    } else if let Some(first) = win.mech.sketches.first() {
        // the sketches of a project are in its file; the one next to it is only an import
        sketch.load_from_project(first.clone());
    } else if start.view == StartView::Sketch {
        // the sketch of a project is the file next to it
        let next_to = win.path.as_deref().map(sketch_file::sibling_path);
        if let Some(path) = next_to.filter(|p| p.is_file()) {
            sketch.open_file(&path);
        }
    }
    Ok((win, sketch, start.view))
}

/// Opens the window. A project given here is opened at once.
pub fn run(start: Start) -> Result<(), String> {
    let (size, minimum) = (start.ui.window_size, start.ui.window_min);
    let (window, config) = (start.window, start.config.clone());
    let viewer = start.viewer.clone();
    let settings = config
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let (win, sketch, view) = make_window(start)?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([size.0 as f32, size.1 as f32])
        .with_min_inner_size([minimum.0 as f32, minimum.1 as f32])
        .with_title("EH3D-PHOENIX-CAD");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "EH3D-PHOENIX-CAD",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx, ShotTheme::Forge);
            let mut app = App::new(win, sketch, view);
            if let Some(file) = viewer {
                app.open_viewer(crate::modelview::Origin::File(file));
            }
            let mut shell = Shell::new(app, window, &settings, config);
            // the list of recent projects lives in the folder of the user, with or without `--config`
            shell.dock.keep_user_recent(Phoenix::Cad);
            Ok(Box::new(shell))
        }),
    )
    .map_err(|e| e.to_string())
}

/// How the picture mode ended: nothing yet, the file was written, or the reason it was not.
type ShotOutcome = Arc<Mutex<Option<Result<(), String>>>>;

/// The picture being taken.
struct ShotState {
    request: ShotRequest,
    frame: u32,
    outcome: ShotOutcome,
}

impl App {
    /// Counts the frames, asks for the picture after the frames waited, writes it and closes the
    /// window when it is written or when it does not come.
    fn shot_tick(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else {
            return;
        };
        shot.frame += 1;
        ctx.request_repaint();
        let due = 3 + shot.request.wait;
        let finished = match shot.outcome.lock() {
            Ok(slot) => slot.is_some(),
            Err(_) => true,
        };
        if finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if shot.frame == due {
            let (output, replace) = (shot.request.output.clone(), shot.request.replace);
            let (outcome, repaint) = (shot.outcome.clone(), ctx.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |image| {
                    let result = save_shot_png(&image, &output, replace);
                    if let Ok(mut slot) = outcome.lock() {
                        *slot = Some(result);
                    }
                    repaint.request_repaint();
                }),
            ));
        } else if shot.frame > due + PATIENCE_FRAMES {
            if let Ok(mut slot) = shot.outcome.lock() {
                *slot = Some(Err(
                    "the picture did not come back from the window".to_owned()
                ));
            }
        }
    }
}

/// Writes the picture the window gave back as a PNG file, a new file unless `replace` is true.
fn save_shot_png(image: &egui::ColorImage, path: &Path, replace: bool) -> Result<(), String> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true);
    if replace {
        open.create(true).truncate(true);
    } else {
        open.create_new(true);
    }
    let file = open
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        image.size[0] as u32,
        image.size[1] as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let data: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    writer.write_image_data(&data).map_err(|e| e.to_string())?;
    // the end of the file is written here, so that a failure is reported and not lost in a drop
    writer.finish().map_err(|e| e.to_string())
}

/// Opens the window at the size asked, shows the project, saves the picture as the request says
/// and closes by itself. The window keeps no settings. The answer is the reason when the picture
/// could not be written.
pub fn run_shot(start: Start, request: ShotRequest) -> Result<(), String> {
    if !request.replace && request.output.exists() {
        return Err(format!(
            "{} exists; give --replace to write over it",
            request.output.display()
        ));
    }
    let folder = request
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    if folder.is_some_and(|dir| !dir.is_dir()) {
        return Err(format!(
            "the folder of {} does not exist",
            request.output.display()
        ));
    }
    let missing = start
        .project
        .iter()
        .chain(start.sketch.iter())
        .chain(start.viewer.iter())
        .find(|f| !f.is_file());
    if let Some(missing) = missing {
        return Err(format!("{} is not a file", missing.display()));
    }
    let (window, config) = (start.window, start.config.clone());
    let viewer = start.viewer.clone();
    let settings = config
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let (win, sketch, view) = make_window(start)?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([request.size.0 as f32, request.size.1 as f32])
        .with_title("EH3D-PHOENIX-CAD");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let outcome: ShotOutcome = Arc::new(Mutex::new(None));
    let outcome_for_app = outcome.clone();
    let theme = request.theme;
    eframe::run_native(
        "EH3D-PHOENIX-CAD",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx, theme);
            let mut app = App::new(win, sketch, view);
            if let Some(file) = viewer {
                app.open_viewer(crate::modelview::Origin::File(file));
                app.settle_viewer();
            }
            app.shot = Some(ShotState {
                request,
                frame: 0,
                outcome: outcome_for_app,
            });
            Ok(Box::new(Shell::new(app, window, &settings, config)))
        }),
    )
    .map_err(|e| e.to_string())?;
    let taken = outcome.lock().map_err(|e| e.to_string())?.take();
    match taken {
        Some(result) => result,
        None => Err("the window closed before the picture was taken".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_of_the_attachments_follows_the_suite_file_and_the_settings_of_the_window() {
        let mut ui_settings = UiSettings::defaults();
        assert!(ui_settings.attachments.apply("attachments.max_mb", "16"));
        let win = Window::new(Lang::En, crate::config::Settings::defaults(), ui_settings).unwrap();
        let sketch = SketchEditor::new(
            Lang::En,
            crate::config::Settings::defaults(),
            SketchSettings::defaults(),
            SketchUiSettings::defaults(),
        );
        let mut app = App::new(win, sketch, StartView::Model);
        let folder =
            std::env::temp_dir().join(format!("phoenix-cad-attach-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("box.eh3d");
        let _ = std::fs::remove_file(&file);
        assert!(suitewin::save_as(&mut app.win, &file, false));
        let ctx = egui::Context::default();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            app.begin_frame(&ctx);
            app.panel_attachments(ui);
        });
        out.drop_without_applying_deltas();
        assert_eq!(app.attachments.model.project(), Some(file.as_path()));
        assert_eq!(
            app.attachments.limits().attachments.max_attachment_bytes,
            16 << 20
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_name_typed_to_save_gets_the_extension_of_the_projects_or_of_the_sketches() {
        let with_extension = |name: &str| with_extension_of(name, &PROJECT_EXTENSIONS);
        assert_eq!(with_extension("box"), "box.phoenixproj");
        assert_eq!(with_extension(" box.phoenixproj "), "box.phoenixproj");
        assert_eq!(with_extension("BOX.PHOENIXPROJ"), "BOX.PHOENIXPROJ");
        assert_eq!(with_extension("box.txt"), "box.txt.phoenixproj");
        let sketch = |name: &str| with_extension_of(name, &SKETCH_EXTENSIONS);
        assert_eq!(sketch("plate"), "plate.phoenixsketch");
        assert_eq!(sketch("plate.phoenixsketch"), "plate.phoenixsketch");
        assert_eq!(
            sketch("plate.phoenixproj"),
            "plate.phoenixproj.phoenixsketch"
        );
    }

    #[test]
    fn every_kind_of_stroke_of_the_sketch_has_a_colour_and_a_width() {
        use SketchStroke::*;
        for stroke in [
            GridMinor,
            GridMajor,
            Axis,
            Normal,
            Construction,
            Selected,
            Related,
            Conflict,
            Redundant,
            Preview,
            Dimension,
            Marker,
        ] {
            let (_, width) = sketch_style(stroke);
            assert!(width > 0.0, "{stroke:?}");
        }
        assert_ne!(tone_colour(Tone::Green), tone_colour(Tone::Red));
        assert_ne!(tone_colour(Tone::Amber), tone_colour(Tone::Red));
    }

    #[test]
    fn titles_take_the_colour_of_the_program_and_read_in_every_theme() {
        let rgb = |c: Color32| [c.r(), c.g(), c.b()];
        for theme in [ShotTheme::Forge, ShotTheme::Dark, ShotTheme::Light] {
            let v = theme_visuals(theme);
            let title = title_colour(&v);
            assert_ne!(title, ACCENT);
            assert!(title.b() > title.r(), "{theme:?}: not a blue");
            for ground in [v.panel_fill, v.window_fill, v.extreme_bg_color] {
                let contrast = palette::contrast(rgb(title), rgb(ground));
                assert!(contrast >= palette::TEXT_CONTRAST, "{theme:?}: {contrast}");
            }
        }
        let canvas = canvas_title_colour();
        assert!(palette::contrast(rgb(canvas), rgb(BACKGROUND)) >= palette::TEXT_CONTRAST);
        // what asks for attention stays amber
        assert_eq!(tone_colour(Tone::Amber), ACCENT);
    }

    #[test]
    fn the_selection_is_drawn_last() {
        let kinds = [
            Kind::LidPlane,
            Kind::Cavity,
            Kind::Outer,
            Kind::Board,
            Kind::Post,
            Kind::Hole,
            Kind::Screw,
            Kind::Conflict,
            Kind::Selected,
        ];
        for (k, kind) in kinds.into_iter().enumerate() {
            assert_eq!(usize::from(rank(kind)), k);
        }
    }
}
