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
