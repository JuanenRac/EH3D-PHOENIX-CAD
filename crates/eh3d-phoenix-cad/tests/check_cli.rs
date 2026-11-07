//! The commands of the program that need no window: `check` reads a project and says what the
//! model makes of it, `config` writes the settings with their defaults.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");
const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixproj");

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
