//! Writing what the library can make of the model to a file chosen from the window: the script,
//! the STEP file and the text of the fit report. Everything that decides is here and is tested
//! without a screen: which formats there are, the name suggested for the file, the checks of the
//! target (it must not write over a file unless it was ordered to, nor over the project that is
//! open, nor to a name that belongs to a project or a sketch) and the writing itself.
//!
//! The formats are the ones the library has. The extrusions and pockets of the sketches are not
//! drawn by the STEP file (the answer is `PHOENIX-CAD-022`, as in the library) and the script
//! says so in a comment line.

use crate::browse::matches_extension;
use crate::enclosure::Enclosure;
use crate::scad::to_scad;
use crate::sketch::file as sketch_file;
use crate::step::export_step;
use crate::store;
use crate::window::PROJECT_EXTENSIONS;
use crate::CadError;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What can be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The model as an OpenSCAD-style script.
    Script,
    /// The model as a STEP file.
    Step,
    /// The text of the fit report: the measures, whether the board fits and the state of every
    /// step.
    Report,
    /// The bill of materials of the parts placed in the model, as comma separated values.
    Bill,
}

impl Format {
    /// Every format, in the order of the menu.
    pub const ALL: [Format; 4] = [Format::Script, Format::Step, Format::Report, Format::Bill];

    /// The extension of the files of the format, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Script => "scad",
            Format::Step => "step",
            Format::Report => "txt",
            Format::Bill => "csv",
        }
    }

    /// The word of the settings file for the format.
    pub fn name(self) -> &'static str {
        match self {
            Format::Script => "script",
            Format::Step => "step",
            Format::Report => "report",
            Format::Bill => "bill",
        }
    }

    /// The format of a word of the settings file.
    pub fn from_name(name: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.name() == name)
    }
}

/// The text of a format for a model. `text` is the text of the formats that are not a property of
/// the enclosure alone: the fit report for [`Format::Report`] and the bill of materials for
/// [`Format::Bill`], which need the settings and the language. The STEP file can be refused with
/// `PHOENIX-CAD-022`.
pub fn render(format: Format, enclosure: &Enclosure, text: &str) -> Result<String, CadError> {
    match format {
        Format::Script => Ok(to_scad(enclosure)),
        Format::Step => export_step(enclosure),
        Format::Report | Format::Bill => Ok(text.to_owned()),
    }
}

/// The name suggested for the file: the name of the project without its extension (`box` when
/// the project has no file yet) and the extension of the format.
pub fn suggested_name(project: Option<&Path>, format: Format) -> String {
    let stem = project
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "box".to_owned());
    format!("{stem}.{}", format.extension())
}

fn refuse(why: &str) -> CadError {
    CadError::Export(why.to_owned())
}

/// Whether a file can be written at `path`. Nothing is written. The answers:
///
/// * `PHOENIX-CAD-041` when the path has no file name, ends in the extension of a project or of
///   a sketch, is the project that is open, is a folder or is in a folder that does not exist;
/// * `PHOENIX-CAD-040` when a file is already there and `overwrite` is false.
pub fn check_target(
    path: &Path,
    overwrite: bool,
    open_project: Option<&Path>,
) -> Result<(), CadError> {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err(refuse("the path has no file name"));
    };
    let protected = [PROJECT_EXTENSIONS[0], sketch_file::EXTENSION];
    if matches_extension(&name, &protected) {
        return Err(refuse(
            "the name ends in the extension of a project or of a sketch",
        ));
    }
    if let Some(project) = open_project {
        let same = path == project
            || match (fs::canonicalize(path), fs::canonicalize(project)) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            };
        if same {
            return Err(refuse("it is the project that is open"));
        }
    }
    let folder: PathBuf = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    if !folder.is_dir() {
        return Err(refuse("the folder does not exist"));
    }
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Err(refuse("it is a folder")),
        Ok(_) if !overwrite => Err(CadError::ExportExists(name)),
        _ => Ok(()),
    }
}

