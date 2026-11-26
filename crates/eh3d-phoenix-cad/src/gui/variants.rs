// SPDX-License-Identifier: GPL-3.0-or-later
//! The window of the variants: the list with its values, keeping, changing, applying and removing
//! one, checking them all and exporting them all. Everything it does is decided in
//! [`crate::window`]; this only draws it.

use super::*;
use crate::window::format_msg;

/// What is typed and chosen in the window of the variants.
pub(super) struct VariantsForm {
    name: String,
    only: String,
    variable: String,
    value: String,
    folder: String,
    format: Format,
    overwrite: bool,
    /// What the last check or export said, and whether all was in order.
    lines: Vec<(String, bool)>,
}

/// What a button of the window asks for.
enum Ask {
    Keep,
    Set,
    Apply(String),
    Remove(String),
    Check,
    Export,
}

impl App {
    /// Opens the window of the variants, with the folder of the project for the exports.
    pub(super) fn open_variants(&mut self) {
        self.variants = Some(VariantsForm {
            name: String::new(),
            only: String::new(),
            variable: String::new(),
            value: String::new(),
            folder: self.folder().display().to_string(),
            format: self.win.export_format,
            overwrite: false,
            lines: Vec::new(),
        });
    }

    /// The window of the variants, while it is open.
    pub(super) fn variants_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.variants else {
            return;
        };
        let lang = self.win.lang;
        let editable = !self.win.read_only;
        let rows = self.win.variant_rows();
        let mut open = true;
        let mut ask = None;
        egui::Window::new(Msg::VariantsMenu.tr(lang))
            .id(egui::Id::unique("variants"))
            .open(&mut open)
            .default_size([620.0, 420.0])
            .show(ctx, |ui| {
                if rows.is_empty() {
                    ui.label(Msg::VariantNone.tr(lang));
                }
                egui::Grid::new("variant-rows")
                    .striped(true)
                    .show(ui, |ui| {
                        for row in &rows {
                            if ui
                                .selectable_label(form.name == row.name, &row.name)
                                .clicked()
                            {
                                form.name = row.name.clone();
                            }
                            ui.label(&row.values);
                            if row.missing.is_empty() {
                                ui.label("");
                            } else {
                                ui.colored_label(
                                    DANGER,
                                    fill(Msg::VariantMissing.tr(lang), &[&row.missing.join(", ")]),
                                );
                            }
                            let apply = egui::Button::new(Msg::VariantApplyButton.tr(lang));
                            if ui.add_enabled(editable, apply).clicked() {
                                ask = Some(Ask::Apply(row.name.clone()));
                            }
                            let remove = egui::Button::new(Msg::VariantRemoveButton.tr(lang));
                            if ui.add_enabled(editable, remove).clicked() {
                                ask = Some(Ask::Remove(row.name.clone()));
                            }
                            ui.end_row();
                        }
                    });
                ui.separator();
                egui::Grid::new("variant-fields").show(ui, |ui| {
                    for (words, text) in [
                        (Msg::VariantNameField, &mut form.name),
                        (Msg::VariantOnlyField, &mut form.only),
                        (Msg::VariantVariableField, &mut form.variable),
                        (Msg::VariantValueField, &mut form.value),
                    ] {
                        ui.label(words.tr(lang));
                        ui.add(egui::TextEdit::singleline(text).desired_width(240.0));
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    let keep = egui::Button::new(Msg::VariantKeepButton.tr(lang));
                    if ui.add_enabled(editable, keep).clicked() {
                        ask = Some(Ask::Keep);
                    }
                    let set = egui::Button::new(Msg::VariantSetButton.tr(lang));
                    if ui.add_enabled(editable, set).clicked() {
                        ask = Some(Ask::Set);
                    }
                    if ui.button(Msg::VariantCheckButton.tr(lang)).clicked() {
                        ask = Some(Ask::Check);
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(Msg::VariantFolderField.tr(lang));
                    ui.add(egui::TextEdit::singleline(&mut form.folder).desired_width(320.0));
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("variant-format")
                        .selected_text(format_msg(form.format).tr(lang))
                        .show_ui(ui, |ui| {
                            for f in Format::ALL {
                                ui.selectable_value(&mut form.format, f, format_msg(f).tr(lang));
                            }
                        });
                    ui.checkbox(&mut form.overwrite, Msg::ExportOverwrite.tr(lang));
                    if ui.button(Msg::VariantExportButton.tr(lang)).clicked() {
                        ask = Some(Ask::Export);
                    }
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (line, ok) in &form.lines {
                        if *ok {
                            ui.label(line);
                        } else {
                            ui.colored_label(DANGER, line);
                        }
                    }
                });
            });
        match ask {
            Some(Ask::Keep) => {
                self.win.keep_variant(&form.name, &form.only);
            }
            Some(Ask::Set) => {
                self.win
                    .set_variant_value(&form.name, &form.variable, &form.value);
            }
            Some(Ask::Apply(name)) => {
                self.win.apply_variant(&name);
            }
            Some(Ask::Remove(name)) => {
                self.win.remove_variant(&name);
            }
            Some(Ask::Check) => {
                form.lines = self
                    .win
                    .check_variants()
                    .into_iter()
                    .map(|c| {
                        let line = fill(Msg::VariantFailed.tr(lang), &[&c.name, &c.text]);
                        (line, c.ok)
                    })
                    .collect();
            }
            Some(Ask::Export) => {
                let folder = PathBuf::from(form.folder.trim());
                form.lines = match self
                    .win
                    .export_variants(form.format, &folder, form.overwrite)
                {
                    Ok(files) => {
                        let count = files.len().to_string();
                        let mut lines: Vec<(String, bool)> = files
                            .iter()
                            .map(|f| (f.path.display().to_string(), true))
                            .collect();
                        lines.push((
                            fill(
                                Msg::VariantExported.tr(lang),
                                &[&count, &folder.display().to_string()],
                            ),
                            true,
                        ));
                        lines
                    }
                    Err((name, e)) => {
                        let why = error_text(&e, lang);
                        vec![(fill(Msg::VariantFailed.tr(lang), &[&name, &why]), false)]
                    }
                };
            }
            None => {}
        }
        if !open {
            self.variants = None;
        }
    }
}
