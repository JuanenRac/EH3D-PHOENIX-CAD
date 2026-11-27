//! Writing what the library can make of the model to a file chosen from the window or by the
//! `export` command: the script, the STEP file, the STL file (triangles, binary or text), the DXF
//! and SVG drawings of the plan, the front and the side, the text of the fit report and the bill
//! of materials. Everything that decides is here and is tested without a screen: which formats
//! there are, the name suggested for the file, the checks of the target (it must not write over a
//! file unless it was ordered to, nor over the project that is open, nor to a name that belongs to
//! a project or a sketch) and the writing itself.
//!
//! The STEP file holds the whole model (see [`crate::step::export_step_with`]); the script does
//! not draw the extrusions and pockets of the sketches and says so in a comment line; the STL file
//! and the drawings do draw them (see [`crate::solid`] for what the STL file can be made of).

use crate::assembly::Dim;
use crate::browse::matches_extension;
use crate::config::Settings;
use crate::drawing::{Drawing, View};
use crate::enclosure::Enclosure;
use crate::interchange;
use crate::lang::Lang;
use crate::mesh;
use crate::scad::to_scad;
use crate::sketch::file as sketch_file;
use crate::solid;
use crate::step::export_step_with;
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
    /// The solid of the model as triangles (binary or text as the settings say).
    Stl,
    /// The plan as a DXF drawing.
    DxfTop,
    /// The front as a DXF drawing.
    DxfFront,
    /// The side as a DXF drawing.
    DxfSide,
    /// The plan, the front and the side on one sheet, as a DXF drawing.
    DxfSheet,
    /// The plan as an SVG drawing.
    SvgTop,
    /// The front as an SVG drawing.
    SvgFront,
    /// The side as an SVG drawing.
    SvgSide,
    /// The plan, the front and the side on one sheet, as an SVG drawing.
    SvgSheet,
    /// The plan, the front and the side on one sheet, as a PDF file of one page.
    PdfSheet,
    /// The text of the fit report: the measures, whether the board fits and the state of every
    /// step.
    Report,
    /// The bill of materials of the parts placed in the model, as comma separated values.
    Bill,
    /// The solids of the model as a Wavefront OBJ file.
    Obj,
    /// The solids of the model as a 3MF package.
    ThreeMf,
    /// The order sheet: the bars to buy, the holes to drill and the parts with their cost, as one
    /// self-contained HTML page.
    Order,
}

impl Format {
    /// Every format, in the order of the menu.
    pub const ALL: [Format; 17] = [
        Format::Script,
        Format::Step,
        Format::Stl,
        Format::DxfTop,
        Format::DxfFront,
        Format::DxfSide,
        Format::DxfSheet,
        Format::SvgTop,
        Format::SvgFront,
        Format::SvgSide,
        Format::SvgSheet,
        Format::PdfSheet,
        Format::Report,
        Format::Bill,
        Format::Obj,
        Format::ThreeMf,
        Format::Order,
    ];

