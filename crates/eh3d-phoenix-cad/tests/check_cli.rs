//! The commands of the program that need no window: `check` reads a project and says what the
//! model makes of it, `config` writes the settings with their defaults.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");
const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixproj");
const FRAME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/frame.phoenixproj");
const LIBRARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/chassis.phoenixlib");
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

#[test]
fn the_example_is_checked_step_by_step() {
    let (code, text, _) = run(&["check", EXAMPLE, "--lang", "en"]);
    assert_eq!(code, 0, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "Outside 104 x 64 x 35 mm - inside 100 x 60 x 30 mm - material 53.0 cm3"
    );
    assert_eq!(lines[1], "It fits, with 39.6 / 19.6 / 23 mm to spare");
    assert_eq!(lines.len(), 2 + 8, "{text}");
    assert!(lines[2].ends_with("usb port  valid"), "{}", lines[2]);
    assert!(lines[9].ends_with("lid  valid"), "{}", lines[9]);
}

#[test]
fn the_check_speaks_the_language_asked_for() {
    let (code, text, _) = run(&["check", EXAMPLE, "--lang", "es"]);
    assert_eq!(code, 0);
    assert!(text.contains("Exterior 104 x 64 x 35 mm"), "{text}");
    assert!(
        text.contains("Cabe, con 39.6 / 19.6 / 23 mm de sobra"),
        "{text}"
    );
}

