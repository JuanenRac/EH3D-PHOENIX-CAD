//! The window itself (built with the `gui` feature): it draws the state of `window::Window` and
//! passes on what the person does. Everything that can be decided without a screen is decided
//! there, in code that is tested.

use crate::body::StepState;
use crate::browse::{self, Browser};
use crate::catalog;
use crate::config::Settings;
use crate::export::Format;
use crate::lang::{Lang, Msg};
use crate::scene::{Kind, Line2};
use crate::shot::{ShotRequest, ShotTheme, StartView, PATIENCE_FRAMES};
use crate::sketch::file as sketch_file;
use crate::sketch::settings::SketchSettings;
use crate::sketch_ui::{
    ConstraintTool, DimTool, Prim, RowMark, SketchEditor, SketchUiSettings, Stroke as SketchStroke,
    Tone, Tool,
};
use crate::uiconfig::UiSettings;
use crate::window::{
    error_text, format_msg, Input, Mark, MeasureTool, NewStep, Selection, ViewKind, Window,
    PROJECT_EXTENSIONS,
};
use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, Pos2, Rect, Stroke, Vec2,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// the colours of the theme Forge, the one the other programs of the suite start with: amber on charcoal
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

enum Outcome {
    Cancel,
    Pick(PathBuf),
}

/// The window that opens a project or asks for the name of the file to write.
struct Dialog {
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
    /// True from the moment a drag of a hole or a post is cancelled with Esc until the button
    /// is let go, so that the rest of that drag does not turn or move the view.
    ignore_drag: bool,
    /// Whether the window with the text of the fit report is open.
    report_open: bool,
}

