// SPDX-License-Identifier: GPL-3.0-or-later
//! The command line of the small tools that read a file or a few numbers and print an answer:
//! `mesh-check`, `convert`, `bolt-circle`, `bom-costs`, `templates`, `new`, `measure`,
//! `config-diff` and `sketch-info`. Each one answers with a text that the program prints and with
//! whether everything was in order (the exit code of the program: 0 for yes, 1 for no).
//!
//! The reports that have words use the language of `--lang` (English without it, so a report is the
//! same text on every computer); the comma separated values never change with the language.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::bolt;
use crate::bom;
use crate::bomcosts;
use crate::config::Settings;
use crate::configdiff;
use crate::convert;
use crate::export::write_bytes;
use crate::interchange;
use crate::lang::{fill, Lang, Msg};
use crate::measurecmd;
use crate::mesh::{stl_binary, Mesh};
use crate::meshcheck;
use crate::sketch::file as sketch_file;
use crate::sketch::settings::SketchSettings;
use crate::sketchinfo;
use crate::step_import::StepLimits;
use crate::templates;
use crate::uiconfig::UiSettings;
use crate::units::parse_angle;
use crate::window::{error_text, Window};

/// The commands of this module, for the program to know which words are theirs.
pub const COMMANDS: [&str; 9] = [
    "mesh-check",
    "convert",
    "bolt-circle",
    "bom-costs",
    "templates",
    "new",
    "measure",
    "config-diff",
    "sketch-info",
];

/// The arguments of a command: the words that are not options, the options with a value and the
/// switches that were given.
struct Args {
    words: Vec<String>,
    values: BTreeMap<String, String>,
    switches: Vec<String>,
    lang: Lang,
}

impl Args {
    fn parse(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<Args, String> {
        let mut out = Args {
            words: Vec::new(),
            values: BTreeMap::new(),
            switches: Vec::new(),
            lang: Lang::En,
        };
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if arg == "--lang" {
                let code = iter.next().ok_or("--lang needs a language code")?;
                out.lang = Lang::from_code(code).ok_or("unknown language code")?;
            } else if with_value.contains(&arg.as_str()) {
                let value = iter.next().ok_or_else(|| format!("{arg} needs a value"))?;
                if out.values.insert(arg.clone(), value.clone()).is_some() {
                    return Err(format!("{arg} is given twice"));
                }
            } else if switches.contains(&arg.as_str()) {
                out.switches.push(arg.clone());
            } else if arg.starts_with("--") {
                return Err(format!("unknown option {arg}"));
            } else {
                out.words.push(arg.clone());
            }
        }
        Ok(out)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }

    /// The only word, which is what the command works on.
    fn only_word(&self, what: &str) -> Result<&str, String> {
        match self.words.as_slice() {
            [one] => Ok(one),
            [] => Err(format!("this command needs a {what}")),
            _ => Err(format!("only one {what} can be given")),
        }
    }
}

