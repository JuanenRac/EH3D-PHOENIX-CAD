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
fn the_arguments_are_checked() {
    let (code, _, err) = run(&["export", EXAMPLE]);
    assert_eq!(code, 1);
    assert!(err.contains("needs --format"), "{err}");
    let (code, _, err) = run(&["export", EXAMPLE, "--format", "obj"]);
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
    assert!(err.contains("top, front or side"), "{err}");
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
