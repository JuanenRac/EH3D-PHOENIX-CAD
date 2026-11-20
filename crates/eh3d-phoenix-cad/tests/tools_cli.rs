//! The small tools of the command line, run as the program is run: `convert`, `bolt-circle`,
//! `measure`, `templates`, `new`, `bom-costs`, `config-diff`, `sketch-info` and `mesh-check`, with
//! their texts, their exit codes and the eight languages.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");
const SKETCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixsketch");
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
            "phoenix-cad-tools-cli-{name}-{}",
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

/// An ASCII STL file of a cube of 10 mm, wound outwards or (with `inside`) inside out.
fn cube_stl(inside: bool) -> String {
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
    let mut out = String::from("solid cube\n");
    for f in faces {
        let order = if inside { [f[0], f[2], f[1]] } else { f };
        out.push_str("facet normal 0 0 0\n outer loop\n");
        for i in order {
            let c = corner(i);
            out.push_str(&format!("  vertex {} {} {}\n", c[0], c[1], c[2]));
        }
        out.push_str(" endloop\nendfacet\n");
    }
    out.push_str("endsolid cube\n");
    out
}

#[test]
fn convert_writes_one_unit_in_another() {
    let (code, text, err) = run(&["convert", "1 1/2 in"]);
    assert_eq!((code, text.as_str()), (0, "38.100 mm\n"), "{err}");
    let (_, text, _) = run(&["convert", "38.1 mm", "--to", "in", "--fraction", "16"]);
    assert_eq!(text, "1 1/2 in\n");
    let (_, text, _) = run(&["convert", "12.5", "--to", "dms"]);
    assert_eq!(text, "12°30'00\"\n");
    let (_, text, _) = run(&["convert", "180", "--to", "rad", "--decimals", "4"]);
    assert_eq!(text, "3.1416 rad\n");
    let (code, _, err) = run(&["convert", "1 ft"]);
    assert_eq!(code, 1);
    assert!(err.starts_with("error: PHOENIX-CAD-008"), "{err}");
}

#[test]
fn the_bolt_circle_and_the_measures_print_what_the_drill_and_the_person_read() {
    let (code, csv, _) = run(&["bolt-circle", "--count", "4", "--diameter", "20 mm"]);
    assert_eq!(code, 0);
    assert_eq!(
        csv,
        "hole,angle_deg,x_mm,y_mm\n1,0,10,0\n2,90,0,10\n3,180,-10,0\n4,270,0,-10\n"
    );
    let (_, text, _) = run(&[
        "bolt-circle",
        "--count",
        "6",
        "--diameter",
        "40",
        "--text",
        "--lang",
        "pt",
    ]);
    assert!(text.starts_with("Círculo de 6 furos"), "{text}");
    let (_, text, _) = run(&["measure", "distance", "0:0:0", "3:4:0", "--lang", "de"]);
    assert_eq!(
        text,
        "Abstand 5.0000 mm (x 3.0000, y 4.0000, z 0.0000 mm)\n"
    );
    let (_, text, _) = run(&["measure", "angle", "1:0:0", "0:0:0", "0:1:0"]);
    assert_eq!(text, "Angle 90.000 degrees\n");
    assert_eq!(run(&["measure", "angle", "1:0:0"]).0, 1);
}

#[test]
fn a_new_project_comes_from_a_template_and_its_cost_is_shared_out() {
    let (code, list, _) = run(&["templates", "--lang", "es"]);
    assert_eq!(code, 0);
    assert_eq!(list.lines().count(), 4);
    let dir = Scratch::new("new");
    let file = dir.file("mine.phoenixproj");
    let (code, text, err) = run(&["new", "--template", "frame", "--out", &file]);
    assert_eq!(code, 0, "{err}");
    assert!(text.starts_with("Saved "), "{text}");
    assert_eq!(run(&["new", "--template", "box", "--out", &file]).0, 1);
    assert_eq!(fs::read(&file).unwrap(), fs::read(FRAME).unwrap());
    let (code, csv, err) = run(&["bom-costs", &file, "--tax", "21", "--top", "2"]);
    assert_eq!(code, 0, "{err}");
    assert!(csv.contains(",tax,21%,"), "{csv}");
    assert_eq!(csv.lines().count(), 1 + 2 + 5);
    let (_, text, _) = run(&["bom-costs", &file, "--text", "--lang", "ja"]);
    assert!(!text.contains('{') && text.lines().count() > 3, "{text}");
}

#[test]
fn the_settings_and_the_sketches_are_read_back_as_a_summary() {
    let dir = Scratch::new("cfg");
    let cfg = dir.file("mine.cfg");
    fs::write(&cfg, "[printer]\nhole_compensation = 0.2 mm\n").unwrap();
    let (code, csv, _) = run(&["config-diff", &cfg]);
    assert_eq!(code, 0);
    assert_eq!(
        csv,
        "key,default,value,status\nprinter.hole_compensation,0.15 mm,0.2 mm,changed\n"
    );
    fs::write(&cfg, "[nonsense]\nfoo = 1\n").unwrap();
    assert_eq!(run(&["config-diff", &cfg, "--text"]).0, 1);
    let (code, text, err) = run(&["sketch-info", SKETCH]);
    assert_eq!(code, 0, "{err}");
    assert!(text.starts_with("Sketch "), "{text}");
    assert_eq!(text.lines().count(), 3);
    assert_eq!(run(&["sketch-info", &dir.file("none.phoenixsketch")]).0, 1);
}

#[test]
fn a_mesh_is_checked_and_a_surface_that_is_inside_out_is_turned_right() {
    let dir = Scratch::new("mesh");
    let good = dir.file("good.stl");
    let bad = dir.file("bad.stl");
    let fixed = dir.file("fixed.obj");
    fs::write(&good, cube_stl(false)).unwrap();
    fs::write(&bad, cube_stl(true)).unwrap();
    let (code, text, err) = run(&["mesh-check", &good]);
    assert_eq!(code, 0, "{err}{text}");
    assert!(text.contains("Euler characteristic 2, genus 0"), "{text}");
    assert!(text.contains("Printable"), "{text}");
    let (code, text, _) = run(&["mesh-check", &bad, "--lang", "es"]);
    assert_eq!(code, 1);
    assert!(text.contains("del revés"), "{text}");
    let (code, text, err) = run(&["mesh-check", &bad, "--repair", &fixed]);
    assert_eq!(code, 0, "{err}{text}");
    assert_eq!(run(&["mesh-check", &fixed]).0, 0);
    assert_eq!(run(&["mesh-check", &bad, "--repair", &fixed]).0, 1);
    let (code, text, _) = run(&["mesh-check", &good, "--bed", "5:5:5"]);
    assert_eq!(code, 1);
    assert!(text.contains("Does not fit"), "{text}");
}
