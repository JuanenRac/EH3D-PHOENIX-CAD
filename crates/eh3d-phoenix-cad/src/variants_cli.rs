// SPDX-License-Identifier: GPL-3.0-or-later
//! The command `variants`: the variants of a project listed, kept, changed, taken away, applied,
//! checked and exported from the command line, with the same code as the window
//! ([`crate::window`]). One action at a time; the actions that change the project write it back to
//! its file.

use std::path::Path;

use crate::export::Format;
use crate::lang::{fill, Msg};
use crate::suite_cli;
use crate::tools_cli::Args;
use crate::window::{error_text, StepChoice, Window};

const WITH_VALUE: [&str; 12] = [
    "--keep",
    "--only",
    "--set",
    "--variable",
    "--value",
    "--remove",
    "--apply",
    "--export",
    "--format",
    "--step",
    "--state",
    "--import",
];

const SWITCHES: [&str; 6] = [
    "--check",
    "--overwrite",
    "--text",
    "--steps",
    "--figures",
    "--grid",
];

fn csv(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// `variants PROJECT`: the answer is the text to print and whether all was in order (`--check`
/// answers no when a variant cannot be built, has a refused step or does not fit its board).
pub fn run(args: &[String]) -> Result<(String, bool), String> {
    let mut with_value = WITH_VALUE.to_vec();
    with_value.push("--config");
    let a = Args::parse(args, &with_value, &SWITCHES)?;
    let project = a.only_word("project file")?.to_owned();
    let actions = [
        "--keep", "--set", "--remove", "--apply", "--export", "--import",
    ]
    .iter()
    .filter(|name| a.value(name).is_some())
    .count()
        + ["--check", "--steps", "--figures", "--grid"]
            .iter()
            .filter(|name| a.has(name))
            .count();
    if actions > 1 {
        return Err("variants does one thing at a time".to_owned());
    }
    for (option, of) in [
        ("--only", "--keep"),
        ("--variable", "--set"),
        ("--value", "--set"),
        ("--step", "--set"),
        ("--state", "--set"),
        ("--format", "--export"),
    ] {
        if a.value(option).is_some() && a.value(of).is_none() {
            return Err(format!("{option} goes with {of}"));
        }
    }
    let settings = suite_cli::settings(a.value("--config"))?;
    let mut win = suite_cli::window(Path::new(&project), a.lang, &settings)?;

    if let Some(name) = a.value("--keep") {
        let done = win.keep_variant(name, a.value("--only").unwrap_or(""));
        return saved(win, done);
    }
    if let Some(name) = a.value("--set") {
        let value = (a.value("--variable"), a.value("--value"));
        let step = (a.value("--step"), a.value("--state"));
        let done = match (value, step) {
            ((Some(variable), Some(value)), (None, None)) => {
                win.set_variant_value(name, variable, value)
            }
            ((None, None), (Some(which), Some(state))) => {
                let choice = StepChoice::from_word(state)
                    .ok_or_else(|| format!("{state}: the state is off, on or keep"))?;
                win.set_variant_step(name, which, choice)
            }
            _ => return Err("--set needs --variable and --value, or --step and --state".to_owned()),
        };
        return saved(win, done);
    }
    if let Some(file) = a.value("--import") {
        let done = win.read_variant_grid(Path::new(file));
        return saved(win, done);
    }
    if a.has("--steps") {
        let mut out = "number,key,step,state\n".to_owned();
        for row in win.variant_step_rows() {
            let state = if row.suppressed { "off" } else { "on" };
            out.push_str(&format!(
                "{},{},{},{state}\n",
                row.number,
                row.key,
                csv(&row.label)
            ));
        }
        return Ok((out, true));
    }
    if a.has("--figures") {
        return Ok((win.variant_figures_csv(), true));
    }
    if a.has("--grid") {
        return Ok((win.variant_grid_csv(), true));
    }
    if let Some(name) = a.value("--remove") {
        let done = win.remove_variant(name);
        return saved(win, done);
    }
    if let Some(name) = a.value("--apply") {
        let done = win.apply_variant(name);
        return saved(win, done);
    }
    if let Some(folder) = a.value("--export") {
        return export(&win, &a, Path::new(folder));
    }
    let rows = win.variant_rows();
    if a.has("--check") {
        let checks = win.check_variants();
        let mut out = String::new();
        if a.has("--text") {
            for c in &checks {
                out.push_str(&fill(Msg::VariantFailed.tr(a.lang), &[&c.name, &c.text]));
                out.push('\n');
            }
            if checks.is_empty() {
                out = format!("{}\n", Msg::VariantNone.tr(a.lang));
            }
        } else {
            out.push_str("variant,ok,detail\n");
            for c in &checks {
                out.push_str(&format!("{},{},{}\n", c.name, c.ok, csv(&c.text)));
            }
        }
        return Ok((out, checks.iter().all(|c| c.ok)));
    }
    let mut out = String::new();
    if a.has("--text") {
        for row in &rows {
            out.push_str(&fill(
                Msg::VariantFailed.tr(a.lang),
                &[&row.name, &row.values],
            ));
            if !row.steps.is_empty() {
                out.push_str("; ");
                out.push_str(&row.steps);
            }
            if row.current {
                out.push_str("; ");
                out.push_str(Msg::VariantCurrent.tr(a.lang));
            }
            if !row.missing.is_empty() {
                out.push_str("; ");
                out.push_str(&fill(
                    Msg::VariantMissing.tr(a.lang),
                    &[&row.missing.join(", ")],
                ));
            }
            out.push('\n');
        }
        if rows.is_empty() {
            out = format!("{}\n", Msg::VariantNone.tr(a.lang));
        }
    } else {
        out.push_str("variant,variable,value_nm\n");
        for row in &rows {
            if let Ok(values) = win.mech.variants.values(&row.name) {
                for (variable, value) in values {
                    out.push_str(&format!("{},{variable},{}\n", row.name, value.get()));
                }
            }
            for (key, off) in win.mech.variants.steps_of(&row.name) {
                let state = if *off { "off" } else { "on" };
                out.push_str(&format!("{},step:{key},{state}\n", row.name));
            }
        }
    }
    Ok((out, true))
}

/// `variables PROJECT`: the variables with their values and the drawing they follow; `--follow`
/// makes one follow a reference dimension of a sketch of the project and `--stop` takes the link
/// away. The answer is no when a link is broken.
pub fn run_variables(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &["--follow", "--sketch", "--constraint", "--stop", "--config"],
        &["--text"],
    )?;
    let project = a.only_word("project file")?.to_owned();
    if a.value("--follow").is_some() && a.value("--stop").is_some() {
        return Err("variables does one thing at a time".to_owned());
    }
    for option in ["--sketch", "--constraint"] {
        if a.value(option).is_some() && a.value("--follow").is_none() {
            return Err(format!("{option} goes with --follow"));
        }
    }
    let settings = suite_cli::settings(a.value("--config"))?;
    let mut win = suite_cli::window(Path::new(&project), a.lang, &settings)?;
    if let Some(name) = a.value("--follow") {
        let (Some(sketch), Some(constraint)) = (a.value("--sketch"), a.value("--constraint"))
        else {
            return Err("--follow needs --sketch and --constraint".to_owned());
        };
        let done = win.follow_dimension(name, sketch, constraint);
        return saved(win, done);
    }
    if let Some(name) = a.value("--stop") {
        let done = win.stop_following(name);
        return saved(win, done);
    }
    let links = win.link_rows();
    let mut out = String::new();
    if !a.has("--text") {
        out.push_str("variable,value_nm,sketch,constraint,broken\n");
    }
    for row in win.sheet() {
        let link = links.iter().find(|l| l.variable == row.name);
        let broken = link.and_then(|l| l.broken.as_deref());
        if a.has("--text") {
            out.push_str(&format!("{} = {}", row.name, row.text));
            if let Some(l) = link {
                let words = fill(Msg::FollowRow.tr(a.lang), &[&l.constraint, &l.sketch]);
                out.push_str(&format!("; {words}"));
            }
            if let Some(why) = broken {
                out.push_str(&format!("; {}", fill(Msg::FollowBroken.tr(a.lang), &[why])));
            }
            out.push('\n');
        } else {
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                row.name,
                row.value.get(),
                link.map_or("", |l| l.sketch.as_str()),
                link.map_or("", |l| l.constraint.as_str()),
                csv(broken.unwrap_or(""))
            ));
        }
    }
    Ok((out, links.iter().all(|l| l.broken.is_none())))
}