/// The settings of the library and of the window with the keys of a user file over the defaults.
fn load_settings(config: Option<&str>) -> Result<(Settings, UiSettings), String> {
    let Some(path) = config else {
        return Ok((Settings::defaults(), UiSettings::defaults()));
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let settings = Settings::with_user_config(&text).map_err(|e| format!("{path}: {e}"))?;
    let ui = UiSettings::with_user_config(&text).map_err(|e| format!("{path}: {e}"))?;
    Ok((settings, ui))
}

fn number(text: &str, what: &str, low: u32, high: u32) -> Result<u32, String> {
    match text.parse::<u32>() {
        Ok(n) if (low..=high).contains(&n) => Ok(n),
        _ => Err(format!(
            "{text}: {what} must be a whole number from {low} to {high}"
        )),
    }
}

/// Runs one of the commands of [`COMMANDS`] and returns what it prints and whether all was in order.
pub fn run(command: &str, args: &[String]) -> Result<(String, bool), String> {
    match command {
        "mesh-check" => mesh_check(args),
        "convert" => convert_command(args),
        "bolt-circle" => bolt_circle(args),
        "bom-costs" => bom_costs(args),
        "templates" => {
            let a = Args::parse(args, &[], &[])?;
            if !a.words.is_empty() {
                return Err("templates takes no arguments".to_owned());
            }
            Ok((templates::list_text(a.lang), true))
        }
        "new" => new_project(args),
        "measure" => measure(args),
        "config-diff" => config_diff(args),
        "sketch-info" => sketch_info(args),
        other => Err(format!("{other}: unknown command (see help)")),
    }
}

/// `mesh-check FILE`: the check of [`crate::meshcheck`] for each solid of a file of triangles.
fn mesh_check(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &["--config", "--repair", "--weld", "--bed"],
        &["--overwrite"],
    )?;
    let file = PathBuf::from(a.only_word("mesh file")?);
    let (settings, ui) = load_settings(a.value("--config"))?;
    let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let limits = StepLimits::from_settings(&settings.mesh);
    let mut bodies = interchange::read_file_bytes_limited(&file.to_string_lossy(), &bytes, &limits)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let mut out = format!("{}: {} solids\n", file.display(), bodies.len());
    let bed = match a.value("--bed") {
        Some(text) => {
            let point = measurecmd::parse_point(text).map_err(|e| error_text(&e, a.lang))?;
            if point.iter().any(|v| *v <= 0.0) {
                return Err(format!(
                    "{text}: the build volume must have a size along every axis"
                ));
            }
            Some(point)
        }
        None => None,
    };
    if let Some(text) = a.value("--weld") {
        let tolerance = measurecmd::length_or_mm(text).map_err(|e| error_text(&e, a.lang))?;
        if tolerance.get() < 0 {
            return Err(format!("{text}: --weld must not be negative"));
        }
        let (mut merged, mut dropped) = (0usize, 0usize);
        for (_, mesh) in &mut bodies {
            let welded = meshcheck::weld(mesh, tolerance.get() as f64);
            merged += welded.merged_vertices;
            dropped += welded.dropped_triangles;
            *mesh = welded.mesh;
        }
        out.push_str(&fill(
            Msg::MeshCheckWelded.tr(a.lang),
            &[&merged.to_string(), &dropped.to_string()],
        ));
        out.push('\n');
    }
    let mut turned = 0usize;
    if a.value("--repair").is_some() {
        for (_, mesh) in &mut bodies {
            if meshcheck::analyse(mesh).volume_mm3 < 0.0 {
                *mesh = meshcheck::flipped(mesh);
                turned += 1;
            }
        }
    }
    let mut all_good = true;
    for (name, mesh) in &bodies {
        let report = meshcheck::analyse(mesh);
        all_good &= report.printable();
        out.push_str(&format!("{name}:\n"));
        for line in meshcheck::text_in(&report, a.lang).lines() {
            out.push_str(&format!("  {line}\n"));
        }
        if let Some(bed) = bed {
            let fit = meshcheck::fits(report.size_mm, bed);
            all_good &= fit != meshcheck::Fit::No;
            out.push_str(&format!("  {}\n", meshcheck::bed_text(fit, bed, a.lang)));
        }
    }
    if let Some(target) = a.value("--repair") {
        let target = Path::new(target);
        let refs: Vec<(&str, &Mesh)> = bodies.iter().map(|(n, m)| (n.as_str(), m)).collect();
        let extension = target
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let bytes = match extension.as_str() {
            "stl" => stl_binary(&refs),
            "obj" => interchange::obj(&refs).into_bytes(),
            "3mf" => interchange::three_mf(&refs),
            _ => return Err("--repair must name a file that ends in .stl, .obj or .3mf".to_owned()),
        };
        write_bytes(
            target,
            &bytes,
            a.has("--overwrite"),
            None,
            ui.export.max_kib,
        )
        .map_err(|e| error_text(&e, a.lang))?;
        out.push_str(&fill(
            Msg::MeshCheckRepaired.tr(a.lang),
            &[&turned.to_string(), &target.display().to_string()],
        ));
        out.push('\n');
    } else if a.has("--overwrite") {
        return Err("--overwrite needs --repair".to_owned());
    }
    Ok((out, all_good))
}