impl App {
    fn new(win: Window, sketch: SketchEditor, workspace: StartView) -> App {
        App {
            win,
            fit_next: !sketch.sketch.elements().is_empty(),
            sketch,
            workspace,
            dim_edits: HashMap::new(),
            shot: None,
            dialog: None,
            var_edits: HashMap::new(),
            new_name: String::new(),
            new_value: String::new(),
            ignore_drag: false,
            report_open: false,
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
            browser: Browser::new(self.folder(), &PROJECT_EXTENSIONS),
            save: None,
            sketch: false,
            export: None,
            overwrite: false,
            library: false,
        });
    }

    /// Asks which library of parts to open.
    fn open_library_dialog(&mut self) {
        self.dialog = Some(Dialog {
            browser: Browser::new(self.folder(), &LIBRARY_EXTENSIONS),
            save: None,
            sketch: false,
            export: None,
            overwrite: false,
            library: true,
        });
    }

    /// Asks where to write the model in a format. The box to write over a file that exists
    /// starts as the settings say, and the name suggested follows the project.
    fn export_dialog(&mut self, format: Format) {
        self.win.export_format = format;
        self.dialog = Some(Dialog {
            browser: Browser::new(self.folder(), &[format.extension()]),
            save: Some(self.win.export_name(format)),
            sketch: false,
            export: Some(format),
            overwrite: self.win.export_overwrite,
            library: false,
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
            browser: Browser::new(self.folder(), &PROJECT_EXTENSIONS),
            save: Some(name),
            sketch: false,
            export: None,
            overwrite: false,
            library: false,
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

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        match self.workspace {
            StartView::Model => self.model_top_bar(ui),
            StartView::Sketch => self.sketch_top_bar(ui),
        }
    }

    fn model_top_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("EH3D-PHOENIX-CAD")
                        .strong()
                        .size(18.0)
                        .color(ACCENT),
                );
                ui.separator();
                self.workspace_switch(ui);
                ui.separator();
                if ui.button(Msg::New.tr(lang)).clicked() {
                    self.win.new_project();
                }
                if ui.button(Msg::Open.tr(lang)).clicked() {
                    self.open_dialog();
                }
                if ui.button(Msg::OpenLibrary.tr(lang)).clicked() {
                    self.open_library_dialog();
                }
                let can_save = !self.win.read_only;
                if ui
                    .add_enabled(can_save, egui::Button::new(Msg::Save.tr(lang)))
                    .clicked()
                {
                    self.save_or_ask();
                }
                if ui
                    .add_enabled(can_save, egui::Button::new(Msg::SaveAs.tr(lang)))
                    .clicked()
                {
                    self.save_dialog();
                }
                ui.menu_button(Msg::ExportMenu.tr(lang), |ui| {
                    for format in Format::ALL {
                        if ui.button(format_msg(format).tr(lang)).clicked() {
                            self.export_dialog(format);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button(Msg::ReportShow.tr(lang)).clicked() {
                        self.report_open = true;
                        ui.close();
                    }
                });
                ui.separator();
                if ui
                    .add_enabled(self.win.can_undo(), egui::Button::new(Msg::Undo.tr(lang)))
                    .clicked()
                {
                    self.win.undo();
                }
                if ui
                    .add_enabled(self.win.can_redo(), egui::Button::new(Msg::Redo.tr(lang)))
                    .clicked()
                {
                    self.win.redo();
                }
                if ui.button(Msg::FitView.tr(lang)).clicked() {
                    self.win.control_mut(ViewKind::Iso).reset();
                    self.win.control_mut(ViewKind::Plan).reset();
                }
                ui.separator();
                let name = self
                    .win
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                ui.label(egui::RichText::new(name).color(TEXT));
                if self.win.dirty {
                    ui.label(egui::RichText::new(Msg::Unsaved.tr(lang)).color(ACCENT));
                }
                if self.win.read_only {
                    ui.label(egui::RichText::new(Msg::ReadOnlyBadge.tr(lang)).color(DANGER));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(self.win.lang.name())
                        .show_ui(ui, |ui| {
                            for l in Lang::ALL {
                                ui.selectable_value(&mut self.win.lang, l, l.name());
                            }
                        });
                    ui.label(egui::RichText::new(Msg::Language.tr(lang)).color(DIM));
                });
            });
        });
    }

    fn model_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::left("model")
            .default_size(250.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(Msg::ModelTitle.tr(lang))
                        .strong()
                        .color(ACCENT),
                );
                ui.separator();
                let editable = !self.win.read_only;
                let has_step = self.win.selection.step().is_some();
                let mut add = None;
                let mut remove = false;
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
                });
                if let Some(kind) = add {
                    self.win.add_step(kind);
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
                            let text = egui::RichText::new(format!("{}{suffix}", item.label))
                                .color(colour);
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
            });
    }

    fn properties_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::right("properties")
            .default_size(330.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(Msg::PropertiesTitle.tr(lang))
                        .strong()
                        .color(ACCENT),
                );
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.properties_body(ui);
                });
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
        for row in self.win.sheet() {
            let used = self.win.used_by_text(&row);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(row.name.as_str())
                        .strong()
                        .color(ACCENT),
                );
                ui.label(egui::RichText::new(format!("= {}", row.text)).color(TEXT));
            });
            ui.label(
                egui::RichText::new(format!("{}: {used}", Msg::VariableUsedBy.tr(lang))).color(DIM),
            );
            let mut text = self
                .var_edits
                .remove(&row.name)
                .unwrap_or_else(|| row.text.clone());
            ui.horizontal(|ui| {
                let edit = egui::TextEdit::singleline(&mut text).desired_width(150.0);
                let response = ui.add_enabled(editable, edit);
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

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::bottom("status").show(ui, |ui| {
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
        });
    }

    /// The tools of the views: the two measurements, the grid a dragged hole or post snaps to,
    /// and the readings kept.
    fn measure_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::bottom("measure").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(Msg::MeasureTitle.tr(lang))
                        .strong()
                        .color(ACCENT),
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
        });
    }

    fn views(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let total = ui.available_size();
            let gap = 6.0;
            let first = ((total.x - gap) * 0.62).max(120.0);
            let second = (total.x - gap - first).max(120.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                self.pane(ui, ViewKind::Iso, Vec2::new(first, total.y));
                self.pane(ui, ViewKind::Plan, Vec2::new(second, total.y));
            });
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
            ACCENT,
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
                let done = if let Some(format) = dialog.export {
                    self.win.export_to(format, &path, dialog.overwrite)
                } else if dialog.library {
                    self.win.open_library(&path)
                } else {
                    match (dialog.sketch, dialog.save.is_some()) {
                        (false, true) => self.win.save_to(&path),
                        (false, false) => self.win.open(&path),
                        (true, true) => self.sketch.save_to(&path),
                        (true, false) => {
                            let opened = self.sketch.open_file(&path);
                            self.fit_next = opened;
                            opened
                        }
                    }
                };
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
        _ if d.export.is_some() => Msg::ExportTitle,
        _ if d.library => Msg::OpenLibraryTitle,
        (false, true) => Msg::SaveTitle,
        (false, false) => Msg::OpenTitle,
        (true, true) => Msg::SaveSketchTitle,
        (true, false) => Msg::OpenSketchTitle,
    };
    let export_extension = d.export.map(Format::extension);
    let export_extensions: Vec<&str> = export_extension.into_iter().collect();
    let extensions: &[&str] = if d.export.is_some() {
        &export_extensions
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
            if d.export.is_some() {
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
            browser: Browser::new(self.sketch_folder(), &SKETCH_EXTENSIONS),
            save: None,
            sketch: true,
            export: None,
            overwrite: false,
            library: false,
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
            browser: Browser::new(self.sketch_folder(), &SKETCH_EXTENSIONS),
            save: Some(name),
            sketch: true,
            export: None,
            overwrite: false,
            library: false,
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

    fn workspace_switch(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        ui.selectable_value(
            &mut self.workspace,
            StartView::Model,
            Msg::GroupBox.tr(lang),
        );
        ui.selectable_value(
            &mut self.workspace,
            StartView::Sketch,
            Msg::WorkspaceSketch.tr(lang),
        );
    }

    fn sketch_top_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("EH3D-PHOENIX-CAD")
                        .strong()
                        .size(18.0)
                        .color(ACCENT),
                );
                ui.separator();
                self.workspace_switch(ui);
                ui.separator();
                if ui.button(Msg::New.tr(lang)).clicked() {
                    self.sketch.new_sketch();
                }
                if ui.button(Msg::Open.tr(lang)).clicked() {
                    self.open_sketch_dialog();
                }
                if ui.button(Msg::Save.tr(lang)).clicked() {
                    self.save_sketch_or_ask();
                }
                if ui.button(Msg::SaveAs.tr(lang)).clicked() {
                    self.save_sketch_dialog();
                }
                ui.separator();
                let name =
                    egui::TextEdit::singleline(&mut self.sketch.name_input).desired_width(110.0);
                ui.add(name);
                if ui
                    .add_enabled(
                        !self.win.read_only,
                        egui::Button::new(Msg::KeepSketch.tr(lang)),
                    )
                    .clicked()
                {
                    if let Some(kept) = self.sketch.sketch_for_project() {
                        self.win.keep_sketch(kept);
                        self.sketch.mark_kept();
                    }
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.sketch.can_undo(),
                        egui::Button::new(Msg::Undo.tr(lang)),
                    )
                    .clicked()
                {
                    self.sketch.undo();
                }
                if ui
                    .add_enabled(
                        self.sketch.can_redo(),
                        egui::Button::new(Msg::Redo.tr(lang)),
                    )
                    .clicked()
                {
                    self.sketch.redo();
                }
                if ui.button(Msg::FitView.tr(lang)).clicked() {
                    self.sketch.fit();
                }
                ui.separator();
                let name = self
                    .sketch
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                ui.label(egui::RichText::new(name).color(TEXT));
                if self.sketch.dirty {
                    ui.label(egui::RichText::new(Msg::Unsaved.tr(lang)).color(ACCENT));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(self.win.lang.name())
                        .show_ui(ui, |ui| {
                            for l in Lang::ALL {
                                ui.selectable_value(&mut self.win.lang, l, l.name());
                            }
                        });
                    ui.label(egui::RichText::new(Msg::Language.tr(lang)).color(DIM));
                });
            });
        });
    }

    fn sketch_tools_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::top("sketch_tools").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(Msg::ToolsTitle.tr(lang))
                        .strong()
                        .color(ACCENT),
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
            });
        });
    }

    /// The constraints that tie the geometry, the buttons that add one to the selection and the
    /// elements of the sketch.
    fn sketch_constraints_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::left("sketch_constraints")
            .default_size(280.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(Msg::ConstraintsTitle.tr(lang))
                        .strong()
                        .color(ACCENT),
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
                        .color(ACCENT),
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
                            .color(if row.construction {
                                HOLE
                            } else {
                                TEXT
                            });
                            if ui.selectable_label(row.selected, text).clicked() {
                                chosen = Some(row.id);
                            }
                        }
                    });
                if let Some(id) = chosen {
                    self.sketch.select_element(id, extend);
                }
            });
    }

    /// The dimensions: the value (or variable) to type, the buttons that add one to the selection
    /// and the dimensions of the sketch with their values to edit.
    fn sketch_dimensions_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::right("sketch_dimensions")
            .default_size(300.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(Msg::DimensionsTitle.tr(lang))
                        .strong()
                        .color(ACCENT),
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
                let extend = ui.input(|i| i.modifiers.command);
                let rows: Vec<_> = self
                    .sketch
                    .constraint_rows()
                    .into_iter()
                    .filter(|r| r.value.is_some())
                    .collect();
                let mut edited = None;
                let mut toggled = None;
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
                                let edit =
                                    egui::TextEdit::singleline(&mut text).desired_width(170.0);
                                let response = ui.add_enabled(row.enabled, edit);
                                if response.lost_focus() && text != shown {
                                    edited = Some((row.id, text.clone()));
                                }
                                if !response.has_focus() {
                                    text.clone_from(&shown);
                                }
                                let value = row.value.clone().unwrap_or_default();
                                ui.label(egui::RichText::new(format!("= {value}")).color(DIM));
                            });
                            self.dim_edits.insert(row.id.0, text);
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
                if let Some((id, text)) = edited {
                    self.sketch.edit_dimension(id, &text);
                    self.dim_edits.remove(&id.0);
                }
            });
    }

    /// The indicator of the state of the sketch, the constraints that repeat or contradict the
    /// others with the button that switches them off, and the status line.
    fn sketch_status_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::Panel::bottom("sketch_status").show(ui, |ui| {
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
        });
    }

    /// The canvas: the grid, the axes and the geometry, and what the pointer does on them.
    fn sketch_canvas(&mut self, ui: &mut egui::Ui) {
        let lang = self.win.lang;
        egui::CentralPanel::no_frame().show(ui, |ui| {
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
                ACCENT,
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
        });
    }
}

