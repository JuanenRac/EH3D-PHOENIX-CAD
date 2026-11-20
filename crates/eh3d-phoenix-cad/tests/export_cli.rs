//! The command `export`: it writes the solid, the drawings and the other files of the export menu,
//! and never writes over a file unless it is told to.

use eh3d_phoenix_cad::mesh::read_stl;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");
const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixproj");
const FRAME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/frame.phoenixproj");

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the program runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "phoenix-cad-export-cli-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn file(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_solid_is_written_as_a_binary_file_by_default_and_as_text_when_asked() {
    let dir = Scratch::new("stl");
    let bin = dir.file("box.stl");
    let (code, text, err) = run(&[
        "export", EXAMPLE, "--format", "stl", "--out", &bin, "--lang", "en",
    ]);
    assert_eq!(code, 0, "{text} {err}");
    assert!(text.starts_with("Exported box.stl ("), "{text}");
    let bytes = fs::read(&bin).unwrap();
    assert!(!bytes.starts_with(b"solid"));
    let triangles = read_stl(&bytes).unwrap();
    assert!(triangles.len() > 1000);
    let ascii = dir.file("box-text.stl");
    let (code, _, err) = run(&[
        "export", EXAMPLE, "--format", "stl", "--ascii", "--out", &ascii,
    ]);
    assert_eq!(code, 0, "{err}");
    let text = fs::read(&ascii).unwrap();
    assert!(text.starts_with(b"solid enclosure\n"));
    assert_eq!(read_stl(&text).unwrap().len(), triangles.len());
}

#[test]
fn the_parts_of_a_frame_are_solids_of_their_own_in_the_same_file() {
    let dir = Scratch::new("frame");
    let out = dir.file("frame.stl");
    let (code, _, err) = run(&["export", FRAME, "--format", "stl", "--ascii", "--out", &out]);
    assert_eq!(code, 0, "{err}");
    let text = fs::read_to_string(&out).unwrap();
    assert_eq!(
        text.matches("\nendsolid ").count() + usize::from(text.starts_with("endsolid")),
        19
    );
    assert!(text.contains("solid front_rail\n"));
    assert!(text.contains("solid bracket_1\n"));
}

#[test]
fn the_drawings_are_written_for_each_view() {
    let dir = Scratch::new("views");
    for (format, view, ext) in [
        ("dxf", "top", "dxf"),
        ("dxf", "front", "dxf"),
        ("svg", "side", "svg"),
    ] {
        let out = dir.file(&format!("{format}-{view}.{ext}"));
        let (code, _, err) = run(&[
            "export", EXAMPLE, "--format", format, "--view", view, "--out", &out,
        ]);
        assert_eq!(code, 0, "{err}");
        let text = fs::read_to_string(&out).unwrap();
        if format == "dxf" {
            assert!(text.contains("CONTORNO") && text.ends_with("0\nEOF\n"));
        } else {
            assert!(text.contains("id=\"AGUJEROS\"") && text.trim_end().ends_with("</svg>"));
        }
    }
    // the plan of the example has the arcs of its rounded corners; the front has the two holes
    let top = fs::read_to_string(dir.file("dxf-top.dxf")).unwrap();
    assert_eq!(top.matches("\n0\nARC\n").count(), 4);
    let front = fs::read_to_string(dir.file("dxf-front.dxf")).unwrap();
    assert_eq!(front.matches("\n0\nCIRCLE\n").count(), 1);
}

#[test]
fn the_sheet_has_the_three_views_on_one_drawing() {
    let dir = Scratch::new("sheet");
    let single = dir.file("front.dxf");
    let (code, _, err) = run(&[
        "export", EXAMPLE, "--format", "dxf", "--view", "front", "--out", &single,
    ]);
    assert_eq!(code, 0, "{err}");
    for (format, name) in [("dxf", "sheet.dxf"), ("svg", "sheet.svg")] {
        let out = dir.file(name);
        let (code, _, err) = run(&[
            "export", EXAMPLE, "--format", format, "--view", "sheet", "--out", &out,
        ]);
        assert_eq!(code, 0, "{err}");
        let text = fs::read_to_string(&out).unwrap();
        if format == "dxf" {
            assert!(text.ends_with(
                "0
EOF
"
            ));
            // the plan has the four arcs of its rounded corners: the sheet has them too
            assert_eq!(
                text.matches(
                    "
0
ARC
"
                )
                .count(),
                4
            );
            let front = fs::read_to_string(&single).unwrap();
            assert!(
                text.matches(
                    "
0
LINE
"
                )
                .count()
                    > 2 * front
                        .matches(
                            "
0
LINE
"
                        )
                        .count()
            );
        } else {
            assert!(text.contains("id=\"CONTORNO\"") && text.trim_end().ends_with("</svg>"));
        }
    }
    // a sheet takes no other view, and the other formats take no sheet
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "stl", "--view", "sheet"]);
    assert_eq!(code, 1);
    assert!(
        err.contains("--view is only for the formats dxf and svg"),
        "{err}"
    );
}