/// `convert VALUE [--to UNIT] [--decimals N] [--fraction N]`.
fn convert_command(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--to", "--decimals", "--fraction"], &[])?;
    let value = a.only_word("value")?;
    let to = a
        .value("--to")
        .unwrap_or(if convert::looks_like_dms(value) {
            "deg"
        } else {
            "mm"
        });
    // seconds of arc are whole unless asked for, lengths have three decimals
    let decimals = match a.value("--decimals") {
        Some(t) => number(t, "--decimals", 0, 12)?,
        None if to == "dms" => 0,
        None => 3,
    };
    let fraction = a
        .value("--fraction")
        .map(|t| number(t, "--fraction", 1, convert::MAX_DENOMINATOR))
        .transpose()?;
    let text =
        convert::convert(value, to, decimals, fraction).map_err(|e| error_text(&e, a.lang))?;
    Ok((format!("{text}\n"), true))
}

/// `bolt-circle --count N --diameter LENGTH [--start DEGREES] [--center X:Y] [--text]`.
fn bolt_circle(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &["--count", "--diameter", "--start", "--center"],
        &["--text"],
    )?;
    if !a.words.is_empty() {
        return Err("bolt-circle takes only options".to_owned());
    }
    let count = number(
        a.value("--count").ok_or("bolt-circle needs --count")?,
        "--count",
        1,
        bolt::MAX_HOLES,
    )?;
    let diameter = measurecmd::length_or_mm(
        a.value("--diameter")
            .ok_or("bolt-circle needs --diameter")?,
    )
    .map_err(|e| error_text(&e, a.lang))?;
    let start = match a.value("--start") {
        Some(t) => parse_angle(t).map_err(|e| error_text(&e, a.lang))?,
        None => parse_angle("0").map_err(|e| error_text(&e, a.lang))?,
    };
    let center = match a.value("--center") {
        Some(t) => {
            let parts: Vec<&str> = t.split(':').collect();
            let [x, y] = parts.as_slice() else {
                return Err(format!("{t}: the centre is written X:Y"));
            };
            (
                measurecmd::length_or_mm(x).map_err(|e| error_text(&e, a.lang))?,
                measurecmd::length_or_mm(y).map_err(|e| error_text(&e, a.lang))?,
            )
        }
        None => (crate::Length::ZERO, crate::Length::ZERO),
    };
    let circle =
        bolt::bolt_circle(count, diameter, start, center).map_err(|e| error_text(&e, a.lang))?;
    let out = if a.has("--text") {
        bolt::text_in(&circle, a.lang)
    } else {
        bolt::csv(&circle)
    };
    Ok((out, true))
}

/// `bom-costs PROJECT [--tax PERCENT] [--top N] [--text]`.
fn bom_costs(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--config", "--tax", "--top"], &["--text"])?;
    let file = PathBuf::from(a.only_word("project file")?);
    let tax = a
        .value("--tax")
        .map(bomcosts::parse_tax)
        .transpose()
        .map_err(|e| error_text(&e, a.lang))?;
    let top = a
        .value("--top")
        .map(|t| number(t, "--top", 1, 100_000))
        .transpose()?;
    let (settings, ui) = load_settings(a.value("--config"))?;
    let mut win = Window::new(a.lang, settings, ui).map_err(|e| error_text(&e, a.lang))?;
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(a.lang)));
    }
    let bill: bom::Bom = win.bom().map_err(|e| error_text(&e, a.lang))?;
    let mut shares = bomcosts::shares(&bill);
    if let Some(n) = top {
        shares = bomcosts::top(shares, n as usize);
    }
    let taxed = match tax {
        Some(bp) => Some(bomcosts::with_tax(&bill, bp).map_err(|e| error_text(&e, a.lang))?),
        None => None,
    };
    let out = if a.has("--text") {
        bomcosts::text_in(&bill, &shares, taxed.as_ref(), a.lang)
    } else {
        bomcosts::csv(&bill, &shares, taxed.as_ref())
    };
    Ok((out, win.info.failed == 0))
}

