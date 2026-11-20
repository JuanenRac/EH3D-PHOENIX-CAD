// SPDX-License-Identifier: GPL-3.0-or-later
//! Starting points for a new project: the four sample projects that ship with the program (an
//! enclosure, the same enclosure with a plate raised from a sketch, a frame of tubes and brackets,
//! and a small assembly with mates), kept inside the program so that `new` writes one to a file
//! wherever the program is installed.
//!
//! A template is the project file byte for byte, so it opens in the window and in every command
//! like any other project. [`create`] never writes over a file that exists unless it is told to,
//! and a name that is not a template is refused with the code of an unknown name.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use crate::lang::{Lang, Msg};
use crate::store;
use crate::CadError;

/// A sample project that a new one can start from.
#[derive(Debug, Clone, Copy)]
pub struct Template {
    /// The name that `new --template` takes.
    pub name: &'static str,
    /// What the project is, in the languages of the window.
    pub about: Msg,
    /// The project file.
    pub bytes: &'static [u8],
}

/// The templates, in the order they are listed.
pub const ALL: [Template; 4] = [
    Template {
        name: "box",
        about: Msg::TemplateBox,
        bytes: include_bytes!("../examples/box.phoenixproj"),
    },
    Template {
        name: "plate",
        about: Msg::TemplatePlate,
        bytes: include_bytes!("../examples/plate.phoenixproj"),
    },
    Template {
        name: "frame",
        about: Msg::TemplateFrame,
        bytes: include_bytes!("../examples/frame.phoenixproj"),
    },
    Template {
        name: "assembly",
        about: Msg::TemplateAssembly,
        bytes: include_bytes!("../examples/assembly.phoenixproj"),
    },
];

/// The template with a name (letter case matters), or `None`.
pub fn find(name: &str) -> Option<&'static Template> {
    ALL.iter().find(|t| t.name == name)
}

/// The templates, one to a line: the name, two spaces and what it is, in a language.
pub fn list_text(lang: Lang) -> String {
    let width = ALL.iter().map(|t| t.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    for t in &ALL {
        out.push_str(&format!("{:<width$}  {}\n", t.name, t.about.tr(lang)));
    }
    out
}

/// Writes the template `name` to `out` and returns the bytes written. Without `overwrite` a file
/// that exists is never touched (`PHOENIX-CAD-040`); with it the file is replaced atomically. The
/// folder of the file must exist and the file must not be a folder.
pub fn create(name: &str, out: &Path, overwrite: bool) -> Result<usize, CadError> {
    let template = find(name).ok_or_else(|| CadError::UnknownName(name.to_owned()))?;
    let file_name = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| CadError::Io("the path has no file name".to_owned()))?;
    let folder = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if !folder.is_dir() || out.is_dir() {
        return Err(CadError::Io(format!(
            "{file_name}: the folder does not exist or the name is a folder"
        )));
    }
    if overwrite {
        store::save_atomic(out, template.bytes, 0)?;
        return Ok(template.bytes.len());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => CadError::ExportExists(file_name.clone()),
            _ => CadError::Io(format!("{file_name}: {e}")),
        })?;
    if let Err(e) = file
        .write_all(template.bytes)
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(out);
        return Err(CadError::Io(format!("{file_name}: {e}")));
    }
    Ok(template.bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::uiconfig::UiSettings;
    use crate::window::Window;
    use std::fs;
    use std::path::PathBuf;

    fn temp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cad18_templates_{}_{tag}.phoenixproj",
            std::process::id()
        ))
    }

    #[test]
    fn the_names_are_unique_and_the_files_are_those_of_the_examples() {
        let names: Vec<&str> = ALL.iter().map(|t| t.name).collect();
        assert_eq!(names, ["box", "plate", "frame", "assembly"]);
        for (i, a) in ALL.iter().enumerate() {
            assert!(ALL[i + 1..].iter().all(|b| b.name != a.name));
            let on_disk = fs::read(format!(
                "{}/examples/{}.phoenixproj",
                env!("CARGO_MANIFEST_DIR"),
                a.name
            ))
            .unwrap();
            assert_eq!(a.bytes, on_disk.as_slice(), "{}", a.name);
            assert!(a.bytes.starts_with(b"PHOENIX-CAD-PROJECT 1"));
        }
        assert!(find("frame").is_some());
        assert!(find("Frame").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn every_template_is_written_and_opens_as_a_project() {
        for t in &ALL {
            let path = temp(t.name);
            let _ = fs::remove_file(&path);
            let written = create(t.name, &path, false).unwrap();
            assert_eq!(written, t.bytes.len());
            assert_eq!(fs::read(&path).unwrap(), t.bytes);
            let mut win =
                Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
            assert!(win.open(&path), "{}", t.name);
            if t.name == "frame" || t.name == "assembly" {
                assert!(!win.bom().unwrap().lines.is_empty(), "{}", t.name);
            }
            let _ = fs::remove_file(&path);
        }
    }

    #[test]
    fn a_file_that_exists_is_kept_unless_the_person_says_otherwise_and_an_unknown_name_is_refused()
    {
        let path = temp("again");
        let _ = fs::remove_file(&path);
        create("box", &path, false).unwrap();
        let e = create("plate", &path, false).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-040");
        assert_eq!(fs::read(&path).unwrap(), find("box").unwrap().bytes);
        create("plate", &path, true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), find("plate").unwrap().bytes);
        let _ = fs::remove_file(&path);
        let e = create("nothing", &path, false).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-014");
        assert!(!path.exists());
        // a folder that is not there, and a folder given as the file
        let missing = std::env::temp_dir()
            .join("cad18_no_such_folder")
            .join("a.phoenixproj");
        assert_eq!(
            create("box", &missing, false).unwrap_err().code(),
            "PHOENIX-CAD-012"
        );
        assert_eq!(
            create("box", &std::env::temp_dir(), true)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-012"
        );
    }

    #[test]
    fn the_list_has_one_line_for_each_template_in_every_language() {
        for lang in Lang::ALL {
            let text = list_text(lang);
            assert_eq!(text.lines().count(), 4, "{}", lang.code());
            for (line, t) in text.lines().zip(&ALL) {
                assert!(line.starts_with(t.name));
                assert!(line.ends_with(t.about.tr(lang)));
            }
        }
        let en = list_text(Lang::En);
        assert!(en.starts_with("box       An enclosure"), "{en}");
        assert_ne!(list_text(Lang::En), list_text(Lang::Pt));
    }
}