#[test]
fn a_file_that_exists_is_kept_unless_the_order_to_write_over_it_is_given() {
    let dir = Scratch::new("overwrite");
    let out = dir.file("box.scad");
    fs::write(&out, "mine").unwrap();
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "script", "--out", &out]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD-040"), "{err}");
    assert_eq!(fs::read_to_string(&out).unwrap(), "mine");
    let (code, _, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "script",
        "--out",
        &out,
        "--overwrite",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(fs::read_to_string(&out).unwrap().contains("inner_length"));
    // the project itself is never written over, even with the order
    let copy = dir.file("copy.phoenixproj");
    fs::copy(EXAMPLE, &copy).unwrap();
    let (code, _, err) = run(&[
        "export",
        &copy,
        "--format",
        "report",
        "--out",
        &copy,
        "--overwrite",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD-041"), "{err}");
}

#[test]
fn a_drawing_gets_its_title_block_and_its_cutting_line_from_the_command_line() {
    let dir = Scratch::new("title");
    let plain = dir.file("plain.svg");
    let (code, _, err) = run(&[
        "export", EXAMPLE, "--format", "svg", "--out", &plain, "--lang", "en",
    ]);
    assert_eq!(code, 0, "{err}");
    let plain_text = fs::read_to_string(&plain).unwrap();
    assert!(!plain_text.contains("Front plate"));
    // the texts go into the block, which is turned on by them
    let titled = dir.file("titled.svg");
    let (code, _, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "svg",
        "--out",
        &titled,
        "--lang",
        "en",
        "--title-field",
        "title=Front plate",
        "--title-field",
        "author=J. Doe",
        "--title-field",
        "revision=B",
        "--title-field",
        "number=EH-7",
    ]);
    assert_eq!(code, 0, "{err}");
    let text = fs::read_to_string(&titled).unwrap();
    for word in ["Front plate", "J. Doe", "EH-7"] {
        assert!(text.contains(word), "{word}");
    }
    // and in the language asked for the names of the cells
    let es = dir.file("es.svg");
    let (code, _, _) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "svg",
        "--out",
        &es,
        "--lang",
        "es",
        "--title-field",
        "title=Placa",
    ]);
    assert_eq!(code, 0);
    assert!(fs::read_to_string(&es).unwrap().contains("Placa"));
    // a cutting line adds lines and the two letters
    let cut = dir.file("cut.svg");
    let (code, _, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "svg",
        "--out",
        &cut,
        "--lang",
        "en",
        "--cut-line",
        "x=20 mm:K",
    ]);
    assert_eq!(code, 0, "{err}");
    let cut_text = fs::read_to_string(&cut).unwrap();
    assert_eq!(cut_text.matches(">K</text>").count(), 2);
    assert!(cut_text.len() > plain_text.len());
    // what cannot be used is refused with a reason and nothing is written
    for (args, reason) in [
        (
            vec!["--title-field", "colour=red"],
            "not a text of the title block",
        ),
        (vec!["--title-field", "title"], "KEY=TEXT"),
        (vec!["--cut-line", "w=3 mm"], "AXIS=POSITION"),
        (vec!["--cut-line", "x=3"], "drawing.cut_at"),
        (vec!["--cut-line", "x=3 mm:TOOLONG"], "one to three letters"),
    ] {
        let out = dir.file("refused.svg");
        let mut all = vec!["export", EXAMPLE, "--format", "svg", "--out", out.as_str()];
        all.extend(args.iter());
        let (code, _, err) = run(&all);
        assert_eq!(code, 1, "{args:?}");
        assert!(err.contains(reason), "{args:?} {err}");
        assert!(!std::path::Path::new(&out).exists());
    }
    // they are for the drawings only
    let stl = dir.file("x.stl");
    let (code, _, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "stl",
        "--out",
        &stl,
        "--cut-line",
        "x=3 mm",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("only for the drawings"), "{err}");
}

