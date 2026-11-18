// EH3D-PHOENIX-CAD - command line and window.
// Copyright (C) 2026 JuanenRac (Electro Hobby 3D). GPL-3.0-or-later.
#![forbid(unsafe_code)]

use eh3d_phoenix_cad::catalog;
use eh3d_phoenix_cad::config::{Settings, DEFAULT_CONFIG};
use eh3d_phoenix_cad::drawing::View;
use eh3d_phoenix_cad::export::Format;
use eh3d_phoenix_cad::lang::{Lang, Msg};
use eh3d_phoenix_cad::shot;
use eh3d_phoenix_cad::sketch::settings::SketchSettings;
#[cfg(feature = "gui")]
use eh3d_phoenix_cad::sketch_ui::SketchUiSettings;
use eh3d_phoenix_cad::uiconfig::{default_file, UiSettings};
use eh3d_phoenix_cad::window::{error_text, Window};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "eh3d-phoenix-cad: parametric enclosures: the model tree, the properties, a 3D view and a plan, and the fit check of a board

  eh3d-phoenix-cad [gui] [PROJECT.phoenixproj | SKETCH.phoenixsketch] [--view model|sketch] [--lang CODE] [--config FILE]   the window (a build with --features gui): with no arguments it opens with a new project; the sketches of a project are kept in the project file, and a .phoenixsketch file is opened to edit it and put in the project, or written from the sketch as a copy
  eh3d-phoenix-cad gui [PROJECT | SKETCH] [--view model|sketch] --shot OUT.png [--size WxH] [--theme forge|dark|light] [--wait N] [--lang CODE] [--config FILE] [--replace]   a picture of the window (a build with --features gui)
  eh3d-phoenix-cad check PROJECT.phoenixproj [--lang CODE] [--config FILE]       reads a project, builds the box (with the extrusions and pockets of its sketches) and prints its measures, the fit of the board and the state of every step
  eh3d-phoenix-cad interference PROJECT.phoenixproj [--lang CODE] [--config FILE]  reads a project, puts the parts where their mates place them and lists the pairs of parts that share volume (touching is not sharing; a screw through the hole of a bracket is allowed)
  eh3d-phoenix-cad bill PROJECT.phoenixproj [--config FILE]                       the bill of materials of the parts placed in a project (profiles cut to a length, screws and brackets) as comma separated values, with the volume and the mass for the density of each material
  eh3d-phoenix-cad cutplan PROJECT.phoenixproj [--text] [--lang CODE] [--config FILE]            the cutting plan of the profiles placed in a project: the pieces of the cut list fitted into bars (the longest cut each profile comes in) with the kerf of the saw and the trimmed ends, as comma separated values or, with --text, as lines for a person
  eh3d-phoenix-cad clearance PROJECT.phoenixproj [--text] [--lang CODE] [--config FILE]                  lists, as comma separated values, the pairs of placed parts that do not touch and are closer than clearance.min_gap (exit 1 when there is one)
  eh3d-phoenix-cad placements PROJECT.phoenixproj [--explode PERCENT] [--text] [--lang CODE] [--config FILE]   where each placed part is (the corner and the size in millimetres and the turn) and the size of the whole assembly, as comma separated values; with --explode the parts are first pulled apart from the middle of the assembly by that percentage
  eh3d-phoenix-cad partlist PROJECT.phoenixproj [--text] [--lang CODE] [--config FILE]           the placed parts with their properties (name, kind, part, material, density, volume, mass, corner and size of the box) and the totals, as comma separated values; --text prints lines for a person in the language of --lang (any of the eight)
  eh3d-phoenix-cad holes PROJECT.phoenixproj [--tools] [--text] [--lang CODE] [--config FILE]            the schedule of the holes of the box for the person who drills (a tag, the face, the position, the diameter and the shape of each) as comma separated values, or with --tools the drills and cutters they need grouped by diameter and shape
  eh3d-phoenix-cad sketch-export PROJECT.phoenixproj|SKETCH.phoenixsketch --format dxf|svg [--sketch NAME] [--out FILE] [--overwrite] [--lang CODE] [--config FILE]   writes a sketch (the only one of the project, or the one named) as a flat drawing of its closed outlines for a laser or a router; never over a file that exists unless --overwrite is given
  eh3d-phoenix-cad balance PROJECT.phoenixproj [--lang CODE] [--config FILE]                     the mass of the placed parts and where their centre of mass is (exact for profiles, an estimate for screws and brackets, and the number of estimated parts is said)
  eh3d-phoenix-cad revolve PROJECT.phoenixproj|SKETCH.phoenixsketch --axis x|y [--angle DEGREES] [--sketch NAME] [--material NAME] [--out FILE.stl|.obj|.3mf [--overwrite]] [--lang CODE] [--config FILE]   turns a sketch (the only one of the project, or the one named) about its first or second axis, by a full turn unless --angle says less, and prints the volume, the surface and the distances from the axis of the solid made and, with a material of the settings, its mass; the profile must not cross the axis; with --out the triangles of the solid are written to a file of the format its extension says (never over a file that exists unless --overwrite is given)
  eh3d-phoenix-cad sweep PROFILE --path PATH [--profile-sketch NAME] [--path-sketch NAME] [--material NAME] [--out FILE.stl|.obj|.3mf [--overwrite]] [--lang CODE] [--config FILE]   sweeps the profile (a project or a sketch file, and the sketch of it) along the path (the lines and arcs of another sketch, or one circle) and prints the length of the path, the volume and the surface of the solid made and, with a material of the settings, its mass; the profile starts on the path, its first axis to the left of the way the path is walked from the free end of its first element; with --out the triangles of the solid are written as for revolve
  eh3d-phoenix-cad library FILE.phoenixlib [--config FILE]                        reads a library of parts, checks it and lists its parts
  eh3d-phoenix-cad export PROJECT.phoenixproj --format stl|obj|3mf|dxf|svg|step|script|report|bill [--view top|front|side] [--out FILE] [--overwrite] [--ascii|--binary] [--lang CODE] [--config FILE]   writes the model to a file: the solid as triangles (stl), as a Wavefront file (obj) or as a 3MF package (3mf), the plan, front or side as a drawing (dxf, svg), the STEP file, the OpenSCAD-style script, the fit report or the bill of materials
  eh3d-phoenix-cad config                                                         every setting of the library, of the window and of the sketch with its default value, as a file for --config

