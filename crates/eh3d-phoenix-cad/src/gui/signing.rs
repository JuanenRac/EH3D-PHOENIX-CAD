// SPDX-License-Identifier: GPL-3.0-or-later
//! The window that signs the project of the suite: a secret key file chosen (the path used last is
//! offered again) or made there, its password, what is signed, the question before a signature of
//! another key is replaced and what was done. The signing itself is [`crate::signing`].
//!
//! A password lives only in the box it is typed in until it is used: it is then taken out, wiped
//! and never written to the status line, a settings file or a message.

use super::*;
use crate::signing::{self, Held, Request};
use eh3d_project::ed25519::{public_key_path, Ed25519Signer};
use eh3d_project::keylock::{MAX_PASSWORD_BYTES, MIN_PASSWORD_CHARS};
use eh3d_project::suitetext::fill as suite_fill;
use eh3d_project::{KeyProtection, Password};

/// The name offered for a new secret key.
pub(super) const NEW_KEY_NAME: &str = "eh3d-signing.key";

/// The boxes of the window where passwords are typed.
const PASSWORD_BOXES: [&str; 3] = ["sign-password", "sign-new-password", "sign-new-again"];

/// A signature that waits for the person to agree to replace another.
struct Replacing {
    file: PathBuf,
    /// The file as it was when the question was asked.
    mark: Option<String>,
    key: PathBuf,
    signer: Ed25519Signer,
    request: Request,
    held: Vec<Held>,
}

/// The window, while it is open.
pub(super) struct SignForm {
    /// The secret key file, as typed or chosen.
    key: String,
    branch: bool,
    project: bool,
    /// The password of the key file.
    password: String,
    /// The password of a key to be made or protected, twice.
    new_password: String,
    again: String,
    /// How the key file of the text is kept, looked at when the text changes.
    looked: Option<(String, Option<KeyProtection>)>,
    replacing: Option<Replacing>,
    /// What the last thing done said, and whether it went well.
    lines: Vec<(String, bool)>,
}

/// An empty box with room for any password, so that typing does not leave copies behind.
fn password_box() -> String {
    String::with_capacity(MAX_PASSWORD_BYTES)
}

/// Takes what was typed out of a box; it is wiped when the password is dropped.
fn take(text: &mut String) -> Password {
    Password::new(std::mem::replace(text, password_box()))
}

impl Drop for SignForm {
    fn drop(&mut self) {
        for text in [&mut self.password, &mut self.new_password, &mut self.again] {
            drop(Password::new(std::mem::take(text)));
        }
    }
}

impl SignForm {
    fn say(&mut self, words: impl Into<String>, good: bool) {
        self.lines = vec![(words.into(), good)];
    }

    /// The two boxes of a new password, taken and wiped; nothing, and why, when they cannot be
    /// used.
    fn new_password(&mut self, lang: Lang) -> Option<Password> {
        let first = take(&mut self.new_password);
        let second = take(&mut self.again);
        if !first.is_long_enough() {
            let least = MIN_PASSWORD_CHARS.to_string();
            self.say(
                suite_fill(S::KeyShortPassword.tr(lang.code()), &[&least]),
                false,
            );
            None
        } else if !first.same_as(&second) {
            self.say(Msg::SignPasswordsDiffer.tr(lang), false);
            None
        } else {
            Some(first)
        }
    }

    fn protection(&mut self) -> Option<KeyProtection> {
        if self.looked.as_ref().map(|(key, _)| key) != Some(&self.key) {
            let found = signing::protection(Path::new(self.key.trim()));
            self.looked = Some((self.key.clone(), found));
        }
        self.looked.as_ref().and_then(|(_, found)| found.clone())
    }
}

impl App {
    /// Opens the window with the key used last, when it is still there.
    pub(super) fn open_signing(&mut self) {
        let key = self
            .key_store
            .as_ref()
            .and_then(KeyStore::load)
            .filter(|key| key.is_file())
            .map(|key| key.display().to_string())
            .unwrap_or_default();
        self.signing = Some(SignForm {
            key,
            branch: true,
            project: false,
            password: password_box(),
            new_password: password_box(),
            again: password_box(),
            looked: None,
            replacing: None,
            lines: Vec::new(),
        });
    }

    /// Keeps the path of the key, and nothing else, for the next time. A folder of the user that
    /// cannot be written leaves nothing kept.
    fn remember_key(&self, key: &Path) {
        if let (Some(store), Ok(full)) = (&self.key_store, std::path::absolute(key)) {
            let _ = store.remember(&full);
        }
    }

    /// The secret key chosen in a dialog.
    pub(super) fn take_sign_key(&mut self, path: &Path) -> bool {
        if let Some(form) = &mut self.signing {
            form.key = path.display().to_string();
            form.looked = None;
            form.replacing = None;
            form.lines.clear();
        }
        true
    }