/// Writes `text` at `path` after the checks of [`check_target`] and returns the bytes written.
/// Without `overwrite` the file is created only if it does not exist (so that a file that appears
/// between the check and the writing is not lost); with it, the file is replaced atomically. A
/// text larger than `max_kib` KiB is refused (`PHOENIX-CAD-041`), and so is a failed writing,
/// which leaves no half-written file.
pub fn write_file(
    path: &Path,
    text: &str,
    overwrite: bool,
    open_project: Option<&Path>,
    max_kib: u32,
) -> Result<usize, CadError> {
    if text.len() as u64 > u64::from(max_kib) * 1024 {
        return Err(CadError::Export(format!(
            "the text has {} bytes, more than the {max_kib} KiB allowed",
            text.len()
        )));
    }
    check_target(path, overwrite, open_project)?;
    if overwrite {
        store::save_atomic(path, text.as_bytes(), 0)
            .map_err(|e| CadError::Export(format!("cannot write the file: {e}")))?;
        return Ok(text.len());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => CadError::ExportExists(
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            _ => CadError::Export(format!("cannot create the file: {e}")),
        })?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(CadError::Export(format!("cannot write the file: {e}")));
    }
    Ok(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::EnclosureParams;
    use crate::Length;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("phoenix-cad-export-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn enclosure() -> Enclosure {
        let mm = |v| Length::from_mm(v).unwrap();
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    #[test]
    fn the_formats_have_their_extensions_and_words() {
        let all: Vec<_> = Format::ALL
            .iter()
            .map(|f| (f.extension(), f.name()))
            .collect();
        assert_eq!(
            all,
            [
                ("scad", "script"),
                ("step", "step"),
                ("txt", "report"),
                ("csv", "bill")
            ]
        );
        for f in Format::ALL {
            assert_eq!(Format::from_name(f.name()), Some(f));
        }
        assert_eq!(Format::from_name("stl"), None);
        assert_eq!(Format::from_name(""), None);
    }

    #[test]
    fn the_suggested_name_follows_the_project_and_the_format() {
        let project = Path::new("some/where/my box.phoenixproj");
        assert_eq!(suggested_name(Some(project), Format::Script), "my box.scad");
        assert_eq!(suggested_name(Some(project), Format::Step), "my box.step");
        assert_eq!(suggested_name(Some(project), Format::Report), "my box.txt");
        assert_eq!(suggested_name(Some(project), Format::Bill), "my box.csv");
        assert_eq!(suggested_name(None, Format::Step), "box.step");
        assert_eq!(
            suggested_name(Some(Path::new("")), Format::Report),
            "box.txt"
        );
    }

    #[test]
    fn the_formats_render_the_script_the_step_file_and_the_report_as_given() {
        let e = enclosure();
        let script = render(Format::Script, &e, "r").unwrap();
        // the same text as the library writes: the six master lengths in millimetres
        assert_eq!(script, to_scad(&e));
        assert!(script.contains("inner_length = 100;"), "{script}");
        assert!(script.contains("wall = 2;"), "{script}");
        let step = render(Format::Step, &e, "r").unwrap();
        assert!(step.starts_with("ISO-10303-21;"), "{step}");
        assert_eq!(render(Format::Report, &e, "one\ntwo").unwrap(), "one\ntwo");
        assert_eq!(render(Format::Bill, &e, "a,b\n").unwrap(), "a,b\n");
    }

    #[test]
    fn a_file_is_not_written_over_unless_it_was_ordered() {
        let dir = Scratch::new("overwrite");
        let target = dir.0.join("a.scad");
        // 5 bytes, then a second writing of 3 bytes without the order is refused with 040
        assert_eq!(write_file(&target, "hello", false, None, 64), Ok(5));
        let e = write_file(&target, "bye", false, None, 64).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-040");
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
        // with the order the file is replaced and nothing else is left in the folder
        assert_eq!(write_file(&target, "bye", true, None, 64), Ok(3));
        assert_eq!(fs::read_to_string(&target).unwrap(), "bye");
        let names: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.scad"]);
    }

    #[test]
    fn the_target_is_checked_before_anything_is_written() {
        let dir = Scratch::new("target");
        // a project may have been opened from a file of any name
        let project = dir.0.join("p.dat");
        fs::write(&project, "keep").unwrap();
        let cases = [
            (dir.0.join("p.dat"), "project that is open"),
            (dir.0.join("q.PHOENIXPROJ"), "extension"),
            (dir.0.join("q.phoenixsketch"), "extension"),
            (
                dir.0.join("no").join("such").join("a.scad"),
                "does not exist",
            ),
            (dir.0.clone(), "folder"),
        ];
        for (path, word) in cases {
            let e = write_file(&path, "x", true, Some(&project), 64).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-041", "{path:?}");
            assert!(e.to_string().contains(word), "{path:?}: {e}");
        }
        assert_eq!(fs::read_to_string(&project).unwrap(), "keep");
        // a folder with the name of the file is refused even with the order to overwrite
        let folder = dir.0.join("d.scad");
        fs::create_dir(&folder).unwrap();
        let e = check_target(&folder, true, None).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-041");
    }

    #[test]
    fn a_path_that_leads_to_the_open_project_by_another_way_is_refused() {
        let dir = Scratch::new("same");
        fs::create_dir(dir.0.join("sub")).unwrap();
        let project = dir.0.join("p.dat");
        fs::write(&project, "keep").unwrap();
        // the same file spelled with a folder that is entered and left again: the paths differ
        // as text, so the check that catches it is the identity of the file
        let roundabout = dir.0.join("sub").join("..").join("p.dat");
        assert_ne!(roundabout, project);
        let e = check_target(&roundabout, true, Some(&project)).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-041");
        assert!(e.to_string().contains("project that is open"), "{e}");
        assert_eq!(fs::read_to_string(&project).unwrap(), "keep");
    }

    #[test]
    fn a_text_larger_than_the_setting_is_refused_at_the_byte() {
        let dir = Scratch::new("size");
        let target = dir.0.join("big.txt");
        // 1 KiB is 1024 bytes: 1024 are written, 1025 are not
        let fits = "x".repeat(1024);
        let over = "x".repeat(1025);
        assert_eq!(write_file(&target, &fits, false, None, 1), Ok(1024));
        let e = write_file(&target, &over, true, None, 1).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-041");
        assert!(e.to_string().contains("1025 bytes"), "{e}");
        assert_eq!(fs::read_to_string(&target).unwrap().len(), 1024);
    }

    #[test]
    fn a_relative_name_without_a_folder_is_looked_for_in_the_current_one() {
        // the check alone writes nothing: a name with no folder is judged in the current folder
        let e = check_target(Path::new("p.phoenixproj"), true, None).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-041");
        assert!(check_target(
            Path::new("phoenix-cad-export-never-there.scad"),
            false,
            None
        )
        .is_ok());
        let e = check_target(Path::new(""), false, None).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-041");
    }
}
