// SPDX-License-Identifier: GPL-3.0-or-later
//! What the submenu of the window for a project file of the suite (`.eh3d`) does: open it (also
//! one that holds no box yet), save it and save it under another name without losing a branch,
//! keep the original of a file of another program with the report of what was read of it, list the
//! branches with their state, and take the board of the branch `pcb` for the box.
//!
//! The window (`gui`) only draws and asks; everything that decides is here and is tested without
//! a screen. Only the branches `cad`, `docs/cad` and the metadata of `common` are written; the
//! branch of the board is only read. Nothing here reads the clock.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eh3d_project::meta::{ConversionReport, LocalText};
use eh3d_project::SignatureStatus;

use crate::config::MeshSettings;
use crate::eh3dfile::{self, BranchLine, SaveSettings, Summary, EXTENSION};
use crate::interchange::{self, ReadLimits};
use crate::lang::{fill, Lang, Msg};
use crate::step_import::StepLimits;
use crate::suite_cli;
use crate::window::{error_text, Status, Window};
use crate::{store, CadError};

/// The extensions of the files of other programs whose original an import keeps.
pub const IMPORT_EXTENSIONS: [&str; 8] = ["stl", "obj", "3mf", "step", "stp", "ply", "off", "dxf"];

/// The extensions of the files that the dialogs of the submenu open and write.
pub const SUITE_EXTENSIONS: [&str; 1] = [EXTENSION];

/// The largest file of another program that an import keeps.
pub const MAX_IMPORT_BYTES: u64 = 256 << 20;

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The file of the suite that the window is working on: the one of its project.
pub fn current_file(win: &Window) -> Option<PathBuf> {
    win.path
        .as_deref()
        .filter(|p| eh3dfile::is_eh3d(p))
        .map(Path::to_path_buf)
}

/// How a file of the suite was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// Its box is in the window.
    Project,
    /// It holds no box of this program: the window has a new one that is saved into it.
    NoBox,
    /// It was not opened; the status line says why.
    Refused,
}

/// Opens a file of the suite. A file that holds only the branches of other programs gives a new
/// box that is saved into it, so that the box can then be made from its board.
pub fn open(win: &mut Window, file: &Path) -> Opened {
    match eh3dfile::has_project(file) {
        Err(e) => {
            win.status = Status::Error(e);
            Opened::Refused
        }
        Ok(true) => {
            if win.open(file) {
                Opened::Project
            } else {
                Opened::Refused
            }
        }
        Ok(false) => {
            win.new_project_in(file);
            if win.path.as_deref() != Some(file) {
                return Opened::Refused;
            }
            win.status = Status::Done(Msg::SuiteNoCad, vec![name_of(file)]);
            Opened::NoBox
        }
    }
}

/// Opens whatever project file was chosen, dropped or picked from the recent ones: a file of the
/// suite as [`open`] does, any other as the window does. False when it was not opened.
pub fn open_any(win: &mut Window, path: &Path) -> bool {
    if eh3dfile::is_eh3d(path) {
        open(win, path) != Opened::Refused
    } else {
        win.open(path)
    }
}

/// The name suggested to save the project as a file of the suite.
pub fn suggested_name(win: &Window) -> String {
    let stem = win
        .path
        .as_deref()
        .and_then(Path::file_stem)
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "box".to_owned());
    format!("{stem}.{EXTENSION}")
}

/// What the entry that saves the suite project did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved {
    /// It was written to its file (true) or refused (false; the status line says why).
    Written(bool),
    /// The project is not a file of the suite yet: a name has to be asked for.
    NeedsName,
}

/// Saves into the file of the suite of the window, or says that it has none.
pub fn save(win: &mut Window) -> Saved {
    if current_file(win).is_some() {
        Saved::Written(win.save())
    } else {
        Saved::NeedsName
    }
}

/// Saves the project as a file of the suite with another name. Every branch of the file that was
/// open goes with it. A file that holds the box of another project is written over only when
/// `overwrite` is set.
pub fn save_as(win: &mut Window, target: &Path, overwrite: bool) -> bool {
    if !eh3dfile::is_eh3d(target) {
        win.status = Status::Error(CadError::SuiteFile(format!(
            "the file of the suite ends in .{EXTENSION}"
        )));
        return false;
    }
    let same = win.path.as_deref() == Some(target);
    if !same && !overwrite && target.exists() {
        match eh3dfile::has_project(target) {
            Ok(false) => {}
            Ok(true) => {
                win.status = Status::Error(CadError::ExportExists(name_of(target)));
                return false;
            }
            Err(e) => {
                win.status = Status::Error(e);
                return false;
            }
        }
    }
    win.save_to(target)
}