#[test]
fn a_settings_file_changes_what_is_checked() {
    // a clearance of 25 mm all round leaves 100 - 60 - 50 < 0 along the length
    let dir = std::env::temp_dir().join(format!("phoenix-cad-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("tight.cfg");
    std::fs::write(&config, "[printer]\nfit_clearance = 25 mm\n").unwrap();
    let (code, text, _) = run(&[
        "check",
        EXAMPLE,
        "--lang",
        "en",
        "--config",
        config.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("It does not fit"), "{text}");
}

#[test]
fn a_missing_project_and_wrong_arguments_are_errors() {
    let (code, _, error) = run(&["check", "no/such/project.phoenixproj"]);
    assert_eq!(code, 1);
    assert!(error.contains("PHOENIX-CAD-012"), "{error}");
    let (code, _, error) = run(&["check"]);
    assert_eq!(code, 1);
    assert!(error.contains("needs a project file"), "{error}");
    let (code, _, error) = run(&["check", EXAMPLE, "--lang", "xx"]);
    assert_eq!(code, 1);
    assert!(error.contains("unknown language code"), "{error}");
    let (code, _, error) = run(&["frobnicate"]);
    assert_eq!(code, 1);
    assert!(error.contains("unknown command"), "{error}");
}

#[test]
fn config_writes_both_files_of_settings_and_they_read_back() {
    let (code, text, _) = run(&["config"]);
    assert_eq!(code, 0);
    assert!(text.contains("[screw.M3]"));
    assert!(text.contains("[view]"));
    let library = eh3d_phoenix_cad::config::Settings::with_user_config(&text).unwrap();
    assert_eq!(library, eh3d_phoenix_cad::config::Settings::defaults());
    let window = eh3d_phoenix_cad::uiconfig::UiSettings::with_user_config(&text).unwrap();
    assert_eq!(window, eh3d_phoenix_cad::uiconfig::UiSettings::defaults());
}

#[test]
fn config_also_writes_the_settings_of_the_sketch_and_they_read_back() {
    let (code, text, _) = run(&["config"]);
    assert_eq!(code, 0);
    assert!(text.contains("[sketch]"));
    assert!(text.contains("[sketch_ui]"));
    let solver =
        eh3d_phoenix_cad::sketch::settings::SketchSettings::with_user_config(&text).unwrap();
    assert_eq!(
        solver,
        eh3d_phoenix_cad::sketch::settings::SketchSettings::defaults()
    );
    let window = eh3d_phoenix_cad::sketch_ui::SketchUiSettings::with_user_config(&text).unwrap();
    assert_eq!(
        window,
        eh3d_phoenix_cad::sketch_ui::SketchUiSettings::defaults()
    );
    // the three files are one: the library and the window read it too
    assert!(eh3d_phoenix_cad::config::Settings::with_user_config(&text).is_ok());
    assert!(eh3d_phoenix_cad::uiconfig::UiSettings::with_user_config(&text).is_ok());
}

const PLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.phoenixproj");

#[test]
fn a_project_with_a_sketch_and_an_extrusion_is_checked_with_them() {
    let (code, text, _) = run(&["check", PLATE, "--lang", "en"]);
    assert_eq!(code, 0, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    // the box of the example (52.96 cm3 of material) and a plate of 100 x 60 mm with two holes of
    // diameter 5 mm raised 5 mm: 5 * (6000 - 2 * pi * 2.5^2) = 29 803.7 mm3
    assert_eq!(
        lines[0],
        "Outside 104 x 64 x 35 mm - inside 100 x 60 x 30 mm - material 82.8 cm3"
    );
    assert_eq!(lines.len(), 2 + 9, "{text}");
    assert!(lines[10].ends_with("Extrusion 1  valid"), "{}", lines[10]);
}

#[test]
fn the_settings_of_the_sketch_in_a_settings_file_apply_to_the_profiles_of_the_check() {
    // an outline must have at least 10 000 mm2 and the plate has 5 960
    let dir = std::env::temp_dir().join(format!("phoenix-cad-plate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("big.cfg");
    std::fs::write(&config, "[sketch]\nprofile_min_area = 10000\n").unwrap();
    let (code, text, _) = run(&[
        "check",
        PLATE,
        "--lang",
        "en",
        "--config",
        config.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("PHOENIX-CAD-036"), "{text}");
    assert!(text.contains("less area"), "{text}");
}

#[test]
fn a_project_written_by_the_window_is_read_by_the_check() {
    use eh3d_phoenix_cad::config::Settings;
    use eh3d_phoenix_cad::lang::Lang;
    use eh3d_phoenix_cad::uiconfig::UiSettings;
    use eh3d_phoenix_cad::window::{NewStep, Window};
    let dir = std::env::temp_dir().join(format!("phoenix-cad-written-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pocket.phoenixproj");
    let mut w = Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
    assert!(w.import_sketch_file(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/box.phoenixsketch"
    ))));
    w.add_step(NewStep::Pocket);
    // a plate of 100 x 60 mm does not keep the margin of the edges of the lid: it is refused
    assert!(w.status.is_problem(), "{:?}", w.status);
    assert!(w.save_to(&path));
    let (code, text, _) = run(&["check", path.to_str().unwrap(), "--lang", "en"]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
}

#[test]
fn the_bill_of_the_example_frame_is_printed_as_comma_separated_values() {
    let (code, text, _) = run(&["bill", FRAME]);
    assert_eq!(code, 0, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 8, "{text}");
    assert_eq!(
        lines[0],
        "kind,part,material,count,cut_mm,volume_mm3,mass_g"
    );
    assert_eq!(
        lines[1],
        "profile,tube-20x20,aluminium,4,150,66600.000,179.820"
    );
    assert_eq!(lines[7], "total,,,18,,251025.901,684.085");
    // the same text twice
    assert_eq!(run(&["bill", FRAME]).1, text);
}

#[test]
fn the_bill_of_a_project_without_parts_has_only_its_totals() {
    let (code, text, _) = run(&["bill", EXAMPLE]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "kind,part,material,count,cut_mm,volume_mm3,mass_g\ntotal,,,0,,0.000,0.000\n"
    );
}

#[test]
fn the_check_of_the_frame_lists_the_parts_and_the_cut_list() {
    let (code, text, _) = run(&["check", FRAME, "--lang", "en"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("Parts: 18 pieces, 251025.901 mm3 of material, 684.085 g"),
        "{text}"
    );
    assert!(
        text.contains("Cut list tube-40x20: 2 x 300 mm, 600 mm in all"),
        "{text}"
    );
    let (_, spanish, _) = run(&["check", FRAME, "--lang", "es"]);
    assert!(
        spanish.contains("Lista de corte tube-40x20: 2 x 300 mm, 600 mm en total"),
        "{spanish}"
    );
}

#[test]
fn a_library_is_read_checked_and_listed() {
    let (code, text, _) = run(&["library", LIBRARY]);
    assert_eq!(code, 0, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 9, "{text}");
    assert_eq!(lines[0], "library chassis-basic: 8 parts");
    assert_eq!(lines[1], "  profile tube-20x20 aluminium");
    assert_eq!(lines[4], "  screw bolt-m3x10 steel");
    assert_eq!(lines[8], "  bracket angle-20 aluminium");
}

#[test]
fn a_library_that_is_not_valid_is_an_error_with_its_code() {
    let (code, _, error) = run(&["library", "no/such/file.phoenixlib"]);
    assert_eq!(code, 1);
    assert!(error.contains("PHOENIX-CAD-012"), "{error}");
    // a project is not a library
    let (code, _, error) = run(&["library", EXAMPLE]);
    assert_eq!(code, 1);
    assert!(error.contains("PHOENIX-CAD-044"), "{error}");
    // the limits of the settings apply
    let dir = std::env::temp_dir().join(format!("phoenix-cad-library-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("small.cfg");
    std::fs::write(&config, "[catalog]\nmax_parts = 3\n").unwrap();
    let (code, _, error) = run(&["library", LIBRARY, "--config", config.to_str().unwrap()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 1);
    assert!(
        error.contains("PHOENIX-CAD-044") && error.contains("more than the 3 parts"),
        "{error}"
    );
    let (code, _, error) = run(&["library"]);
    assert_eq!(code, 1);
    assert!(error.contains("needs a library file"), "{error}");
    let (code, _, error) = run(&["bill"]);
    assert_eq!(code, 1);
    assert!(error.contains("needs a project file"), "{error}");
    let (code, _, error) = run(&["bill", FRAME, "--lang", "en"]);
    assert_eq!(code, 1);
    assert!(error.contains("unknown option"), "{error}");
}

#[test]
fn the_config_command_writes_the_settings_of_the_catalog() {
    let (code, text, _) = run(&["config"]);
    assert_eq!(code, 0);
    assert!(text.contains("[catalog]"), "{text}");
    assert!(text.contains("refuse_profile_overlap = 1"), "{text}");
    assert!(text.contains("part_axis = +z"), "{text}");
    assert!(text.contains("[material.steel]"), "{text}");
}

#[test]
fn the_interference_of_the_assembly_example_is_none_and_the_bolt_is_a_pass() {
    let (code, text, _) = run(&["interference", ASSEMBLY, "--lang", "en"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "Mates: 11\nInterference: none (1 pairs with overlapping boxes looked at, 1 screws through holes)\n"
    );
    // the check of the project says the same in the last lines of its report
    let (code, report, _) = run(&["check", ASSEMBLY, "--lang", "en"]);
    assert_eq!(code, 0, "{report}");
    assert!(report.contains("Mates: 11"), "{report}");
    assert!(
        report.contains("Parts: 6 pieces, 171166.535 mm3 of material, 462.881 g"),
        "{report}"
    );
    // the frame has its bolts through the holes of its brackets
    let (code, text, _) = run(&["interference", FRAME, "--lang", "en"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "Interference: none (4 pairs with overlapping boxes looked at, 4 screws through holes)\n"
    );
    // a project without parts has nothing to look at
    let (code, text, _) = run(&["interference", EXAMPLE, "--lang", "en"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "Interference: none (0 pairs with overlapping boxes looked at, 0 screws through holes)\n"
    );
}

#[test]
fn the_interference_lists_the_pairs_in_conflict_and_exits_with_one() {
    // a hole must be 2 mm wider than the shank: the 4.5 mm hole no longer takes the 3 mm bolt
    let dir = std::env::temp_dir().join(format!("phoenix-cad-interference-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("wide.cfg");
    std::fs::write(&config, "[assembly]\nhole_clearance = 2 mm\n").unwrap();
    let config = config.to_str().unwrap();
    let (code, text, _) = run(&["interference", ASSEMBLY, "--lang", "en", "--config", config]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "Mates: 11\nInterference: 1 pairs share volume (1 pairs with overlapping boxes looked at, 0 screws through holes)\n  shelf bracket / shelf bolt: up to 181.500 mm3 shared by the bounding boxes\n"
    );
    let (code, spanish, _) = run(&["interference", ASSEMBLY, "--lang", "es", "--config", config]);
    assert_eq!(code, 1);
    assert!(
        spanish.contains("Interferencias: 1 pares comparten volumen"),
        "{spanish}"
    );
    assert!(
        spanish.contains("hasta 181.500 mm3 compartidos por las cajas envolventes"),
        "{spanish}"
    );
    // only the pairs the settings allow are listed, the rest are counted
    std::fs::write(
        dir.join("none.cfg"),
        "[assembly]\nhole_clearance = 2 mm\nmax_report_pairs = 1\n",
    )
    .unwrap();
    let (code, text, _) = run(&[
        "interference",
        FRAME,
        "--lang",
        "en",
        "--config",
        dir.join("none.cfg").to_str().unwrap(),
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("Interference: 4 pairs share volume (4 pairs with overlapping boxes looked at, 0 screws through holes)"),
        "{text}"
    );
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.ends_with("  and 3 more\n"), "{text}");
}

#[test]
fn the_interference_asks_for_a_project_and_refuses_a_file_that_is_not_one() {
    let (code, _, error) = run(&["interference"]);
    assert_eq!(code, 1);
    assert!(
        error.contains("interference needs a project file"),
        "{error}"
    );
    let (code, _, error) = run(&["interference", ASSEMBLY, EXAMPLE]);
    assert_eq!(code, 1);
    assert!(error.contains("only one project"), "{error}");
    let (code, _, error) = run(&["interference", "no/such/file.phoenixproj"]);
    assert_eq!(code, 1);
    assert!(error.contains("PHOENIX-CAD-012"), "{error}");
    let (code, _, error) = run(&["interference", ASSEMBLY, "--colour"]);
    assert_eq!(code, 1);
    assert!(error.contains("unknown option --colour"), "{error}");
    // the help lists the command
    let (code, help, _) = run(&["help"]);
    assert_eq!(code, 0);
    assert!(
        help.contains("eh3d-phoenix-cad interference PROJECT.phoenixproj"),
        "{help}"
    );
}
