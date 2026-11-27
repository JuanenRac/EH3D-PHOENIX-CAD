// SPDX-License-Identifier: GPL-3.0-or-later
//! Signing a project of the suite from this program: Ed25519, with the shared code of the suite.
//!
//! A branch (the branch `cad` unless others are named) or the whole project is signed with a
//! secret key file that the person gives: this program never keeps it or its password, never writes
//! it into a project and never shows what it holds (the window remembers its path and nothing
//! else). A key file may be under a password. Everything is checked before the first signature
//! (the file, the branches, the seals, the signatures that would be replaced), so a refusal leaves
//! the file as it was, and a signature of another key is replaced only when that was agreed. Saving
//! what a signature covers removes that signature; [`standing`] tells which ones there are, so
//! that the person is told.

use std::path::Path;

use eh3d_project::ed25519::{
    generate_key_files, generate_locked_key_files, key_protection, protect_key_file, Ed25519Signer,
    KeyPair,
};
use eh3d_project::keylock::MIN_PASSWORD_CHARS;
use eh3d_project::suitetext::{fill, S};
use eh3d_project::{
    Ed25519Keyring, KeyError, KeyProtection, OpenOptions, Password, Project, SaveOptions,
    SignatureStatus, Signer, Verifier,
};

use crate::eh3dfile::{self, BRANCH, EXTENSION};
use crate::lang::Lang;
use crate::CadError;

/// What is signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The branches, by name.
    pub branches: Vec<String>,
    /// Whether the record of the whole project is signed too, after the branches.
    pub whole: bool,
}

impl Request {
    /// The branch of this program and nothing else.
    pub fn own() -> Request {
        Request {
            branches: vec![BRANCH.to_owned()],
            whole: false,
        }
    }
}

/// What was signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    /// The identifier of the key.
    pub key_id: String,
    /// The branches that were signed.
    pub branches: Vec<String>,
    /// Whether the whole project was.
    pub whole: bool,
}

/// The signatures a write of the branch `cad` would remove.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Standing {
    /// The branch `cad` is signed.
    pub branch: bool,
    /// The record of the project is signed.
    pub project: bool,
}

impl Standing {
    /// Whether one of the signatures that were there is not there any more.
    pub fn lost(self, after: Standing) -> bool {
        (self.branch && !after.branch) || (self.project && !after.project)
    }
}

fn refused(why: impl ToString) -> CadError {
    CadError::SuiteFile(why.to_string())
}

/// What is signed in the file, as far as the branch `cad` goes: nothing for a file that is not a
/// project of the suite or cannot be read.
pub fn standing(file: &Path) -> Standing {
    if !eh3dfile::is_eh3d(file) || !file.is_file() {
        return Standing::default();
    }
    // only the records are looked at: the files of a large project are not hashed for this
    let options = OpenOptions {
        verify_integrity: false,
        ..OpenOptions::default()
    };
    let Ok(project) = Project::open(file, &options) else {
        return Standing::default();
    };
    let found = project.signatures(None);
    Standing {
        branch: found
            .branches
            .get(BRANCH)
            .is_some_and(|s| *s != SignatureStatus::Unsigned),
        project: found.project != SignatureStatus::Unsigned,
    }
}

/// The name that stands for the record of the whole project among the signatures.
pub const WHOLE: &str = "project.json";

/// A signature of another key that signing would put away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The branch that has it, or [`WHOLE`] for the record of the project.
    pub what: String,
    /// The identifier of the key that made it.
    pub key_id: String,
}

impl Held {
    /// The question about it, in the words every program of the suite uses.
    pub fn question(&self, lang: Lang) -> String {
        fill(
            S::SignatureReplace.tr(lang.code()),
            &[&self.what, &self.key_id],
        )
    }
}

/// How the secret key file is kept; nothing when the file is not a secret key that can be read.
pub fn protection(key: &Path) -> Option<KeyProtection> {
    key_protection(key).ok()
}

/// Reads the secret key of a file, with its password when it has one.
pub fn read_key(key: &Path, password: Option<&Password>) -> Result<Ed25519Signer, KeyError> {
    Ed25519Signer::read_with(key, password)
}

