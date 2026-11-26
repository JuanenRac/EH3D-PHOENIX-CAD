// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests of what is asked before a write. The keys are made for each test and thrown away.

use std::fs;
use std::path::PathBuf;

use super::*;
use crate::eh3dfile::{tests::Scratch, SaveSettings};
use crate::signing::{new_key, sign, Request};

fn window() -> Window {
    Window::new(
        Lang::En,
        crate::config::Settings::defaults(),
        crate::uiconfig::UiSettings::defaults(),
    )
    .unwrap()
}

/// A window on a new file of the suite, and a key beside it.
fn saved(dir: &Scratch) -> (Window, PathBuf, PathBuf) {
    let file = dir.path("box.eh3d");
    let key = dir.path("mine.key");
    new_key(&key).unwrap();
    let mut win = window();
    assert!(suitewin::save_as(&mut win, &file, false));
    (win, file, key)
}

fn words(win: &Window, write: &Write) -> Vec<String> {
    question(win, write)
        .unwrap()
        .iter()
        .map(|said| said.text("en"))
        .collect()
}

/// Every entry of the window that writes into the file of the window.
fn writes(file: &Path) -> Vec<Write> {
    vec![
        Write::Save(file.to_path_buf()),
        Write::SaveAs {
            target: file.to_path_buf(),
            overwrite: false,
        },
        Write::Import(file.with_file_name("part.stl")),
        Write::Texts {
            language: Lang::En,
            title: "Box".to_owned(),
            description: String::new(),
        },
        Write::TakeBoard,
        Write::Board(BoardWork::CutOpenings),
        Write::Board(BoardWork::HandBack),
    ]
}

#[test]
fn nothing_is_asked_while_nothing_is_signed() {
    let dir = Scratch::new("consent-unsigned");
    let (win, file, _) = saved(&dir);
    for write in writes(&file) {
        assert!(words(&win, &write).is_empty(), "{write:?}");
    }
}

#[test]
fn a_signed_branch_is_asked_about_before_what_writes_it() {
    let dir = Scratch::new("consent-branch");
    let (win, file, key) = saved(&dir);
    sign(&file, &key, &Request::own()).unwrap();
    let before = fs::read(&file).unwrap();
    for write in writes(&file) {
        let asked = words(&win, &write);
        // the title and the description are in `common`, which is not signed
        if matches!(write, Write::Texts { .. }) {
            assert!(asked.is_empty(), "{asked:?}");
        } else {
            assert_eq!(asked.len(), 1, "{write:?}: {asked:?}");
            assert!(asked[0].contains("The branch cad is signed"), "{asked:?}");
        }
    }
    assert!(words(&win, &Write::Board(BoardWork::CheckFit)).is_empty());
    // asking writes nothing
    assert_eq!(fs::read(&file).unwrap(), before);
}

#[test]
fn a_signed_project_is_asked_about_before_any_write_into_it() {
    let dir = Scratch::new("consent-project");
    let (win, file, key) = saved(&dir);
    let request = Request {
        branches: Vec::new(),
        whole: true,
    };
    sign(&file, &key, &request).unwrap();
    for write in writes(&file) {
        let asked = words(&win, &write);
        assert_eq!(asked.len(), 1, "{write:?}: {asked:?}");
        assert!(asked[0].contains("The project is signed"), "{asked:?}");
    }
}

#[test]
fn a_copy_under_a_new_name_is_asked_about_and_a_new_file_of_nothing_is_not() {
    let dir = Scratch::new("consent-copy");
    let (win, file, key) = saved(&dir);
    sign(&file, &key, &Request::own()).unwrap();
    let copy = Write::SaveAs {
        target: dir.path("copy.eh3d"),
        overwrite: false,
    };
    assert_eq!(words(&win, &copy).len(), 1);
    // a window that comes from no file of the suite takes no signature anywhere
    assert!(words(&window(), &copy).is_empty());
    // a project of this program alone has no signatures
    let alone = Write::Save(dir.path("box.phoenixproj"));
    assert!(words(&win, &alone).is_empty());
}

#[test]
fn what_would_be_refused_is_not_asked_about() {
    let dir = Scratch::new("consent-refused");
    let (mut win, file, key) = saved(&dir);
    let other = dir.path("other.eh3d");
    eh3dfile::save_cad_project(&other, b"project", &SaveSettings::default()).unwrap();
    sign(&other, &key, &Request::own()).unwrap();
    // the box of another project is only written over when that is agreed to first
    let kept = Write::SaveAs {
        target: other.clone(),
        overwrite: false,
    };
    assert!(words(&win, &kept).is_empty());
    let over = Write::SaveAs {
        target: other,
        overwrite: true,
    };
    assert_eq!(words(&win, &over).len(), 1);
    sign(&file, &key, &Request::own()).unwrap();
    win.dirty = true;
    assert!(words(&win, &Write::Board(BoardWork::HandBack)).is_empty());
    win.read_only = true;
    assert!(words(&win, &Write::Save(file)).is_empty());
}

#[test]
fn a_project_that_cannot_be_looked_at_is_not_written() {
    let dir = Scratch::new("consent-damaged");
    let (win, file, _) = saved(&dir);
    fs::write(&file, b"not a project").unwrap();
    assert!(question(&win, &Write::Save(file)).is_err());
}
