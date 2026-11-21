// SPDX-License-Identifier: GPL-3.0-or-later
//! The commands that look at the drawing sheet of a project: `drawing-check` builds its history
//! and its sketches, works the sheet out against them and reports every view, dimension and balloon;
//! `drawing-export` writes the sheet as SVG, PDF or DXF, or its bill as comma separated values.
//!
//! The exit code is the one of every check of the suite: 0 when the sheet agrees with the model, 2
//! when the verification did not pass (a view that cannot be made, a dimension whose thing is gone
//! or that cannot be drawn, a balloon whose part is gone, a view that does not fit the sheet), and
//! 1 when the command could not run (a file that cannot be read, a drawing that is not one).
//! `drawing-export` writes nothing while the check does not pass, unless `--force` is given. The
//! reports use the language of `--lang`; the comma separated values never change with it.

use std::fs;
use std::path::Path;

use crate::config::Settings;
use crate::export::write_bytes;
use crate::featuredag::{FeatureDag, HistorySettings};
use crate::lang::{fill, Lang, Msg};
use crate::plans::bom::to_csv;
use crate::plans::measure::Model;
use crate::plans::model::{Place, Plan, Source, View, ViewKind};
use crate::plans::out;
use crate::plans::panel::{issue_text, lines_of};
use crate::plans::scene::{evaluate, Evaluation};
use crate::plans::settings::{parse_scale, SheetSettings};
use crate::project::Document;
use crate::sketch::settings::SketchSettings;
use crate::sketch::Sketch;
use crate::tools_cli::Args;
use crate::uiconfig::UiSettings;
use crate::window::error_text;
use crate::CadError;

/// The commands of this module, for the program to know which words are theirs.
pub const COMMANDS: [&str; 2] = ["drawing-check", "drawing-export"];

/// The largest project file that these commands read.
const MAX_PROJECT_BYTES: usize = 256 << 20;

/// Runs one of the commands of [`COMMANDS`] and returns what it prints and whether the check
/// passed.
pub fn run(command: &str, args: &[String]) -> Result<(String, bool), String> {
    match command {
        "drawing-check" => check(args),
        "drawing-export" => export(args),
        other => Err(format!("{other}: unknown command (see help)")),
    }
}

struct Everything {
    lib: Settings,
    hist: HistorySettings,
    sketch: SketchSettings,
    sheet: SheetSettings,
    ui: UiSettings,
}

