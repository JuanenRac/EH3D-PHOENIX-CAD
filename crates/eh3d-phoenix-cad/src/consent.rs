// SPDX-License-Identifier: GPL-3.0-or-later
//! What a person has to agree to before the window writes into a project of the suite (`.eh3d`):
//! writing a branch removes the signature of that branch and the one of the whole project. The
//! window asks first and writes nothing unless it is agreed. A project that another program saved
//! since the window read it is refused here instead of asked about, and the window looks again
//! after the answer ([`mark`]). The commands of the command line do not ask, and the panel of the
//! attachments has its own question.
//!
//! What would be removed and the words that tell it are the ones every program of the suite uses;
//! here it is only decided which branches each entry of the window writes.

use std::path::{Path, PathBuf};

use eh3d_project::suitetext::Said;
use eh3d_project::suiteview;

use crate::eh3dfile::{self, BRANCH, COMMON_BRANCH, DOCS_BRANCH};
use crate::lang::Lang;
use crate::suitewin::{self, BoardWork};
use crate::window::Window;

/// Something the window does that writes into a project of the suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// Save the project into this file.
    Save(PathBuf),
    /// Save as a file of the suite with this name.
    SaveAs {
        /// The file.
        target: PathBuf,
        /// A file that holds the box of another project is written over.
        overwrite: bool,
    },
    /// Keep the original of this file of another program.
    Import(PathBuf),
    /// Write the title and the description in one language.
    Texts {
        /// The language of the texts.
        language: Lang,
        /// The title.
        title: String,
        /// The description.
        description: String,
    },
    /// Take the board of the branch `pcb` for the box.
    TakeBoard,
    /// Work with the board; the check of the fit writes nothing.
    Board(BoardWork),
}

/// The file that doing `write` now would write and its branches; nothing when it writes no file of
/// the suite, also when it would be refused.
fn written(win: &Window, write: &Write) -> Option<(PathBuf, &'static [&'static str])> {
    const OWN: &[&str] = &[BRANCH];
    let current = || suitewin::current_file(win);
    match write {
        Write::Save(path) => {
            if win.read_only || !eh3dfile::is_eh3d(path) {
                return None;
            }
            // a copy under a new name takes the signatures along and then writes over them
            let from = win.carried_from(path).unwrap_or_else(|| path.clone());
            Some((from, OWN))
        }
        Write::SaveAs { target, overwrite } => {
            if !eh3dfile::is_eh3d(target) || suitewin::refusal(win, target, *overwrite).is_some() {
                return None;
            }
            written(win, &Write::Save(target.clone()))
        }
        Write::Import(_) => Some((current()?, OWN)),
        Write::Texts { .. } => {
            if win.read_only {
                return None;
            }
            Some((current()?, &[COMMON_BRANCH]))
        }
        Write::TakeBoard => {
            let file = current()?;
            let has = eh3dfile::has_project(&file).unwrap_or(false);
            // changes that are not saved are refused first
            let refused = win.dirty && (has || win.can_undo());
            (!refused).then_some((file, OWN))
        }
        Write::Board(work) => {
            let branches: &'static [&'static str] = match work {
                BoardWork::CheckFit => return None,
                BoardWork::CutOpenings => OWN,
                BoardWork::HandBack => &[BRANCH, DOCS_BRANCH],
            };
            if win.dirty {
                return None;
            }
            Some((current()?, branches))
        }
    }
}

/// What writing the branches of `file` would take away. A file that is not there loses nothing.
/// `opened` is the mark the window took of the file it writes: another program having saved it
/// since then is a refusal.
fn lost(file: &Path, branches: &[&str], opened: Option<&str>) -> Result<Vec<Said>, String> {
    let mut out: Vec<Said> = Vec::new();
    if let Some(opened) = opened {
        suiteview::check_unchanged(file, opened)?;
    }
    if !file.is_file() {
        return Ok(out);
    }
    for branch in branches {
        // the mark of the branch of this program is the one of when the file was opened; the
        // other branches are looked at as they are now
        let seen = match opened {
            Some(opened) if *branch == BRANCH => Some(opened.to_owned()),
            _ => suiteview::view(file, branch)?.own_hash,
        };
        for said in suiteview::warnings(file, Some(file), seen.as_deref(), branch)? {
            if !out.contains(&said) {
                out.push(said);
            }
        }
    }
    Ok(out)
}

/// What the person has to agree to before the window does `write`; an empty list asks nothing. The
/// error says why the project could not be looked at, and then nothing is to be written.
pub fn question(win: &Window, write: &Write) -> Result<Vec<Said>, String> {
    match written(win, write) {
        Some((file, branches)) => {
            // a copy under another name only reads the file of the window, so a save of another
            // program does not stop it: it is the way out that is offered then
            let copy = match write {
                Write::Save(target) | Write::SaveAs { target, .. } => {
                    win.carried_from(target).is_some()
                }
                _ => false,
            };
            let opened = win.seen_mark(&file).filter(|_| !copy);
            lost(&file, branches, opened)
        }
        None => Ok(Vec::new()),
    }
}

/// The file `write` would write (or copy from) with its mark as it is while the person is asked,
/// to be looked at again after the answer: another program may have saved it meanwhile.
pub fn mark(win: &Window, write: &Write) -> Option<(PathBuf, String)> {
    let (file, _) = written(win, write)?;
    let mark = suiteview::view(&file, BRANCH).ok()?.own_hash?;
    Some((file, mark))
}

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;
