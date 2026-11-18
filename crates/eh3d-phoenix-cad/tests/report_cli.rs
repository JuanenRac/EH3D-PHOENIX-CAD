//! The reports of the commands `cutplan`, `holes`, `clearance`, `placements`, `balance` and
//! `sketch-export`: comma separated values for a program, and with `--text` or `--lang` lines for
//! a person in any of the eight languages.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");
const BOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixproj");
const SKETCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixsketch");
const FRAME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/frame.phoenixproj");
const ASSEMBLY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/assembly.phoenixproj");

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
            "phoenix-cad-report-cli-{name}-{}",
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
fn the_reports_are_english_unless_a_language_is_given() {
    let (code, text, err) = run(&["balance", FRAME]);
    assert_eq!(code, 0, "{err}");
    assert!(text.starts_with("parts: 18\nmass: "), "{text}");
    let (code, text, _) = run(&["balance", FRAME, "--lang", "pt"]);
    assert_eq!(code, 0);
    assert!(text.starts_with("peças: 18\nmassa: "), "{text}");
    assert!(text.contains("centro de massa: "), "{text}");
    let (code, text, _) = run(&["balance", FRAME, "--lang", "ja"]);
    assert_eq!(code, 0);
    assert!(text.starts_with("部品: 18\n質量: "), "{text}");
}

#[test]
fn the_cutting_plan_is_in_the_language_asked_for_and_the_csv_does_not_change() {
    let (code, csv, _) = run(&["cutplan", FRAME, "--lang", "de"]);
    assert_eq!(code, 0);
    assert!(csv.starts_with("part,material,bar,bar_mm,"), "{csv}");
    let (code, text, _) = run(&["cutplan", FRAME, "--text", "--lang", "de"]);
    assert_eq!(code, 0);
    assert!(text.contains(" Stangen zu "), "{text}");
    assert!(text.ends_with("insgesamt 2 Stangen\n"), "{text}");
    let (_, english, _) = run(&["cutplan", FRAME, "--text"]);
    assert!(english.ends_with("2 bars in all\n"), "{english}");
}

#[test]
fn the_hole_schedule_is_a_list_for_the_person_who_drills() {
    let (code, text, err) = run(&["holes", BOX, "--text"]);
    assert_eq!(code, 0, "{err}");
    assert!(text.starts_with("H1 "), "{text}");
    assert!(text.ends_with("holes: 2\n"), "{text}");
    let (code, text, _) = run(&["holes", BOX, "--text", "--lang", "es"]);
    assert_eq!(code, 0);
    assert!(text.contains(": cara "), "{text}");
    assert!(text.ends_with("agujeros: 2\n"), "{text}");
    let (code, tools, _) = run(&["holes", BOX, "--tools", "--text", "--lang", "fr"]);
    assert_eq!(code, 0);
    assert!(tools.contains(" mm, simple : 1\n"), "{tools}");
    assert!(tools.ends_with("trous : 2\n"), "{tools}");
}

