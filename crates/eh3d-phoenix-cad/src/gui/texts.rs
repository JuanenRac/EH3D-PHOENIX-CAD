// SPDX-License-Identifier: GPL-3.0-or-later
//! The window of the title and the description of the project of the suite, in each of the eight
//! languages. Reading and writing are decided in [`crate::suitewin`]; this only draws them.

use super::*;

/// What is typed in the window of the texts.
pub(super) struct TextsForm {
    /// The language of the texts shown.
    language: Lang,
    title: String,
    description: String,
    /// What the last reading or writing said, and whether all was in order.
    said: Option<(String, bool)>,
}

impl TextsForm {
    fn read(win: &Window, language: Lang) -> TextsForm {
        let lang = win.lang;
        let read = suitewin::current_file(win)
            .ok_or_else(|| Msg::SuiteNeedFile.tr(lang).to_owned())
            .and_then(|file| {
                suitewin::read_texts(&file, language).map_err(|e| error_text(&e, lang))
            });
        let (title, description, said) = match read {
            Ok((title, description)) => (title, description, None),
            Err(why) => (String::new(), String::new(), Some((why, false))),
        };
        TextsForm {
            language,
            title,
            description,
            said,
        }
    }
}

impl App {
    /// Opens the window of the texts in the language of the program.
    pub(super) fn open_texts(&mut self) {
        self.texts = Some(TextsForm::read(&self.win, self.win.lang));
    }

    /// The window of the title and the description, while it is open.
    pub(super) fn texts_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.texts else {
            return;
        };
        let lang = self.win.lang;
        let editable = !self.win.read_only;
        let mut open = true;
        let mut chosen = None;
        let mut write = false;
        egui::Window::new(Msg::SuiteTextsMenu.tr(lang))
            .id(egui::Id::unique("suite-texts"))
            .open(&mut open)
            .default_size([520.0, 300.0])
            .show(ctx, |ui| {
                egui::ComboBox::from_label(Msg::SuiteLanguageField.tr(lang))
                    .selected_text(form.language.name())
                    .show_ui(ui, |ui| {
                        for l in Lang::ALL {
                            if ui.selectable_label(l == form.language, l.name()).clicked() {
                                chosen = Some(l);
                            }
                        }
                    });
                ui.label(Msg::SuiteTitleField.tr(lang));
                ui.add(egui::TextEdit::singleline(&mut form.title).desired_width(f32::INFINITY));
                ui.label(Msg::SuiteDescriptionField.tr(lang));
                ui.add(
                    egui::TextEdit::multiline(&mut form.description).desired_width(f32::INFINITY),
                );
                ui.label(egui::RichText::new(Msg::SuiteTextsEnglish.tr(lang)).color(DIM));
                let button = egui::Button::new(Msg::SuiteTextsWrite.tr(lang));
                if ui.add_enabled(editable, button).clicked() {
                    write = true;
                }
                if let Some((text, ok)) = &form.said {
                    if *ok {
                        ui.label(text);
                    } else {
                        ui.colored_label(DANGER, text);
                    }
                }
            });
        if let Some(language) = chosen {
            *form = TextsForm::read(&self.win, language);
        }
        let asked = write.then(|| (form.language, form.title.clone(), form.description.clone()));
        if !open {
            self.texts = None;
        }
        if let Some((language, title, description)) = asked {
            let write = Write::Texts {
                language,
                title: title.clone(),
                description: description.clone(),
            };
            if !self.ask_first(write) {
                self.write_texts_now(language, &title, &description);
            }
        }
    }

    /// Writes the title and the description and says how it went in the window of the texts.
    pub(super) fn write_texts_now(&mut self, language: Lang, title: &str, description: &str) {
        let said = suitewin::write_texts(&mut self.win, language, title, description);
        if let Some(form) = &mut self.texts {
            form.said = Some(match said {
                Ok(text) => (text, true),
                Err(why) => (why, false),
            });
        }
        self.refresh_branches();
    }
}