    /// Asks for the name of a new key, once the password typed twice can protect it.
    pub(super) fn ask_new_key(&mut self) {
        let lang = self.win.lang;
        let Some(form) = &mut self.signing else {
            return;
        };
        let usable = Password::new(form.new_password.clone()).is_long_enough()
            && form.new_password == form.again;
        if usable {
            form.lines.clear();
            self.suite_dialog(SuiteAsk::NewKey);
        } else {
            // says why, and wipes the boxes
            drop(form.new_password(lang));
        }
    }

    /// Makes a new key under the password of the window at the name chosen in a dialog.
    pub(super) fn make_sign_key(&mut self, path: &Path) -> bool {
        let lang = self.win.lang;
        let Some(form) = &mut self.signing else {
            return true;
        };
        let Some(password) = form.new_password(lang) else {
            return true;
        };
        match signing::new_locked_key(path, &password) {
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
                form.looked = None;
                form.say(words, true);
                self.remember_key(&pair.secret_path);
            }
            Err(e) => {
                let words = signing::key_words(&e, path, lang);
                form.say(words, false);
            }
        }
        true
    }

    /// Puts the key of the window, which has no password, under the one typed twice.
    pub(super) fn protect_sign_key(&mut self) {
        let lang = self.win.lang;
        let Some(form) = &mut self.signing else {
            return;
        };
        let Some(password) = form.new_password(lang) else {
            return;
        };
        let key = PathBuf::from(form.key.trim());
        match signing::protect(&key, &password) {
            Ok(()) => {
                let name = key.display().to_string();
                form.say(fill(Msg::SignProtected.tr(lang), &[&name]), true);
                form.looked = None;
                self.remember_key(&key);
            }
            Err(e) => {
                let words = signing::key_words(&e, &key, lang);
                form.say(words, false);
            }
        }
    }

    /// Signs what the window says with its key. A signature of another key is asked about first.
    pub(super) fn sign_now(&mut self) {
        let lang = self.win.lang;
        let file = suitewin::current_file(&self.win);
        let dirty = self.win.dirty;
        let Some(form) = &mut self.signing else {
            return;
        };
        form.replacing = None;
        // the password is taken out of its box whatever happens next
        let password = (!form.password.is_empty()).then(|| take(&mut form.password));
        let Some(file) = file else {
            form.say(Msg::SuiteNeedFile.tr(lang), false);
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
            form.say(Msg::SignNeedsChoice.tr(lang), false);
            return;
        }
        if dirty {
            form.say(Msg::SignSaveFirst.tr(lang), false);
            return;
        }
        // what another program saved was not seen here, so it is not signed
        if !self.win.is_unchanged(&file) {
            form.say(self.win.status.text(lang), false);
            return;
        }
        let key = PathBuf::from(form.key.trim());
        let signer = match signing::read_key(&key, password.as_ref()) {
            Ok(signer) => signer,
            Err(e) => {
                let words = signing::key_words(&e, &key, lang);
                form.say(words, false);
                // the file may not be what it was when the window looked at it
                form.looked = None;
                return;
            }
        };
        drop(password);
        match signing::to_replace(&file, &signer, &request) {
            Ok(held) if held.is_empty() => self.sign_file(&file, &key, &signer, &request, false),
            Ok(held) => {
                let mark = suiteview::view(&file, crate::eh3dfile::BRANCH)
                    .ok()
                    .and_then(|view| view.own_hash);
                form.lines.clear();
                form.replacing = Some(Replacing {
                    file,
                    mark,
                    key,
                    signer,
                    request,
                    held,
                });
            }
            Err(e) => form.say(error_text(&e, lang), false),
        }
    }

    /// The person agreed to replace the signature that is there.
    pub(super) fn replace_signature(&mut self) {
        let lang = self.win.lang;
        let current = suitewin::current_file(&self.win);
        let Some(asked) = self.signing.as_mut().and_then(|f| f.replacing.take()) else {
            return;
        };
        // another project was opened meanwhile: nothing was agreed for it
        if current.as_deref() != Some(asked.file.as_path()) || self.win.dirty {
            return;
        }
        // another program saved the file while the question was open
        let changed = !self.win.is_unchanged(&asked.file)
            || asked
                .mark
                .as_deref()
                .is_some_and(|mark| suiteview::check_unchanged(&asked.file, mark).is_err());
        if changed {
            self.win.refuse_changed(&asked.file);
            let words = self.win.status.text(lang);
            if let Some(form) = &mut self.signing {
                form.say(words, false);
            }
            return;
        }
        self.sign_file(&asked.file, &asked.key, &asked.signer, &asked.request, true);
    }

    /// Signs, says what was done and lists the branches again with the public key beside the
    /// secret one, so that the new signature is seen as checked.
    fn sign_file(
        &mut self,
        file: &Path,
        key: &Path,
        signer: &Ed25519Signer,
        request: &Request,
        replace: bool,
    ) {
        let lang = self.win.lang;
        let result = signing::sign_with(file, signer, request, replace);
        self.win.seen_again();
        if result.is_ok() {
            self.remember_key(key);
        }
        let Some(form) = &mut self.signing else {
            return;
        };
        match result {
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
                let public = public_key_path(key);
                let public_text = public.display().to_string();
                if public.is_file() && !self.chosen_keys.contains(&public_text) {
                    self.chosen_keys.push(public_text);
                }
            }
            Err(e) => form.say(error_text(&e, lang), false),
        }
        self.refresh_branches();
    }

    /// The window, while it is open.
    pub(super) fn signing_window(&mut self, ctx: &egui::Context) {
        let lang = self.win.lang;
        let code = lang.code();
        let dirty = self.win.dirty;
        let Some(form) = &mut self.signing else {
            return;
        };
        let protection = form.protection();
        let mut open = true;
        let (mut choose, mut make, mut protect, mut sign) = (false, false, false, false);
        let (mut replace, mut keep) = (false, false);
        egui::Window::new(Msg::SignMenu.tr(lang))
            .id(egui::Id::unique("suite-sign"))
            .open(&mut open)
            .default_pos([420.0, 140.0])
            .default_size([560.0, 300.0])
            .show(ctx, |ui| {
                ui.label(Msg::SignKeyFile.tr(lang));
                let typed =
                    ui.add(egui::TextEdit::singleline(&mut form.key).desired_width(f32::INFINITY));
                if typed.changed() {
                    form.replacing = None;
                }
                choose = ui.button(Msg::SignChoose.tr(lang)).clicked();
                match &protection {
                    Some(KeyProtection::Password { .. }) => {
                        let name = form.key.trim().to_owned();
                        ui.label(suite_fill(S::KeyAskPassword.tr(code), &[&name]));
                        password_edit(ui, &mut form.password, PASSWORD_BOXES[0]);
                    }
                    Some(KeyProtection::Unprotected { .. }) => {
                        let name = form.key.trim().to_owned();
                        ui.colored_label(
                            ACCENT,
                            suite_fill(S::KeyWithoutPassword.tr(code), &[&name]),
                        );
                    }
                    _ => {}
                }
                let unprotected = matches!(protection, Some(KeyProtection::Unprotected { .. }));
                let mut new_key = |ui: &mut egui::Ui| {
                    ui.label(Msg::SignPasswordNew.tr(lang));
                    password_edit(ui, &mut form.new_password, PASSWORD_BOXES[1]);
                    ui.label(Msg::SignPasswordAgain.tr(lang));
                    password_edit(ui, &mut form.again, PASSWORD_BOXES[2]);
                    ui.horizontal_wrapped(|ui| {
                        if unprotected {
                            protect = ui.button(Msg::SignProtect.tr(lang)).clicked();
                        }
                        make = ui.button(Msg::SignNewKey.tr(lang)).clicked();
                    });
                };
                // a key without a password shows the way to protect it at once
                if unprotected {
                    new_key(ui);
                } else {
                    ui.collapsing(Msg::SignNewKey.tr(lang), new_key);
                }
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
                // no button has the keyboard, so Enter does not answer
                if let Some(asked) = &form.replacing {
                    ui.separator();
                    for held in &asked.held {
                        ui.colored_label(DANGER, held.question(lang));
                    }
                    ui.horizontal_wrapped(|ui| {
                        replace = ui.button(Msg::SignReplace.tr(lang)).clicked();
                        keep = ui.button(Msg::Cancel.tr(lang)).clicked();
                    });
                }
                for (words, good) in &form.lines {
                    ui.colored_label(if *good { OK } else { DANGER }, words.as_str());
                }
            });
        if keep {
            form.replacing = None;
        }
        if !open {
            self.signing = None;
        } else if choose {
            self.suite_dialog(SuiteAsk::SignKey);
        } else if make {
            self.ask_new_key();
        } else if protect {
            self.protect_sign_key();
        } else if replace {
            self.replace_signature();
        } else if sign {
            self.sign_now();
        }
        // the boxes forget what was typed in them, also what could be undone
        if !open || make || protect || sign {
            for id in PASSWORD_BOXES {
                ctx.data_mut(|d| d.remove::<egui::text_edit::TextEditState>(egui::Id::unique(id)));
            }
        }
    }
}

/// A box where a password is typed: it is shown as dots and cannot be copied.
fn password_edit(ui: &mut egui::Ui, text: &mut String, id: &str) {
    ui.add(
        egui::TextEdit::singleline(text)
            .id(egui::Id::unique(id))
            .password(true)
            .char_limit(MAX_PASSWORD_BYTES / 4)
            .desired_width(f32::INFINITY),
    );
}

#[cfg(test)]
#[path = "signing_tests.rs"]
mod tests;
