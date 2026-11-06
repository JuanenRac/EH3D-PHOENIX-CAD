// EH3D-PHOENIX-CAD - command line and window.
// Copyright (C) 2026 JuanenRac (Electro Hobby 3D). GPL-3.0-or-later.
#![forbid(unsafe_code)]

use eh3d_phoenix_cad::body::StepState;
use eh3d_phoenix_cad::config::{Settings, DEFAULT_CONFIG};
use eh3d_phoenix_cad::lang::{Lang, Msg};
use eh3d_phoenix_cad::shot;
use eh3d_phoenix_cad::uiconfig::{default_file, UiSettings};
use eh3d_phoenix_cad::window::{error_text, step_label, Window};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "eh3d-phoenix-cad: parametric enclosures: the model tree, the properties, a 3D view and a plan, and the fit check of a board

  eh3d-phoenix-cad [gui] [PROJECT.phoenixproj] [--lang CODE] [--config FILE]   the window (a build with --features gui): with no arguments it opens with a new project
  eh3d-phoenix-cad gui [PROJECT] --shot OUT.png [--size WxH] [--theme forge|dark|light] [--wait N] [--lang CODE] [--config FILE] [--replace]   a picture of the window (a build with --features gui)
  eh3d-phoenix-cad check PROJECT.phoenixproj [--lang CODE] [--config FILE]       reads a project, builds the box and prints its measures, the fit of the board and the state of every step
  eh3d-phoenix-cad config                                                         every setting of the library and of the window with its default value, as a file for --config

--open PROJECT is the same as giving the project by itself. --lang takes en, es, de, fr, it, ja, zh or pt; without it the language of the system is used.
--shot writes a PNG file of the window (the exit code is 0 when it was written, 1 when it was not and 2 when the arguments are wrong); the window keeps no settings in that mode.
--config FILE reads a settings file over the defaults: it only needs the keys it changes (see the output of config).
check exits with 0 when every step was applied and the board fits, and with 1 when a step was refused or the board does not fit.";

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

/// `check`: reads a project and says what the model makes of it. The answer is whether every step
/// was applied and the board fits.
fn run_check(args: &[String]) -> Result<bool, String> {
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
    let file = file.ok_or("check needs a project file")?;
    let (settings, ui) = load_settings(config.as_deref())?;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    if win.status.is_problem() {
        eprintln!("note: {}", win.status.text(lang));
    }
    println!("{}", win.measures_text());
    println!("{}", win.fit_text());
    for (i, step) in win.mech.steps.iter().enumerate() {
        let state = match win.states().get(i) {
            Some(StepState::Valid) | None => Msg::StateValid.tr(lang).to_owned(),
            Some(StepState::Suppressed) => Msg::StateSuppressed.tr(lang).to_owned(),
            Some(StepState::AfterTip) => Msg::StateAfterTip.tr(lang).to_owned(),
            Some(StepState::Failed(e)) => {
                format!("{}: {}", Msg::StateFailed.tr(lang), error_text(e, lang))
            }
        };
        println!("{:>3}  {}  {state}", i + 1, step_label(step));
    }
    Ok(win.info.failed == 0 && win.info.fit.fits)
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
            print!("{DEFAULT_CONFIG}\n{}", default_file());
            Ok(true)
        }
        "check" => run_check(rest),
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
        Some("help" | "--help" | "-h" | "config" | "check") => return None,
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
    Ok(eh3d_phoenix_cad::gui::Start {
        lang: parsed.lang.unwrap_or_else(Lang::from_env),
        settings,
        ui,
        project: parsed.files.first().cloned(),
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