#[test]
fn the_obj_and_3mf_files_have_colours_when_the_settings_say_so() {
    let dir = Scratch::new("colours");
    let config = dir.file("settings.cfg");
    fs::write(&config, "[mesh]\ncolors = 1\n").unwrap();
    let plain = dir.file("plain.obj");
    let coloured = dir.file("coloured.obj");
    let (code, _, err) = run(&["export", FRAME, "--format", "obj", "--out", &plain]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = run(&[
        "export", FRAME, "--format", "obj", "--out", &coloured, "--config", &config,
    ]);
    assert_eq!(code, 0, "{err}");
    let plain_text = fs::read_to_string(&plain).unwrap();
    let text = fs::read_to_string(&coloured).unwrap();
    assert!(text.starts_with("# EH3D PHOENIX-CAD, millimetres, colours by material\n"));
    let first = |t: &str| t.lines().find(|l| l.starts_with("v ")).unwrap().to_owned();
    assert_eq!(first(&plain_text).split(' ').count(), 4);
    assert_eq!(first(&text).split(' ').count(), 7);
    let package = dir.file("coloured.3mf");
    let (code, _, err) = run(&[
        "export", FRAME, "--format", "3mf", "--out", &package, "--config", &config,
    ]);
    assert_eq!(code, 0, "{err}");
    let bytes = fs::read(&package).unwrap();
    let entries = eh3d_phoenix_cad::interchange::read_stored_zip(&bytes).unwrap();
    let model = String::from_utf8(entries[2].1.clone()).unwrap();
    assert!(model.contains("<basematerials id=\""), "{model}");
    // a value that is not 0 or 1 is refused
    fs::write(&config, "[mesh]\ncolors = 2\n").unwrap();
    let (code, _, err) = run(&[
        "export",
        FRAME,
        "--format",
        "obj",
        "--out",
        &dir.file("x.obj"),
        "--config",
        &config,
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("mesh.colors must be 0 or 1"), "{err}");
}

#[test]
fn the_arguments_are_checked() {
    let (code, _, err) = run(&["export", EXAMPLE]);
    assert_eq!(code, 1);
    assert!(err.contains("needs --format"), "{err}");
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "xyz"]);
    assert_eq!(code, 1);
    assert!(err.contains("unknown format"), "{err}");
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "stl", "--view", "top"]);
    assert_eq!(code, 1);
    assert!(err.contains("only for the formats dxf and svg"), "{err}");
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "dxf", "--ascii"]);
    assert_eq!(code, 1);
    assert!(err.contains("only for the format stl"), "{err}");
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "dxf", "--view", "back"]);
    assert_eq!(code, 1);
    assert!(err.contains("top, front, side or sheet"), "{err}");
    let (code, _, err) = run(&["export", "--format", "stl"]);
    assert_eq!(code, 1);
    assert!(err.contains("needs a project file"), "{err}");
}

