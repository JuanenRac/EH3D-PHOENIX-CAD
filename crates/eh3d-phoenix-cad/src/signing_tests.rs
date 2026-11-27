// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests of signing from this program. The keys are made for each test and thrown away.

use std::fs;
use std::path::PathBuf;

use super::*;
use crate::eh3dfile::{tests::Scratch, SaveSettings};
use crate::lang::{Lang, Msg};
use crate::window::{Status, Window};

/// A project of the suite with a box in the branch `cad`, and a new key beside it.
fn signed_up(dir: &Scratch) -> (PathBuf, PathBuf) {
    let file = dir.path("p.eh3d");
    eh3dfile::save_cad_project(&file, b"project", &SaveSettings::default()).unwrap();
    let key = dir.path("mine.key");
    new_key(&key).unwrap();
    (file, key)
}

fn status_of_cad(file: &Path, public: &Path) -> SignatureStatus {
    let (keys, problems) = eh3dfile::keyring(&[public.display().to_string()]);
    assert!(problems.is_empty(), "{problems:?}");
    let found = eh3dfile::summary_with(file, Some(&keys)).unwrap();
    let line = found.branches.into_iter().find(|b| b.id == BRANCH).unwrap();
    line.signature
}

#[test]
fn a_new_key_is_two_files_and_never_replaces_one() {
    let dir = Scratch::new("sign-key");
    let secret = dir.path("a.key");
    let pair = new_key(&secret).unwrap();
    assert!(pair.secret_path.is_file() && pair.public_path.is_file());
    assert!(pair.public_path.to_string_lossy().ends_with("a.key.pub"));
    assert!(pair.key_id.starts_with("ed25519:"));
    let before = fs::read(&secret).unwrap();
    assert!(new_key(&secret).is_err());
    assert_eq!(fs::read(&secret).unwrap(), before);
    assert!(new_key(&dir.path("a.eh3d")).is_err());
}

#[test]
fn the_branch_of_this_program_is_signed_and_checked_with_the_public_key() {
    let dir = Scratch::new("sign-branch");
    let (file, key) = signed_up(&dir);
    assert_eq!(standing(&file), Standing::default());
    let done = sign(&file, &key, &Request::own()).unwrap();
    assert_eq!(done.branches, vec![BRANCH.to_owned()]);
    assert!(!done.whole);
    assert_eq!(
        standing(&file),
        Standing {
            branch: true,
            project: false
        }
    );
    let public = dir.path("mine.key.pub");
    assert_eq!(
        status_of_cad(&file, &public),
        SignatureStatus::Valid(done.key_id)
    );
    // another key does not vouch for it
    let other = dir.path("other.key");
    new_key(&other).unwrap();
    assert_eq!(
        status_of_cad(&file, &dir.path("other.key.pub")),
        SignatureStatus::UnknownKey
    );
}

#[test]
fn the_whole_project_is_signed_after_its_branch() {
    let dir = Scratch::new("sign-project");
    let (file, key) = signed_up(&dir);
    let request = Request {
        branches: vec![BRANCH.to_owned()],
        whole: true,
    };
    sign(&file, &key, &request).unwrap();
    assert_eq!(
        standing(&file),
        Standing {
            branch: true,
            project: true
        }
    );
}

#[test]
fn a_refusal_leaves_the_file_as_it_was() {
    let dir = Scratch::new("sign-refused");
    let (file, key) = signed_up(&dir);
    let before = fs::read(&file).unwrap();
    // one branch of two is not there: nothing is signed
    let request = Request {
        branches: vec![BRANCH.to_owned(), "pcb".to_owned()],
        whole: false,
    };
    assert!(sign(&file, &key, &request).is_err());
    // the public key does not sign, nothing is not a request and a key is not a project
    assert!(sign(&file, &dir.path("mine.key.pub"), &Request::own()).is_err());
    let nothing = Request {
        branches: Vec::new(),
        whole: false,
    };
    assert!(sign(&file, &key, &nothing).is_err());
    assert!(sign(&key, &key, &Request::own()).is_err());
    assert!(sign(&file, &dir.path("gone.key"), &Request::own()).is_err());
    assert_eq!(fs::read(&file).unwrap(), before);
    assert_eq!(standing(&dir.path("gone.eh3d")), Standing::default());
}