/// Writes the project back after a change, or says why the change was refused.
fn saved(mut win: Window, done: bool) -> Result<(String, bool), String> {
    let lang = win.lang;
    if !done {
        return Err(win.status.text(lang));
    }
    let said = win.status.text(lang);
    // nothing to write when the project already was like that
    if win.dirty && !win.save() {
        return Err(win.status.text(lang));
    }
    Ok((format!("{said}\n"), true))
}

fn export(win: &Window, a: &Args, folder: &Path) -> Result<(String, bool), String> {
    let word = a.value("--format").unwrap_or("stl");
    let format =
        Format::from_name(word).ok_or_else(|| format!("{word}: not a format of export"))?;
    let files = win
        .export_variants(format, folder, a.has("--overwrite"))
        .map_err(|(name, e)| {
            fill(
                Msg::VariantFailed.tr(a.lang),
                &[&name, &error_text(&e, a.lang)],
            )
        })?;
    if files.is_empty() {
        return Ok((format!("{}\n", Msg::VariantNone.tr(a.lang)), false));
    }
    let mut out = String::new();
    for file in &files {
        out.push_str(&format!("{}\n", file.path.display()));
    }
    out.push_str(&fill(
        Msg::VariantExported.tr(a.lang),
        &[&files.len().to_string(), &folder.display().to_string()],
    ));
    out.push('\n');
    Ok((out, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::lang::Lang;
    use crate::parametric::Parameter;
    use crate::project::Document;
    use crate::uiconfig::UiSettings;
    use crate::vars::Param;

    struct Folder(std::path::PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A folder with a project whose inner width follows the variable `width`.
    fn project(name: &str) -> (Folder, String) {
        let dir = std::env::temp_dir().join(format!(
            "phoenix-cad-variants-cli-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("family.phoenixproj");
        let settings = Settings::defaults();
        let mut win = Window::new(Lang::En, settings.clone(), UiSettings::defaults()).unwrap();
        win.add_variable("width", "60 mm");
        assert!(win.save_to(&path), "{:?}", win.status);
        let mut document = Document::from_bytes(&std::fs::read(&path).unwrap(), 1 << 24).unwrap();
        let mut mech = document
            .mechanical(&settings.expr, &settings.mechanical)
            .unwrap()
            .unwrap();
        mech.model
            .set(Param::InnerWidth, Parameter::Variable("width".to_string()))
            .unwrap();
        document
            .set_mechanical(&mech, &settings.mechanical)
            .unwrap();
        std::fs::write(&path, document.to_bytes()).unwrap();
        let text = path.to_string_lossy().into_owned();
        (Folder(dir), text)
    }

    fn call(words: &[&str]) -> Result<(String, bool), String> {
        run(&words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn the_variants_are_kept_changed_listed_applied_and_removed_in_the_file() {
        let (_folder, p) = project("keep");
        assert_eq!(
            call(&[&p]).unwrap(),
            ("variant,variable,value_nm\n".to_owned(), true)
        );
        call(&[&p, "--keep", "small"]).unwrap();
        call(&[&p, "--keep", "large", "--only", "width"]).unwrap();
        call(&[
            &p,
            "--set",
            "large",
            "--variable",
            "width",
            "--value",
            "90 mm",
        ])
        .unwrap();
        assert_eq!(
            call(&[&p]).unwrap().0,
            "variant,variable,value_nm\nlarge,width,90000000\nsmall,width,60000000\n"
        );
        assert!(call(&[&p, "--text"])
            .unwrap()
            .0
            .contains("large: width = 90"));
        call(&[&p, "--apply", "large"]).unwrap();
        call(&[&p, "--keep", "now"]).unwrap();
        assert!(call(&[&p]).unwrap().0.contains("now,width,90000000\n"));
        call(&[&p, "--remove", "now"]).unwrap();
        assert!(!call(&[&p]).unwrap().0.contains("now,"));
    }

    #[test]
    fn a_refused_change_says_why_and_leaves_the_file() {
        let (_folder, p) = project("refused");
        let before = std::fs::read(&p).unwrap();
        for words in [
            vec![p.as_str(), "--apply", "none"],
            vec![p.as_str(), "--remove", "none"],
            vec![p.as_str(), "--keep", "two words"],
            vec![p.as_str(), "--keep", "a", "--only", "depth"],
            vec![p.as_str(), "--set", "a", "--variable", "width"],
            vec![p.as_str(), "--keep", "a", "--check"],
            vec![p.as_str(), "--value", "1"],
            vec![p.as_str(), "--no-such-option"],
        ] {
            assert!(call(&words).is_err(), "{words:?}");
        }
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }

    #[test]
    fn steps_figures_and_the_table_work_from_the_command_line() {
        let (folder, p) = project("table");
        call(&[&p, "--keep", "small"]).unwrap();
        assert_eq!(call(&[&p, "--steps"]).unwrap().0, "number,key,step,state\n");
        assert!(call(&[&p, "--set", "small", "--step", "1", "--state", "off"]).is_err());
        assert!(call(&[&p, "--set", "small", "--step", "1", "--state", "up"]).is_err());
        assert!(call(&[&p, "--set", "small", "--step", "1"]).is_err());
        assert!(call(&[&p, "--figures", "--grid"]).is_err());
        assert!(call(&[&p, "--state", "off"]).is_err());
        assert!(call(&[&p, "--text"])
            .unwrap()
            .0
            .contains(Msg::VariantCurrent.tr(Lang::En)));
        let (figures, ok) = call(&[&p, "--figures"]).unwrap();
        assert!(ok);
        assert_eq!(figures.lines().count(), 2, "{figures}");
        let (grid, _) = call(&[&p, "--grid"]).unwrap();
        assert_eq!(grid, "variant,width\nsmall,60 mm\n");
        let table = folder.0.join("table.csv");
        std::fs::write(&table, "variant,width\nlarge,90 mm\n").unwrap();
        let t = table.to_string_lossy().into_owned();
        call(&[&p, "--import", &t]).unwrap();
        assert_eq!(
            call(&[&p]).unwrap().0,
            "variant,variable,value_nm\nlarge,width,90000000\nsmall,width,60000000\n"
        );
        let before = std::fs::read(&p).unwrap();
        std::fs::write(&table, "variant,height\nlarge,1\n").unwrap();
        assert!(call(&[&p, "--import", &t])
            .unwrap_err()
            .contains("PHOENIX-CAD-014"));
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }

    #[test]
    fn the_command_variables_lists_and_refuses_what_it_cannot_follow() {
        let (_folder, p) = project("variables");
        let variables = |words: &[&str]| {
            run_variables(&words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            variables(&[&p]).unwrap(),
            (
                "variable,value_nm,sketch,constraint,broken\nwidth,60000000,,,\n".to_owned(),
                true
            )
        );
        assert!(variables(&[&p, "--text"])
            .unwrap()
            .0
            .starts_with("width = 60"));
        let before = std::fs::read(&p).unwrap();
        for words in [
            vec![p.as_str(), "--follow", "width"],
            vec![
                p.as_str(),
                "--follow",
                "width",
                "--sketch",
                "none",
                "--constraint",
                "c1",
            ],
            vec![p.as_str(), "--stop", "width"],
            vec![p.as_str(), "--sketch", "a"],
        ] {
            assert!(variables(&words).is_err(), "{words:?}");
        }
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }

    #[test]
    fn the_check_and_the_export_answer_for_every_variant() {
        let (folder, p) = project("export");
        let out = folder.0.to_string_lossy().into_owned();
        let (none, ok) = call(&[&p, "--export", &out]).unwrap();
        assert!(!ok, "{none}");
        call(&[&p, "--keep", "small"]).unwrap();
        call(&[&p, "--keep", "large"]).unwrap();
        call(&[
            &p,
            "--set",
            "large",
            "--variable",
            "width",
            "--value",
            "90 mm",
        ])
        .unwrap();
        let (checked, ok) = call(&[&p, "--check"]).unwrap();
        assert!(ok, "{checked}");
        assert_eq!(checked.lines().count(), 3);
        let (written, ok) = call(&[&p, "--export", &out, "--format", "script"]).unwrap();
        assert!(ok, "{written}");
        assert!(folder.0.join("family-large.scad").is_file());
        assert!(folder.0.join("family-small.scad").is_file());
        assert!(call(&[&p, "--export", &out, "--format", "script"])
            .unwrap_err()
            .contains("PHOENIX-CAD-040"));
        assert!(call(&[&p, "--export", &out, "--format", "word"]).is_err());
        call(&[
            &p,
            "--set",
            "small",
            "--variable",
            "width",
            "--value",
            "-1 mm",
        ])
        .unwrap();
        let (checked, ok) = call(&[&p, "--check", "--text"]).unwrap();
        assert!(!ok, "{checked}");
        assert!(checked.contains("small: PHOENIX-CAD-"), "{checked}");
    }
}