impl App {
    /// The keys of the sketch: open, save, undo, redo, select all, Escape, Enter and Delete. They
    /// are ignored while a text is being typed.
    fn sketch_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let keys = ctx.input(|i| {
            let command = i.modifiers.command;
            [
                command && i.key_pressed(egui::Key::O),
                command && i.key_pressed(egui::Key::S),
                command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                command
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                command && i.key_pressed(egui::Key::A),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            ]
        });
        let [open, save, undo, redo, all, escape, enter, delete] = keys;
        if open {
            self.open_sketch_dialog();
        }
        if save {
            self.save_sketch_or_ask();
        }
        if undo {
            self.sketch.undo();
        }
        if redo {
            self.sketch.redo();
        }
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // a file dropped on the window is opened
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(p) = dropped.first() {
            if crate::shot::is_sketch_file(p) {
                self.fit_next = self.sketch.open_file(p);
                self.workspace = StartView::Sketch;
            } else {
                self.win.open(p);
            }
        }
        if self.workspace == StartView::Sketch {
            self.sketch.lang = self.win.lang;
            self.sketch.set_variables(&self.win.mech.vars);
            self.sketch_keys(&ctx);
        } else if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                if self.win.dragging() {
                    self.win.drag_cancel();
                    self.ignore_drag = true;
                } else if self.win.measure_tool != MeasureTool::Off {
                    self.win.set_measure_tool(MeasureTool::Off);
                }
            }
            let (open, save, undo, redo) = ctx.input(|i| {
                let command = i.modifiers.command;
                (
                    command && i.key_pressed(egui::Key::O),
                    command && i.key_pressed(egui::Key::S),
                    command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                    command
                        && (i.key_pressed(egui::Key::Y)
                            || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                )
            });
            if open {
                self.open_dialog();
            }
            if save && !self.win.read_only {
                self.save_or_ask();
            }
            if undo {
                self.win.undo();
            }
            if redo {
                self.win.redo();
            }
        }
        self.top_bar(ui);
        match self.workspace {
            StartView::Model => {
                self.status_bar(ui);
                self.measure_panel(ui);
                self.model_panel(ui);
                self.properties_panel(ui);
                self.views(ui);
            }
            StartView::Sketch => {
                self.sketch_tools_bar(ui);
                self.sketch_status_bar(ui);
                self.sketch_constraints_panel(ui);
                self.sketch_dimensions_panel(ui);
                self.sketch_canvas(ui);
            }
        }
        self.report_window(&ctx);
        self.dialog_window(&ctx);
        self.shot_tick(&ctx);
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
    match theme {
        ShotTheme::Forge => {}
        ShotTheme::Dark => {
            ctx.set_visuals(egui::Visuals::dark());
            return;
        }
        ShotTheme::Light => {
            ctx.set_visuals(egui::Visuals::light());
            return;
        }
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
    ctx.set_visuals(v);
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
            Ok(Box::new(App::new(win, sketch, view)))
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
        .find(|f| !f.is_file());
    if let Some(missing) = missing {
        return Err(format!("{} is not a file", missing.display()));
    }
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
            app.shot = Some(ShotState {
                request,
                frame: 0,
                outcome: outcome_for_app,
            });
            Ok(Box::new(app))
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