/// What an import read and kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    /// The name of the file.
    pub name: String,
    /// The solids read from it.
    pub solids: usize,
    /// Their triangles.
    pub triangles: usize,
    /// The report that was kept next to the original.
    pub report: ConversionReport,
}

/// Why a file was not imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// The file could not be read as what its extension says.
    Unreadable(String),
    /// It could not be loaded or kept.
    Suite(CadError),
}

impl ImportError {
    /// The reason as a line of text.
    pub fn text(&self, lang: Lang) -> String {
        match self {
            ImportError::Unreadable(why) => why.clone(),
            ImportError::Suite(e) => error_text(e, lang),
        }
    }
}

/// Keeps the original of a file of another program in the branch `cad`, byte for byte, with the
/// report of what was read of it: the solids, the triangles and the warnings of the reader as
/// what was not read.
pub fn import_original(
    target: &Path,
    foreign: &Path,
    mesh: &MeshSettings,
    how: &SaveSettings,
) -> Result<Imported, ImportError> {
    let name = foreign
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ImportError::Unreadable("the file to keep has no name".to_owned()))?;
    let format = foreign
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = store::load_limited(foreign, MAX_IMPORT_BYTES).map_err(ImportError::Suite)?;
    let limits = ReadLimits {
        step: StepLimits::from_settings(mesh),
        ..ReadLimits::shipped()
    };
    let read =
        interchange::read_file_checked(name, &bytes, &limits).map_err(ImportError::Unreadable)?;
    let triangles: usize = read.bodies.iter().map(|(_, m)| m.triangles.len()).sum();
    let mut warned: BTreeMap<&str, u64> = BTreeMap::new();
    for warning in &read.warnings {
        *warned.entry(warning.name()).or_insert(0) += 1;
    }
    let count = eh3dfile::ImportCount {
        read: vec![
            ("solids".to_owned(), read.bodies.len() as u64),
            ("triangles".to_owned(), triangles as u64),
        ],
        not_read: warned.into_iter().map(|(n, c)| (n.to_owned(), c)).collect(),
    };
    let report = eh3dfile::keep_origin(target, &format, name, &bytes, &count, how)
        .map_err(ImportError::Suite)?;
    Ok(Imported {
        name: name.to_owned(),
        solids: read.bodies.len(),
        triangles,
        report,
    })
}

