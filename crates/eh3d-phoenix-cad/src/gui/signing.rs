// SPDX-License-Identifier: GPL-3.0-or-later
//! The window that signs the project of the suite: a secret key file chosen each time (or made
//! there), what is signed, and what was done. The signing itself is [`crate::signing`].

use super::*;
use crate::signing::{self, Request};

/// The name offered for a new secret key.
pub(super) const NEW_KEY_NAME: &str = "eh3d-signing.key";

/// The window, while it is open.
pub(super) struct SignForm {
    /// The secret key file, as typed or chosen. It is not kept after the window is closed.
    key: String,
    branch: bool,
    project: bool,
    /// What the last thing done said, and whether it went well.
    lines: Vec<(String, bool)>,
}

impl App {
    /// Opens the window.
    pub(super) fn open_signing(&mut self) {
        self.signing = Some(SignForm {
            key: String::new(),
            branch: true,
            project: false,
            lines: Vec::new(),
        });
    }

    /// The secret key chosen in a dialog.
    pub(super) fn take_sign_key(&mut self, path: &Path) -> bool {
        if let Some(form) = &mut self.signing {
            form.key = path.display().to_string();
            form.lines.clear();
        }
        true
    }

    /// Makes a new key at the name chosen in a dialog.
    pub(super) fn make_sign_key(&mut self, path: &Path) -> bool {
        let lang = self.win.lang;
        let Some(form) = &mut self.signing else {
            return true;
        };
        match signing::new_key(path) {
            Ok(pair) => {
                let words = fill(
                    Msg::SignKeyMade.tr(lang),
                    &[
                        &pair.key_id,
                        &pair.secret_path.display().to_string(),
                        &pair.public_path.display().to_string(),
                    ],
                );
                form.key = pair.secret_path.display().to_string();
                form.lines = vec![(words, true)];
            }
            Err(e) => form.lines = vec![(error_text(&e, lang), false)],
        }
        true
    }

    /// Signs what the window says with its key, and lists the branches again with the public key
    /// beside the secret one, so that the new signature is seen as checked.
    pub(super) fn sign_now(&mut self) {
        let lang = self.win.lang;
        let file = suitewin::current_file(&self.win);
        let Some(form) = &mut self.signing else {
            return;
        };
        let Some(file) = file else {
            form.lines = vec![(Msg::SuiteNeedFile.tr(lang).to_owned(), false)];
            return;
        };
        let request = Request {
            branches: if form.branch {
                Request::own().branches
            } else {
                Vec::new()
            },
            whole: form.project,
        };
        if request.branches.is_empty() && !request.whole {
            form.lines = vec![(Msg::SignNeedsChoice.tr(lang).to_owned(), false)];
            return;
        }
        if self.win.dirty {
            form.lines = vec![(Msg::SignSaveFirst.tr(lang).to_owned(), false)];
            return;
        }
        let key = PathBuf::from(form.key.trim());
        match signing::sign(&file, &key, &request) {
            Ok(done) => {
                form.lines = done
                    .branches
                    .iter()
                    .map(|b| fill(Msg::SignedBranchWith.tr(lang), &[b, &done.key_id]))
                    .chain(
                        done.whole
                            .then(|| fill(Msg::SignedProjectWith.tr(lang), &[&done.key_id])),
                    )
                    .map(|words| (words, true))
                    .collect();
                let public = eh3d_project::ed25519::public_key_path(&key);
                let public_text = public.display().to_string();
                if public.is_file() && !self.chosen_keys.contains(&public_text) {
                    self.chosen_keys.push(public_text);
                }
            }
            Err(e) => form.lines = vec![(error_text(&e, lang), false)],
        }
        self.refresh_branches();
    }

    /// The window, while it is open.
    pub(super) fn signing_window(&mut self, ctx: &egui::Context) {
        let lang = self.win.lang;
        let dirty = self.win.dirty;
        let Some(form) = &mut self.signing else {
            return;
        };
        let mut open = true;
        let mut choose = false;
        let mut make = false;
        let mut sign = false;
        egui::Window::new(Msg::SignMenu.tr(lang))
            .id(egui::Id::unique("suite-sign"))
            .open(&mut open)
            .default_size([560.0, 300.0])
            .show(ctx, |ui| {
                ui.label(Msg::SignKeyFile.tr(lang));
                ui.add(egui::TextEdit::singleline(&mut form.key).desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    choose = ui.button(Msg::SignChoose.tr(lang)).clicked();
                    make = ui.button(Msg::SignNewKey.tr(lang)).clicked();
                });
                ui.label(egui::RichText::new(Msg::SignKeyStays.tr(lang)).color(DIM));
                ui.separator();
                ui.checkbox(&mut form.branch, Msg::SignThisBranch.tr(lang));
                ui.checkbox(&mut form.project, Msg::SignWholeProject.tr(lang));
                if dirty {
                    ui.colored_label(ACCENT, Msg::SignSaveFirst.tr(lang));
                }
                let ready = !dirty && !form.key.trim().is_empty() && (form.branch || form.project);
                let button = egui::Button::new(Msg::SignDo.tr(lang));
                sign = ui.add_enabled(ready, button).clicked();
                for (words, good) in &form.lines {
                    ui.colored_label(if *good { OK } else { DANGER }, words.as_str());
                }
            });
        if !open {
            self.signing = None;
            return;
        }
        if choose {
            self.suite_dialog(SuiteAsk::SignKey);
        }
        if make {
            self.suite_dialog(SuiteAsk::NewKey);
        }
        if sign {
            self.sign_now();
        }
    }
}