#[test]
fn saving_over_what_was_signed_removes_the_signature() {
    let dir = Scratch::new("sign-save");
    let (file, key) = signed_up(&dir);
    sign(&file, &key, &Request::own()).unwrap();
    let signed = standing(&file);
    assert!(!signed.lost(signed));
    eh3dfile::save_cad_project(&file, b"another project", &SaveSettings::default()).unwrap();
    let after = standing(&file);
    assert_eq!(after, Standing::default());
    assert!(signed.lost(after));
    // gaining a signature is not losing one
    assert!(!after.lost(signed));
}

#[test]
fn the_window_says_that_a_save_removed_the_signature() {
    let dir = Scratch::new("sign-window");
    let file = dir.path("box.eh3d");
    let key = dir.path("mine.key");
    new_key(&key).unwrap();
    let mut win = Window::new(
        Lang::En,
        crate::config::Settings::defaults(),
        crate::uiconfig::UiSettings::defaults(),
    )
    .unwrap();
    assert!(crate::suitewin::save_as(&mut win, &file, false));
    sign(&file, &key, &Request::own()).unwrap();
    assert!(win.save());
    assert!(
        matches!(&win.status, Status::Done(Msg::SignatureRemoved, _)),
        "{:?}",
        win.status
    );
    // the next save has nothing to remove
    assert!(win.save());
    assert!(
        matches!(&win.status, Status::Done(Msg::Saved, _)),
        "{:?}",
        win.status
    );
    // a copy under another name takes the signed file along and writes over it: it is told too
    sign(&file, &key, &Request::own()).unwrap();
    assert!(crate::suitewin::save_as(
        &mut win,
        &dir.path("copy.eh3d"),
        false
    ));
    assert!(
        matches!(&win.status, Status::Done(Msg::SignatureRemoved, _)),
        "{:?}",
        win.status
    );
}

#[test]
fn opening_a_file_of_the_suite_tells_what_is_known_of_its_signatures() {
    let dir = Scratch::new("sign-open");
    let file = dir.path("box.eh3d");
    let key = dir.path("mine.key");
    new_key(&key).unwrap();
    let window = |trusted: Vec<String>| {
        let mut ui = crate::uiconfig::UiSettings::defaults();
        ui.suite.trusted_keys = trusted;
        Window::new(Lang::En, crate::config::Settings::defaults(), ui).unwrap()
    };
    let mut win = window(Vec::new());
    assert!(crate::suitewin::save_as(&mut win, &file, false));
    let open = |trusted: Vec<String>| {
        let mut win = window(trusted);
        assert_eq!(
            crate::suitewin::open(&mut win, &file),
            crate::suitewin::Opened::Project
        );
        win.status
    };
    // unsigned: the usual words
    assert!(matches!(open(Vec::new()), Status::Done(Msg::Opened, _)));
    sign(&file, &key, &Request::own()).unwrap();
    // signed and no key to check with: only seen
    let seen = open(Vec::new());
    assert!(
        matches!(&seen, Status::Done(Msg::SuiteOpenedSigned, _)),
        "{seen:?}"
    );
    assert!(seen.text(Lang::En).contains("cad: "), "{seen:?}");
    // checked with its key: valid
    let public = dir.path("mine.key.pub").display().to_string();
    let valid = open(vec![public]);
    assert!(
        valid.text(Lang::En).contains("signature valid"),
        "{valid:?}"
    );
    // checked with another key: a problem
    let other = dir.path("other.key");
    new_key(&other).unwrap();
    let unknown = open(vec![dir.path("other.key.pub").display().to_string()]);
    assert!(
        matches!(&unknown, Status::Problem(Msg::SuiteOpenedSigned, _)),
        "{unknown:?}"
    );
}