/// `new --template NAME --out FILE [--overwrite]`.
fn new_project(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--template", "--out"], &["--overwrite"])?;
    if !a.words.is_empty() {
        return Err("new takes only options".to_owned());
    }
    let name = a
        .value("--template")
        .ok_or("new needs --template (see templates)")?;
    let out = a.value("--out").ok_or("new needs --out FILE")?;
    templates::create(name, Path::new(out), a.has("--overwrite"))
        .map_err(|e| error_text(&e, a.lang))?;
    Ok((format!("{}\n", fill(Msg::Saved.tr(a.lang), &[out])), true))
}

/// `measure distance|angle POINT...`.
fn measure(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &[], &[])?;
    let [what, points @ ..] = a.words.as_slice() else {
        return Err("measure needs distance or angle and the points".to_owned());
    };
    let reading = match what.as_str() {
        "distance" => measurecmd::measure_distance(points),
        "angle" => measurecmd::measure_angle(points),
        other => return Err(format!("{other}: measure takes distance or angle")),
    }
    .map_err(|e| error_text(&e, a.lang))?;
    Ok((format!("{}\n", measurecmd::text_in(&reading, a.lang)), true))
}

/// `config-diff FILE [--text]`.
fn config_diff(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &[], &["--text"])?;
    let path = a.only_word("settings file")?;
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let rows =
        configdiff::diff(&text).map_err(|e| format!("{path}: {}", error_text(&e, a.lang)))?;
    let out = if a.has("--text") {
        configdiff::text_in(&rows, a.lang)
    } else {
        configdiff::csv(&rows)
    };
    Ok((out, configdiff::counts(&rows).2 == 0))
}