/// The lines that tell what the report of an original says.
pub fn report_lines(report: &ConversionReport, lang: Lang) -> Vec<String> {
    let mut out = vec![fill(
        Msg::SuiteOriginal.tr(lang),
        &[&report.source.name, &report.source.size.to_string()],
    )];
    let list = |items: &[eh3d_project::meta::Counted]| {
        items
            .iter()
            .map(|c| format!("{} {}", c.item, c.count))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !report.read.is_empty() {
        out.push(fill(Msg::SuiteReportRead.tr(lang), &[&list(&report.read)]));
    }
    if !report.not_read.is_empty() {
        let text = list(&report.not_read);
        out.push(fill(Msg::SuiteReportNotRead.tr(lang), &[&text]));
    }
    out
}

/// The branches of a file of the suite with what is kept about the last import.
#[derive(Debug, Clone)]
pub struct Branches {
    /// The file.
    pub file: PathBuf,
    /// The branches and what the checks found.
    pub summary: Summary,
    /// The report of the original that the branch `cad` keeps.
    pub original: Option<ConversionReport>,
    /// The public key files that could not be used, and why.
    pub key_problems: Vec<String>,
}

/// Reads the branches of a file of the suite.
pub fn branches(file: &Path) -> Result<Branches, CadError> {
    branches_with(file, &[])
}

/// [`branches`], with the signatures checked with the public key files named (those of
/// `suite.trusted_keys` and the ones chosen in the window).
pub fn branches_with(file: &Path, key_files: &[String]) -> Result<Branches, CadError> {
    let (keys, key_problems) = eh3dfile::keyring(key_files);
    Ok(Branches {
        file: file.to_path_buf(),
        summary: eh3dfile::summary_with(file, Some(&keys))?,
        // a report that cannot be read leaves the list of branches whole
        original: eh3dfile::origin_report(file).unwrap_or(None),
        key_problems,
    })
}

/// What is known about a signature, in words; `None` when there is none.
pub fn signature_text(signature: &SignatureStatus, lang: Lang) -> Option<String> {
    Some(match signature {
        SignatureStatus::Unsigned => return None,
        SignatureStatus::Unchecked => Msg::SuiteStateSigned.tr(lang).to_owned(),
        SignatureStatus::Valid(key) => fill(Msg::SuiteSigValid.tr(lang), &[key]),
        SignatureStatus::Invalid => Msg::SuiteSigInvalid.tr(lang).to_owned(),
        SignatureStatus::UnknownKey => Msg::SuiteSigUnknown.tr(lang).to_owned(),
    })
}

/// The title and the description of the project of the suite in a language, empty where it has
/// none.
pub fn read_texts(file: &Path, lang: Lang) -> Result<(String, String), CadError> {
    let metadata = eh3dfile::read_metadata(file)?;
    let key = eh3dfile::language_key(lang.code());
    let of = |texts: &LocalText| texts.get(key).cloned().unwrap_or_default();
    Ok((of(&metadata.title), of(&metadata.description)))
}

/// Writes the title and the description of one language into the file of the suite of the window;
/// an empty text takes that language away. Only the shared branch `common` is written, so the
/// changes of the box that are not saved stay as they are. The answer is what to say, or why
/// nothing was written (a read-only project, no file yet, or a language other than English before
/// the project has the text in English).
pub fn write_texts(
    win: &Window,
    lang_of_texts: Lang,
    title: &str,
    description: &str,
) -> Result<String, String> {
    let lang = win.lang;
    if win.read_only {
        return Err(Msg::ReadOnlyBadge.tr(lang).to_owned());
    }
    let file = current_file(win)
        .filter(|f| f.is_file())
        .ok_or_else(|| Msg::SuiteNeedFile.tr(lang).to_owned())?;
    let how = eh3dfile::SaveSettings {
        keep_backup: win.settings.backups > 0,
        ..eh3dfile::SaveSettings::default()
    };
    eh3dfile::set_metadata(
        &file,
        lang_of_texts.code(),
        Some(title.trim()),
        Some(description.trim()),
        &how,
    )
    .map_err(|e| error_text(&e, lang))?;
    Ok(fill(
        Msg::SuiteMetaSet.tr(lang),
        &[&file.display().to_string()],
    ))
}

/// The state of a branch in words: current, or out of date with the branches that changed, and
/// then whether it is signed, sealed or encrypted.
pub fn state_text(line: &BranchLine, lang: Lang) -> String {
    let mut words = vec![if line.stale.is_empty() {
        Msg::SuiteStateCurrent.tr(lang).to_owned()
    } else {
        fill(Msg::SuiteStateStale.tr(lang), &[&line.stale.join(", ")])
    }];
    words.extend(signature_text(&line.signature, lang));
    if line.sealed {
        words.push(Msg::SuiteStateSealed.tr(lang).to_owned());
    }
    if line.encrypted {
        words.push(Msg::SuiteStateEncrypted.tr(lang).to_owned());
    }
    words.join("; ")
}

/// The files of the suite among the files opened last, the newest first.
pub fn recent_projects(recent: &[String]) -> Vec<String> {
    recent
        .iter()
        .filter(|p| eh3dfile::is_eh3d(Path::new(p.as_str())))
        .cloned()
        .collect()
}

/// What taking the board did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardTaken {
    /// The lines to show.
    pub text: String,
    /// False when something is left for the person: a post without its hole, a hole without a
    /// post or a step that no longer applies.
    pub complete: bool,
    /// True when the box was made now, false when the one there was followed the board.
    pub made: bool,
}