    /// The extension of the files of the format, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Script => "scad",
            Format::Step => "step",
            Format::Stl => "stl",
            Format::DxfTop | Format::DxfFront | Format::DxfSide | Format::DxfSheet => "dxf",
            Format::SvgTop | Format::SvgFront | Format::SvgSide | Format::SvgSheet => "svg",
            Format::PdfSheet => "pdf",
            Format::Report => "txt",
            Format::Bill => "csv",
            Format::Obj => "obj",
            Format::ThreeMf => "3mf",
            Format::Order => "html",
        }
    }

    /// The word of the settings file for the format.
    pub fn name(self) -> &'static str {
        match self {
            Format::Script => "script",
            Format::Step => "step",
            Format::Stl => "stl",
            Format::DxfTop => "dxf-top",
            Format::DxfFront => "dxf-front",
            Format::DxfSide => "dxf-side",
            Format::DxfSheet => "dxf-sheet",
            Format::SvgTop => "svg-top",
            Format::SvgFront => "svg-front",
            Format::SvgSide => "svg-side",
            Format::SvgSheet => "svg-sheet",
            Format::PdfSheet => "pdf-sheet",
            Format::Report => "report",
            Format::Bill => "bill",
            Format::Obj => "obj",
            Format::ThreeMf => "3mf",
            Format::Order => "order",
        }
    }

    /// The format of a word of the settings file.
    pub fn from_name(name: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.name() == name)
    }

    /// The format of a drawing of a view: DXF or SVG.
    pub fn drawing(dxf: bool, view: View) -> Format {
        match (dxf, view) {
            (true, View::Top) => Format::DxfTop,
            (true, View::Front) => Format::DxfFront,
            (true, View::Side) => Format::DxfSide,
            (false, View::Top) => Format::SvgTop,
            (false, View::Front) => Format::SvgFront,
            (false, View::Side) => Format::SvgSide,
        }
    }

    /// The format of the sheet of several views: DXF or SVG.
    pub fn sheet(dxf: bool) -> Format {
        if dxf {
            Format::DxfSheet
        } else {
            Format::SvgSheet
        }
    }

    /// Whether the format is a sheet of several views.
    pub fn is_sheet(self) -> bool {
        matches!(self, Format::DxfSheet | Format::SvgSheet | Format::PdfSheet)
    }

    /// The view of a drawing format, or `None` for the others and for the sheets.
    pub fn view(self) -> Option<View> {
        match self {
            Format::DxfTop | Format::SvgTop => Some(View::Top),
            Format::DxfFront | Format::SvgFront => Some(View::Front),
            Format::DxfSide | Format::SvgSide => Some(View::Side),
            _ => None,
        }
    }
}

/// The bytes of a format for a model: text, or the binary STL file. `text` is the text of the
/// formats that are not a property of the enclosure alone: the fit report for [`Format::Report`]
/// the bill of materials for [`Format::Bill`] and the order sheet for [`Format::Order`], which need the language. The STEP file can be
/// refused with `PHOENIX-CAD-022`, `-048`, `-049` or `-050`; the STL file with the last three; a
/// drawing with `PHOENIX-CAD-050`.
pub fn render(
    format: Format,
    enclosure: &Enclosure,
    text: &str,
    settings: &Settings,
) -> Result<Vec<u8>, CadError> {
    render_titled(format, enclosure, text, settings, Lang::En, "box")
}

