//! The window itself (built with the `gui` feature): it draws the state of `window::Window` and
//! passes on what the person does. Everything that can be decided without a screen is decided
//! there, in code that is tested.

use crate::body::StepState;
use crate::browse::{self, Browser};
use crate::config::Settings;
use crate::lang::{Lang, Msg};
use crate::scene::{Kind, Line2};
use crate::shot::{ShotRequest, ShotTheme, PATIENCE_FRAMES};
use crate::uiconfig::UiSettings;
use crate::window::{
    error_text, Input, Mark, NewStep, Selection, ViewKind, Window, PROJECT_EXTENSIONS,
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

enum Outcome {
    Cancel,
    Pick(PathBuf),
}

/// The window that opens a project or asks for the name of the file to write.
struct Dialog {
    browser: Browser,
    /// The name being typed when the window asks for a file to write.
    save: Option<String>,
}

struct App {
    win: Window,
    /// The picture mode, while the window is there only to be photographed.
    shot: Option<ShotState>,
    dialog: Option<Dialog>,
    /// The text being typed in the value of each row of the sheet of variables.
    var_edits: HashMap<String, String>,
    /// The name and the value being typed for a new variable.
    new_name: String,
    new_value: String,
}

impl App {
    fn new(win: Window) -> App {
        App {
            win,
            shot: None,
            dialog: None,
            var_edits: HashMap::new(),
            new_name: String::new(),
            new_value: String::new(),
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
                if ui.button(Msg::New.tr(lang)).clicked() {
                    self.win.new_project();
                }
                if ui.button(Msg::Open.tr(lang)).clicked() {
                    self.open_dialog();
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
        if response.dragged() {
            let d = response.drag_delta();
            match view {
                ViewKind::Iso => self.win.rotate_3d(f64::from(d.x), f64::from(d.y)),
                ViewKind::Plan => self
                    .win
                    .control_mut(ViewKind::Plan)
                    .pan_by(f64::from(d.x), f64::from(d.y)),
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

    fn dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        match show_dialog(ctx, &mut dialog, self.win.lang) {
            None => self.dialog = Some(dialog),
            Some(Outcome::Cancel) => {}
            Some(Outcome::Pick(path)) => {
                let done = if dialog.save.is_some() {
                    self.win.save_to(&path)
                } else {
                    self.win.open(&path)
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
        Kind::Selected => 7,
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
    }
}

/// Draws the window that opens a file (or asks for the name of the file to write) and says what
/// the person chose.
fn show_dialog(ctx: &egui::Context, d: &mut Dialog, lang: Lang) -> Option<Outcome> {
    let mut outcome = None;
    let mut open = true;
    let title = if d.save.is_some() {
        Msg::SaveTitle
    } else {
        Msg::OpenTitle
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
            ui.horizontal(|ui| {
                if let Some(name) = d.save.as_mut() {
                    ui.label(Msg::FileName.tr(lang));
                    ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
                    if ui.button(Msg::Save.tr(lang)).clicked() && !name.trim().is_empty() {
                        outcome = Some(Outcome::Pick(d.browser.dir.join(with_extension(name))));
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

/// A file name typed to save a project: it gets the extension of the projects when it has none.
fn with_extension(name: &str) -> String {
    let name = name.trim();
    if browse::matches_extension(name, &PROJECT_EXTENSIONS) {
        name.to_string()
    } else {
        format!("{name}.{}", PROJECT_EXTENSIONS[0])
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
            self.win.open(p);
        }
        if !ctx.egui_wants_keyboard_input() {
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
        self.status_bar(ui);
        self.model_panel(ui);
        self.properties_panel(ui);
        self.views(ui);
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
    /// A project to open at once.
    pub project: Option<PathBuf>,
}

fn make_window(start: Start) -> Result<Window, String> {
    let mut win = Window::new(start.lang, start.settings, start.ui).map_err(|e| e.to_string())?;
    if let Some(path) = start.project {
        win.open(&path);
    }
    Ok(win)
}

/// Opens the window. A project given here is opened at once.
pub fn run(start: Start) -> Result<(), String> {
    let (size, minimum) = (start.ui.window_size, start.ui.window_min);
    let win = make_window(start)?;
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
            Ok(Box::new(App::new(win)))
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
    if let Some(missing) = start.project.as_ref().filter(|f| !f.is_file()) {
        return Err(format!("{} is not a file", missing.display()));
    }
    let win = make_window(start)?;
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
            let mut app = App::new(win);
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
    fn a_name_typed_to_save_gets_the_extension_of_the_projects() {
        assert_eq!(with_extension("box"), "box.phoenixproj");
        assert_eq!(with_extension(" box.phoenixproj "), "box.phoenixproj");
        assert_eq!(with_extension("BOX.PHOENIXPROJ"), "BOX.PHOENIXPROJ");
        assert_eq!(with_extension("box.txt"), "box.txt.phoenixproj");
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
            Kind::Selected,
        ];
        for (k, kind) in kinds.into_iter().enumerate() {
            assert_eq!(usize::from(rank(kind)), k);
        }
    }
}