/// Takes the board of the branch `pcb` into the box of the file of the suite of the window: makes
/// the box when the file has none, or moves the box that is there with the changes of the board.
/// The file is read and written, so changes that are not saved are refused first; the window then
/// shows what is in the file.
pub fn take_board(win: &mut Window) -> Result<BoardTaken, String> {
    let lang = win.lang;
    let file = current_file(win).ok_or_else(|| Msg::SuiteNeedFile.tr(lang).to_owned())?;
    let say = |e: CadError| error_text(&e, lang);
    let has = eh3dfile::has_project(&file).map_err(say)?;
    // a box that was never saved and never edited has nothing to lose
    if win.dirty && (has || win.can_undo()) {
        return Err(Msg::SuiteSaveFirst.tr(lang).to_owned());
    }
    let settings = suite_cli::All {
        lib: win.settings.clone(),
        ui: win.ui.clone(),
        sketch: win.sketch_settings.clone(),
    };
    let how = SaveSettings {
        keep_backup: win.settings.backups > 0,
        ..SaveSettings::default()
    };
    let taken = if has {
        let (text, complete) = suite_cli::follow_board(&file, &settings, lang, &how)?;
        BoardTaken {
            text,
            complete,
            made: false,
        }
    } else {
        let request = suite_cli::board_request(&settings.ui);
        let text = suite_cli::make_box(&file, &settings, &request, lang, &how)?;
        BoardTaken {
            text,
            complete: true,
            made: true,
        }
    };
    if !win.open(&file) {
        return Err(win.status.text(lang));
    }
    Ok(taken)
}

/// What the window can ask of a box made from a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardWork {
    /// Does the box agree with the board and does every part have room?
    CheckFit,
    /// Cut the openings of the connectors and write them into the recipe.
    CutOpenings,
    /// Hand the solid, the flat faces, the posts and the documents back to the other programs.
    HandBack,
}

/// What it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worked {
    /// The lines to show.
    pub text: String,
    /// False when the verification did not pass.
    pub passed: bool,
}

