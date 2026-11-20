//! The tools for files of triangles and for tolerances, run as the program is run: `mesh-convert`,
//! `mesh-transform`, `mesh-pattern`, `mesh-slice`, `mesh-clip` and `iso-fit`, and the import of an
//! SVG drawing, with their texts and their exit codes.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");

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
            "phoenix-cad-mesh-cli-{name}-{}",
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

/// An ASCII STL file of a cube of 10 mm with a corner at the origin, without the last two
/// triangles when `open` is true.
fn cube_stl(open: bool) -> String {
    let corner = |i: usize| -> [f64; 3] {
        [
            if i & 1 == 1 { 10.0 } else { 0.0 },
            if i & 2 == 2 { 10.0 } else { 0.0 },
            if i & 4 == 4 { 10.0 } else { 0.0 },
        ]
    };
    let faces = [
        [0, 2, 1],
        [1, 2, 3],
        [4, 5, 6],
        [5, 7, 6],
        [0, 1, 4],
        [1, 5, 4],
        [2, 6, 3],
        [3, 6, 7],
        [0, 4, 2],
        [2, 4, 6],
        [1, 3, 5],
        [3, 7, 5],
    ];
    let mut text = String::from("solid cube\n");
    let count = if open { 10 } else { 12 };
    for face in faces.iter().take(count) {
        text.push_str("facet normal 0 0 0\nouter loop\n");
        for i in face {
            let c = corner(*i);
            text.push_str(&format!("vertex {} {} {}\n", c[0], c[1], c[2]));
        }
        text.push_str("endloop\nendfacet\n");
    }
    text.push_str("endsolid cube\n");
    text
}

#[test]
fn a_cube_goes_through_the_formats_and_is_still_a_printable_solid() {
    let dir = Scratch::new("convert");
    let stl = dir.file("cube.stl");
    fs::write(&stl, cube_stl(false)).unwrap();
    let mut current = stl.clone();
    for ext in ["3mf", "ply", "off", "dxf", "step", "obj"] {
        let next = dir.file(&format!("cube.{ext}"));
        let (code, text, err) = run(&["mesh-convert", &current, &next]);
        assert_eq!(code, 0, "{ext}: {err}");
        assert!(text.contains("Wrote 1 solid(s) and 12 triangles"), "{text}");
        current = next;
    }
    let (code, text, _) = run(&["mesh-check", &current]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("1000"), "{text}");
    // a second conversion over the same name is refused, and allowed with the switch
    let (code, _, err) = run(&["mesh-convert", &stl, &current]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD"), "{err}");
    let (code, _, _) = run(&["mesh-convert", &stl, &current, "--overwrite"]);
    assert_eq!(code, 0);
    let (code, _, err) = run(&["mesh-convert", &stl, &dir.file("cube.xyz")]);
    assert_eq!(code, 1);
    assert!(err.contains(".step or .stp"), "{err}");
}

#[test]
fn a_cube_is_moved_repeated_cut_and_sectioned() {
    let dir = Scratch::new("ops");
    let stl = dir.file("cube.stl");
    fs::write(&stl, cube_stl(false)).unwrap();
    let moved = dir.file("moved.stl");
    let (code, _, err) = run(&[
        "mesh-transform",
        &stl,
        &moved,
        "--scale",
        "2",
        "--move",
        "5:0:0",
    ]);
    assert_eq!(code, 0, "{err}");
    let (code, text, _) = run(&["mesh-slice", &moved, "--axis", "z", "--at", "10", "--csv"]);
    assert_eq!(code, 0);
    assert!(
        text.ends_with("solid,1,1,0,400.000000,80.000000,0\n"),
        "{text}"
    );
    let row = dir.file("row.stl");
    let (code, _, err) = run(&[
        "mesh-pattern",
        &stl,
        &row,
        "--linear",
        "3",
        "--step",
        "20:0:0",
    ]);
    assert_eq!(code, 0, "{err}");
    let (code, text, _) = run(&["mesh-check", &row]);
    assert_eq!(code, 0, "{text}");
    let cut = dir.file("cut.3mf");
    let (code, text, _) = run(&["mesh-clip", &stl, &cut, "--box", "2:2:2..6:20:20"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("kept 256.000000 mm3"), "{text}");
    let (code, text, _) = run(&["mesh-check", &cut]);
    assert_eq!(code, 0, "{text}");
    let (code, text, _) = run(&[
        "mesh-clip",
        &stl,
        &dir.file("none.stl"),
        "--axis",
        "z",
        "--at",
        "99",
    ]);
    assert_eq!(code, 1);
    assert!(text.contains("nothing of the solid is left"), "{text}");
}

#[test]
fn a_section_that_does_not_close_exits_with_one() {
    let dir = Scratch::new("open");
    let stl = dir.file("open.stl");
    fs::write(&stl, cube_stl(true)).unwrap();
    let (code, text, _) = run(&["mesh-slice", &stl, "--axis", "y", "--at", "5"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("do not close"), "{text}");
    let (code, text, _) = run(&[
        "mesh-slice",
        &stl,
        "--axis",
        "y",
        "--at",
        "5",
        "--lang",
        "es",
    ]);
    assert_eq!(code, 1);
    assert!(text.contains("no se cierran"), "{text}");
}

#[test]
fn the_fits_say_that_the_table_is_not_verified() {
    let (code, text, _) = run(&["iso-fit", "25", "H7/g6"]);
    assert_eq!(code, 0);
    assert!(text.contains("clearance from 7 to 41 um"), "{text}");
    assert!(text.contains("not verified"), "{text}");
    let (code, text, _) = run(&["iso-fit", "25", "H7/p6", "--lang", "pt"]);
    assert_eq!(code, 0);
    assert!(text.contains("ajuste com aperto"), "{text}");
    let (code, text, _) = run(&["iso-fit", "25", "H7/k6", "--csv"]);
    assert_eq!(code, 0);
    assert!(text.ends_with(",transition\n"), "{text}");
    let (code, _, err) = run(&["iso-fit", "600", "H7/g6"]);
    assert_eq!(code, 1);
    assert!(err.contains("at most 500"), "{err}");
}

#[test]
fn an_svg_drawing_is_read_into_a_sketch() {
    let dir = Scratch::new("svg");
    let svg = dir.file("panel.svg");
    fs::write(
        &svg,
        "<svg xmlns='http://www.w3.org/2000/svg' width='80mm' height='40mm' viewBox='0 0 80 40'>\
         <rect x='0' y='0' width='80' height='40'/><circle cx='40' cy='20' r='5'/><text>x</text></svg>",
    )
    .unwrap();
    let out = dir.file("panel.phoenixsketch");
    let (code, text, err) = run(&["sketch-import", &svg, "--out", &out]);
    assert_eq!(code, 0, "{err}");
    assert!(text.contains("4 lines"), "{text}");
    assert!(text.contains("1 circle"), "{text}");
    assert!(fs::metadata(&out).unwrap().len() > 0);
    let (code, text, _) = run(&["sketch-info", &out]);
    assert_eq!(code, 0);
    assert!(text.contains("4 lines"), "{text}");
    let bad = dir.file("bad.svg");
    fs::write(&bad, "<svg><g></svg>").unwrap();
    let (code, _, err) = run(&["sketch-import", &bad, "--out", &dir.file("x.phoenixsketch")]);
    assert_eq!(code, 1);
    assert!(err.contains("not well formed"), "{err}");
}
