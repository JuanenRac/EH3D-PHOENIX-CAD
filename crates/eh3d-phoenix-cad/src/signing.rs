// SPDX-License-Identifier: GPL-3.0-or-later
//! Signing a project of the suite from this program: Ed25519, with the shared code of the suite.
//!
//! A branch (the branch `cad` unless others are named) or the whole project is signed with a
//! secret key file that the person gives each time: this program never keeps it, never writes it
//! into a project and never shows what it holds. Everything is checked before the first signature
//! (the file, the branches, the seals), so a refusal leaves the file as it was. Saving what a
//! signature covers removes that signature; [`standing`] tells which ones there are, so that the
//! person is told.

use std::path::Path;

use eh3d_project::ed25519::{generate_key_files, Ed25519Signer, KeyPair};
use eh3d_project::{OpenOptions, Project, SaveOptions, SignatureStatus, Signer};

use crate::eh3dfile::{self, BRANCH, EXTENSION};
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

/// Signs what is asked with the secret key of the file `key`. Nothing is written when a branch
/// cannot be signed (it is not there, it is sealed, its files are not those of its record).
pub fn sign(file: &Path, key: &Path, request: &Request) -> Result<Signed, CadError> {
    if !eh3dfile::is_eh3d(file) {
        return Err(refused(format!(
            "the file of the suite ends in .{EXTENSION}"
        )));
    }
    if request.branches.is_empty() && !request.whole {
        return Err(refused("there is nothing to sign"));
    }
    let signer = Ed25519Signer::read(key).map_err(refused)?;
    let mut project = eh3dfile::open_project(file)?;
    for branch in &request.branches {
        project.check_signable(branch).map_err(refused)?;
    }
    if request.whole && project.record().seal.is_some() {
        return Err(refused("the project is sealed read-only"));
    }
    let options = SaveOptions::default();
    for branch in &request.branches {
        project = project
            .sign_branch(branch, &signer, &options)
            .map_err(refused)?;
    }
    if request.whole {
        project.sign_project(&signer, &options).map_err(refused)?;
    }
    Ok(Signed {
        key_id: signer.key_id().to_owned(),
        branches: request.branches.clone(),
        whole: request.whole,
    })
}

/// Makes a new key: the secret file at `secret`, which must not exist, and the public one beside
/// it with `.pub` added. A project or a package is never taken as the name of a key.
pub fn new_key(secret: &Path) -> Result<KeyPair, CadError> {
    generate_key_files(secret).map_err(refused)
}

#[cfg(test)]
#[path = "signing_tests.rs"]
mod tests;