/// The same as [`render`] with the language of the cells of the title block of the drawings and
/// the name of the model, which is the title of the block when the settings give none. A
/// drawing has a title block only when `title_block.enabled` is 1.
pub fn render_titled(
    format: Format,
    enclosure: &Enclosure,
    text: &str,
    settings: &Settings,
    lang: Lang,
    name: &str,
) -> Result<Vec<u8>, CadError> {
    let text_of = |s: String| Ok(s.into_bytes());
    match format {
        Format::Script => text_of(to_scad(enclosure)),
        Format::Step => text_of(export_step_with(enclosure, &settings.mesh)?),
        Format::Report | Format::Bill | Format::Order => text_of(text.to_owned()),
        Format::Stl | Format::Obj | Format::ThreeMf => {
            let bodies = solid::bodies(enclosure, &settings.mesh)?;
            let list: Vec<(&str, &mesh::Mesh)> =
                bodies.iter().map(|b| (b.name.as_str(), &b.mesh)).collect();
            let coloured: Vec<(&str, &mesh::Mesh, &str)> = bodies
                .iter()
                .map(|b| (b.name.as_str(), &b.mesh, b.material.as_str()))
                .collect();
            match format {
                Format::Obj if settings.mesh.colors => {
                    text_of(interchange::obj_coloured(&coloured))
                }
                Format::ThreeMf if settings.mesh.colors => {
                    Ok(interchange::three_mf_coloured(&coloured))
                }
                Format::Obj => text_of(interchange::obj(&list)),
                Format::ThreeMf => Ok(interchange::three_mf(&list)),
                _ if settings.mesh.binary => Ok(mesh::stl_binary(&list)),
                _ => text_of(mesh::stl_ascii(&list)),
            }
        }
        _ => {
            let sheet_cut = (format.is_sheet()
                && settings.drawing.sheet_section
                && settings.drawing.cut_line != 0)
                .then(|| {
                    (
                        Dim::ALL[usize::from(settings.drawing.cut_line) - 1],
                        settings.drawing.cut_at,
                        settings.drawing.cut_letters(),
                    )
                });
            let mut drawing = if format.is_sheet() {
                let cut = sheet_cut
                    .as_ref()
                    .map(|(axis, at, label)| (*axis, *at, label.as_str()));
                Drawing::sheet_cut(enclosure, &settings.drawing, cut)?
            } else {
                let view = format.view().unwrap_or(View::Top);
                Drawing::of(enclosure, view, &settings.drawing)?
            };
            if !format.is_sheet() && settings.drawing.cut_line != 0 {
                let axis = Dim::ALL[usize::from(settings.drawing.cut_line) - 1];
                drawing.add_cut_line(
                    axis,
                    settings.drawing.cut_at,
                    &settings.drawing.cut_letters(),
                );
            }
            if let Some((axis, at, label)) = &sheet_cut {
                let bodies = solid::bodies(enclosure, &settings.mesh)?;
                let section = crate::section::cut(&bodies, *axis, *at);
                // a plane that cuts nothing leaves the sheet without the view
                if !section.contours.is_empty() {
                    let view = crate::section::hatched_drawing(&section, &settings.drawing)?;
                    drawing.add_section_view(
                        view,
                        &format!("{label}-{label}"),
                        settings.drawing.sheet_gap.get() as f64 / 1_000_000.0,
                    );
                    if drawing.entities.len() > settings.drawing.max_entities as usize {
                        return Err(CadError::ExportLimit(format!(
                            "the sheet has {} entities and the settings allow {}",
                            drawing.entities.len(),
                            settings.drawing.max_entities
                        )));
                    }
                }
            }
            if format.is_sheet() && settings.drawing.sheet_symbol {
                drawing.add_projection_symbol(
                    settings.drawing.sheet_angle == 3,
                    settings.drawing.sheet_symbol_size.get() as f64 / 1_000_000.0,
                );
            }
            if settings.drawing.scale_bar > 0 {
                drawing.add_scale_bar(u32::from(settings.drawing.scale_bar));
            }
            if settings.drawing.legend {
                if format.is_sheet() && settings.drawing.sheet_legends {
                    drawing.add_sheet_legend(enclosure, &settings.drawing, lang);
                } else {
                    drawing.add_legend(enclosure, &settings.drawing, lang);
                }
            }
            if settings.drawing.frame {
                drawing.add_frame(settings.drawing.frame_margin.get() as f64 / 1_000_000.0);
            }
            let tb = &settings.title_block;
            let dxf = format.extension() == "dxf";
            let pdf = format == Format::PdfSheet;
            if !tb.enabled {
                if dxf {
                    if let Some(sheet) = drawing.paper_sheet(
                        settings.drawing.paper,
                        settings.drawing.paper_portrait,
                        settings.drawing.paper_margin.get() as f64 / 1_000_000.0,
                        None,
                    ) {
                        drawing.add_paper_sheet(&sheet);
                    }
                }
                if pdf {
                    return Ok(drawing.to_pdf_with(&settings.drawing, None, name));
                }
                text_of(if dxf {
                    drawing.to_dxf()
                } else {
                    drawing.to_svg(&settings.drawing)
                })
            } else if dxf {
                let gap = 6.0 + drawing.dimension_reach();
                let first = drawing.title_block(tb, name, "1:1", gap, lang);
                let sheet = drawing.paper_sheet(
                    settings.drawing.paper,
                    settings.drawing.paper_portrait,
                    settings.drawing.paper_margin.get() as f64 / 1_000_000.0,
                    Some((first.lo, first.hi)),
                );
                let label = sheet.map_or_else(
                    || "1:1".to_owned(),
                    |sheet| crate::drawing::scale_label(sheet.scale, true),
                );
                let block = drawing.title_block(tb, name, &label, gap, lang);
                if let Some(sheet) = &sheet {
                    drawing.add_paper_sheet(sheet);
                }
                text_of(drawing.to_dxf_with(Some(&block)))
            } else if pdf {
                Ok(drawing.to_pdf_with(&settings.drawing, Some((tb, name, lang)), name))
            } else {
                text_of(drawing.to_svg_with(&settings.drawing, Some((tb, name, lang))))
            }
        }
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

/// Writes `text` at `path` after the checks of [`check_target`] and returns the bytes written;
/// see [`write_bytes`].
pub fn write_file(
    path: &Path,
    text: &str,
    overwrite: bool,
    open_project: Option<&Path>,
    max_kib: u32,
) -> Result<usize, CadError> {
    write_bytes(path, text.as_bytes(), overwrite, open_project, max_kib)
}

/// Writes `bytes` at `path` after the checks of [`check_target`] and returns the bytes written.
/// Without `overwrite` the file is created only if it does not exist (so that a file that appears
/// between the check and the writing is not lost); with it, the file is replaced atomically. A
/// file larger than `max_kib` KiB is refused (`PHOENIX-CAD-041`), and so is a failed writing,
/// which leaves no half-written file.
pub fn write_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
    open_project: Option<&Path>,
    max_kib: u32,
) -> Result<usize, CadError> {
    if bytes.len() as u64 > u64::from(max_kib) * 1024 {
        return Err(CadError::Export(format!(
            "the file has {} bytes, more than the {max_kib} KiB allowed",
            bytes.len()
        )));
    }
    check_target(path, overwrite, open_project)?;
    if overwrite {
        store::save_atomic(path, bytes, 0)
            .map_err(|e| CadError::Export(format!("cannot write the file: {e}")))?;
        return Ok(bytes.len());
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
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(CadError::Export(format!("cannot write the file: {e}")));
    }
    Ok(bytes.len())
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
                ("stl", "stl"),
                ("dxf", "dxf-top"),
                ("dxf", "dxf-front"),
                ("dxf", "dxf-side"),
                ("dxf", "dxf-sheet"),
                ("svg", "svg-top"),
                ("svg", "svg-front"),
                ("svg", "svg-side"),
                ("svg", "svg-sheet"),
                ("pdf", "pdf-sheet"),
                ("txt", "report"),
                ("csv", "bill"),
                ("obj", "obj"),
                ("3mf", "3mf"),
                ("html", "order")
            ]
        );
        for f in Format::ALL {
            assert_eq!(Format::from_name(f.name()), Some(f));
        }
        assert_eq!(Format::from_name("dxf"), None);
        assert_eq!(Format::from_name(""), None);
        assert_eq!(Format::drawing(true, View::Front), Format::DxfFront);
        assert_eq!(Format::drawing(false, View::Side), Format::SvgSide);
        assert_eq!(Format::SvgTop.view(), Some(View::Top));
        assert_eq!(Format::Stl.view(), None);
    }

    #[test]
    fn the_obj_and_3mf_files_hold_the_same_triangles_as_the_stl_file() {
        let e = enclosure();
        let settings = Settings::defaults();
        let stl = mesh::read_stl(&render(Format::Stl, &e, "", &settings).unwrap()).unwrap();
        let obj = String::from_utf8(render(Format::Obj, &e, "", &settings).unwrap()).unwrap();
        assert!(obj.starts_with(
            "# EH3D PHOENIX-CAD, millimetres
o enclosure
"
        ));
        assert_eq!(
            obj.lines().filter(|l| l.starts_with("f ")).count(),
            stl.len()
        );
        let bytes = render(Format::ThreeMf, &e, "", &settings).unwrap();
        let entries = interchange::read_stored_zip(&bytes).unwrap();
        let model = String::from_utf8(entries[2].1.clone()).unwrap();
        assert_eq!(model.matches("<triangle ").count(), stl.len());
        assert_eq!(
            model.matches("<vertex ").count(),
            obj.lines().filter(|l| l.starts_with("v ")).count()
        );
        assert_eq!(Format::from_name("obj"), Some(Format::Obj));
        assert_eq!(Format::from_name("3mf"), Some(Format::ThreeMf));
        let project = Path::new("a/my box.phoenixproj");
        assert_eq!(suggested_name(Some(project), Format::Obj), "my box.obj");
        assert_eq!(suggested_name(Some(project), Format::ThreeMf), "my box.3mf");
    }

    #[test]
    fn the_suggested_name_follows_the_project_and_the_format() {
        let project = Path::new("some/where/my box.phoenixproj");
        assert_eq!(suggested_name(Some(project), Format::Script), "my box.scad");
        assert_eq!(suggested_name(Some(project), Format::Step), "my box.step");
        assert_eq!(suggested_name(Some(project), Format::Report), "my box.txt");
        assert_eq!(suggested_name(Some(project), Format::Bill), "my box.csv");
        assert_eq!(suggested_name(None, Format::Step), "box.step");
        assert_eq!(suggested_name(Some(project), Format::Stl), "my box.stl");
        assert_eq!(suggested_name(Some(project), Format::DxfTop), "my box.dxf");
        assert_eq!(suggested_name(Some(project), Format::SvgSide), "my box.svg");
        assert_eq!(
            suggested_name(Some(Path::new("")), Format::Report),
            "box.txt"
        );
    }

    #[test]
    fn the_formats_render_the_script_the_step_file_and_the_report_as_given() {
        let e = enclosure();
        let settings = Settings::defaults();
        let text =
            |f: Format, t: &str| String::from_utf8(render(f, &e, t, &settings).unwrap()).unwrap();
        let script = text(Format::Script, "r");
        // the same text as the library writes: the six master lengths in millimetres
        assert_eq!(script, to_scad(&e));
        assert!(script.contains("inner_length = 100;"), "{script}");
        assert!(script.contains("wall = 2;"), "{script}");
        let step = text(Format::Step, "r");
        assert!(step.starts_with("ISO-10303-21;"), "{step}");
        assert_eq!(text(Format::Report, "one\ntwo"), "one\ntwo");
        assert_eq!(text(Format::Bill, "a,b\n"), "a,b\n");
    }

    #[test]
    fn the_solid_and_the_drawings_render_from_the_settings() {
        let e = enclosure();
        let mut settings = Settings::defaults();
        // a plain box: 12 + 12 triangles, 84 + 50 * 24 bytes
        let binary = render(Format::Stl, &e, "", &settings).unwrap();
        assert_eq!(binary.len(), 84 + 50 * 24);
        assert_eq!(mesh::read_stl(&binary).unwrap().len(), 24);
        settings.mesh.binary = false;
        let text = render(Format::Stl, &e, "", &settings).unwrap();
        assert!(text.starts_with(b"solid enclosure\n"));
        assert_eq!(
            mesh::read_stl(&text).unwrap(),
            mesh::read_stl(&binary).unwrap()
        );
        let dxf = String::from_utf8(render(Format::DxfTop, &e, "", &settings).unwrap()).unwrap();
        assert!(dxf.contains("\n0\nEOF\n"));
        let svg = String::from_utf8(render(Format::SvgFront, &e, "", &settings).unwrap()).unwrap();
        assert!(svg.contains("<svg "));
        settings.mesh.max_triangles = 12;
        let err = render(Format::Stl, &e, "", &settings).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
    }

    #[test]
    fn a_binary_file_is_written_whole_and_not_over_another() {
        let dir = Scratch::new("binary");
        let target = dir.0.join("box.stl");
        let bytes: Vec<u8> = (0..=255u8).collect();
        assert_eq!(write_bytes(&target, &bytes, false, None, 64).unwrap(), 256);
        assert_eq!(fs::read(&target).unwrap(), bytes);
        let err = write_bytes(&target, b"x", false, None, 64).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-040");
        assert_eq!(fs::read(&target).unwrap(), bytes);
        assert_eq!(write_bytes(&target, b"xy", true, None, 64).unwrap(), 2);
        assert_eq!(fs::read(&target).unwrap(), b"xy");
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

    #[test]
    fn a_drawing_has_a_title_block_only_when_the_settings_ask_for_it() {
        let e = enclosure();
        // the legend is written in the language, so it is off to see that the language changes
        // nothing in a drawing without a title block
        let off = Settings::with_user_config("[drawing]\nlegend = 0\n").unwrap();
        for format in [Format::DxfTop, Format::SvgFront] {
            let plain = render(format, &e, "", &off).unwrap();
            assert!(!String::from_utf8(plain.clone()).unwrap().contains("ROTULO"));
            // the language and the name change nothing while it is off
            assert_eq!(
                render_titled(format, &e, "", &off, Lang::De, "plate").unwrap(),
                plain
            );
        }
        let on = Settings::with_user_config(
            "[title_block]
enabled = 1
number = P-7
",
        )
        .unwrap();
        let dxf = String::from_utf8(
            render_titled(Format::DxfTop, &e, "", &on, Lang::De, "plate").unwrap(),
        )
        .unwrap();
        assert!(dxf.contains("ROTULO"));
        assert!(dxf.contains("1\nZeichnungsnummer\n"));
        assert!(dxf.contains("1\nplate\n"));
        assert!(dxf.contains("1\nP-7\n"));
        let svg = String::from_utf8(
            render_titled(Format::SvgSide, &e, "", &on, Lang::Fr, "plate").unwrap(),
        )
        .unwrap();
        assert!(svg.contains(">Numéro de plan</text>"));
        assert!(svg.contains(">plate</text>"));
        // the other formats do not have one
        let step = render_titled(Format::Script, &e, "", &on, Lang::En, "plate").unwrap();
        assert!(!String::from_utf8(step).unwrap().contains("ROTULO"));
    }

    #[test]
    fn a_drawing_has_a_frame_only_when_the_settings_ask_for_it() {
        let e = enclosure();
        let off = Settings::with_user_config(
            "[drawing]
legend = 0
",
        )
        .unwrap();
        let on = Settings::with_user_config(
            "[drawing]
legend = 0
frame = 1
frame_margin = 8 mm
",
        )
        .unwrap();
        for format in [Format::DxfTop, Format::SvgFront] {
            let plain = String::from_utf8(render(format, &e, "", &off).unwrap()).unwrap();
            let framed = String::from_utf8(render(format, &e, "", &on).unwrap()).unwrap();
            assert!(!plain.contains("MARCO"), "{format:?}");
            assert!(framed.contains("MARCO"), "{format:?}");
        }
        // the other formats are not drawings
        let script = String::from_utf8(render(Format::Script, &e, "", &on).unwrap()).unwrap();
        assert!(!script.contains("MARCO"));
    }

    #[test]
    fn the_sheet_formats_are_drawings_of_all_the_views_and_have_no_single_view() {
        assert_eq!(Format::sheet(true), Format::DxfSheet);
        assert_eq!(Format::sheet(false), Format::SvgSheet);
        assert!(Format::DxfSheet.is_sheet() && Format::SvgSheet.is_sheet());
        assert!(!Format::DxfTop.is_sheet() && !Format::Order.is_sheet());
        assert_eq!(Format::DxfSheet.view(), None);
        assert_eq!(Format::SvgSheet.view(), None);
        assert_eq!(Format::from_name("dxf-sheet"), Some(Format::DxfSheet));
        assert_eq!(Format::from_name("svg-sheet"), Some(Format::SvgSheet));
        let e = enclosure();
        let off = Settings::with_user_config("[drawing]\nlegend = 0\nhidden_lines = 0\n").unwrap();
        let dxf = String::from_utf8(render(Format::DxfSheet, &e, "", &off).unwrap()).unwrap();
        // three views: three outlines of four lines or more, and the sheet is wider than a view
        let top = String::from_utf8(render(Format::DxfTop, &e, "", &off).unwrap()).unwrap();
        assert!(dxf.matches("0\nLINE\n8\nCONTORNO\n").count() >= 3 * 4);
        assert!(dxf.len() > top.len());
        // a frame and a legend go round the whole sheet
        let on = Settings::with_user_config("[drawing]\nframe = 1\n").unwrap();
        let svg = String::from_utf8(render(Format::SvgSheet, &e, "", &on).unwrap()).unwrap();
        assert!(svg.contains("<g id=\"MARCO\""));
        assert!(svg.contains(">Dimensions (mm)</text>"));
        assert_eq!(suggested_name(None, Format::SvgSheet), "box.svg");
    }

    /// The width in millimetres that the SVG sheet says it has.
    fn svg_width_mm(svg: &str) -> f64 {
        let at = svg.find("width=\"").unwrap() + 7;
        svg[at..].split("mm").next().unwrap().parse().unwrap()
    }

    #[test]
    fn the_sheet_is_also_a_pdf_file_with_the_page_of_the_svg_sheet() {
        assert_eq!(Format::from_name("pdf-sheet"), Some(Format::PdfSheet));
        assert!(Format::PdfSheet.is_sheet() && Format::PdfSheet.view().is_none());
        assert_eq!(suggested_name(None, Format::PdfSheet), "box.pdf");
        let e = enclosure();
        for config in [
            "[drawing]\nframe = 1\n",
            "[drawing]\npaper = 4\nframe = 1\n[title_block]\nenabled = 1\nnumber = P-7\n",
            "[drawing]\npaper = 3\npaper_portrait = 1\ndimensions = 0\nlegend = 0\n",
        ] {
            let settings = Settings::with_user_config(config).unwrap();
            let pdf =
                render_titled(Format::PdfSheet, &e, "", &settings, Lang::En, "plate").unwrap();
            let svg = String::from_utf8(
                render_titled(Format::SvgSheet, &e, "", &settings, Lang::En, "plate").unwrap(),
            )
            .unwrap();
            let (w, h, objects) = crate::pdf::check(&pdf).expect("a well formed file");
            assert_eq!(objects, 6);
            // the page is the one of the SVG sheet, in points
            let width_mm = svg_width_mm(&svg);
            assert!(
                (w / crate::pdf::POINTS_PER_MM - width_mm).abs() < 0.01,
                "{config}: {w} {width_mm}"
            );
            assert!(h > 0.0);
            // the same marks: every line, circle and arc of the SVG sheet is in the file
            let text = String::from_utf8_lossy(&pdf).to_string();
            assert_eq!(
                text.matches(" l S\n").count(),
                svg.matches("<line ").count(),
                "{config}"
            );
            assert_eq!(
                text.matches(" h S\n").count(),
                svg.matches("<circle ").count(),
                "{config}"
            );
            assert_eq!(
                text.matches(" c S\n").count(),
                svg.matches("<path ").count(),
                "{config}"
            );
            assert!(text.contains("/Title (plate)"));
            // the same settings give the same bytes
            assert_eq!(
                pdf,
                render_titled(Format::PdfSheet, &e, "", &settings, Lang::En, "plate").unwrap()
            );
        }
    }

    #[test]
    fn the_pdf_sheet_carries_the_dimensions_the_title_block_and_the_scale() {
        let e = enclosure();
        let settings = Settings::with_user_config(
            "[drawing]\npaper = 4\n[title_block]\nenabled = 1\nnumber = P-7\nrevision = B\n",
        )
        .unwrap();
        let pdf = render_titled(Format::PdfSheet, &e, "", &settings, Lang::En, "plate").unwrap();
        let text = String::from_utf8_lossy(&pdf).to_string();
        // the cells and their values, the scale and a dimension of the box
        for word in ["(Drawing number)", "(P-7)", "(B)", "(plate)", "(1:"] {
            assert!(text.contains(word), "{word}");
        }
        // the dashed layers have their pattern and the file goes back to continuous lines
        assert!(text.contains(" 0 d\n") && text.contains("[] 0 d\n"));
        // in German the cells have umlauts: they are written as the codes of the encoding, not as UTF-8
        let de = render_titled(Format::PdfSheet, &e, "", &settings, Lang::De, "plate").unwrap();
        assert!(de.is_ascii());
        // Japanese has no glyph in the font: question marks, and the file is still well formed
        let ja = render_titled(Format::PdfSheet, &e, "", &settings, Lang::Ja, "plate").unwrap();
        assert!(crate::pdf::check(&ja).is_some() && ja.is_ascii());
    }

    /// A small generator with a fixed seed (xorshift), so that a failure can be repeated.
    struct Rng(u64);

    impl Rng {
        fn draw(&mut self, below: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % below
        }
    }

    #[test]
    fn sheets_of_random_boxes_and_papers_are_well_formed_pdf_files_with_a_fixed_seed() {
        for seed in 1..=30u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mm = |v: u64| Length::from_mm(v as i64).unwrap();
            let e = Enclosure::new(EnclosureParams {
                inner_length: mm(40 + rng.draw(160)),
                inner_width: mm(30 + rng.draw(120)),
                inner_height: mm(15 + rng.draw(60)),
                wall: mm(2),
                floor: mm(2),
                lid: mm(3),
                edge_margin: mm(3),
                min_web: mm(2),
            })
            .unwrap();
            let config = format!(
                "[drawing]\npaper = {}\npaper_portrait = {}\ndimensions = {}\nlegend = {}\nframe = {}\nhidden_lines = {}\n[title_block]\nenabled = {}\nnumber = N{seed}\n",
                rng.draw(6),
                rng.draw(2),
                rng.draw(2),
                rng.draw(2),
                rng.draw(2),
                rng.draw(2),
                rng.draw(2)
            );
            let settings = Settings::with_user_config(&config).unwrap();
            let lang = Lang::ALL[rng.draw(8) as usize];
            let pdf = render_titled(Format::PdfSheet, &e, "", &settings, lang, "box").unwrap();
            let svg = String::from_utf8(
                render_titled(Format::SvgSheet, &e, "", &settings, lang, "box").unwrap(),
            )
            .unwrap();
            let (w, _, objects) = crate::pdf::check(&pdf)
                .unwrap_or_else(|| panic!("seed {seed}: not well formed\n{config}"));
            assert_eq!(objects, 6, "seed {seed}");
            assert!(pdf.is_ascii(), "seed {seed}");
            let width_mm = svg_width_mm(&svg);
            assert!(
                (w / crate::pdf::POINTS_PER_MM - width_mm).abs() < 0.01,
                "seed {seed}: {w} {width_mm}\n{config}"
            );
            let text = String::from_utf8_lossy(&pdf).to_string();
            assert_eq!(
                text.matches(" l S\n").count(),
                svg.matches("<line ").count(),
                "seed {seed}\n{config}"
            );
        }
    }
}