/// Why a key could not be used, in the words of the suite where it has them. Neither a password nor
/// what a key file holds is ever in them.
pub fn key_words(error: &KeyError, key: &Path, lang: Lang) -> String {
    let code = lang.code();
    let name = key.display().to_string();
    match error {
        KeyError::NeedsPassword(_) => fill(S::KeyAskPassword.tr(code), &[&name]),
        KeyError::WrongPassword(_) => fill(S::KeyWrongPassword.tr(code), &[&name]),
        KeyError::UnusablePassword => fill(
            S::KeyShortPassword.tr(code),
            &[&MIN_PASSWORD_CHARS.to_string()],
        ),
        other => other.to_string(),
    }
}

/// The file of the suite opened, with everything that is asked checked to be signable.
fn signable(file: &Path, request: &Request) -> Result<Project, CadError> {
    if !eh3dfile::is_eh3d(file) {
        return Err(refused(format!(
            "the file of the suite ends in .{EXTENSION}"
        )));
    }
    if request.branches.is_empty() && !request.whole {
        return Err(refused("there is nothing to sign"));
    }
    let project = eh3dfile::open_project(file)?;
    for branch in &request.branches {
        project.check_signable(branch).map_err(refused)?;
    }
    if request.whole && project.record().seal.is_some() {
        return Err(refused("the project is sealed read-only"));
    }
    Ok(project)
}

/// The signatures that signing with `signer` would replace: the ones that are there and are not a
/// valid signature of its key. Nothing is written.
pub fn to_replace(
    file: &Path,
    signer: &Ed25519Signer,
    request: &Request,
) -> Result<Vec<Held>, CadError> {
    let project = signable(file, request)?;
    let mut ring = Ed25519Keyring::new();
    ring.add(&signer.public_key()).map_err(refused)?;
    let found = project.signatures(Some(&ring as &dyn Verifier));
    let ours = SignatureStatus::Valid(signer.key_id().to_owned());
    let mut held = Vec::new();
    for branch in &request.branches {
        if let Some(old) = project.signature_to_replace(Some(branch)) {
            if found.branches.get(branch) != Some(&ours) {
                held.push(Held {
                    what: branch.clone(),
                    key_id: old.key_id.clone(),
                });
            }
        }
    }
    if request.whole && found.project != ours {
        if let Some(old) = project.signature_to_replace(None) {
            held.push(Held {
                what: WHOLE.to_owned(),
                key_id: old.key_id.clone(),
            });
        }
    }
    Ok(held)
}

/// Signs what is asked with a key that was read. Without `replace`, nothing is signed when a
/// signature would be replaced; with it, the person agreed to that.
pub fn sign_with(
    file: &Path,
    signer: &Ed25519Signer,
    request: &Request,
    replace: bool,
) -> Result<Signed, CadError> {
    if !replace {
        if let Some(held) = to_replace(file, signer, request)?.first() {
            return Err(refused(format!(
                "EH3D-PKG-026 {} is signed already (key {}); replacing the signature needs to be agreed",
                held.what, held.key_id
            )));
        }
    }
    let mut project = signable(file, request)?;
    let options = SaveOptions::default();
    for branch in &request.branches {
        project = if replace {
            project.sign_branch_replacing(branch, signer, &options)
        } else {
            project.sign_branch(branch, signer, &options)
        }
        .map_err(refused)?;
    }
    if request.whole {
        if replace {
            project.sign_project_replacing(signer, &options)
        } else {
            project.sign_project(signer, &options)
        }
        .map_err(refused)?;
    }
    Ok(Signed {
        key_id: signer.key_id().to_owned(),
        branches: request.branches.clone(),
        whole: request.whole,
    })
}

/// Signs what is asked with the secret key of the file `key`, one without a password, and replaces
/// no signature. Nothing is written when a branch cannot be signed (it is not there, it is sealed,
/// its files are not those of its record).
pub fn sign(file: &Path, key: &Path, request: &Request) -> Result<Signed, CadError> {
    let signer = read_key(key, None).map_err(refused)?;
    sign_with(file, &signer, request, false)
}

/// Makes a new key **without a password**: the secret file at `secret`, which must not exist, and
/// the public one beside it with `.pub` added. A project or a package is never taken as the name of
/// a key.
pub fn new_key(secret: &Path) -> Result<KeyPair, CadError> {
    generate_key_files(secret).map_err(refused)
}

/// Like [`new_key`], with the secret key under a password.
pub fn new_locked_key(secret: &Path, password: &Password) -> Result<KeyPair, KeyError> {
    generate_locked_key_files(secret, password)
}

/// Puts a secret key file that has no password under one.
pub fn protect(key: &Path, password: &Password) -> Result<(), KeyError> {
    protect_key_file(key, password)
}

#[cfg(test)]
#[path = "signing_tests.rs"]
mod tests;