fn settings(config: Option<&str>) -> Result<Everything, String> {
    let Some(path) = config else {
        return Ok(Everything {
            lib: Settings::defaults(),
            hist: HistorySettings::defaults(),
            sketch: SketchSettings::defaults(),
            sheet: SheetSettings::defaults(),
            ui: UiSettings::defaults(),
        });
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let w = |e: CadError| format!("{path}: {e}");
    Ok(Everything {
        lib: Settings::with_user_config(&text).map_err(w)?,
        hist: HistorySettings::with_user_config(&text).map_err(w)?,
        sketch: SketchSettings::with_user_config(&text).map_err(w)?,
        sheet: SheetSettings::with_user_config(&text).map_err(w)?,
        ui: UiSettings::with_user_config(&text).map_err(w)?,
    })
}

struct Opened {
    plan: Option<Plan>,
    dag: Option<FeatureDag>,
    sketches: Vec<Sketch>,
}

fn open(file: &str, e: &Everything, lang: Lang) -> Result<Opened, String> {
    let bytes = fs::read(file).map_err(|x| format!("{file}: {x}"))?;
    let doc = Document::from_bytes(&bytes, MAX_PROJECT_BYTES)
        .map_err(|x| format!("{file}: {}", error_text(&x, lang)))?;
    let fail = |x: CadError| format!("{file}: {}", error_text(&x, lang));
    let mut dag = doc.history(e.hist.clone(), &e.lib).map_err(fail)?;
    if let Some(d) = dag.as_mut() {
        d.rebuild(&e.lib).map_err(fail)?;
    }
    let sketches = doc.sketches(&e.sketch).map_err(fail)?;
    let plan = doc.plan(&e.sheet).map_err(fail)?;
    Ok(Opened {
        plan,
        dag,
        sketches,
    })
}

fn csv(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn rows(plan: &Plan, eval: &Evaluation) -> String {
    let mut out = String::from("record,name,state,code,detail\n");
    let mut seen: Vec<usize> = Vec::new();
    let mut row = |kind: &str, name: &str, out: &mut String| {
        let subject = format!("{kind} {name}");
        match eval.issues.iter().position(|i| i.subject == subject) {
            Some(k) => {
                seen.push(k);
                let i = &eval.issues[k];
                out.push_str(&format!(
                    "{kind},{},problem,{},{}\n",
                    csv(name),
                    i.code,
                    csv(&i.detail)
                ));
            }
            None => out.push_str(&format!("{kind},{},ok,,\n", csv(name))),
        }
    };
    for v in &plan.views {
        row("view", &v.name, &mut out);
    }
    for d in &plan.dims {
        row("dimension", &d.name, &mut out);
    }
    for b in &plan.balloons {
        row("balloon", &b.name, &mut out);
    }
    for (k, i) in eval.issues.iter().enumerate() {
        if !seen.contains(&k) {
            out.push_str(&format!("sheet,,problem,{},{}\n", i.code, csv(&i.detail)));
        }
    }
    out
}

fn check(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--config"], &["--text"])?;
    let file = a.only_word("project file")?;
    let e = settings(a.value("--config"))?;
    let o = open(file, &e, a.lang)?;
    let Some(plan) = o.plan else {
        return if a.has("--text") {
            Ok((format!("{}\n", Msg::PlNone.tr(a.lang)), true))
        } else {
            Ok(("record,name,state,code,detail\n".to_owned(), true))
        };
    };
    let model = Model {
        dag: o.dag.as_ref(),
        sketches: &o.sketches,
    };
    let eval = evaluate(&plan, &model, &e.lib, &e.sheet, a.lang)
        .map_err(|x| format!("{file}: {}", error_text(&x, a.lang)))?;
    let ok = eval.is_clean();
    if !a.has("--text") {
        return Ok((rows(&plan, &eval), ok));
    }
    let lines = lines_of(&plan, &eval, &e.sheet, a.lang);
    let mut out = String::new();
    let mut push = |t: &str| {
        out.push_str(t);
        out.push('\n');
    };
    if let Some(h) = &lines.head {
        push(&h.text);
    }
    for l in lines.views.iter().chain(&lines.dims).chain(&lines.balloons) {
        push(&l.text);
    }
    if !lines.bom.is_empty() {
        push(&fill(
            Msg::PlBomHead.tr(a.lang),
            &[&lines.bom.len().to_string()],
        ));
        for l in &lines.bom {
            push(&l.text);
        }
    }
    for i in &eval.issues {
        push(&issue_text(i, a.lang));
    }
    if ok {
        push(Msg::PlClean.tr(a.lang));
    } else {
        push(&fill(
            Msg::PlProblems.tr(a.lang),
            &[&eval.issues.len().to_string()],
        ));
    }
    Ok((out, ok))
}

fn export(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &["--config", "--format", "--sheet", "--scale"],
        &["--overwrite", "--create", "--portrait", "--force"],
    )?;
    let (file, target) = match a.words.as_slice() {
        [p, t] => (p.as_str(), t.as_str()),
        _ => return Err("this command needs a project file and the file to write".to_owned()),
    };
    let e = settings(a.value("--config"))?;
    let mut o = open(file, &e, a.lang)?;
    let fmt = match a.value("--format") {
        Some(f) => f.to_string(),
        None => Path::new(target)
            .extension()
            .map(|x| x.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default(),
    };
    if !["svg", "pdf", "dxf", "csv", "bom"].contains(&fmt.as_str()) {
        return Err(format!(
            "{fmt}: the format is svg, pdf, dxf or bom (or the extension csv)"
        ));
    }
    let fail = |x: CadError| format!("{file}: {}", error_text(&x, a.lang));
    if o.plan.is_none() {
        if !a.has("--create") {
            return Err(format!(
                "{file}: {} (--create makes one for the export)",
                Msg::PlNone.tr(a.lang)
            ));
        }
        if o.dag.is_none() {
            return Err(format!("{file}: {}", Msg::FeatNone.tr(a.lang)));
        }
        let mut plan = Plan::new(&e.sheet);
        let mut gen = eh3d_container::IdGenerator::system();
        for (kind, name) in [
            (ViewKind::Front, "front"),
            (ViewKind::Top, "top"),
            (ViewKind::Right, "right"),
            (ViewKind::Iso, "iso"),
        ] {
            let id = gen.generate().map_err(|x| x.to_string())?;
            plan.add_view(
                View {
                    id,
                    name: name.to_string(),
                    kind,
                    source: Source::Tip,
                    place: Place::Auto,
                    scale: None,
                    hidden: e.sheet.hidden_lines,
                    extra: Vec::new(),
                },
                &e.sheet,
            )
            .map_err(fail)?;
        }
        o.plan = Some(plan);
    }
    let mut plan = o.plan.take().ok_or("no sheet")?;
    if let Some(name) = a.value("--sheet") {
        plan.format = name.to_string();
    }
    if a.has("--portrait") {
        plan.portrait = true;
    }
    if let Some(s) = a.value("--scale") {
        plan.scale = if s == "auto" {
            crate::plans::model::SheetScale::Auto
        } else {
            let (n, d) = parse_scale(s).ok_or_else(|| format!("{s}: a scale is auto or n:d"))?;
            crate::plans::model::SheetScale::Ratio(n, d)
        };
    }
    let model = Model {
        dag: o.dag.as_ref(),
        sketches: &o.sketches,
    };
    let eval = evaluate(&plan, &model, &e.lib, &e.sheet, a.lang).map_err(fail)?;
    let mut out = String::new();
    if !eval.is_clean() && !a.has("--force") {
        for i in &eval.issues {
            out.push_str(&issue_text(i, a.lang));
            out.push('\n');
        }
        out.push_str(&fill(
            Msg::PlProblems.tr(a.lang),
            &[&eval.issues.len().to_string()],
        ));
        out.push('\n');
        return Ok((out, false));
    }
    let title = Path::new(file)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes: Vec<u8> = match fmt.as_str() {
        "svg" => out::to_svg(&eval.scene, &title).into_bytes(),
        "dxf" => out::to_dxf(&eval.scene).into_bytes(),
        "pdf" => out::to_pdf(&eval.scene, &title),
        _ => to_csv(&eval.bom).into_bytes(),
    };
    write_bytes(
        Path::new(target),
        &bytes,
        a.has("--overwrite"),
        Some(Path::new(file)),
        e.ui.export.max_kib,
    )
    .map_err(|x| error_text(&x, a.lang))?;
    out.push_str(&fill(
        Msg::PlWrote.tr(a.lang),
        &[target, &eval.scene.prims.len().to_string()],
    ));
    out.push('\n');
    Ok((out, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Id;
    use crate::featuredag::Op;
    use crate::plans::model::{Axis, Dim, DimKind, Ref, Side};
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn id(n: u16) -> Id {
        Id::from_parts(1_700_000_000_000, n, u64::from(n) + 3).unwrap()
    }

    /// A project file in a folder of its own: a plate in a history, and, when asked, a sheet with
    /// the four views and a dimension of the plate (or of a node that is not there).
    fn project(side: &str, sheet: bool, orphan: bool) -> String {
        let c = Settings::defaults();
        let mut d = FeatureDag::new(HistorySettings::defaults());
        d.add_parameter(id(1), "side", side, &c).unwrap();
        d.add_feature(
            id(2),
            "plate",
            Op::Box,
            &[],
            &[
                ("x", "0 mm"),
                ("y", "0 mm"),
                ("z", "0 mm"),
                ("sx", "side"),
                ("sy", "20 mm"),
                ("sz", "10 mm"),
            ],
            &c,
        )
        .unwrap();
        let mut doc = Document::new();
        doc.set_history(&d).unwrap();
        if sheet {
            let s = SheetSettings::defaults();
            let mut p = Plan::new(&s);
            for (n, k) in [
                ViewKind::Front,
                ViewKind::Top,
                ViewKind::Right,
                ViewKind::Iso,
            ]
            .into_iter()
            .enumerate()
            {
                p.add_view(
                    View {
                        id: id(100 + n as u16),
                        name: k.name().to_string(),
                        kind: k,
                        source: Source::Tip,
                        place: Place::Auto,
                        scale: None,
                        hidden: true,
                        extra: Vec::new(),
                    },
                    &s,
                )
                .unwrap();
            }
            let node = if orphan { id(77) } else { id(2) };
            p.add_dim(
                Dim {
                    id: id(200),
                    name: "width".into(),
                    kind: DimKind::Linear,
                    refs: vec![
                        Ref::Face {
                            node,
                            axis: Axis::X,
                            side: Side::Min,
                        },
                        Ref::Face {
                            node,
                            axis: Axis::X,
                            side: Side::Max,
                        },
                    ],
                    view: id(100),
                    offset_um: None,
                    layer: "DIMENSION".into(),
                    extra: Vec::new(),
                },
                &s,
            )
            .unwrap();
            doc.set_plan(&p).unwrap();
        }
        let dir = std::env::temp_dir().join(format!(
            "cad21_plancli_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.phoenixproj");
        fs::write(&path, doc.to_bytes()).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_string()).collect()
    }

    fn beside(path: &str, name: &str) -> String {
        Path::new(path)
            .with_file_name(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn a_sheet_that_agrees_with_the_model_passes_and_is_listed_record_by_record() {
        let path = project("40 mm", true, false);
        let (out, ok) = run("drawing-check", &s(&[&path])).unwrap();
        assert!(ok, "{out}");
        assert_eq!(
            out,
            "record,name,state,code,detail\nview,front,ok,,\nview,top,ok,,\nview,right,ok,,\nview,iso,ok,,\ndimension,width,ok,,\n"
        );
        let (text, ok) = run("drawing-check", &s(&[&path, "--text"])).unwrap();
        assert!(ok);
        assert!(
            text.starts_with("Sheet A4 landscape, scale 1:1: 4 views, 1 dimensions, 0 balloons\n"),
            "{text}"
        );
        assert!(
            text.contains("Dimension width: 40.00") && text.contains("Bill of materials: 1 items"),
            "{text}"
        );
        assert!(
            text.ends_with("The drawing agrees with the model\n"),
            "{text}"
        );
        let (de, _) = run("drawing-check", &s(&[&path, "--text", "--lang", "de"])).unwrap();
        assert!(
            de.contains("Blatt A4 Querformat") && de.contains("Maß width: 40.00"),
            "{de}"
        );
    }

    #[test]
    fn a_dimension_whose_node_is_gone_does_not_pass_and_says_which_in_the_language() {
        let path = project("40 mm", true, true);
        let (out, ok) = run("drawing-check", &s(&[&path])).unwrap();
        assert!(!ok);
        assert!(
            out.contains("dimension,width,problem,PHOENIX-CAD-055,"),
            "{out}"
        );
        assert!(out.contains("view,front,ok,,"), "{out}");
        let (text, ok) = run("drawing-check", &s(&[&path, "--text", "--lang", "es"])).unwrap();
        assert!(!ok);
        assert!(
            text.contains("dimension width: PHOENIX-CAD-055 Cota sin el elemento que mide"),
            "{text}"
        );
        assert!(text.ends_with("1 problemas en el plano\n"), "{text}");
        // a node that fails to build leaves the dimension an orphan too
        let failing = project("0 mm", true, false);
        let (out, ok) = run("drawing-check", &s(&[&failing])).unwrap();
        assert!(!ok);
        assert!(
            out.contains("PHOENIX-CAD-055") && out.contains("view,front,problem,PHOENIX-CAD-054,"),
            "{out}"
        );
    }

    #[test]
    fn a_project_without_a_sheet_passes_and_says_so() {
        let path = project("40 mm", false, false);
        let (text, ok) = run("drawing-check", &s(&[&path, "--text"])).unwrap();
        assert!(ok && text == "The project has no drawing sheet\n", "{text}");
        let (out, ok) = run("drawing-check", &s(&[&path])).unwrap();
        assert!(ok && out == "record,name,state,code,detail\n");
    }

    #[test]
    fn the_sheet_is_written_as_svg_pdf_dxf_and_the_bill_as_csv_and_never_over_a_file() {
        let path = project("40 mm", true, false);
        for (name, start) in [
            ("a.svg", "<?xml"),
            ("a.dxf", "0\nSECTION"),
            (
                "a.csv",
                "item,quantity,description,material\n1,1,plate,unspecified\n",
            ),
            ("a.pdf", "%PDF"),
        ] {
            let target = beside(&path, name);
            let (out, ok) = run("drawing-export", &s(&[&path, &target])).unwrap();
            assert!(ok, "{out}");
            assert!(out.starts_with(&format!("Wrote {target} (")), "{out}");
            let bytes = fs::read(&target).unwrap();
            assert!(String::from_utf8_lossy(&bytes).starts_with(start), "{name}");
            // never over a file that exists unless it is ordered
            let e = run("drawing-export", &s(&[&path, &target])).unwrap_err();
            assert!(e.contains("PHOENIX-CAD-040"), "{e}");
            assert!(run("drawing-export", &s(&[&path, &target, "--overwrite"])).is_ok());
            assert_eq!(
                fs::read(&target).unwrap(),
                bytes,
                "the same project gives the same bytes"
            );
        }
        // a sheet of another format, upright, at another scale
        let big = beside(&path, "big.svg");
        let (_, ok) = run(
            "drawing-export",
            &s(&[&path, &big, "--sheet", "A3", "--portrait", "--scale", "1:2"]),
        )
        .unwrap();
        assert!(ok);
        assert!(fs::read_to_string(&big)
            .unwrap()
            .contains("width=\"297mm\" height=\"420mm\""));
        // the options are checked
        assert!(run("drawing-export", &s(&[&path, &beside(&path, "x.txt")])).is_err());
        assert!(run(
            "drawing-export",
            &s(&[&path, &beside(&path, "y.svg"), "--scale", "1:0"])
        )
        .is_err());
        assert!(run("drawing-export", &s(&[&path])).is_err());
    }

    #[test]
    fn nothing_is_written_while_the_check_does_not_pass_unless_it_is_forced() {
        let path = project("40 mm", true, true);
        let target = beside(&path, "o.svg");
        let (out, ok) = run("drawing-export", &s(&[&path, &target])).unwrap();
        assert!(!ok);
        assert!(
            out.contains("PHOENIX-CAD-055") && out.contains("1 problems in the drawing"),
            "{out}"
        );
        assert!(!Path::new(&target).exists());
        let (_, ok) = run("drawing-export", &s(&[&path, &target, "--force"])).unwrap();
        assert!(ok && Path::new(&target).exists());
    }

    #[test]
    fn a_project_without_a_sheet_is_exported_only_when_one_is_made_for_it() {
        let path = project("40 mm", false, false);
        let target = beside(&path, "n.svg");
        let e = run("drawing-export", &s(&[&path, &target])).unwrap_err();
        assert!(e.contains("--create"), "{e}");
        let (_, ok) = run("drawing-export", &s(&[&path, &target, "--create"])).unwrap();
        assert!(ok);
        let svg = fs::read_to_string(&target).unwrap();
        assert!(svg.contains("layer-OUTLINE") && svg.contains("layer-HIDDEN"));
        // the project file was not touched
        let doc = Document::from_bytes(&fs::read(&path).unwrap(), 1 << 20).unwrap();
        assert!(doc.get("drawing").is_none());
    }

    #[test]
    fn what_cannot_be_read_is_an_error_and_not_a_verdict() {
        assert!(run("drawing-check", &s(&[])).is_err());
        assert!(run("drawing-check", &s(&["no-such-file.phoenixproj"])).is_err());
        assert!(run("drawing-check", &s(&["a", "b"])).is_err());
        assert!(run("drawing-check", &s(&["--nothing"])).is_err());
        assert!(run("drawing-check", &s(&["a", "--lang", "xx"])).is_err());
        assert!(run("drawing-other", &s(&[])).is_err());
        let path = project("40 mm", true, false);
        let dir = Path::new(&path).parent().unwrap().to_path_buf();
        let bad = dir.join("bad.phoenixproj");
        fs::write(&bad, b"not a project").unwrap();
        assert!(run("drawing-check", &s(&[&bad.to_string_lossy()])).is_err());
        // a drawing that is not one
        let mut doc = Document::from_bytes(&fs::read(&path).unwrap(), 1 << 20).unwrap();
        doc.set("drawing", false, b"drawing 1\nnonsense\n".to_vec())
            .unwrap();
        let broken = dir.join("broken.phoenixproj");
        fs::write(&broken, doc.to_bytes()).unwrap();
        let e = run("drawing-check", &s(&[&broken.to_string_lossy()])).unwrap_err();
        assert!(e.contains("PHOENIX-CAD-054"), "{e}");
        // a settings file that is not one
        let cfg = dir.join("bad.cfg");
        fs::write(&cfg, "[sheet]\nmax_views = 0\n").unwrap();
        let e = run(
            "drawing-check",
            &s(&[&path, "--config", &cfg.to_string_lossy()]),
        )
        .unwrap_err();
        assert!(e.contains("Settings not valid"), "{e}");
        // a settings file with another format still checks the sheet
        let ok_cfg = dir.join("b5.cfg");
        fs::write(&ok_cfg, "[sheet]\nformat.B5 = 176 mm x 250 mm\n").unwrap();
        let (_, ok) = run(
            "drawing-check",
            &s(&[&path, "--config", &ok_cfg.to_string_lossy()]),
        )
        .unwrap();
        assert!(ok);
    }
}