#[test]
fn the_commands_make_a_key_sign_and_say_when_a_signature_goes() {
    let dir = Scratch::new("sign-cli");
    let file = dir.path("p.eh3d");
    let key = dir.path("mine.key");
    let words = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let run = |command: &str, args: &[&str]| crate::suite_cli::run(command, &words(args));
    let (file_text, key_text) = (file.display().to_string(), key.display().to_string());
    let (out, ok) = run("eh3d-keygen", &["--out", &key_text]).unwrap();
    assert!(ok && out.contains("ed25519:"), "{out}");
    assert!(run("eh3d-keygen", &["--out", &key_text]).is_err());
    assert!(run("eh3d-keygen", &[]).is_err());
    eh3dfile::save_cad_project(&file, b"project", &SaveSettings::default()).unwrap();
    assert!(run("eh3d-sign", &[&file_text]).is_err());
    let (out, ok) = run("eh3d-sign", &[&file_text, "--key", &key_text]).unwrap();
    assert!(ok && out.starts_with("Signed the branch cad"), "{out}");
    let (out, _) = run("eh3d-sign", &[&file_text, "--key", &key_text, "--project"]).unwrap();
    assert!(out.starts_with("Signed the whole project"), "{out}");
    assert_eq!(
        standing(&file),
        Standing {
            branch: true,
            project: true
        }
    );
    // a command that writes the branch says that the signature went
    let (out, _) = run("eh3d-meta", &[&file_text, "--title", "Box", "--lang", "en"]).unwrap();
    assert!(out.contains("The signature was removed"), "{out}");
    assert!(standing(&file).branch);
    assert!(!standing(&file).project);
}

/// A key under a password, with a cost small enough for a test.
fn locked_key(dir: &Scratch, name: &str, password: &str) -> PathBuf {
    let path = dir.path(name);
    let cheap = eh3d_project::KdfParams {
        memory_kib: 64,
        passes: 1,
        lanes: 1,
    };
    eh3d_project::ed25519::generate_locked_key_files_with(
        &path,
        &Password::new(password.to_owned()),
        cheap,
    )
    .unwrap();
    path
}

#[test]
fn a_key_under_a_password_signs_only_with_it_and_the_password_is_never_told() {
    let dir = Scratch::new("sign-locked");
    let (file, _) = signed_up(&dir);
    // made up for this run
    let typed = format!("made-up-{}", std::process::id());
    let key = locked_key(&dir, "locked.key", &typed);
    assert!(matches!(
        protection(&key),
        Some(KeyProtection::Password { .. })
    ));
    let before = fs::read(&file).unwrap();
    for (given, lang) in [(None, Lang::En), (Some("not the one at all"), Lang::Es)] {
        let password = given.map(|text| Password::new(text.to_owned()));
        let error = read_key(&key, password.as_ref()).unwrap_err();
        let words = key_words(&error, &key, lang);
        assert!(words.contains("locked.key"), "{words}");
        assert!(!words.contains(&typed), "{words}");
    }
    assert!(sign(&file, &key, &Request::own()).is_err());
    assert_eq!(fs::read(&file).unwrap(), before);
    let signer = read_key(&key, Some(&Password::new(typed.clone()))).unwrap();
    sign_with(&file, &signer, &Request::own(), false).unwrap();
    assert!(standing(&file).branch);
    assert!(!String::from_utf8_lossy(&fs::read(&file).unwrap()).contains(&typed));
}

