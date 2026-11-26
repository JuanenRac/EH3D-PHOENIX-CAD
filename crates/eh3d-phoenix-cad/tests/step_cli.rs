//! STEP files on the command line: `sketch-export --format step` raises a sketch into a prism and
//! `mesh-info` reads a STEP file back, with the limits of the settings, and tells the mass.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");
const SKETCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixsketch");

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
            "phoenix-cad-step-cli-{name}-{}",
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

/// The number after `word` in a line of the report.
fn number_after(text: &str, word: &str) -> f64 {
    let rest = text
        .split(word)
        .nth(1)
        .unwrap_or_else(|| panic!("{word} in {text}"));
    rest.split_whitespace()
        .next()
        .unwrap()
        .trim_end_matches(',')
        .parse()
        .unwrap_or_else(|_| panic!("a number after {word} in {text}"))
}

fn plate_step(dir: &Scratch) -> String {
    let out = dir.file("plate.step");
    let (code, text, err) = run(&[
        "sketch-export",
        SKETCH,
        "--format",
        "step",
        "--height",
        "4 mm",
        "--out",
        &out,
    ]);
    assert_eq!(code, 0, "{text} {err}");
    out
}

#[test]
fn a_sketch_is_raised_into_a_step_file_that_reads_back_as_a_prism_with_its_holes() {
    let dir = Scratch::new("raise");
    let out = plate_step(&dir);
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("ISO-10303-21;"), "{text}");
    assert!(text.contains("MANIFOLD_SOLID_BREP('plate',"));
    let (code, report, err) = run(&["mesh-info", &out]);
    assert_eq!(code, 0, "{report} {err}");
    assert!(report.contains(" 1 solids"), "{report}");
    assert!(report.contains("plate: "), "{report}");
    assert!(report.contains(", closed, volume "), "{report}");
    assert!(
        report.contains("prism along z: 4.000 mm high from z = 0.000 mm, 3 outlines with "),
        "{report}"
    );
    // 100 x 60 mm less two holes of 5 mm across, 4 mm high: the holes are polygons, a little smaller than circles
    let volume = number_after(&report, "volume ");
    assert!((23_700.0..23_850.0).contains(&volume), "{volume}");
    assert!(
        report.contains("size 100.000 x 60.000 x 4.000 mm"),
        "{report}"
    );
    // the same sketch gives the same file, and a file that exists is kept
    let again = dir.file("again.step");
    let (code, _, _) = run(&[
        "sketch-export",
        SKETCH,
        "--format",
        "step",
        "--height",
        "4mm",
        "--out",
        &again,
    ]);
    assert_eq!(code, 0);
    assert_eq!(fs::read_to_string(&again).unwrap(), text);
    let (code, _, err) = run(&[
        "sketch-export",
        SKETCH,
        "--format",
        "step",
        "--height",
        "4 mm",
        "--out",
        &out,
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD-040"), "{err}");
}

#[test]
fn the_step_export_of_a_sketch_needs_a_height_and_a_height_needs_the_step_format() {
    let dir = Scratch::new("height");
    let out = dir.file("a.step");
    let (code, _, err) = run(&["sketch-export", SKETCH, "--format", "step", "--out", &out]);
    assert_eq!(code, 1);
    assert!(err.contains("needs --height"), "{err}");
    let (code, _, err) = run(&[
        "sketch-export",
        SKETCH,
        "--format",
        "dxf",
        "--height",
        "4 mm",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("--height is for --format step"), "{err}");
    for bad in ["0 mm", "-3 mm", "tall"] {
        let (code, _, err) = run(&[
            "sketch-export",
            SKETCH,
            "--format",
            "step",
            "--height",
            bad,
            "--out",
            &out,
        ]);
        assert_eq!(code, 1, "{bad}");
        assert!(err.contains("PHOENIX-CAD"), "{bad}: {err}");
        assert!(!std::path::Path::new(&out).exists(), "{bad}");
    }
    let (code, _, err) = run(&["sketch-export", SKETCH, "--format", "iges"]);
    assert_eq!(code, 1);
    assert!(err.contains("dxf, svg or step"), "{err}");
}

#[test]
fn a_density_gives_the_mass_the_centre_and_the_inertia_of_the_solid() {
    let dir = Scratch::new("mass");
    let out = plate_step(&dir);
    let (code, report, err) = run(&["mesh-info", &out, "--density", "1240"]);
    assert_eq!(code, 0, "{report} {err}");
    let volume = number_after(&report, "volume ");
    let grams = number_after(&report, "mass ");
    // a cubic millimetre of a material of 1240 kg/m3 weighs 0.00124 g
    assert!((grams - volume * 1.24e-3).abs() < 0.002, "{grams} {volume}");
    assert!(
        report.contains("at 1240 kg/m3, centre of mass "),
        "{report}"
    );
    assert!(
        report.contains("inertia about the centre of mass (g mm2): xx "),
        "{report}"
    );
    // the plate is nearly symmetric: the centre is near the middle, 2 mm up
    let after = report.split("centre of mass ").nth(1).unwrap();
    let c: Vec<f64> = after
        .split_whitespace()
        .take(3)
        .map(|w| w.parse().unwrap())
        .collect();
    assert!((c[2] - 2.0).abs() < 1e-6, "{c:?}");
    assert!(
        (c[0] - 50.0).abs() < 0.2 && (c[1] - 30.0).abs() < 0.2,
        "{c:?}"
    );
    // a material of the settings is the same as its density
    let (code, by_material, _) = run(&["mesh-info", &out, "--material", "PETG"]);
    assert_eq!(code, 0);
    assert!(by_material.contains("mass "), "{by_material}");
    // what is wrong with the options
    for (args, message) in [
        (vec!["--density", "0"], "from 1 to 100000"),
        (vec!["--density", "100001"], "from 1 to 100000"),
        (vec!["--density", "heavy"], "from 1 to 100000"),
        (vec!["--density"], "--density needs"),
        (vec!["--material", "unobtainium"], "not a material"),
        (
            vec!["--density", "1000", "--material", "PLA"],
            "cannot be given together",
        ),
    ] {
        let mut full = vec!["mesh-info", out.as_str()];
        full.extend(args.iter().copied());
        let (code, _, err) = run(&full);
        assert_eq!(code, 1, "{args:?}");
        assert!(err.contains(message), "{args:?}: {err}");
    }
    // without a density nothing about mass is said
    let (_, plain, _) = run(&["mesh-info", &out]);
    assert!(!plain.contains("mass "), "{plain}");
}

#[test]
fn the_limits_of_the_settings_refuse_a_step_file_with_their_name() {
    let dir = Scratch::new("limits");
    let out = plate_step(&dir);
    for (key, message) in [
        ("step_max_faces = 6", "faces that the settings allow"),
        ("step_max_entities = 10", "entities that the settings allow"),
        ("max_triangles = 12", "triangles that the settings allow"),
    ] {
        let config = dir.file("limit.cfg");
        fs::write(&config, format!("[mesh]\n{key}\n")).unwrap();
        let (code, _, err) = run(&["mesh-info", &out, "--config", &config]);
        assert_eq!(code, 1, "{key}");
        assert!(err.contains(message), "{key}: {err}");
    }
    let config = dir.file("size.cfg");
    fs::write(&config, "[mesh]\nstep_max_kib = 1\n").unwrap();
    let (code, _, err) = run(&["mesh-info", &out, "--config", &config]);
    assert_eq!(code, 1);
    assert!(err.contains("1 KiB"), "{err}");
    // the keys are in the file that the config command writes
    let (code, defaults, _) = run(&["config"]);
    assert_eq!(code, 0);
    for key in [
        "step_max_kib = 65536",
        "step_max_entities = 1000000",
        "step_max_faces = 100000",
    ] {
        assert!(defaults.contains(key), "{key}");
    }
}

#[test]
fn a_face_that_cannot_be_read_is_refused_by_name_and_a_stp_file_is_read() {
    let dir = Scratch::new("refuse");
    let plate = plate_step(&dir);
    // the first plane of the file becomes a torus
    let text = fs::read_to_string(&plate).unwrap();
    let curved = dir.file("curved.step");
    fs::write(
        &curved,
        text.replacen("=PLANE('',", "=TOROIDAL_SURFACE('',", 1),
    )
    .unwrap();
    let (code, _, err) = run(&["mesh-info", &curved]);
    assert_eq!(code, 1);
    assert!(
        err.contains("TOROIDAL_SURFACE") && err.contains("only planes, cylinders and cones"),
        "{err}"
    );
    assert!(err.contains("curved.step"), "{err}");
    // a file with the extension .stp is a STEP file too, in any case
    let stp = dir.file("PLATE.STP");
    fs::copy(&plate, &stp).unwrap();
    let (code, report, err) = run(&["mesh-info", &stp]);
    assert_eq!(code, 0, "{err}");
    assert!(report.contains("prism along z"), "{report}");
    // a file that says it is STEP and is not one
    let fake = dir.file("fake.step");
    fs::write(&fake, "solid text").unwrap();
    let (code, _, err) = run(&["mesh-info", &fake]);
    assert_eq!(code, 1);
    assert!(err.contains("not a STEP file"), "{err}");
}