#[test]
fn a_model_the_solid_cannot_be_made_of_writes_nothing_and_gives_the_code() {
    let dir = Scratch::new("refused");
    let plate = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.phoenixproj");
    let out = dir.file("plate.stl");
    let (code, _, err) = run(&["export", plate, "--format", "stl", "--out", &out]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD-048"), "{err}");
    assert!(!PathBuf::from(&out).exists());
    // the drawing of the same model is possible
    let svg = dir.file("plate.svg");
    let (code, _, err) = run(&["export", plate, "--format", "svg", "--out", &svg]);
    assert_eq!(code, 0, "{err}");
}

#[test]
fn a_settings_file_changes_the_sides_of_the_circles_and_the_limit() {
    let dir = Scratch::new("config");
    let config = dir.file("coarse.cfg");
    fs::write(&config, "[mesh]\ncircle_segments = 16\nstl_binary = 0\n").unwrap();
    let out = dir.file("coarse.stl");
    let (code, _, err) = run(&[
        "export", EXAMPLE, "--format", "stl", "--out", &out, "--config", &config,
    ]);
    assert_eq!(code, 0, "{err}");
    let coarse = fs::read(&out).unwrap();
    assert!(coarse.starts_with(b"solid "));
    let fine = dir.file("fine.stl");
    let (code, _, _) = run(&[
        "export", EXAMPLE, "--format", "stl", "--ascii", "--out", &fine,
    ]);
    assert_eq!(code, 0);
    assert!(read_stl(&coarse).unwrap().len() < read_stl(&fs::read(&fine).unwrap()).unwrap().len());
    let low = dir.file("low.cfg");
    fs::write(&low, "[mesh]\nmax_triangles = 100\n").unwrap();
    let (code, _, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "stl",
        "--out",
        &dir.file("low.stl"),
        "--config",
        &low,
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD-050"), "{err}");
    let bad = dir.file("bad.cfg");
    fs::write(&bad, "[mesh]\ncircle_segments = 10\n").unwrap();
    let (code, _, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "stl",
        "--out",
        &dir.file("bad.stl"),
        "--config",
        &bad,
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("multiple of 4"), "{err}");
}

#[test]
fn the_order_sheet_is_written_as_one_html_page_in_the_language_asked() {
    let dir = Scratch::new("order");
    let out = dir.file("frame.html");
    let (code, text, error) = run(&["export", FRAME, "--format", "order", "--out", &out]);
    assert_eq!(code, 0, "{error}");
    assert!(text.contains("frame.html"), "{text}");
    let page = fs::read_to_string(&out).unwrap();
    assert!(page.starts_with("<!DOCTYPE html>"), "{page}");
    assert!(page.contains("<h2>Profile cuts</h2>"), "{page}");
    assert!(page.contains("<h2>Parts of the catalog</h2>"), "{page}");
    assert!(!page.contains("<script"));
    // never over a file that exists, and the words follow --lang
    let (code, _, error) = run(&["export", FRAME, "--format", "order", "--out", &out]);
    assert_ne!(code, 0);
    assert!(error.contains("PHOENIX-CAD-040"), "{error}");
    let (code, _, error) = run(&[
        "export",
        FRAME,
        "--format",
        "order",
        "--out",
        &out,
        "--overwrite",
        "--lang",
        "es",
    ]);
    assert_eq!(code, 0, "{error}");
    let page = fs::read_to_string(&out).unwrap();
    assert!(page.contains("<h2>Cortes de perfil</h2>"), "{page}");
    assert!(page.contains("<html lang=\"es\">"), "{page}");
}

#[test]
fn the_files_the_export_writes_are_read_back_by_mesh_info_in_every_format() {
    let dir = Scratch::new("meshinfo");
    let mut lines = Vec::new();
    for format in ["stl", "obj", "3mf"] {
        let file = dir.file(&format!("box.{format}"));
        let (code, text, err) = run(&["export", EXAMPLE, "--format", format, "--out", &file]);
        assert_eq!(code, 0, "{text} {err}");
        let (code, text, err) = run(&["mesh-info", &file]);
        assert_eq!(code, 0, "{text} {err}");
        assert!(text.contains(" 1 solids"), "{text}");
        assert!(text.contains(", closed, volume "), "{text}");
        // the line of the solid, without its name (a file of the STL format has none)
        let line = text.lines().nth(1).unwrap().trim().to_owned();
        lines.push(line.split_once(": ").unwrap().1.to_owned());
    }
    // the three formats hold the same surface (the STL file keeps single precision, so only the
    // counts and the size to the micron are compared)
    let counts = |s: &str| s.split(", closed").next().unwrap().to_owned();
    assert_eq!(counts(&lines[0]), counts(&lines[1]));
    assert_eq!(counts(&lines[1]), counts(&lines[2]));
}

#[test]
fn mesh_info_refuses_what_it_cannot_read_and_exits_with_one_for_an_open_surface() {
    let dir = Scratch::new("meshbad");
    let (code, _, err) = run(&["mesh-info", &dir.file("missing.obj")]);
    assert_eq!(code, 1);
    assert!(err.contains("missing.obj"), "{err}");
    let step = dir.file("a.xyz");
    fs::write(&step, "x").unwrap();
    let (code, _, err) = run(&["mesh-info", &step]);
    assert_eq!(code, 1);
    assert!(
        err.contains(".stl, .obj, .3mf, .step, .stp, .ply, .off or .dxf"),
        "{err}"
    );
    let (code, _, err) = run(&["mesh-info"]);
    assert_eq!(code, 1);
    assert!(err.contains("mesh file"), "{err}");
    // one triangle is an open surface: the file is read, the answer is 1
    let one = dir.file("one.obj");
    fs::write(&one, "o t\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    let (code, text, _) = run(&["mesh-info", &one]);
    assert_eq!(code, 1);
    assert!(text.contains("not closed"), "{text}");
    assert!(text.contains("t: 3 vertices, 1 triangles"), "{text}");
}

/// The corners, across and up, of the first four lines on the layer of the border of a DXF file.
fn first_border(dxf: &str) -> (f64, f64) {
    let lines: Vec<&str> = dxf.lines().collect();
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let mut found = 0;
    for i in 0..lines.len().saturating_sub(15) {
        if lines[i] == "LINE" && lines[i + 2] == "MARCO" {
            let number = |k: usize| lines[i + k].parse::<f64>().unwrap();
            xs.extend([number(4), number(10)]);
            ys.extend([number(6), number(12)]);
            found += 1;
            if found == 4 {
                break;
            }
        }
    }
    assert_eq!(found, 4, "four lines of the border");
    let span = |v: &[f64]| {
        v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min)
    };
    (span(&xs), span(&ys))
}

#[test]
fn a_dxf_drawing_on_paper_carries_the_sheet_and_the_scale_in_its_title_block() {
    let dir = Scratch::new("dxfpaper");
    let plain = dir.file("plain.dxf");
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "dxf", "--out", &plain]);
    assert_eq!(code, 0, "{err}");
    assert!(!fs::read_to_string(&plain).unwrap().contains("MARCO"));
    let config = dir.file("paper.cfg");
    fs::write(
        &config,
        "[drawing]\npaper = 4\npaper_margin = 10 mm\n[title_block]\nenabled = 1\n",
    )
    .unwrap();
    let sheet = dir.file("sheet.dxf");
    let (code, _, err) = run(&[
        "export", EXAMPLE, "--format", "dxf", "--config", &config, "--out", &sheet,
    ]);
    assert_eq!(code, 0, "{err}");
    let text = fs::read_to_string(&sheet).unwrap();
    let (w, h) = first_border(&text);
    // the paper is an A4 across, and the drawing is full size at a standard scale
    let scale = 297.0 / w;
    assert!((210.0 / h - scale).abs() < 1e-9, "{w} x {h}");
    let label = eh3d_phoenix_cad::drawing::scale_label(scale, true);
    assert!(
        text.contains(&format!("\n1\n{label}\n")),
        "the scale {label} is in the title block"
    );
    // the same drawing without the title block has the sheet too, and one on upright paper has it upright
    fs::write(&config, "[drawing]\npaper = 4\npaper_portrait = 1\n").unwrap();
    let upright = dir.file("upright.dxf");
    let (code, _, err) = run(&[
        "export", EXAMPLE, "--format", "dxf", "--config", &config, "--out", &upright,
    ]);
    assert_eq!(code, 0, "{err}");
    let (w, h) = first_border(&fs::read_to_string(&upright).unwrap());
    assert!(h > w && (210.0 / w - 297.0 / h).abs() < 1e-9, "{w} x {h}");
}