/// Does one of the works of the board on the file of the window, as the commands `eh3d-check`,
/// `eh3d-openings --apply` and `eh3d-publish` do with the section `[suite]` of the settings. The
/// file is what is worked on, so changes that are not saved are refused first.
pub fn board_work(win: &mut Window, work: BoardWork) -> Result<Worked, String> {
    let lang = win.lang;
    let file = current_file(win).ok_or_else(|| Msg::SuiteNeedFile.tr(lang).to_owned())?;
    if win.dirty {
        return Err(Msg::SuiteSaveFirst.tr(lang).to_owned());
    }
    let settings = suite_cli::All {
        lib: win.settings.clone(),
        ui: win.ui.clone(),
        sketch: win.sketch_settings.clone(),
    };
    let how = SaveSettings {
        keep_backup: win.settings.backups > 0,
        ..SaveSettings::default()
    };
    let suite = &settings.ui.suite;
    let (text, passed) = match work {
        BoardWork::CheckFit => suite_cli::check_fit(&file, &settings, suite.part_clearance, lang)?,
        BoardWork::CutOpenings => {
            suite_cli::cut_openings(&file, &settings, suite.opening_clearance, lang, Some(&how))?
        }
        BoardWork::HandBack => {
            let sheet = (suite.sheet_width, suite.sheet_gap);
            suite_cli::hand_back(&file, &settings, sheet, lang, &how)?
        }
    };
    // the recipe changed in the file: the window shows what is there
    if work == BoardWork::CutOpenings && !win.open(&file) {
        return Err(win.status.text(lang));
    }
    Ok(Worked { text, passed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boardart::tests::{mm, sample};
    use crate::boardart::{self, SOURCE_BRANCH, SOURCE_ENTRY};
    use crate::config::Settings;
    use crate::eh3dfile::tests::{other_branch, Scratch, Seeded};
    use crate::eh3dfile::BRANCH;
    use crate::uiconfig::UiSettings;

    fn window() -> Window {
        Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap()
    }

    fn bytes_of(file: &Path, branch: &str) -> BTreeMap<String, Vec<u8>> {
        let mut project = eh3dfile::open_project(file).unwrap();
        eh3dfile::branch_files(&mut project, branch).unwrap()
    }

    fn has_board(file: &Path) -> bool {
        let cad = bytes_of(file, BRANCH);
        cad.contains_key(boardart::SNAPSHOT_ENTRY)
    }

    fn board(file: &Path, board: &boardart::BoardArtifact) {
        other_branch(
            file,
            SOURCE_BRANCH,
            SOURCE_ENTRY,
            &boardart::to_bytes(board).unwrap(),
        );
    }

    const STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\n\
                       vertex 0 1 0\nendloop\nendfacet\nendsolid t\n";

    #[test]
    fn a_file_with_only_a_board_opens_as_a_new_box_that_is_saved_into_it() {
        let dir = Scratch::new("win-open");
        let file = dir.path("job.eh3d");
        board(&file, &sample());
        let before = std::fs::read(&file).unwrap();
        let mut win = window();
        assert_eq!(open(&mut win, &file), Opened::NoBox);
        assert_eq!(current_file(&win), Some(file.clone()));
        assert!(win.dirty);
        assert!(win.status.text(Lang::En).contains("job.eh3d"));
        // opening writes nothing
        assert_eq!(std::fs::read(&file).unwrap(), before);
        let pcb = bytes_of(&file, SOURCE_BRANCH);
        assert_eq!(save(&mut win), Saved::Written(true));
        assert!(eh3dfile::has_project(&file).unwrap());
        assert_eq!(bytes_of(&file, SOURCE_BRANCH), pcb);
        assert_eq!(open(&mut window(), &file), Opened::Project);
    }

    #[test]
    fn a_file_that_is_not_of_the_suite_is_refused_and_the_window_stays() {
        let dir = Scratch::new("win-junk");
        let junk = dir.path("junk.eh3d");
        std::fs::write(&junk, b"not a zip").unwrap();
        let mut win = window();
        assert_eq!(open(&mut win, &junk), Opened::Refused);
        assert_eq!(win.path, None);
        assert!(win.status.text(Lang::En).contains("PHOENIX-CAD-064"));
        assert!(!open_any(&mut win, &dir.path("nothing.eh3d")));
        assert!(!open_any(&mut win, &dir.path("nothing.phoenixproj")));
    }

    #[test]
    fn save_as_takes_every_branch_along_and_asks_before_writing_over_another_box() {
        let dir = Scratch::new("win-saveas");
        let first = dir.path("a.eh3d");
        let mut random = Seeded(0x00CA_DB27);
        for id in ["pcb", "laser", "3dp", "cnc"] {
            other_branch(&first, id, "data.bin", &random.bytes(120));
        }
        let mut win = window();
        assert_eq!(open(&mut win, &first), Opened::NoBox);
        assert!(win.save());
        let second = dir.path("b.eh3d");
        assert!(save_as(&mut win, &second, false), "{:?}", win.status);
        assert_eq!(win.path.as_deref(), Some(second.as_path()));
        for id in ["pcb", "laser", "3dp", "cnc", BRANCH] {
            assert_eq!(bytes_of(&first, id), bytes_of(&second, id), "{id}");
        }
        // the plain Save as of the window keeps them too
        let third = dir.path("c.eh3d");
        assert!(win.save_to(&third));
        assert_eq!(bytes_of(&first, "laser"), bytes_of(&third, "laser"));
        // a file that holds a box is not written over unless that is asked
        let kept = std::fs::read(&first).unwrap();
        assert!(!save_as(&mut win, &first, false));
        assert!(win.status.text(Lang::En).contains("PHOENIX-CAD-040"));
        assert_eq!(std::fs::read(&first).unwrap(), kept);
        assert!(save_as(&mut win, &first, true));
        // a file with only other branches is not a box to lose
        let boards = dir.path("d.eh3d");
        other_branch(&boards, "pcb", "x.bin", b"x");
        assert!(save_as(&mut win, &boards, false));
        assert_eq!(bytes_of(&boards, "pcb").get("x.bin").unwrap(), b"x");
        assert!(!save_as(&mut win, &dir.path("e.phoenixproj"), true));
    }

    #[test]
    fn a_project_that_is_not_of_the_suite_asks_for_a_name_and_gets_one_suggested() {
        let mut win = window();
        assert_eq!(save(&mut win), Saved::NeedsName);
        assert_eq!(suggested_name(&win), "box.eh3d");
        win.path = Some(PathBuf::from("dir/case.phoenixproj"));
        assert_eq!(save(&mut win), Saved::NeedsName);
        assert_eq!(suggested_name(&win), "case.eh3d");
        assert_eq!(current_file(&win), None);
    }

    #[test]
    fn an_import_keeps_the_original_and_says_what_was_read() {
        let dir = Scratch::new("win-import");
        let file = dir.path("p.eh3d");
        let stl = dir.path("part.stl");
        std::fs::write(&stl, STL).unwrap();
        let mesh = Settings::defaults().mesh;
        let how = SaveSettings::default();
        let done = import_original(&file, &stl, &mesh, &how).unwrap();
        assert_eq!((done.solids, done.triangles), (1, 1));
        assert_eq!(
            bytes_of(&file, BRANCH).get("origin/part.stl").unwrap(),
            STL.as_bytes()
        );
        let lines = report_lines(&done.report, Lang::En);
        assert!(lines[0].contains("part.stl"), "{lines:?}");
        assert!(lines[1].contains("triangles 1"), "{lines:?}");
        let listed = branches(&file).unwrap();
        assert_eq!(listed.original, Some(done.report));
        // a file that cannot be read keeps nothing
        let bad = dir.path("bad.off");
        std::fs::write(&bad, b"nonsense").unwrap();
        let before = std::fs::read(&file).unwrap();
        let error = import_original(&file, &bad, &mesh, &how).unwrap_err();
        assert!(!error.text(Lang::En).is_empty());
        assert!(matches!(error, ImportError::Unreadable(_)));
        assert_eq!(std::fs::read(&file).unwrap(), before);
        let missing = import_original(&file, &dir.path("no.stl"), &mesh, &how);
        assert!(matches!(missing, Err(ImportError::Suite(_))));
        for e in IMPORT_EXTENSIONS {
            assert_eq!(e, e.to_ascii_lowercase());
        }
    }

    #[test]
    fn the_state_of_a_branch_is_told_in_every_language() {
        let mut line = BranchLine {
            id: "cad".to_owned(),
            app: "cad".to_owned(),
            format: "1.0".to_owned(),
            files: 2,
            bytes: 10,
            sealed: false,
            encrypted: false,
            signature: SignatureStatus::Unsigned,
            stale: Vec::new(),
        };
        assert_eq!(state_text(&line, Lang::En), "current");
        line.stale = vec!["pcb".to_owned()];
        line.signature = SignatureStatus::Unchecked;
        line.sealed = true;
        line.encrypted = true;
        assert_eq!(
            state_text(&line, Lang::En),
            "out of date: pcb has changed; signed (not checked); sealed; encrypted"
        );
        for l in Lang::ALL {
            assert_eq!(state_text(&line, l).matches("; ").count(), 3, "{l:?}");
        }
    }

    #[test]
    fn the_title_and_the_description_are_written_and_read_in_each_language() {
        let dir = crate::eh3dfile::tests::Scratch::new("texts");
        let file = dir.path("box.eh3d");
        let mut win = Window::new(
            Lang::En,
            crate::config::Settings::defaults(),
            crate::uiconfig::UiSettings::defaults(),
        )
        .unwrap();
        // there is no file of the suite yet
        let why = write_texts(&win, Lang::En, "Box", "").unwrap_err();
        assert_eq!(why, Msg::SuiteNeedFile.tr(Lang::En));
        assert!(win.save_to(&file), "{:?}", win.status);
        assert_eq!(read_texts(&file, Lang::En).unwrap(), Default::default());
        // another language needs the English text first, and the refusal writes nothing
        let before = std::fs::read(&file).unwrap();
        assert!(write_texts(&win, Lang::Es, "Caja", "").is_err());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        write_texts(&win, Lang::En, " Box ", "Two lines:\nlid and body").unwrap();
        write_texts(&win, Lang::Pt, "Caixa", "").unwrap();
        assert_eq!(
            read_texts(&file, Lang::En).unwrap(),
            ("Box".to_owned(), "Two lines:\nlid and body".to_owned())
        );
        assert_eq!(
            read_texts(&file, Lang::Pt).unwrap(),
            ("Caixa".to_owned(), String::new())
        );
        // an empty field takes the text of that language away
        write_texts(&win, Lang::Pt, "", "").unwrap();
        assert_eq!(read_texts(&file, Lang::Pt).unwrap(), Default::default());
        write_texts(&win, Lang::En, "Box", "").unwrap();
        assert_eq!(read_texts(&file, Lang::En).unwrap().1, "");
        // the box itself is still there and opens
        assert!(win.open(&file), "{:?}", win.status);
        win.read_only = true;
        assert!(write_texts(&win, Lang::En, "Other", "").is_err());
        assert_eq!(read_texts(&file, Lang::En).unwrap().0, "Box");
    }

    #[test]
    fn a_signature_is_checked_only_with_a_public_key() {
        use eh3d_project::ed25519::generate_key_files;
        use eh3d_project::{Ed25519Signer, SaveOptions};

        let dir = crate::eh3dfile::tests::Scratch::new("signatures");
        let file = dir.path("box.eh3d");
        eh3dfile::save_cad_project(&file, b"project", &SaveSettings::default()).unwrap();
        let mine = generate_key_files(&dir.path("mine.key")).unwrap();
        let other = generate_key_files(&dir.path("other.key")).unwrap();
        let text = |p: &Path| p.display().to_string();
        let cad = |found: &Branches| {
            found
                .summary
                .branches
                .iter()
                .find(|b| b.id == "cad")
                .unwrap()
                .signature
                .clone()
        };
        assert_eq!(cad(&branches(&file).unwrap()), SignatureStatus::Unsigned);
        assert_eq!(signature_text(&SignatureStatus::Unsigned, Lang::En), None);

        let signer = Ed25519Signer::read(&mine.secret_path).unwrap();
        eh3dfile::open_project(&file)
            .unwrap()
            .sign_branch("cad", &signer, &SaveOptions::default())
            .unwrap();
        // without a key it is only seen
        let seen = branches(&file).unwrap();
        assert_eq!(cad(&seen), SignatureStatus::Unchecked);
        assert!(!seen.summary.any_wrong_signature());
        // with its public key it is valid, with another one the key is not trusted
        let checked = branches_with(&file, &[text(&mine.public_path)]).unwrap();
        assert_eq!(cad(&checked), SignatureStatus::Valid(mine.key_id.clone()));
        assert!(checked.key_problems.is_empty());
        let words = signature_text(&cad(&checked), Lang::En).unwrap();
        assert!(words.contains(&mine.key_id), "{words}");
        let stranger = branches_with(&file, &[text(&other.public_path)]).unwrap();
        assert_eq!(cad(&stranger), SignatureStatus::UnknownKey);
        // a secret key and a file that is not there are told and not used
        let bad =
            branches_with(&file, &[text(&mine.secret_path), "no-such.pub".to_owned()]).unwrap();
        assert_eq!(bad.key_problems.len(), 2, "{:?}", bad.key_problems);
        assert_eq!(cad(&bad), SignatureStatus::Unchecked);
        for lang in Lang::ALL {
            for status in [
                SignatureStatus::Unchecked,
                SignatureStatus::Invalid,
                SignatureStatus::UnknownKey,
            ] {
                assert!(!signature_text(&status, lang).unwrap().is_empty());
            }
        }

        // the command line says the same and takes the key
        let f = text(&file);
        let run = |extra: &[&str]| {
            let mut args = vec![f.clone()];
            args.extend(extra.iter().map(|w| (*w).to_owned()));
            suite_cli::run("eh3d-info", &args)
        };
        let (out, ok) = run(&[]).unwrap();
        assert!(ok && out.contains(",unchecked\n"), "{out}");
        let key = text(&mine.public_path);
        let (out, ok) = run(&["--key", &key]).unwrap();
        assert!(ok && out.contains(",valid\n"), "{out}");
        let (out, _) = run(&["--key", &key, "--text"]).unwrap();
        assert!(out.contains(&mine.key_id), "{out}");
        assert!(run(&["--key", "no-such.pub"]).is_err());
    }

    #[test]
    fn only_the_suite_files_are_listed_among_the_recent_ones() {
        let recent: Vec<String> = ["a.eh3d", "b.phoenixproj", "c.EH3D", "d.phoenixsketch"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(recent_projects(&recent), ["a.eh3d", "c.EH3D"]);
    }

    #[test]
    fn the_window_makes_the_box_from_the_board_and_then_follows_it() {
        let dir = Scratch::new("win-board");
        let file = dir.path("job.eh3d");
        let mut win = window();
        assert_eq!(
            take_board(&mut win).unwrap_err(),
            Msg::SuiteNeedFile.tr(Lang::En)
        );
        board(&file, &sample());
        assert_eq!(open(&mut win, &file), Opened::NoBox);
        assert!(!has_board(&file));
        let pcb = bytes_of(&file, SOURCE_BRANCH);
        let made = take_board(&mut win).unwrap();
        assert!(made.made && made.complete, "{made:?}");
        assert!(made.text.contains("84 x 54 x 24.6 mm"), "{}", made.text);
        assert!(!win.dirty && has_board(&file));
        assert_eq!(bytes_of(&file, SOURCE_BRANCH), pcb);
        let same = take_board(&mut win).unwrap();
        assert!(!same.made && same.complete, "{same:?}");
        assert!(same.text.contains("has not changed"), "{}", same.text);
        // the board moves a hole: the list says the box is out of date until it takes it
        let mut moved = sample();
        moved.board.holes[0].x = mm(10);
        board(&file, &moved);
        let cad = |f: &Path| {
            let found = branches(f).unwrap().summary;
            let line = found.branches.into_iter().find(|b| b.id == BRANCH);
            line.unwrap().stale
        };
        assert_eq!(cad(&file), ["pcb"]);
        let followed = take_board(&mut win).unwrap();
        assert!(followed.text.contains("moved"), "{}", followed.text);
        assert!(cad(&file).is_empty());
        // changes that are not saved are never thrown away
        win.add_variable("depth", "3 mm");
        assert!(win.dirty);
        assert_eq!(
            take_board(&mut win).unwrap_err(),
            Msg::SuiteSaveFirst.tr(Lang::En)
        );
    }

    #[test]
    fn the_window_checks_the_fit_cuts_the_openings_and_hands_the_box_back() {
        let dir = Scratch::new("win-work");
        let file = dir.path("job.eh3d");
        let mut win = window();
        let need = board_work(&mut win, BoardWork::CheckFit).unwrap_err();
        assert_eq!(need, Msg::SuiteNeedFile.tr(Lang::En));
        board(&file, &sample());
        assert_eq!(open(&mut win, &file), Opened::NoBox);
        let first = board_work(&mut win, BoardWork::CheckFit).unwrap_err();
        assert_eq!(first, Msg::SuiteSaveFirst.tr(Lang::En));
        take_board(&mut win).unwrap();
        let pcb = bytes_of(&file, SOURCE_BRANCH);
        let before = std::fs::read(&file).unwrap();
        let checked = board_work(&mut win, BoardWork::CheckFit).unwrap();
        assert!(checked.passed, "{checked:?}");
        assert!(checked.text.contains("has room"), "{}", checked.text);
        // a check writes nothing
        assert_eq!(std::fs::read(&file).unwrap(), before);
        let cut = board_work(&mut win, BoardWork::CutOpenings).unwrap();
        assert!(cut.passed, "{cut:?}");
        assert_eq!(cut.text, "J1: opening added\n");
        let again = board_work(&mut win, BoardWork::CutOpenings).unwrap();
        assert!(again.text.contains("already in place"), "{}", again.text);
        assert!(!win.dirty);
        let handed = board_work(&mut win, BoardWork::HandBack).unwrap();
        assert!(handed.text.contains("cad/mounts2d.json"), "{}", handed.text);
        assert!(handed.text.contains("docs/cad/bill.txt"), "{}", handed.text);
        assert!(bytes_of(&file, BRANCH).contains_key("export/solid.stl"));
        assert_eq!(bytes_of(&file, SOURCE_BRANCH), pcb);
        // a part too tall is found
        let mut tall = sample();
        tall.parts[0].height = mm(20);
        board(&file, &tall);
        let found = board_work(&mut win, BoardWork::CheckFit).unwrap();
        assert!(!found.passed, "{found:?}");
        assert!(found.text.contains("reaches the lid"), "{}", found.text);
    }

    #[test]
    fn a_file_without_a_board_says_so_and_writes_nothing() {
        let dir = Scratch::new("win-noboard");
        let file = dir.path("p.eh3d");
        other_branch(&file, "laser", "x.bin", b"x");
        let mut win = window();
        assert_eq!(open(&mut win, &file), Opened::NoBox);
        let before = std::fs::read(&file).unwrap();
        let why = take_board(&mut win).unwrap_err();
        assert!(why.contains("PHOENIX-CAD-065"), "{why}");
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }
}
