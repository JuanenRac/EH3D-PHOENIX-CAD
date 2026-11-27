// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests of the signing window and of what the window offers when another program saved the
//! project, with no screen. Keys and passwords are made for each run and thrown away.

use std::fs;
use std::path::PathBuf;

use super::*;
use crate::eh3dfile::tests::{other_branch, Scratch};

fn app() -> App {
    let win = Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
    let sketch = SketchEditor::new(
        Lang::En,
        Settings::defaults(),
        SketchSettings::defaults(),
        SketchUiSettings::defaults(),
    );
    App::new(win, sketch, StartView::Model)
}

/// A window on a new project of the suite that keeps the key path in a folder of the test.
fn saved(dir: &Scratch) -> (App, PathBuf) {
    let mut app = app();
    app.key_store = Some(KeyStore::in_dir(&dir.path("settings"), Phoenix::Cad));
    let file = dir.path("box.eh3d");
    assert!(suitewin::save_as(&mut app.win, &file, false));
    (app, file)
}

/// What another program does: its save of a copy takes the place of the file.
fn saved_by_another(dir: &Scratch, file: &Path, name: &str) {
    let copy = dir.path("elsewhere.eh3d");
    fs::copy(file, &copy).unwrap();
    other_branch(&copy, "pcb", name, name.as_bytes());
    fs::copy(&copy, file).unwrap();
}

fn frame(app: &mut App, ctx: &egui::Context) {
    let out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let ctx = ui.ctx().clone();
        app.signing_window(&ctx);
        app.changed_window(&ctx);
    });
    out.drop_without_applying_deltas();
}

#[test]
fn a_key_made_in_the_window_has_a_password_and_only_its_path_is_remembered() {
    let dir = Scratch::new("gui-key");
    let (mut app, _) = saved(&dir);
    // made up for this run
    let typed = format!("made-up-{}", std::process::id());
    let key = dir.path("mine.key");
    app.open_signing();
    // two words that differ make nothing and are wiped
    let form = app.signing.as_mut().unwrap();
    form.new_password.push_str(&typed);
    form.again.push_str("something else");
    app.make_sign_key(&key);
    assert!(!key.exists());
    let form = app.signing.as_mut().unwrap();
    assert!(form.new_password.is_empty() && form.again.is_empty());
    form.new_password.push_str(&typed);
    form.again.push_str(&typed);
    app.make_sign_key(&key);
    assert!(matches!(
        signing::protection(&key),
        Some(KeyProtection::Password { .. })
    ));
    let form = app.signing.as_ref().unwrap();
    assert!(form.new_password.is_empty() && form.again.is_empty());
    assert!(form.lines.iter().all(|(words, _)| !words.contains(&typed)));
    let kept = fs::read_to_string(app.key_store.as_ref().unwrap().path()).unwrap();
    assert!(
        kept.contains("mine.key") && !kept.contains(&typed),
        "{kept}"
    );
    // without the password nothing is signed
    app.sign_now();
    assert!(app
        .signing
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .any(|(_, ok)| !ok));
    let file = app.win.path.clone().unwrap();
    assert_eq!(signing::standing(&file), signing::Standing::default());
    app.signing.as_mut().unwrap().password.push_str(&typed);
    app.sign_now();
    assert!(app.signing.as_ref().unwrap().password.is_empty());
    assert!(signing::standing(&file).branch);
    // the next time the path is offered, and nothing else
    app.signing = None;
    app.open_signing();
    let form = app.signing.as_ref().unwrap();
    assert!(form.key.ends_with("mine.key") && form.password.is_empty());
    frame(&mut app, &egui::Context::default());
}

#[test]
fn a_signature_of_another_key_is_asked_about_in_the_window() {
    let dir = Scratch::new("gui-replace");
    let (mut app, file) = saved(&dir);
    let (first, second) = (dir.path("first.key"), dir.path("second.key"));
    signing::new_key(&first).unwrap();
    signing::new_key(&second).unwrap();
    app.open_signing();
    app.take_sign_key(&first);
    app.sign_now();
    let signed = fs::read(&file).unwrap();
    app.take_sign_key(&second);
    app.sign_now();
    assert!(app.signing.as_ref().unwrap().replacing.is_some());
    frame(&mut app, &egui::Context::default());
    assert_eq!(fs::read(&file).unwrap(), signed, "asking writes nothing");
    app.replace_signature();
    assert!(app.signing.as_ref().unwrap().replacing.is_none());
    assert_ne!(fs::read(&file).unwrap(), signed);

    // another program saves while the question is open: nothing is replaced
    app.take_sign_key(&first);
    app.sign_now();
    assert!(app.signing.as_ref().unwrap().replacing.is_some());
    saved_by_another(&dir, &file, "while-asking");
    let theirs = fs::read(&file).unwrap();
    app.replace_signature();
    assert_eq!(fs::read(&file).unwrap(), theirs);
    assert_eq!(app.win.changed.as_deref(), Some(file.as_path()));
}

#[test]
fn a_project_another_program_saved_is_not_written_and_can_be_read_again() {
    let dir = Scratch::new("gui-changed");
    let (mut app, file) = saved(&dir);
    let key = dir.path("mine.key");
    signing::new_key(&key).unwrap();
    signing::sign(&file, &key, &signing::Request::own()).unwrap();

    // the save asks about the signature; another program saves before the answer
    app.save_or_ask();
    assert!(app.asking.is_some());
    saved_by_another(&dir, &file, "while-asking");
    let theirs = fs::read(&file).unwrap();
    app.confirm_write();
    assert!(app.asking.is_none());
    assert_eq!(app.win.changed.as_deref(), Some(file.as_path()));
    assert_eq!(fs::read(&file).unwrap(), theirs);

    // and it is not asked about any more: it is refused
    app.win.changed = None;
    app.save_or_ask();
    assert!(app.asking.is_none() && app.win.changed.is_some());
    assert_eq!(fs::read(&file).unwrap(), theirs);

    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    app.read_changed_again();
    assert!(app.win.changed.is_none());
    app.save_or_ask();
    app.confirm_write();
    assert!(app.win.changed.is_none());
    assert_ne!(fs::read(&file).unwrap(), theirs);
}