/// `sketch-info FILE.phoenixsketch`.
fn sketch_info(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--config"], &[])?;
    let path = PathBuf::from(a.only_word("sketch file")?);
    let settings = match a.value("--config") {
        Some(c) => {
            let text = fs::read_to_string(c).map_err(|e| format!("{c}: {e}"))?;
            SketchSettings::with_user_config(&text).map_err(|e| format!("{c}: {e}"))?
        }
        None => SketchSettings::defaults(),
    };
    let sketch = sketch_file::read_file(&path, &settings)
        .map_err(|e| format!("{}: {}", path.display(), error_text(&e, a.lang)))?;
    Ok((
        sketchinfo::text_in(&sketchinfo::info(&sketch), a.lang),
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meshcheck::tests_support::cuboid;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| (*x).to_owned()).collect()
    }

    fn temp(tag: &str, extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cad18_cli_{}_{tag}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn the_argument_reader_takes_values_switches_words_and_the_language() {
        let a = Args::parse(
            &args(&["file", "--tax", "21", "--text", "--lang", "pt"]),
            &["--tax"],
            &["--text"],
        )
        .unwrap();
        assert_eq!(a.words, ["file"]);
        assert_eq!(a.value("--tax"), Some("21"));
        assert!(a.has("--text") && !a.has("--other"));
        assert_eq!(a.lang, Lang::Pt);
        for bad in [
            args(&["--nope"]),
            args(&["--tax"]),
            args(&["--tax", "1", "--tax", "2"]),
            args(&["--lang", "xx"]),
            args(&["--lang"]),
        ] {
            assert!(Args::parse(&bad, &["--tax"], &[]).is_err(), "{bad:?}");
        }
        assert!(Args::parse(&args(&["a", "b"]), &[], &[])
            .unwrap()
            .only_word("file")
            .is_err());
        assert!(Args::parse(&[], &[], &[])
            .unwrap()
            .only_word("file")
            .is_err());
    }

    #[test]
    fn every_command_of_the_module_is_listed_once_and_known() {
        for (i, c) in COMMANDS.iter().enumerate() {
            assert!(!COMMANDS[i + 1..].contains(c), "{c}");
            // an unknown option reaches the argument reader of the command, which refuses it
            let e = run(c, &args(&["--no-such-option"])).unwrap_err();
            assert!(e.contains("--no-such-option"), "{c}: {e}");
        }
        assert!(run("nothing", &[]).unwrap_err().contains("unknown command"));
    }

    #[test]
    fn convert_reads_one_unit_and_writes_another_through_the_command() {
        let go = |list: &[&str]| run("convert", &args(list));
        assert_eq!(go(&["1 1/2 in"]).unwrap(), ("38.100 mm\n".to_owned(), true));
        assert_eq!(
            go(&["38.1 mm", "--to", "in", "--fraction", "16"])
                .unwrap()
                .0,
            "1 1/2 in\n"
        );
        assert_eq!(go(&["12.5", "--to", "dms"]).unwrap().0, "12°30'00\"\n");
        assert_eq!(go(&["12°30'"]).unwrap().0, "12.5 deg\n");
        assert_eq!(
            go(&["1 in", "--to", "mm", "--decimals", "1"]).unwrap().0,
            "25.4 mm\n"
        );
        assert!(go(&["1 in", "--fraction", "3", "--to", "in"])
            .unwrap_err()
            .starts_with("PHOENIX-CAD-007"));
        assert!(go(&["1 in", "--decimals", "13"]).is_err());
        assert!(go(&["1 ft"]).unwrap_err().starts_with("PHOENIX-CAD-008"));
        assert!(go(&[]).is_err());
    }

    #[test]
    fn the_bolt_circle_is_printed_as_values_or_as_text() {
        let csv = run("bolt-circle", &args(&["--count", "4", "--diameter", "20"]))
            .unwrap()
            .0;
        assert!(csv.starts_with("hole,angle_deg,x_mm,y_mm\n1,0,10,0\n2,90,0,10\n"));
        let text = run(
            "bolt-circle",
            &args(&[
                "--count",
                "3",
                "--diameter",
                "1 in",
                "--start",
                "90",
                "--center",
                "5:10 mm",
                "--text",
                "--lang",
                "es",
            ]),
        )
        .unwrap()
        .0;
        assert!(text.starts_with("Círculo de 3 agujeros"), "{text}");
        assert!(text.contains("\n1,90,5,22.7\n"), "{text}");
        for bad in [
            args(&["--diameter", "20"]),
            args(&["--count", "4"]),
            args(&["--count", "0", "--diameter", "20"]),
            args(&["--count", "4", "--diameter", "0"]),
            args(&["--count", "4", "--diameter", "20", "--center", "1"]),
            args(&["--count", "4", "--diameter", "20", "extra"]),
        ] {
            assert!(run("bolt-circle", &bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn measure_reads_the_points_and_names_what_is_missing() {
        let (text, ok) = run("measure", &args(&["distance", "0:0:0", "3:4:0"])).unwrap();
        assert_eq!(
            text,
            "Distance 5.0000 mm (x 3.0000, y 4.0000, z 0.0000 mm)\n"
        );
        assert!(ok);
        let (text, _) = run(
            "measure",
            &args(&["angle", "1:0:0", "0:0:0", "0:1:0", "--lang", "de"]),
        )
        .unwrap();
        assert!(text.contains("90.000"), "{text}");
        assert!(run("measure", &args(&["distance", "0:0:0"]))
            .unwrap_err()
            .starts_with("PHOENIX-CAD-038"));
        assert!(run("measure", &args(&["area"])).is_err());
        assert!(run("measure", &[]).is_err());
    }

    #[test]
    fn templates_are_listed_and_a_new_project_is_written_without_overwriting() {
        let (list, _) = run("templates", &[]).unwrap();
        assert_eq!(list.lines().count(), 4);
        assert!(run("templates", &args(&["x"])).is_err());
        let path = temp("new", "phoenixproj");
        let _ = fs::remove_file(&path);
        let out = path.to_string_lossy().into_owned();
        let (text, ok) = run("new", &args(&["--template", "frame", "--out", &out])).unwrap();
        assert!(ok && text.starts_with("Saved "), "{text}");
        let again = run("new", &args(&["--template", "box", "--out", &out])).unwrap_err();
        assert!(again.starts_with("PHOENIX-CAD-040"), "{again}");
        run(
            "new",
            &args(&["--template", "box", "--out", &out, "--overwrite"]),
        )
        .unwrap();
        assert!(run("new", &args(&["--template", "nope", "--out", &out]))
            .unwrap_err()
            .starts_with("PHOENIX-CAD-014"));
        assert!(run("new", &args(&["--out", &out])).is_err());
        assert!(run("new", &args(&["--template", "box"])).is_err());
        // the bill of costs of the frame
        let _ = fs::remove_file(&path);
        run("new", &args(&["--template", "frame", "--out", &out])).unwrap();
        let (csv, ok) = run("bom-costs", &args(&[&out, "--tax", "21", "--top", "3"])).unwrap();
        assert!(ok);
        assert!(csv.starts_with(
            "item,name,material,quantity,cost,share_percent,cumulative_percent,currency\n"
        ));
        assert!(csv.contains(",tax,21%,"), "{csv}");
        assert_eq!(csv.lines().count(), 1 + 3 + 5);
        let (text, _) = run("bom-costs", &args(&[&out, "--text", "--lang", "fr"])).unwrap();
        assert!(!text.contains('{') && text.lines().count() > 3, "{text}");
        assert!(run("bom-costs", &args(&[&out, "--tax", "abc"])).is_err());
        assert!(run("bom-costs", &args(&[&out, "--top", "0"])).is_err());
        assert!(run("bom-costs", &args(&["no-such-file.phoenixproj"])).is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn config_diff_lists_the_keys_and_exits_with_no_when_one_is_not_a_default_key() {
        let path = temp("cfg", "cfg");
        let file = path.to_string_lossy().into_owned();
        fs::write(&path, "[printer]\nhole_compensation = 0.2 mm\n").unwrap();
        let (csv, ok) = run("config-diff", &args(&[&file])).unwrap();
        assert!(ok);
        assert_eq!(
            csv,
            "key,default,value,status\nprinter.hole_compensation,0.15 mm,0.2 mm,changed\n"
        );
        fs::write(&path, "[nonsense]\nfoo = 1\n").unwrap();
        let (text, ok) = run("config-diff", &args(&[&file, "--text"])).unwrap();
        assert!(!ok && text.contains("nonsense.foo = 1"), "{text}");
        fs::write(&path, "[printer\n").unwrap();
        assert!(run("config-diff", &args(&[&file])).is_err());
        let _ = fs::remove_file(&path);
        assert!(run("config-diff", &args(&[&file])).is_err());
    }

    #[test]
    fn sketch_info_reads_a_sketch_file() {
        use crate::enclosure::Face;
        use crate::sketch::{Sketch, SketchPlane};
        let mut s = Sketch::new("plate", SketchPlane::on_face(Face::Top)).unwrap();
        let mm = |v: i64| crate::Length::from_mm(v).unwrap();
        s.add_rectangle((mm(0), mm(0)), (mm(20), mm(10))).unwrap();
        let path = temp("sk", "phoenixsketch");
        fs::write(&path, sketch_file::to_text(&s)).unwrap();
        let (text, ok) = run("sketch-info", &args(&[&path.to_string_lossy()])).unwrap();
        assert!(ok);
        assert!(
            text.starts_with("Sketch plate: 4 points, 4 lines"),
            "{text}"
        );
        let (text, _) = run(
            "sketch-info",
            &args(&[&path.to_string_lossy(), "--lang", "pt"]),
        )
        .unwrap();
        assert!(text.starts_with("Esboço plate"), "{text}");
        let _ = fs::remove_file(&path);
        assert!(run("sketch-info", &args(&[&path.to_string_lossy()])).is_err());
    }

    #[test]
    fn mesh_check_reads_a_file_reports_every_solid_and_can_turn_a_solid_the_right_way_out() {
        let mesh = cuboid(10.0, 20.0, 30.0);
        let inside = crate::meshcheck::flipped(&mesh);
        let good = temp("good", "stl");
        let bad = temp("bad", "stl");
        let fixed = temp("fixed", "obj");
        let _ = fs::remove_file(&fixed);
        fs::write(&good, stl_binary(&[("box", &mesh)])).unwrap();
        fs::write(&bad, stl_binary(&[("box", &inside)])).unwrap();
        let (good_s, bad_s, fixed_s) = (
            good.to_string_lossy().into_owned(),
            bad.to_string_lossy().into_owned(),
            fixed.to_string_lossy().into_owned(),
        );
        let (text, ok) = run("mesh-check", &args(&[&good_s])).unwrap();
        assert!(ok && text.contains("Printable"), "{text}");
        let (text, ok) = run("mesh-check", &args(&[&bad_s])).unwrap();
        assert!(!ok && text.contains("inside out"), "{text}");
        let (text, ok) = run("mesh-check", &args(&[&bad_s, "--repair", &fixed_s])).unwrap();
        assert!(ok, "{text}");
        assert!(text.contains("Turned 1 solid(s)"), "{text}");
        assert!(fixed.is_file());
        let (again, ok) = run("mesh-check", &args(&[&fixed_s])).unwrap();
        assert!(ok && again.contains("Printable"), "{again}");
        // a second repair does not write over the file unless told to
        assert!(run("mesh-check", &args(&[&bad_s, "--repair", &fixed_s]))
            .unwrap_err()
            .starts_with("PHOENIX-CAD-040"));
        assert!(run(
            "mesh-check",
            &args(&[&bad_s, "--repair", &fixed_s, "--overwrite"])
        )
        .is_ok());
        assert!(run("mesh-check", &args(&[&bad_s, "--overwrite"])).is_err());
        assert!(run("mesh-check", &args(&[&bad_s, "--repair", "x.txt"])).is_err());
        assert!(run("mesh-check", &args(&["no-such-file.stl"])).is_err());
        // a file that repeats its corners is welded before it is checked, and the build volume is told
        let mut soup = Mesh::default();
        for t in &mesh.triangles {
            let base = soup.vertices.len() as u32;
            for id in t {
                soup.vertices.push(mesh.vertices[*id as usize]);
            }
            soup.triangles.push([base, base + 1, base + 2]);
        }
        // an STL file is welded when it is read, so the soup is written as an OBJ file instead
        let soup_obj = temp("soup", "obj");
        fs::write(&soup_obj, interchange::obj(&[("box", &soup)])).unwrap();
        let soup_s = soup_obj.to_string_lossy().into_owned();
        let (text, ok) = run("mesh-check", &args(&[&soup_s, "--weld", "0"])).unwrap();
        assert!(ok, "{text}");
        assert!(
            text.contains("28 vertices merged, 0 triangles dropped"),
            "{text}"
        );
        let (text, ok) = run(
            "mesh-check",
            &args(&[&soup_s, "--weld", "0", "--bed", "220:220:250"]),
        )
        .unwrap();
        assert!(
            ok && text.contains("Fits the 220 x 220 x 250 mm build volume"),
            "{text}"
        );
        let (text, ok) = run("mesh-check", &args(&[&good_s, "--bed", "10:10:10"])).unwrap();
        assert!(!ok && text.contains("Does not fit"), "{text}");
        let (text, ok) = run("mesh-check", &args(&[&good_s, "--bed", "30:30:10"])).unwrap();
        assert!(ok && text.contains("only when turned"), "{text}");
        for bad in [
            ["--bed", "10:10"],
            ["--bed", "10:0:10"],
            ["--weld", "-1"],
            ["--weld", "x"],
        ] {
            assert!(
                run("mesh-check", &args(&[&good_s, bad[0], bad[1]])).is_err(),
                "{bad:?}"
            );
        }
        let _ = fs::remove_file(&soup_obj);
        for p in [good, bad, fixed] {
            let _ = fs::remove_file(p);
        }
    }
}