--open FILE is the same as giving the file by itself; a sketch file opens the window on the sketch. --lang takes en, es, de, fr, it, ja, zh or pt; without it the language of the system is used (the reports cutplan, clearance, placements, holes and balance and the answer of sketch-export are in English unless --lang is given; with --text the first four are lines for a person in that language instead of comma separated values).
--shot writes a PNG file of the window (the exit code is 0 when it was written, 1 when it was not and 2 when the arguments are wrong); the window keeps no settings in that mode.
--config FILE reads a settings file over the defaults: it only needs the keys it changes (see the output of config).
check exits with 0 when every step was applied and the board fits, and with 1 when a step was refused or the board does not fit.
interference exits with 0 when every step was applied and no two parts share volume, and with 1 when one does.
export never writes over a file that exists unless --overwrite is given; --out defaults to the name of the project with the extension of the format, in the current folder; --view is for dxf and svg (the plan, top, is the default); --ascii or --binary overrides the setting mesh.stl_binary for stl. It exits with 0 when the file was written and with 1 when it was not (the code of the reason is printed).";

/// The settings of the library and of the window, with the keys of a user file put over the defaults.
fn load_settings(config: Option<&Path>) -> Result<(Settings, UiSettings), String> {
    let Some(path) = config else {
        return Ok((Settings::defaults(), UiSettings::defaults()));
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let settings =
        Settings::with_user_config(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let ui = UiSettings::with_user_config(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((settings, ui))
}

/// The settings of the sketch (the solver, the profiles and the file), with the keys of a user
/// file put over the defaults.
fn load_solver_settings(config: Option<&Path>) -> Result<SketchSettings, String> {
    let Some(path) = config else {
        return Ok(SketchSettings::defaults());
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    SketchSettings::with_user_config(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The settings of the sketch and of its window, with the keys of a user file put over the
/// defaults.
#[cfg(feature = "gui")]
fn load_sketch_settings(
    config: Option<&Path>,
) -> Result<(SketchSettings, SketchUiSettings), String> {
    let solver = load_solver_settings(config)?;
    let Some(path) = config else {
        return Ok((solver, SketchUiSettings::defaults()));
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ui = SketchUiSettings::with_user_config(&text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((solver, ui))
}

/// Opens the project that `check` and `interference` are given, with the language and the settings
/// file of their options.
fn open_project(args: &[String], command: &str) -> Result<(Window, Lang), String> {
    let mut lang = Lang::from_env();
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            "--config" => {
                let path = iter.next().ok_or("--config needs a settings file")?;
                config = Some(PathBuf::from(path));
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project can be checked".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or_else(|| format!("{command} needs a project file"))?;
    let (settings, ui) = load_settings(config.as_deref())?;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    win.sketch_settings = load_solver_settings(config.as_deref())?;
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    if win.status.is_problem() {
        eprintln!("note: {}", win.status.text(lang));
    }
    Ok((win, lang))
}

/// `check`: reads a project and says what the model makes of it. The answer is whether every step
/// was applied and the board fits.
fn run_check(args: &[String]) -> Result<bool, String> {
    let (win, _) = open_project(args, "check")?;
    println!("{}", win.report_text());
    Ok(win.info.failed == 0 && win.info.fit.fits)
}

/// `interference`: reads a project, puts its parts where their mates place them and says which
/// pairs of parts share volume. The answer is whether every step was applied and none does.
fn run_interference(args: &[String]) -> Result<bool, String> {
    let (win, _) = open_project(args, "interference")?;
    println!("{}", win.assembly_text());
    Ok(win.info.failed == 0 && win.assembly_clear())
}

/// The project and the settings file of the commands `bill` and `library`.
fn file_and_config(args: &[String], what: &str) -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                let path = iter.next().ok_or("--config needs a settings file")?;
                config = Some(PathBuf::from(path));
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err(format!("only one {what} can be given"));
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or_else(|| format!("this command needs a {what}"))?;
    Ok((file, config))
}

/// Takes `--lang CODE` out of the arguments of a command that prints a report. English without
/// it, so that a report is the same text on every computer.
fn take_lang(args: &[String]) -> Result<(Lang, Vec<String>), String> {
    let mut lang = Lang::En;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--lang" {
            let code = iter.next().ok_or("--lang needs a language code")?;
            lang = Lang::from_code(code).ok_or("unknown language code")?;
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((lang, rest))
}

/// Opens the project that a report is asked for, with the settings file and the language.
fn open_for_report(file: &Path, config: Option<&Path>, lang: Lang) -> Result<Window, String> {
    let (settings, ui) = load_settings(config)?;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    if !win.open(file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    Ok(win)
}

/// `bill`: reads a project and prints the bill of materials of the parts placed in it.
fn run_bill(args: &[String]) -> Result<bool, String> {
    let (file, config) = file_and_config(args, "project file")?;
    let (settings, ui) = load_settings(config.as_deref())?;
    let lang = Lang::En;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    let csv = win.bill_csv_text().map_err(|e| error_text(&e, lang))?;
    print!("{csv}");
    Ok(win.info.failed == 0)
}

/// `cutplan`: reads a project and prints the cutting plan of the profiles placed in it.
fn run_cutplan(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--text").cloned().collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.stock_text()
    } else {
        win.stock_csv_text()
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    Ok(win.info.failed == 0)
}

/// `clearance`: reads a project and prints the pairs of placed parts that do not touch and are
/// closer than the minimum of the settings. The answer is whether there are none.
fn run_clearance(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--text").cloned().collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.clearance_text()
    } else {
        win.clearance_csv_text()
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    let none = win
        .clearance_report()
        .map_err(|e| error_text(&e, lang))?
        .pairs
        .is_empty();
    Ok(win.info.failed == 0 && none)
}

/// `placements`: reads a project and prints where each placed part is and the size of the
/// assembly, optionally with the parts pulled apart (`--explode PERCENT`).
fn run_placements(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let mut explode: Option<u32> = None;
    let mut text = false;
    let mut rest: Vec<String> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--text" {
            text = true;
        } else if arg == "--explode" {
            let value = iter.next().ok_or("--explode needs a percentage")?;
            explode = Some(
                value
                    .parse::<u32>()
                    .map_err(|_| "--explode needs a whole percentage".to_owned())?,
            );
        } else {
            rest.push(arg.clone());
        }
    }
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.placement_text(explode)
    } else {
        win.placement_csv_text(explode)
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    Ok(win.info.failed == 0)
}

/// `partlist`: reads a project and prints the placed parts with their properties and the totals.
fn run_partlist(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--text").cloned().collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.part_list_text()
    } else {
        win.part_list_csv()
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    Ok(win.info.failed == 0)
}

/// `holes`: reads a project and prints the schedule of the holes of the box, or with `--tools`
/// the list of drills and cutters they need.
fn run_holes(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let tools = args.iter().any(|a| a == "--tools");
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args
        .iter()
        .filter(|a| *a != "--tools" && *a != "--text")
        .cloned()
        .collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    if text {
        print!("{}", win.holes_text(tools));
    } else {
        print!("{}", win.holes_csv_text(tools));
    }
    Ok(win.info.failed == 0)
}

/// The sketch that `sketch-export` and `revolve` are given: a sketch file, or the sketch of a
/// project (the only one it has, or the one named), with the project when it is one and the
/// settings.
type ReadSketch = (
    eh3d_phoenix_cad::sketch::Sketch,
    Option<PathBuf>,
    Settings,
    UiSettings,
    SketchSettings,
);

fn read_sketch(
    file: &Path,
    name: Option<&str>,
    config: Option<&Path>,
    lang: Lang,
) -> Result<ReadSketch, String> {
    let (settings, ui) = load_settings(config)?;
    let solver = load_solver_settings(config)?;
    let (sketch, project) = if shot::is_sketch_file(file) {
        let sketch = eh3d_phoenix_cad::sketch::file::read_file(file, &solver)
            .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
        (sketch, None)
    } else {
        let mut win =
            Window::new(lang, settings.clone(), ui.clone()).map_err(|e| error_text(&e, lang))?;
        win.sketch_settings = solver.clone();
        if !win.open(file) {
            return Err(format!("{}: {}", file.display(), win.status.text(lang)));
        }
        let sketch = win.find_sketch(name).map_err(|e| error_text(&e, lang))?;
        (sketch, Some(file.to_path_buf()))
    };
    Ok((sketch, project, settings, ui, solver))
}

/// Writes the triangles of a solid to `out` as the file its extension says (`.stl` binary or text
/// as the setting `mesh.binary` says, `.obj` or `.3mf`) and tells how many bytes it has.
fn write_solid(
    mesh: &eh3d_phoenix_cad::mesh::Mesh,
    name: &str,
    out: &Path,
    overwrite: bool,
    settings: &Settings,
    ui: &UiSettings,
    lang: Lang,
) -> Result<(), String> {
    let extension = out
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let bodies = [(name, mesh)];
    let bytes: Vec<u8> = match extension.as_str() {
        "stl" if settings.mesh.binary => eh3d_phoenix_cad::mesh::stl_binary(&bodies),
        "stl" => eh3d_phoenix_cad::mesh::stl_ascii(&bodies).into_bytes(),
        "obj" => eh3d_phoenix_cad::interchange::obj(&bodies).into_bytes(),
        "3mf" => eh3d_phoenix_cad::interchange::three_mf(&bodies),
        _ => return Err("--out needs a file that ends in .stl, .obj or .3mf".to_owned()),
    };
    let written =
        eh3d_phoenix_cad::export::write_bytes(out, &bytes, overwrite, None, ui.export.max_kib)
            .map_err(|e| error_text(&e, lang))?;
    println!(
        "{}",
        eh3d_phoenix_cad::lang::fill(
            Msg::Exported.tr(lang),
            &[&out.display().to_string(), &written.to_string()]
        )
    );
    Ok(())
}

/// `revolve`: turns a sketch about one of its axes and prints the volume, the surface and, with a
/// material of the settings, the mass of the solid.
fn run_revolve(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut name: Option<String> = None;
    let mut axis: Option<String> = None;
    let mut turn: Option<String> = None;
    let mut material: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut overwrite = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--axis" => axis = Some(iter.next().ok_or("--axis needs x or y")?.clone()),
            "--angle" => turn = Some(iter.next().ok_or("--angle needs degrees")?.clone()),
            "--sketch" => name = Some(iter.next().ok_or("--sketch needs a name")?.clone()),
            "--material" => {
                material = Some(iter.next().ok_or("--material needs a name")?.clone());
            }
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project or sketch can be turned".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("revolve needs a project or a sketch file")?;
    let word = axis.as_deref().ok_or("revolve needs --axis")?;
    let axis = eh3d_phoenix_cad::revolve::Axis::from_name(word)
        .ok_or_else(|| format!("{word}: unknown axis (x or y)"))?;
    let angle = match turn.as_deref() {
        Some(text) => {
            eh3d_phoenix_cad::units::parse_angle(text).map_err(|e| error_text(&e, lang))?
        }
        None => eh3d_phoenix_cad::units::Angle::from_deg(360).map_err(|e| error_text(&e, lang))?,
    };
    let (sketch, _, settings, ui, solver) =
        read_sketch(&file, name.as_deref(), config.as_deref(), lang)?;
    let profile = eh3d_phoenix_cad::profile::Profile::from_sketch(
        &sketch,
        &eh3d_phoenix_cad::profile::ProfileLimits::from_settings(&solver),
    )
    .map_err(|e| error_text(&e, lang))?;
    let turned = eh3d_phoenix_cad::revolve::revolve(&profile, axis, angle)
        .map_err(|e| error_text(&e, lang))?;
    let density = match material.as_deref() {
        Some(word) => Some((
            word,
            *settings
                .materials
                .get(word)
                .ok_or_else(|| format!("{word}: not a material of the settings"))?,
        )),
        None => None,
    };
    print!(
        "{}",
        eh3d_phoenix_cad::revolve::revolve_text(&turned, density, lang)
            .map_err(|e| error_text(&e, lang))?
    );
    if let Some(target) = out {
        let solid = eh3d_phoenix_cad::revolve::mesh(
            &profile,
            axis,
            angle,
            settings.mesh.circle_segments,
            settings.mesh.max_triangles as usize,
        )
        .map_err(|e| error_text(&e, lang))?;
        write_solid(
            &solid,
            sketch.name(),
            &target,
            overwrite,
            &settings,
            &ui,
            lang,
        )?;
    }
    Ok(true)
}

/// `sweep`: sweeps the profile of a sketch along the path of another and prints the length of the
/// path, the volume, the surface and, with a material of the settings, the mass of the solid.
fn run_sweep(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut path_file: Option<PathBuf> = None;
    let mut profile_name: Option<String> = None;
    let mut path_name: Option<String> = None;
    let mut material: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut overwrite = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--path" => {
                path_file = Some(PathBuf::from(iter.next().ok_or("--path needs a file")?));
            }
            "--profile-sketch" => {
                profile_name = Some(iter.next().ok_or("--profile-sketch needs a name")?.clone());
            }
            "--path-sketch" => {
                path_name = Some(iter.next().ok_or("--path-sketch needs a name")?.clone());
            }
            "--material" => {
                material = Some(iter.next().ok_or("--material needs a name")?.clone());
            }
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one profile can be swept".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("sweep needs a project or a sketch file for the profile")?;
    let path_file = path_file.ok_or("sweep needs --path")?;
    let (sketch, _, settings, ui, solver) =
        read_sketch(&file, profile_name.as_deref(), config.as_deref(), lang)?;
    let (path_sketch, _, _, _, _) =
        read_sketch(&path_file, path_name.as_deref(), config.as_deref(), lang)?;
    let profile = eh3d_phoenix_cad::profile::Profile::from_sketch(
        &sketch,
        &eh3d_phoenix_cad::profile::ProfileLimits::from_settings(&solver),
    )
    .map_err(|e| error_text(&e, lang))?;
    let path = eh3d_phoenix_cad::sweep::path_of(&path_sketch).map_err(|e| error_text(&e, lang))?;
    let swept =
        eh3d_phoenix_cad::sweep::sweep(&profile, &path).map_err(|e| error_text(&e, lang))?;
    let density = match material.as_deref() {
        Some(word) => Some((
            word,
            *settings
                .materials
                .get(word)
                .ok_or_else(|| format!("{word}: not a material of the settings"))?,
        )),
        None => None,
    };
    print!(
        "{}",
        eh3d_phoenix_cad::sweep::sweep_text(&swept, density, lang)
            .map_err(|e| error_text(&e, lang))?
    );
    if let Some(target) = out {
        let solid = eh3d_phoenix_cad::sweep::mesh(
            &profile,
            &path,
            settings.mesh.circle_segments,
            settings.mesh.max_triangles as usize,
        )
        .map_err(|e| error_text(&e, lang))?;
        write_solid(
            &solid,
            sketch.name(),
            &target,
            overwrite,
            &settings,
            &ui,
            lang,
        )?;
    }
    Ok(true)
}

/// `sketch-export`: writes a sketch of a project, or a sketch file, as a DXF or SVG drawing. The
/// answer is whether the file was written. Nothing is written over a file that exists unless
/// `--overwrite` is given.
fn run_sketch_export(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut word: Option<String> = None;
    let mut name: Option<String> = None;
    let mut overwrite = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--format" => word = Some(iter.next().ok_or("--format needs dxf or svg")?.clone()),
            "--sketch" => name = Some(iter.next().ok_or("--sketch needs a name")?.clone()),
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project or sketch can be exported".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("sketch-export needs a project or a sketch file")?;
    let dxf = match word.as_deref().ok_or("sketch-export needs --format")? {
        "dxf" => true,
        "svg" => false,
        other => return Err(format!("{other}: unknown format (dxf or svg)")),
    };
    let (sketch, project, settings, ui, solver) =
        read_sketch(&file, name.as_deref(), config.as_deref(), lang)?;
    let text = eh3d_phoenix_cad::sketch_export::render_titled(
        &sketch,
        &solver,
        &settings.drawing,
        dxf,
        &settings.title_block,
        lang,
    )
    .map_err(|e| error_text(&e, lang))?;
    let target = out.unwrap_or_else(|| {
        PathBuf::from(format!(
            "{}.{}",
            sketch.name(),
            if dxf { "dxf" } else { "svg" }
        ))
    });
    let written = eh3d_phoenix_cad::export::write_file(
        &target,
        &text,
        overwrite,
        project.as_deref(),
        ui.export.max_kib,
    )
    .map_err(|e| error_text(&e, lang))?;
    println!(
        "{}",
        eh3d_phoenix_cad::lang::fill(
            Msg::Exported.tr(lang),
            &[&target.display().to_string(), &written.to_string()]
        )
    );
    Ok(true)
}

/// `balance`: reads a project and prints the mass of its placed parts and their centre of mass.
fn run_balance(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let (file, config) = file_and_config(&args, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    print!("{}", win.balance_text().map_err(|e| error_text(&e, lang))?);
    Ok(win.info.failed == 0)
}

/// `library`: reads a library of parts, checks it against the settings and lists its parts.
fn run_library(args: &[String]) -> Result<bool, String> {
    let (file, config) = file_and_config(args, "library file")?;
    let (settings, _) = load_settings(config.as_deref())?;
    let lang = Lang::En;
    let library = catalog::read_file(&file, &settings.design.catalog)
        .and_then(|library| {
            library
                .check_materials(&settings.materials)
                .map(|()| library)
        })
        .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
    println!("library {}: {} parts", library.name, library.parts.len());
    for part in &library.parts {
        println!("  {} {} {}", part.kind().name(), part.name, part.material);
    }
    Ok(true)
}

/// The format of the `export` command: a word, and for the drawings the view.
fn export_format(word: &str, view: Option<View>) -> Result<Format, String> {
    match word {
        "dxf" | "svg" => Ok(Format::drawing(word == "dxf", view.unwrap_or(View::Top))),
        _ if view.is_some() => Err("--view is only for the formats dxf and svg".to_owned()),
        "stl" | "step" | "script" | "report" | "bill" | "obj" | "3mf" => {
            Format::from_name(word).ok_or_else(|| format!("{word}: unknown format"))
        }
        other => Err(format!(
            "{other}: unknown format (stl, obj, 3mf, dxf, svg, step, script, report or bill)"
        )),
    }
}

/// `export`: reads a project and writes the model to a file in a format. The answer is whether the
/// file was written.
fn run_export(args: &[String]) -> Result<bool, String> {
    let mut lang = Lang::from_env();
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut word: Option<String> = None;
    let mut view: Option<View> = None;
    let mut overwrite = false;
    let mut binary: Option<bool> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            "--config" => {
                let path = iter.next().ok_or("--config needs a settings file")?;
                config = Some(PathBuf::from(path));
            }
            "--format" => word = Some(iter.next().ok_or("--format needs a format")?.clone()),
            "--view" => {
                let name = iter.next().ok_or("--view needs top, front or side")?;
                view = Some(View::from_name(name).ok_or("--view is top, front or side")?);
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--ascii" => binary = Some(false),
            "--binary" => binary = Some(true),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project can be exported".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("export needs a project file")?;
    let word = word.ok_or("export needs --format")?;
    let format = export_format(&word, view)?;
    if binary.is_some() && format != Format::Stl {
        return Err("--ascii and --binary are only for the format stl".to_owned());
    }
    let (settings, ui) = load_settings(config.as_deref())?;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    win.sketch_settings = load_solver_settings(config.as_deref())?;
    if let Some(b) = binary {
        win.settings.mesh.binary = b;
    }
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    let path = out.unwrap_or_else(|| PathBuf::from(win.export_name(format)));
    if win.export_to(format, &path, overwrite) {
        println!("{}", win.status.text(lang));
        Ok(true)
    } else {
        Err(win.status.text(lang))
    }
}

fn run(args: &[String]) -> Result<bool, String> {
    let Some((command, rest)) = args.split_first() else {
        println!("{HELP}");
        return Ok(true);
    };
    match command.as_str() {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(true)
        }
        "config" => {
            if !rest.is_empty() {
                return Err("config takes no arguments".to_owned());
            }
            print!(
                "{DEFAULT_CONFIG}\n{}\n{}",
                default_file(),
                eh3d_phoenix_cad::sketch::settings::default_file()
            );
            Ok(true)
        }
        "check" => run_check(rest),
        "interference" => run_interference(rest),
        "bill" => run_bill(rest),
        "cutplan" => run_cutplan(rest),
        "clearance" => run_clearance(rest),
        "placements" => run_placements(rest),
        "holes" => run_holes(rest),
        "partlist" => run_partlist(rest),
        "balance" => run_balance(rest),
        "revolve" => run_revolve(rest),
        "sweep" => run_sweep(rest),
        "sketch-export" => run_sketch_export(rest),
        "library" => run_library(rest),
        "export" => run_export(rest),
        "gui" => Err("this build has no window: build it with --features gui".to_owned()),
        other => Err(format!("{other}: unknown command (see help)")),
    }
}

/// The arguments of the window when they ask for it instead of a command: none, `gui`, an option
/// of the window, or a single project file given by the system to open.
#[cfg(feature = "gui")]
fn window_request(args: &[String]) -> Option<Result<shot::WindowArgs, String>> {
    match args.first().map(String::as_str) {
        None | Some("gui") => {}
        Some(
            "help" | "--help" | "-h" | "config" | "check" | "interference" | "bill" | "cutplan"
            | "clearance" | "placements" | "holes" | "partlist" | "balance" | "revolve" | "sweep"
            | "sketch-export" | "library" | "export",
        ) => return None,
        Some(first) if first.starts_with("--") => {}
        Some(first) => {
            let path = Path::new(first);
            if path.extension().is_none() || !path.is_file() {
                return None;
            }
        }
    }
    Some(shot::parse_window_args(args, 1))
}

#[cfg(feature = "gui")]
fn start_of(parsed: &shot::WindowArgs) -> Result<eh3d_phoenix_cad::gui::Start, String> {
    let (settings, ui) = load_settings(parsed.config.as_deref())?;
    let (solver, sketch_ui) = load_sketch_settings(parsed.config.as_deref())?;
    let (project, sketch) = match parsed.files.first().cloned() {
        Some(f) if shot::is_sketch_file(&f) => (None, Some(f)),
        other => (other, None),
    };
    let view = parsed.view.unwrap_or(if sketch.is_some() {
        shot::StartView::Sketch
    } else {
        shot::StartView::Model
    });
    Ok(eh3d_phoenix_cad::gui::Start {
        lang: parsed.lang.unwrap_or_else(Lang::from_env),
        settings,
        ui,
        solver,
        sketch_ui,
        project,
        sketch,
        view,
    })
}

#[cfg(feature = "gui")]
fn open_window(parsed: &shot::WindowArgs) -> ExitCode {
    let result = start_of(parsed).and_then(eh3d_phoenix_cad::gui::run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: the window could not be opened: {e}");
            ExitCode::from(1)
        }
    }
}

/// The picture mode: `--shot FILE.png` opens the window, saves what it draws as a PNG file and
/// closes. The exit code is 0 when the file was written, 1 when it was not and 2 when the
/// arguments are wrong.
#[cfg(feature = "gui")]
fn take_picture(args: &[String]) -> ExitCode {
    let parsed = match shot::parse_window_args(args, 1) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(request) = parsed.shot.clone() else {
        eprintln!("error: --shot needs a file to write");
        return ExitCode::from(2);
    };
    let result =
        start_of(&parsed).and_then(|start| eh3d_phoenix_cad::gui::run_shot(start, request));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: the picture could not be taken: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if shot::wants_picture(&args) {
        #[cfg(feature = "gui")]
        return take_picture(&args);
        #[cfg(not(feature = "gui"))]
        {
            eprintln!("error: this build has no window: build it with --features gui");
            return ExitCode::from(1);
        }
    }
    #[cfg(feature = "gui")]
    if let Some(parsed) = window_request(&args) {
        return match parsed {
            Ok(parsed) => open_window(&parsed),
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        };
    }
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