#[test]
fn a_new_key_can_be_made_under_a_password_and_an_old_one_protected() {
    let dir = Scratch::new("sign-protect");
    let typed = format!("made-up-{}", std::process::id());
    let short = new_locked_key(&dir.path("short.key"), &Password::new("abc".to_owned()));
    let error = short.unwrap_err();
    assert!(key_words(&error, &dir.path("short.key"), Lang::En).contains('8'));
    assert!(!dir.path("short.key").exists());
    new_locked_key(&dir.path("new.key"), &Password::new(typed.clone())).unwrap();
    assert!(matches!(
        protection(&dir.path("new.key")),
        Some(KeyProtection::Password { .. })
    ));
    let old = dir.path("old.key");
    new_key(&old).unwrap();
    assert!(matches!(
        protection(&old),
        Some(KeyProtection::Unprotected { .. })
    ));
    protect(&old, &Password::new(typed.clone())).unwrap();
    assert!(read_key(&old, None).is_err());
    assert!(read_key(&old, Some(&Password::new(typed))).is_ok());
}

#[test]
fn a_signature_of_another_key_is_replaced_only_when_agreed() {
    let dir = Scratch::new("sign-replace");
    let (file, key) = signed_up(&dir);
    let both = Request {
        branches: vec![BRANCH.to_owned()],
        whole: true,
    };
    let first = sign(&file, &key, &both).unwrap();
    // the same key again replaces nothing of another
    let mine = read_key(&key, None).unwrap();
    assert!(to_replace(&file, &mine, &both).unwrap().is_empty());
    let other = dir.path("other.key");
    new_key(&other).unwrap();
    let theirs = read_key(&other, None).unwrap();
    let held = to_replace(&file, &theirs, &both).unwrap();
    assert_eq!(
        held.iter().map(|h| h.what.as_str()).collect::<Vec<_>>(),
        vec![BRANCH, WHOLE]
    );
    assert!(held.iter().all(|h| h.key_id == first.key_id));
    assert!(held[0].question(Lang::En).contains(&first.key_id));
    let before = fs::read(&file).unwrap();
    let error = sign_with(&file, &theirs, &both, false).unwrap_err();
    assert!(format!("{error:?}").contains("EH3D-PKG-026"), "{error:?}");
    assert_eq!(fs::read(&file).unwrap(), before);
    sign_with(&file, &theirs, &both, true).unwrap();
    assert_eq!(
        status_of_cad(&file, &dir.path("other.key.pub")),
        SignatureStatus::Valid(theirs.key_id().to_owned())
    );
}

#[test]
fn the_commands_take_the_password_from_a_line_and_replace_only_when_told() {
    let dir = Scratch::new("sign-cli-locked");
    let file = dir.path("p.eh3d");
    eh3dfile::save_cad_project(&file, b"project", &SaveSettings::default()).unwrap();
    let typed = format!("made-up-{}", std::process::id());
    let key = locked_key(&dir, "locked.key", &typed);
    let other = dir.path("other.key");
    new_key(&other).unwrap();
    let words = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let (file_text, key_text) = (file.display().to_string(), key.display().to_string());
    let other_text = other.display().to_string();
    // without the password the command says how to give it
    let error =
        crate::suite_cli::run("eh3d-sign", &words(&[&file_text, "--key", &key_text])).unwrap_err();
    assert!(error.contains("--password-stdin"), "{error}");
    let (out, ok) =
        crate::suite_cli::run("eh3d-sign", &words(&[&file_text, "--key", &other_text])).unwrap();
    assert!(ok && out.contains("no password"), "{out}");
    let before = fs::read(&file).unwrap();
    let signer = read_key(&key, Some(&Password::new(typed))).unwrap();
    assert_eq!(
        to_replace(&file, &signer, &Request::own()).unwrap().len(),
        1
    );
    let third = dir.path("third.key");
    new_key(&third).unwrap();
    let third_text = third.display().to_string();
    let args = words(&[&file_text, "--key", &third_text]);
    let error = crate::suite_cli::run("eh3d-sign", &args).unwrap_err();
    assert!(error.contains("EH3D-PKG-026"), "{error}");
    assert_eq!(fs::read(&file).unwrap(), before);
    let args = words(&[&file_text, "--key", &third_text, "--replace-signature"]);
    assert!(crate::suite_cli::run("eh3d-sign", &args).unwrap().1);
}
