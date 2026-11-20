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

#[test]
fn the_triangles_of_a_turned_or_swept_solid_are_written_in_the_format_of_the_extension() {
    let dir = Scratch::new("solids");
    let stl = dir.file("plate.stl");
    let (code, text, err) = run(&[
        "revolve", PLATE, "--axis", "y", "--angle", "90", "--out", &stl,
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(text.contains("volume: "), "{text}");
    assert!(text.contains("plate.stl"), "{text}");
    let bytes = fs::read(&stl).unwrap();
    // a binary STL file: 80 bytes of header, the count and 50 bytes for each triangle
    let triangles = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    assert!(triangles > 100);
    assert_eq!(bytes.len(), 84 + 50 * triangles);
    // it is not written over a file that exists, unless it is asked for
    let (code, _, err) = run(&["revolve", PLATE, "--axis", "y", "--out", &stl]);
    assert_eq!(code, 1);
    assert!(err.contains("PHOENIX-CAD-040"), "{err}");
    let (code, _, err) = run(&[
        "revolve",
        PLATE,
        "--axis",
        "y",
        "--out",
        &stl,
        "--overwrite",
    ]);
    assert_eq!(code, 0, "{err}");
    // an OBJ file and a 3MF file of the same solid, and an extension that is neither
    let obj = dir.file("plate.obj");
    let (code, _, err) = run(&["revolve", PLATE, "--axis", "y", "--out", &obj]);
    assert_eq!(code, 0, "{err}");
    assert!(fs::read_to_string(&obj)
        .unwrap()
        .starts_with("# EH3D PHOENIX-CAD"));
    let mf = dir.file("plate.3mf");
    let (code, _, err) = run(&["revolve", PLATE, "--axis", "y", "--out", &mf]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(&fs::read(&mf).unwrap()[..2], b"PK");
    let (code, _, err) = run(&["revolve", PLATE, "--axis", "y", "--out", &dir.file("x.txt")]);
    assert_eq!(code, 1);
    assert!(err.contains("ends in .stl"), "{err}");
    // the sweep writes its solid the same way
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
    let tube = dir.file("tube.obj");
    let (code, text, err) = run(&["sweep", &profile, "--path", &path, "--out", &tube]);
    assert_eq!(code, 0, "{err}");
    assert!(text.contains("volume: 2827.433 mm3\n"), "{text}");
    let obj = fs::read_to_string(&tube).unwrap();
    assert!(obj.contains("\no profile\n"), "{obj}");
}
#[test]
fn the_part_list_is_csv_for_a_program_and_text_for_a_person() {
    let (code, csv, err) = run(&["partlist", FRAME]);
    assert_eq!(code, 0, "{err}");
    assert!(
        csv.starts_with("name,kind,part,material,density_kg_m3,volume_mm3,mass_g,"),
        "{csv}"
    );
    assert_eq!(csv.lines().count(), 20);
    assert!(
        csv.lines().last().unwrap().starts_with("total,18 parts,"),
        "{csv}"
    );
    // the mass of the list is the mass of the balance
    let (_, balance, _) = run(&["balance", FRAME]);
    let grams = balance
        .lines()
        .find_map(|l| l.strip_prefix("mass: "))
        .unwrap()
        .to_owned();
    assert!(
        csv.lines()
            .last()
            .unwrap()
            .contains(&format!(",{}", grams.trim_end_matches(" g"))),
        "{csv} {grams}"
    );
    let (code, text, _) = run(&["partlist", FRAME, "--text", "--lang", "es"]);
    assert_eq!(code, 0);
    assert!(text.contains(" (perfil, "), "{text}");
    assert!(
        text.lines()
            .last()
            .unwrap()
            .starts_with("piezas: 18, volumen: "),
        "{text}"
    );
    // the language alone is enough, and a language that is not one is refused
    let (_, de, _) = run(&["partlist", FRAME, "--lang", "de"]);
    assert!(de.starts_with("name,kind,"));
    let (code, _, err) = run(&["partlist", FRAME, "--lang", "xx"]);
    assert_eq!(code, 1);
    assert!(err.contains("unknown language"), "{err}");
    let (code, _, _) = run(&["partlist"]);
    assert_eq!(code, 1);
}

#[test]
fn the_bill_of_materials_has_the_mass_of_the_balance_and_a_cost_that_the_settings_decide() {
    let (code, csv, err) = run(&["bom", FRAME]);
    assert_eq!(code, 0, "{err}");
    let rows: Vec<Vec<&str>> = csv.lines().map(|l| l.split(',').collect()).collect();
    assert_eq!(rows[0][0..3], ["item", "name", "kind"]);
    assert!(rows.iter().all(|r| r.len() == 17), "{csv}");
    let n = rows.len();
    assert_eq!(
        [rows[n - 3][0], rows[n - 2][0], rows[n - 1][0]],
        ["subtotal", "markup", "total"]
    );
    // the lines hold the 18 parts of the frame, and their mass is the mass of the balance
    let pieces: u32 = rows[1..n - 3]
        .iter()
        .map(|r| r[8].parse::<u32>().unwrap())
        .sum();
    assert_eq!(pieces, 18);
    assert_eq!(rows[n - 3][8], "18");
    let (_, balance, _) = run(&["balance", FRAME]);
    let grams = balance
        .lines()
        .find_map(|l| l.strip_prefix("mass: "))
        .unwrap()
        .trim_end_matches(" g")
        .to_owned();
    assert_eq!(rows[n - 3][11], grams, "{csv}");
    // every material of the frame has a price in the shipped settings
    assert!(rows[1..n - 3].iter().all(|r| r[16] == "yes"), "{csv}");
    let milli = |text: &str| text.replace('.', "").parse::<i64>().unwrap();
    let by_lines: i64 = rows[1..n - 3].iter().map(|r| milli(r[14])).sum();
    assert_eq!(milli(rows[n - 3][14]), by_lines);
    assert_eq!(milli(rows[n - 1][14]), by_lines);
    // a settings file with what a piece costs and a markup changes the cost and not the mass
    let dir = Scratch::new("bom");
    let config = dir.file("cost.cfg");
    fs::write(
        &config,
        "[cost]
currency = USD
per_piece = 1.5
markup_percent = 20
",
    )
    .unwrap();
    let (code, priced, err) = run(&["bom", FRAME, "--config", &config]);
    assert_eq!(code, 0, "{err}");
    let rows: Vec<Vec<&str>> = priced.lines().map(|l| l.split(',').collect()).collect();
    let n = rows.len();
    let subtotal = milli(rows[n - 3][14]);
    assert_eq!(subtotal, by_lines + 18 * 1_500);
    assert_eq!(milli(rows[n - 2][14]), (subtotal * 20 + 50) / 100);
    assert_eq!(
        milli(rows[n - 1][14]),
        subtotal + (subtotal * 20 + 50) / 100
    );
    assert_eq!(rows[n - 1][15], "USD");
    assert_eq!(rows[n - 3][11], grams);
    // the text is in the language that is asked for
    let (code, text, _) = run(&["bom", FRAME, "--text", "--lang", "de"]);
    assert_eq!(code, 0);
    assert!(text.contains("Zwischensumme "), "{text}");
    assert!(text.contains("Teile: 18, Volumen: "), "{text}");
    let (code, _, _) = run(&["bom"]);
    assert_eq!(code, 1);
    // a price for a material the settings do not have is refused
    fs::write(
        &config,
        "[material.unobtainium]
price_per_kg = 1
",
    )
    .unwrap();
    let (code, _, err) = run(&["bom", FRAME, "--config", &config]);
    assert_eq!(code, 1);
    assert!(err.contains("unobtainium"), "{err}");
}

#[test]
fn a_section_of_the_model_is_printed_and_written_as_a_drawing() {
    let (code, text, err) = run(&["section", BOX, "--axis", "z", "--at", "10"]);
    assert_eq!(code, 0, "{err}");
    assert!(text.starts_with("section plane: z = 10 mm\n"), "{text}");
    assert!(text.contains("\nenclosure: "), "{text}");
    let (code, text, _) = run(&[
        "section", BOX, "--axis", "z", "--at", "10 mm", "--lang", "es",
    ]);
    assert_eq!(code, 0);
    assert!(text.starts_with("plano de sección: z = 10 mm\n"), "{text}");
    // a plane outside the model says so
    let (code, text, _) = run(&["section", BOX, "--axis", "x", "--at", "9000"]);
    assert_eq!(code, 0);
    assert!(text.contains("does not cut the model"), "{text}");
    // the drawing: the format is the extension or --format, and a file is never written over
    let dir = Scratch::new("section");
    let dxf = dir.file("cut.dxf");
    let (code, text, err) = run(&["section", BOX, "--axis", "y", "--at", "10", "--out", &dxf]);
    assert_eq!(code, 0, "{err}");
    assert!(text.contains("cut.dxf"), "{text}");
    assert!(fs::read_to_string(&dxf).unwrap().contains("CONTORNO"));
    let (code, _, err) = run(&["section", BOX, "--axis", "y", "--at", "10", "--out", &dxf]);
    assert_ne!(code, 0);
    assert!(!err.is_empty());
    let (code, _, err) = run(&[
        "section",
        BOX,
        "--axis",
        "y",
        "--at",
        "10",
        "--out",
        &dxf,
        "--overwrite",
    ]);
    assert_eq!(code, 0, "{err}");
    let svg = dir.file("cut.out");
    let (code, _, err) = run(&[
        "section", BOX, "--axis", "x", "--at", "10", "--format", "svg", "--out", &svg,
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(fs::read_to_string(&svg).unwrap().starts_with("<?xml"));
    // what is refused
    for (args, why) in [
        (vec!["section", BOX, "--at", "10"], "needs --axis"),
        (vec!["section", BOX, "--axis", "x"], "needs --at"),
        (
            vec!["section", BOX, "--axis", "w", "--at", "10"],
            "the axis is x, y or z",
        ),
        (
            vec!["section", "--axis", "x", "--at", "10"],
            "needs a project",
        ),
        (
            vec![
                "section", BOX, "--axis", "x", "--at", "10", "--out", "cut.png",
            ],
            "ends in .dxf or .svg",
        ),
    ] {
        let (code, _, err) = run(&args);
        assert_ne!(code, 0, "{args:?}");
        assert!(err.contains(why), "{args:?}: {err}");
    }
}

#[test]
fn a_dxf_drawing_is_read_into_a_sketch_that_can_be_drawn_again() {
    let dir = Scratch::new("sketch-import");
    let drawing = dir.file("plate.dxf");
    let mut text = String::from("0\nSECTION\n2\nENTITIES\n");
    for (a, b) in [
        ((0, 0), (100, 0)),
        ((100, 0), (100, 60)),
        ((100, 60), (0, 60)),
        ((0, 60), (0, 0)),
    ] {
        text.push_str(&format!(
            "0\nLINE\n8\n0\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
            a.0, a.1, b.0, b.1
        ));
    }
    text.push_str(
        "0\nCIRCLE\n8\n0\n10\n20\n20\n30\n40\n5\n0\nTEXT\n8\n0\n1\nnote\n0\nENDSEC\n0\nEOF\n",
    );
    fs::write(&drawing, text).unwrap();
    let sketch = dir.file("plate.phoenixsketch");
    let (code, out, err) = run(&["sketch-import", &drawing, "--out", &sketch]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("read: 4 lines, 1 circles, 0 arcs\n"),
        "{out}"
    );
    assert!(out.contains("left out: TEXT x 1\n"), "{out}");
    assert!(out.contains("plate.phoenixsketch"), "{out}");
    // the sketch is the one of the drawing: its outline is drawn as a DXF again
    let back = dir.file("back.dxf");
    let (code, _, err) = run(&["sketch-export", &sketch, "--format", "dxf", "--out", &back]);
    assert_eq!(code, 0, "{err}");
    assert!(fs::read_to_string(&back).unwrap().contains("CONTORNO"));
    // a file is never written over unless it is ordered to, and the words follow --lang
    let (code, _, err) = run(&["sketch-import", &drawing, "--out", &sketch]);
    assert_ne!(code, 0);
    assert!(err.contains("PHOENIX-CAD-040"), "{err}");
    let (code, out, err) = run(&[
        "sketch-import",
        &drawing,
        "--out",
        &sketch,
        "--overwrite",
        "--lang",
        "es",
        "--unit",
        "in",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("leído: 4 líneas, 1 círculos, 0 arcos\n"),
        "{out}"
    );
    // what is refused
    let other = dir.file("empty.dxf");
    fs::write(&other, "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n").unwrap();
    for (args, why) in [
        (vec!["sketch-import"], "needs a DXF drawing"),
        (vec!["sketch-import", &other], "PHOENIX-CAD-035"),
        (
            vec!["sketch-import", &drawing, "--unit", "yard"],
            "unknown unit",
        ),
        (
            vec!["sketch-import", &drawing, "--face", "middle"],
            "not a face",
        ),
    ] {
        let (code, _, err) = run(&args);
        assert_ne!(code, 0, "{args:?}");
        assert!(err.contains(why), "{args:?}: {err}");
    }
}

#[test]
fn the_layers_of_a_dxf_drawing_are_listed_and_chosen_when_it_is_read() {
    let dir = Scratch::new("sketch-import-layers");
    let drawing = dir.file("layers.dxf");
    let mut text = String::from("0\nSECTION\n2\nENTITIES\n");
    for (a, b) in [((0, 0), (10, 0)), ((10, 0), (10, 10)), ((10, 10), (0, 0))] {
        text.push_str(&format!(
            "0\nLINE\n8\nCUT\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
            a.0, a.1, b.0, b.1
        ));
    }
    text.push_str("0\nCIRCLE\n8\nHOLES\n10\n3\n20\n2\n40\n1\n0\nENDSEC\n0\nEOF\n");
    fs::write(&drawing, text).unwrap();
    let (code, out, err) = run(&["sketch-import", &drawing, "--list-layers"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim_end(), "layers: CUT (3), HOLES (1)");
    let (_, out, _) = run(&["sketch-import", &drawing, "--list-layers", "--lang", "es"]);
    assert_eq!(out.trim_end(), "capas: CUT (3), HOLES (1)");
    let sketch = dir.file("cut.phoenixsketch");
    let (code, out, err) = run(&[
        "sketch-import",
        &drawing,
        "--layers",
        "cut, other",
        "--out",
        &sketch,
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("read: 3 lines, 0 circles, 0 arcs\nleft out by layer: 1 entities\n"),
        "{out}"
    );
    // a layer the drawing does not have leaves nothing to read
    let (code, _, err) = run(&[
        "sketch-import",
        &drawing,
        "--layers",
        "missing",
        "--out",
        &sketch,
    ]);
    assert_ne!(code, 0);
    assert!(err.contains("PHOENIX-CAD-035"), "{err}");
    let (code, _, err) = run(&["sketch-import", &drawing, "--layers"]);
    assert_ne!(code, 0);
    assert!(err.contains("--layers needs"), "{err}");
}

#[test]
fn the_unit_of_a_dxf_drawing_can_be_read_from_its_header() {
    let dir = Scratch::new("sketch-import-auto");
    let body =
        "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n2\n21\n0\n0\nLINE\n8\n0\n10\n2\n20\n0\n11\n2\n21\n2\n\
0\nLINE\n8\n0\n10\n2\n20\n2\n11\n0\n21\n2\n0\nLINE\n8\n0\n10\n0\n20\n2\n11\n0\n21\n0\n";
    let with = |unit: &str| {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{unit}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{body}0\nENDSEC\n0\nEOF\n"
        )
    };
    let inches = dir.file("inches.dxf");
    fs::write(&inches, with("1")).unwrap();
    let unitless = dir.file("unitless.dxf");
    fs::write(&unitless, with("0")).unwrap();
    // a header that says inches makes the square of two units 50.8 mm across
    let sketch = dir.file("inches.phoenixsketch");
    let (code, out, err) = run(&["sketch-import", &inches, "--unit", "auto", "--out", &sketch]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("read: 4 lines, 0 circles, 0 arcs\n"),
        "{out}"
    );
    assert!(out.contains("unit read from the drawing: in\n"), "{out}");
    let svg = dir.file("inches.svg");
    let (code, _, err) = run(&["sketch-export", &sketch, "--format", "svg", "--out", &svg]);
    assert_eq!(code, 0, "{err}");
    // the sheet is the square plus its margin of 14 mm on each side: 50.8 + 28 = 78.8
    let drawn = fs::read_to_string(&svg).unwrap();
    assert!(drawn.contains("viewBox=\"0 0 78.8 78.8\""), "{drawn}");
    // the words follow --lang, and a header without a unit is read in millimetres and says so
    let plain = dir.file("unitless.phoenixsketch");
    let (code, out, err) = run(&[
        "sketch-import",
        &unitless,
        "--unit",
        "auto",
        "--lang",
        "es",
        "--out",
        &plain,
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("el dibujo no dice su unidad, así que se usa mm\n"),
        "{out}"
    );
    // without --unit nothing is said about it and the header is not read
    let quiet = dir.file("quiet.phoenixsketch");
    let (code, out, err) = run(&["sketch-import", &inches, "--out", &quiet]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("unit"), "{out}");
    // --unit still needs a value and refuses a word that is no unit
    let (code, _, err) = run(&["sketch-import", &inches, "--unit"]);
    assert_ne!(code, 0);
    assert!(err.contains("--unit needs"), "{err}");
}

#[test]
fn the_bill_can_be_added_up_by_part_name() {
    let (code, csv, err) = run(&["bom", FRAME, "--group", "name"]);
    assert_eq!(code, 0, "{err}");
    let rows: Vec<Vec<&str>> = csv.lines().map(|l| l.split(',').collect()).collect();
    assert_eq!(
        rows[0],
        [
            "name",
            "lines",
            "pieces",
            "volume_mm3",
            "mass_g",
            "cost",
            "currency"
        ]
    );
    let n = rows.len();
    assert_eq!(rows[n - 1][0], "total");
    assert_eq!(rows[n - 1][2], "18");
    let pieces: u32 = rows[1..n - 1]
        .iter()
        .map(|r| r[2].parse::<u32>().unwrap())
        .sum();
    assert_eq!(pieces, 18, "{csv}");
    // the names come in order and none is repeated
    let names: Vec<&str> = rows[1..n - 1].iter().map(|r| r[0]).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names, sorted);
    let (code, text, _) = run(&["bom", FRAME, "--group", "name", "--text", "--lang", "de"]);
    assert_eq!(code, 0);
    assert!(text.starts_with("Summen nach Teilename\n"), "{text}");
    let (code, _, err) = run(&["bom", FRAME, "--group", "size"]);
    assert_eq!(code, 1);
    assert!(err.contains("not material, kind or name"), "{err}");
}

/// The distance that `faces` printed, in millimetres.
fn faces_distance(from: &str, to: &str) -> f64 {
    let (code, out, err) = run(&["faces", BOX, "--from", from, "--to", to]);
    assert_eq!(code, 0, "{from} {to}: {err}");
    let line = out.lines().next().unwrap();
    line.strip_prefix("distance: ")
        .and_then(|rest| rest.strip_suffix(" mm"))
        .unwrap_or_else(|| panic!("{out}"))
        .parse()
        .unwrap()
}

#[test]
fn the_faces_of_the_box_are_measured_from_the_command_line() {
    // parallel faces: the sides of a wall add up to the outer size
    let outer = faces_distance("bottom", "top");
    let inner = faces_distance("bottom:inner", "top:inner");
    let floor = faces_distance("bottom", "bottom:inner");
    let lid = faces_distance("top", "top:inner");
    assert!(
        (outer - (inner + floor + lid)).abs() < 1e-9,
        "{outer} {inner} {floor} {lid}"
    );
    let width = faces_distance("left", "right");
    let inside = faces_distance("left:inner", "right:inner");
    let wall = faces_distance("left", "left:inner");
    assert!((width - (inside + 2.0 * wall)).abs() < 1e-9);
    // the order does not matter
    assert_eq!(faces_distance("top", "bottom"), outer);
    // faces that meet are at a right angle and no distance apart
    let (code, out, err) = run(&["faces", BOX, "--from", "front", "--to", "left:inner"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "distance: 0 mm\nangle: 90 degrees\n");
    let (code, out, _) = run(&["faces", BOX, "--from", "left", "--to", "right"]);
    assert_eq!(code, 0);
    assert!(out.ends_with("angle: 0 degrees\n"), "{out}");
}

#[test]
fn the_faces_command_refuses_what_it_cannot_measure() {
    for (args, why) in [
        (
            vec!["faces", BOX, "--from", "top", "--to", "top"],
            "PHOENIX-CAD-038",
        ),
        (
            vec!["faces", BOX, "--from", "top", "--to", "middle"],
            "not a face",
        ),
        (
            vec!["faces", BOX, "--from", "top:side", "--to", "left"],
            "inner or outer",
        ),
        (vec!["faces", BOX, "--from", "top"], "needs --to"),
        (vec!["faces", BOX, "--to", "top"], "needs --from"),
        (
            vec!["faces", "--from", "top", "--to", "left"],
            "needs a project file",
        ),
        (vec!["faces", BOX, "--from"], "--from needs a face"),
    ] {
        let (code, _, err) = run(&args);
        assert_ne!(code, 0, "{args:?}");
        assert!(err.contains(why), "{args:?}: {err}");
    }
}

/// A sketch file with a rectangle (e1 to e8, 40 x 25 mm), a line across it (e11, from -10 to 20
/// mm at 10 mm up) and a point (e12).
const EDIT_SKETCH: &str = "phoenix-sketch 1\nname plate\nplane top 0 0 0\nnext 13 1\n\
e1 point 0 0\ne2 point 40000000 0\ne3 point 40000000 25000000\ne4 point 0 25000000\n\
e5 line e1 e2\ne6 line e2 e3\ne7 line e3 e4\ne8 line e4 e1\n\
e9 point -10000000 10000000\ne10 point 20000000 10000000\ne11 line e9 e10\n\
e12 point 5000000 5000000\n";

/// Runs `sketch-edit` on a copy of the plate and returns the text of the edited sketch.
fn edited(dir: &Scratch, extra: &[&str]) -> String {
    let plate = dir.file("plate.phoenixsketch");
    fs::write(&plate, EDIT_SKETCH).unwrap();
    let out = dir.file("edited.phoenixsketch");
    let _ = fs::remove_file(&out);
    let mut args = vec!["sketch-edit", plate.as_str(), "--out", out.as_str()];
    args.extend_from_slice(extra);
    let (code, text, err) = run(&args);
    assert_eq!(code, 0, "{extra:?}: {err}");
    assert!(text.contains("edited.phoenixsketch"), "{text}");
    fs::read_to_string(&out).unwrap()
}

#[test]
fn a_sketch_file_is_edited_from_the_command_line_like_in_the_window() {
    let dir = Scratch::new("sketch-edit");
    // chamfer: both sides are shortened by 3 mm and a line joins the new ends
    let text = edited(
        &dir,
        &["--op", "chamfer", "--lines", "e5,e6", "--distance", "3"],
    );
    assert!(text.contains("e13 point 37000000 0\n"), "{text}");
    assert!(text.contains("e14 point 40000000 3000000\n"), "{text}");
    assert!(
        text.contains("e5 line e1 e13\n") && text.contains("e15 line e13 e14\n"),
        "{text}"
    );
    // a unit after the number
    let text = edited(
        &dir,
        &["--op", "chamfer", "--lines", "e5,e6", "--distance", "0.1in"],
    );
    assert!(text.contains("e13 point 37460000 0\n"), "{text}");
    // trim: the cutter is the left side, which cuts off the shorter side of e11
    let text = edited(&dir, &["--op", "trim", "--line", "e11", "--cutter", "e8"]);
    assert!(text.contains("e11 line e13 e10\n"), "{text}");
    assert!(text.contains("on point_on_object e13 e8\n"), "{text}");
    // the end to cut off can be named
    let text = edited(
        &dir,
        &[
            "--op", "trim", "--line", "e11", "--cutter", "e8", "--drop", "e10",
        ],
    );
    assert!(text.contains("e11 line e9 e13\n"), "{text}");
    // extend to the right side
    let text = edited(&dir, &["--op", "extend", "--line", "e11", "--to", "e6"]);
    assert!(text.contains("e13 point 40000000 10000000\n"), "{text}");
    assert!(text.contains("e11 line e9 e13\n"), "{text}");
    // linear pattern: two copies of the point
    let text = edited(
        &dir,
        &[
            "--op", "linear", "--items", "e12", "--step", "10,5", "--count", "3",
        ],
    );
    assert!(text.contains("e13 point 15000000 10000000\n"), "{text}");
    assert!(text.contains("e14 point 25000000 15000000\n"), "{text}");
    // circular pattern: a full turn shared between four positions
    let text = edited(
        &dir,
        &[
            "--op", "circular", "--items", "e12", "--center", "e1", "--count", "4",
        ],
    );
    for spot in ["-5000000 5000000", "-5000000 -5000000", "5000000 -5000000"] {
        assert!(text.contains(&format!("point {spot}\n")), "{spot}: {text}");
    }
    // and with the step given
    let text = edited(
        &dir,
        &[
            "--op", "circular", "--items", "e12", "--center", "e1", "--count", "2", "--step", "90",
        ],
    );
    assert!(text.contains("e13 point -5000000 5000000\n"), "{text}");
    // offset: outwards, and inwards with a negative distance
    let text = edited(
        &dir,
        &[
            "--op",
            "offset",
            "--items",
            "e5,e6,e7,e8",
            "--distance",
            "2",
        ],
    );
    assert!(text.contains("e13 point -2000000 -2000000\n"), "{text}");
    let text = edited(
        &dir,
        &[
            "--op",
            "offset",
            "--items",
            "e5,e6,e7,e8",
            "--distance",
            "-2",
        ],
    );
    assert!(text.contains("e13 point 2000000 2000000\n"), "{text}");
}

#[test]
fn a_refused_edit_writes_nothing_and_a_file_is_only_written_over_when_ordered() {
    let dir = Scratch::new("sketch-edit-refused");
    let plate = dir.file("plate.phoenixsketch");
    fs::write(&plate, EDIT_SKETCH).unwrap();
    let out = dir.file("edited.phoenixsketch");
    for (args, why) in [
        (
            vec!["--op", "chamfer", "--lines", "e5,e6", "--distance", "40"],
            "PHOENIX-CAD-031",
        ),
        (
            vec!["--op", "chamfer", "--lines", "e5,e7", "--distance", "1"],
            "PHOENIX-CAD-031",
        ),
        (
            vec!["--op", "chamfer", "--lines", "e5", "--distance", "1"],
            "names the two lines",
        ),
        (
            vec!["--op", "chamfer", "--lines", "e5,e6"],
            "needs --distance",
        ),
        (
            vec!["--op", "chamfer", "--lines", "5,e6", "--distance", "1"],
            "not an element",
        ),
        (
            vec!["--op", "chamfer", "--lines", "e5,e6", "--distance", "far"],
            "far",
        ),
        (
            vec!["--op", "trim", "--line", "e11", "--cutter", "e6"],
            "PHOENIX-CAD-031",
        ),
        (
            vec!["--op", "extend", "--line", "e5", "--to", "e6"],
            "PHOENIX-CAD-031",
        ),
        (
            vec![
                "--op", "linear", "--items", "e12", "--step", "10", "--count", "3",
            ],
            "DX,DY",
        ),
        (
            vec![
                "--op", "linear", "--items", "e12", "--step", "10,0", "--count", "1",
            ],
            "PHOENIX-CAD-031",
        ),
        (
            vec![
                "--op", "linear", "--items", "e12", "--step", "10,0", "--count", "x",
            ],
            "whole number",
        ),
        (
            vec![
                "--op", "circular", "--items", "e12", "--center", "e5", "--count", "3",
            ],
            "PHOENIX-CAD",
        ),
        (
            vec![
                "--op", "circular", "--items", "e12", "--center", "e1", "--count", "3", "--step",
                "a",
            ],
            "angle",
        ),
        (
            vec![
                "--op",
                "offset",
                "--items",
                "e5,e6,e7,e8",
                "--distance",
                "-20",
            ],
            "PHOENIX-CAD-031",
        ),
        (
            vec!["--op", "offset", "--items", "e5,e6,e7", "--distance", "1"],
            "PHOENIX-CAD-031",
        ),
        (vec!["--op", "spin"], "not an operation"),
        (vec![], "needs --op"),
        (vec!["--op", "offset", "--bogus", "1"], "unknown option"),
    ] {
        let mut all = vec!["sketch-edit", plate.as_str(), "--out", out.as_str()];
        all.extend_from_slice(&args);
        let (code, _, err) = run(&all);
        assert_ne!(code, 0, "{args:?}");
        assert!(err.contains(why), "{args:?}: {err}");
        assert!(!std::path::Path::new(&out).exists(), "{args:?}");
    }
    let (code, _, err) = run(&["sketch-edit"]);
    assert_ne!(code, 0);
    assert!(err.contains("needs a sketch file"), "{err}");
    // the file the output names exists: it is not written over without --overwrite
    fs::write(&out, "kept").unwrap();
    let ask = [
        "sketch-edit",
        plate.as_str(),
        "--op",
        "offset",
        "--items",
        "e5,e6,e7,e8",
        "--distance",
        "1",
        "--out",
        out.as_str(),
    ];
    let (code, _, err) = run(&ask);
    assert_ne!(code, 0);
    assert!(err.contains("PHOENIX-CAD-040"), "{err}");
    assert_eq!(fs::read_to_string(&out).unwrap(), "kept");
    let mut over = ask.to_vec();
    over.push("--overwrite");
    let (code, _, err) = run(&over);
    assert_eq!(code, 0, "{err}");
    assert!(fs::read_to_string(&out)
        .unwrap()
        .starts_with("phoenix-sketch 1\n"));
    // without --out the sketch is edited in place, and only with --overwrite
    let (code, _, err) = run(&[
        "sketch-edit",
        &plate,
        "--op",
        "offset",
        "--items",
        "e5,e6,e7,e8",
        "--distance",
        "1",
    ]);
    assert_ne!(code, 0);
    assert!(err.contains("PHOENIX-CAD-040"), "{err}");
    assert_eq!(fs::read_to_string(&plate).unwrap(), EDIT_SKETCH);
    let (code, _, err) = run(&[
        "sketch-edit",
        &plate,
        "--op",
        "offset",
        "--items",
        "e5,e6,e7,e8",
        "--distance",
        "1",
        "--overwrite",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_ne!(fs::read_to_string(&plate).unwrap(), EDIT_SKETCH);
}

#[test]
fn an_edited_sketch_is_a_sketch_that_can_be_drawn_again() {
    let dir = Scratch::new("sketch-edit-export");
    let edited_file = dir.file("edited.phoenixsketch");
    let plate = dir.file("plate.phoenixsketch");
    // the rectangle alone, so that its outline is closed
    let rectangle: String = EDIT_SKETCH
        .lines()
        .take(12)
        .map(|l| {
            format!(
                "{l}
"
            )
        })
        .collect();
    fs::write(&plate, rectangle.replace("next 13 1", "next 9 1")).unwrap();
    let (code, _, err) = run(&[
        "sketch-edit",
        &plate,
        "--op",
        "offset",
        "--items",
        "e5,e6,e7,e8",
        "--distance",
        "3",
        "--out",
        &edited_file,
    ]);
    assert_eq!(code, 0, "{err}");
    let drawing = dir.file("edited.svg");
    let (code, _, err) = run(&[
        "sketch-export",
        &edited_file,
        "--format",
        "svg",
        "--out",
        &drawing,
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(fs::read_to_string(&drawing).unwrap().contains("<svg"));
}

#[test]
fn the_bill_by_assembly_has_a_bill_for_each_set_of_parts_that_mates_join() {
    let (code, csv, err) = run(&["bom", ASSEMBLY, "--by-assembly"]);
    assert_eq!(code, 0, "{err}");
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("assembly,item,name,kind,"), "{csv}");
    // the mates of the example join its parts into one assembly: its rows come first, with its totals
    assert!(lines.iter().any(|l| l.contains(",subtotal,")), "{csv}");
    let width = lines[0].split(',').count();
    assert!(lines.iter().all(|l| l.split(',').count() == width), "{csv}");
    // the text is in the language asked for
    let (code, text, _) = run(&["bom", ASSEMBLY, "--by-assembly", "--text", "--lang", "es"]);
    assert_eq!(code, 0);
    assert!(text.starts_with("Conjunto "), "{text}");
    // the two ways of grouping do not go together
    let (code, _, err) = run(&["bom", ASSEMBLY, "--by-assembly", "--group", "name"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("cannot be given together"), "{err}");
}