#[test]
fn clearance_and_placements_have_a_text_form_too() {
    let (code, text, err) = run(&["clearance", FRAME, "--text", "--lang", "it"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(text, "coppie a meno di 2 mm: 0\n");
    let (code, text, _) = run(&[
        "placements",
        ASSEMBLY,
        "--text",
        "--explode",
        "50",
        "--lang",
        "zh",
    ]);
    assert_eq!(code, 0);
    assert!(text.contains("角点 "), "{text}");
    assert!(text.lines().last().unwrap().starts_with("零件："), "{text}");
    // without --text the same command prints comma separated values
    let (_, csv, _) = run(&["placements", ASSEMBLY, "--explode", "50"]);
    assert!(csv.starts_with("name,part,x_mm,"), "{csv}");
}

#[test]
fn an_unknown_language_is_refused_by_every_report() {
    for command in ["cutplan", "holes", "clearance", "placements", "balance"] {
        let (code, _, err) = run(&[command, FRAME, "--lang", "xx"]);
        assert_eq!(code, 1, "{command}");
        assert!(err.contains("unknown language code"), "{command}: {err}");
    }
    let (code, _, err) = run(&["balance", FRAME, "--lang"]);
    assert_eq!(code, 1);
    assert!(err.contains("--lang needs"), "{err}");
}

#[test]
fn the_answer_of_the_sketch_drawing_is_in_the_language_asked_for() {
    let dir = Scratch::new("sketch");
    let out = dir.file("outline.svg");
    let (code, text, err) = run(&[
        "sketch-export",
        SKETCH,
        "--format",
        "svg",
        "--out",
        &out,
        "--lang",
        "es",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(text.starts_with("Exportado "), "{text}");
    assert!(text.contains("outline.svg"), "{text}");
    assert!(fs::read_to_string(&out).unwrap().contains("<svg"));
}

const PLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.phoenixproj");

#[test]
fn a_sketch_is_turned_about_an_axis_and_its_figures_are_printed() {
    // the outline of the example sketch is 100 mm along x and 60 mm along y: it touches the axis
    let (code, text, err) = run(&["revolve", SKETCH, "--axis", "x"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        text.starts_with("turn: 360 degrees\nprofile area: "),
        "{text}"
    );
    assert!(text.contains("\nalong the axis: 0 to 100 mm\n"), "{text}");
    assert!(
        text.ends_with("distance from the axis: 0 to 60 mm\n"),
        "{text}"
    );
    // about the second axis, a quarter turn, with a material of the settings, in German
    let (code, text, err) = run(&[
        "revolve",
        PLATE,
        "--axis",
        "y",
        "--angle",
        "90",
        "--material",
        "PLA",
        "--lang",
        "de",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(text.starts_with("Drehung: 90 Grad\n"), "{text}");
    assert!(
        text.contains("\nentlang der Achse: 0 bis 60 mm\n"),
        "{text}"
    );
    assert!(
        text.ends_with(" g\n") && text.contains("Masse aus PLA: "),
        "{text}"
    );
    // a full turn is the same as no angle
    let (_, full, _) = run(&["revolve", SKETCH, "--axis", "x", "--angle", "360"]);
    let (_, none, _) = run(&["revolve", SKETCH, "--axis", "x"]);
    assert_eq!(full, none);
}

#[test]
fn the_turn_of_a_sketch_refuses_what_it_cannot_do() {
    for (args, why) in [
        (vec!["revolve", SKETCH], "needs --axis"),
        (vec!["revolve", SKETCH, "--axis", "z"], "unknown axis"),
        (
            vec!["revolve", SKETCH, "--axis", "x", "--angle", "0"],
            "PHOENIX-CAD-007",
        ),
        (
            vec!["revolve", SKETCH, "--axis", "x", "--angle", "361"],
            "PHOENIX-CAD-007",
        ),
        (
            vec!["revolve", SKETCH, "--axis", "x", "--material", "wood"],
            "not a material",
        ),
        (
            vec!["revolve", "--axis", "x"],
            "needs a project or a sketch",
        ),
        (
            vec!["revolve", SKETCH, "--axis", "x", "--lang", "xx"],
            "unknown language",
        ),
    ] {
        let (code, _, err) = run(&args);
        assert_eq!(code, 1, "{args:?}");
        assert!(err.contains(why), "{args:?}: {err}");
    }
}

#[test]
fn a_profile_is_swept_along_a_path_and_its_figures_are_printed() {
    let dir = Scratch::new("sweep");
    let profile = dir.file("profile.phoenixsketch");
    fs::write(
        &profile,
        "phoenix-sketch 1\nname profile\nplane top 0 0 0\nnext 3 1\ne1 point 0 0\ne2 circle e1 3000000\n",
    )
    .unwrap();
    let path = dir.file("path.phoenixsketch");
    fs::write(
        &path,
        "phoenix-sketch 1\nname path\nplane top 0 0 0\nnext 4 1\ne1 point 0 0\ne2 point 0 100000000\ne3 line e1 e2\n",
    )
    .unwrap();
    // a tube of 3 mm radius and 100 mm: 900 pi = 2827.433 mm3, the sides 600 pi and the ends 18 pi
    let (code, text, err) = run(&["sweep", &profile, "--path", &path]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        text,
        "path: 100.000 mm, pieces: 1\nprofile area: 28.274 mm2\nperimeter of the profile: 18.850 mm\nvolume: 2827.433 mm3\nsurface: 1941.504 mm2\n"
    );
    // in Spanish and with a material of the settings: 2827.433 mm3 of PLA at 1240 kg/m3
    let (code, text, err) = run(&[
        "sweep",
        &profile,
        "--path",
        &path,
        "--material",
        "PLA",
        "--lang",
        "es",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        text.starts_with("camino: 100.000 mm, tramos: 1\n"),
        "{text}"
    );
    assert!(text.ends_with("masa de PLA: 3.506 g\n"), "{text}");
}

#[test]
fn the_sweep_refuses_what_it_cannot_do() {
    let dir = Scratch::new("sweep-refused");
    let profile = dir.file("profile.phoenixsketch");
    fs::write(
        &profile,
        "phoenix-sketch 1\nname profile\nplane top 0 0 0\nnext 3 1\ne1 point 0 0\ne2 circle e1 3000000\n",
    )
    .unwrap();
    let line = dir.file("line.phoenixsketch");
    fs::write(
        &line,
        "phoenix-sketch 1
name line
plane top 0 0 0
next 4 1
e1 point 0 0
e2 point 0 100000000
e3 line e1 e2
",
    )
    .unwrap();
    let empty = dir.file("empty.phoenixsketch");
    fs::write(
        &empty,
        "phoenix-sketch 1\nname empty\nplane top 0 0 0\nnext 1 1\n",
    )
    .unwrap();
    for (args, why) in [
        (vec!["sweep", profile.as_str()], "needs --path"),
        (vec!["sweep", "--path", profile.as_str()], "needs a project"),
        (
            vec!["sweep", profile.as_str(), "--path", empty.as_str()],
            "PHOENIX-CAD-036",
        ),
        (
            vec![
                "sweep",
                profile.as_str(),
                "--path",
                line.as_str(),
                "--material",
                "wood",
            ],
            "not a material",
        ),
        (
            vec!["sweep", profile.as_str(), "--path", line.as_str(), "--what"],
            "unknown option",
        ),
    ] {
        let (code, _, err) = run(&args);
        assert_eq!(code, 1, "{args:?}");
        assert!(err.contains(why), "{args:?}: {err}");
    }
}