#[test]
fn the_sheet_is_written_as_a_pdf_file_and_a_pdf_takes_no_single_view() {
    let dir = Scratch::new("pdf");
    let out = dir.file("box.pdf");
    let (code, text, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "pdf",
        "--out",
        &out,
        "--lang",
        "en",
        "--title-field",
        "number=P-9",
    ]);
    assert_eq!(code, 0, "{text} {err}");
    assert!(text.starts_with("Exported box.pdf ("), "{text}");
    let bytes = fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4\n"));
    let (width, height, objects) = eh3d_phoenix_cad::pdf::check(&bytes).expect("well formed");
    assert!(width > 100.0 && height > 100.0 && objects == 6);
    let page = String::from_utf8_lossy(&bytes);
    assert!(page.contains("(P-9)") && page.contains("(Drawing number)"));
    // the file is not written over, and the sheet is the only page
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "pdf", "--out", &out]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD-"), "{err}");
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "pdf", "--view", "top"]);
    assert_eq!(code, 1);
    assert!(err.contains("pdf is always the sheet"), "{err}");
    // --view sheet is the same sheet
    let again = dir.file("again.pdf");
    let (code, text, err) = run(&[
        "export",
        EXAMPLE,
        "--format",
        "pdf",
        "--view",
        "sheet",
        "--out",
        &again,
        "--title-field",
        "number=P-9",
    ]);
    assert_eq!(code, 0, "{text} {err}");
    assert_eq!(fs::read(&again).unwrap(), bytes);
}
