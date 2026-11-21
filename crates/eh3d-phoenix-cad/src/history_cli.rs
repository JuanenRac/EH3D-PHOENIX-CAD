// SPDX-License-Identifier: GPL-3.0-or-later
//! The commands that look at the history of features of a project: `history-check` builds it and
//! reports every node (and can write the solid as a glTF or an STL file), `history-graph` writes
//! its graph for Graphviz, `history-measure` measures the solid (volume, surface, centre of mass,
//! box, thinnest walls) and `sketch-points` places points typed as text.
//!
//! The exit code of `history-check` is the one of every check of the suite: 0 when every node was
//! built, 2 when the verification did not pass (a node failed or inherits a failure), and 1 when
//! the command could not run (a file that cannot be read, a history that is not one). The reports
//! use the language of `--lang` (English without it); the comma separated values never change with
//! the language.

use std::fs;
use std::path::Path;

use crate::config::Settings;
use crate::coordentry;
use crate::export::write_bytes;
use crate::featuredag::{BuildState, FeatureDag, HistorySettings};
use crate::glb;
use crate::lang::{fill, Lang, Msg};
use crate::mesh::number;
use crate::prism::Solid;
use crate::project::Document;
use crate::solidquery;
use crate::stlsolid;
use crate::tools_cli::Args;
use crate::uiconfig::UiSettings;
use crate::window::error_text;

/// The commands of this module, for the program to know which words are theirs.
pub const COMMANDS: [&str; 4] = [
    "history-check",
    "history-graph",
    "history-measure",
    "sketch-points",
];

/// The largest project file that these commands read.
const MAX_PROJECT_BYTES: usize = 256 << 20;

/// Runs one of the commands of [`COMMANDS`] and returns what it prints and whether the check
/// passed.
pub fn run(command: &str, args: &[String]) -> Result<(String, bool), String> {
    match command {
        "history-check" => check(args),
        "history-graph" => graph(args),
        "history-measure" => measure(args),
        "sketch-points" => points(args),
        other => Err(format!("{other}: unknown command (see help)")),
    }
}

/// The settings of the library and of the history, with the keys of a user file over the
/// defaults.
fn settings(config: Option<&str>) -> Result<(Settings, HistorySettings, UiSettings), String> {
    let Some(path) = config else {
        return Ok((
            Settings::defaults(),
            HistorySettings::defaults(),
            UiSettings::defaults(),
        ));
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let lib = Settings::with_user_config(&text).map_err(|e| format!("{path}: {e}"))?;
    let hist = HistorySettings::with_user_config(&text).map_err(|e| format!("{path}: {e}"))?;
    let ui = UiSettings::with_user_config(&text).map_err(|e| format!("{path}: {e}"))?;
    Ok((lib, hist, ui))
}

/// The history of the project named, or `None` when it has none.
fn open(args: &Args, lang: Lang) -> Result<(Option<FeatureDag>, Settings, UiSettings), String> {
    let file = args.only_word("project file")?;
    let (lib, hist, ui) = settings(args.value("--config"))?;
    let bytes = fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let doc = Document::from_bytes(&bytes, MAX_PROJECT_BYTES)
        .map_err(|e| format!("{file}: {}", error_text(&e, lang)))?;
    let dag = doc
        .history(hist, &lib)
        .map_err(|e| format!("{file}: {}", error_text(&e, lang)))?;
    Ok((dag, lib, ui))
}

fn state_word(s: BuildState) -> &'static str {
    match s {
        BuildState::Computed => "built",
        BuildState::Reused => "reused",
        BuildState::Suppressed => "suppressed",
        BuildState::Failed => "failed",
        BuildState::Inherited => "inherited",
    }
}

fn csv(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn check(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &["--config", "--glb", "--stl", "--export"],
        &["--text", "--overwrite"],
    )?;
    let (dag, lib, ui) = open(&a, a.lang)?;
    if a.has("--overwrite")
        && a.value("--glb").is_none()
        && a.value("--stl").is_none()
        && a.value("--export").is_none()
    {
        return Err("--overwrite is for --glb, --stl and --export".to_owned());
    }
    let export_format = match a.value("--export") {
        None => None,
        Some(target) => Some(stlsolid::FileFormat::from_name(target).ok_or_else(|| {
            "--export: the name ends in .stl, .obj, .3mf, .ply or .off".to_owned()
        })?),
    };
    let Some(mut dag) = dag else {
        return if a.has("--text") {
            Ok((format!("{}\n", Msg::FeatNone.tr(a.lang)), true))
        } else {
            Ok(("node,name,state,code,detail\n".to_owned(), true))
        };
    };
    let rebuild = dag.rebuild(&lib).map_err(|e| error_text(&e, a.lang))?;
    let mut out = String::new();
    if let Some(target) = a.value("--glb") {
        let solid = dag
            .result()
            .ok_or_else(|| Msg::FeatNoResult.tr(a.lang).to_owned())?;
        let bytes = glb::solid_to_glb(solid, "history", lib.mesh.max_triangles)
            .map_err(|e| error_text(&e, a.lang))?;
        write_bytes(
            Path::new(target),
            &bytes,
            a.has("--overwrite"),
            None,
            ui.export.max_kib,
        )
        .map_err(|e| error_text(&e, a.lang))?;
        out.push_str(&fill(
            Msg::MeshWrote.tr(a.lang),
            &["1", &(solid.prisms().len() * 12).to_string(), target],
        ));
        out.push('\n');
    }
    if let Some(target) = a.value("--stl") {
        let solid = dag
            .result()
            .ok_or_else(|| Msg::FeatNoResult.tr(a.lang).to_owned())?;
        let bytes = stlsolid::solid_to_stl(solid, lib.mesh.max_triangles, &lib_limits(&dag))
            .map_err(|e| error_text(&e, a.lang))?;
        let triangles = (bytes.len() - 84) / 50;
        write_bytes(
            Path::new(target),
            &bytes,
            a.has("--overwrite"),
            None,
            ui.export.max_kib,
        )
        .map_err(|e| error_text(&e, a.lang))?;
        out.push_str(&fill(
            Msg::MeshWrote.tr(a.lang),
            &["1", &triangles.to_string(), target],
        ));
        out.push('\n');
    }
    if let (Some(target), Some(format)) = (a.value("--export"), export_format) {
        let solid = dag
            .result()
            .ok_or_else(|| Msg::FeatNoResult.tr(a.lang).to_owned())?;
        let name = Path::new(target)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("history");
        let limits = lib_limits(&dag);
        let bytes = stlsolid::solid_to_file(solid, format, name, lib.mesh.max_triangles, &limits)
            .map_err(|e| error_text(&e, a.lang))?;
        let triangles = stlsolid::solid_to_mesh(solid, lib.mesh.max_triangles, &limits)
            .map_err(|e| error_text(&e, a.lang))?
            .triangles
            .len();
        write_bytes(
            Path::new(target),
            &bytes,
            a.has("--overwrite"),
            None,
            ui.export.max_kib,
        )
        .map_err(|e| error_text(&e, a.lang))?;
        out.push_str(&fill(
            Msg::MeshWrote.tr(a.lang),
            &["1", &triangles.to_string(), target],
        ));
        out.push('\n');
    }
    if a.has("--text") {
        out.push_str(&fill(
            Msg::FeatHead.tr(a.lang),
            &[
                &dag.len().to_string(),
                &rebuild.computed().to_string(),
                &rebuild.reused().to_string(),
                &rebuild.failed().to_string(),
                &rebuild.inherited().to_string(),
            ],
        ));
        out.push('\n');
        for r in &rebuild.reports {
            if matches!(r.state, BuildState::Failed | BuildState::Inherited) {
                out.push_str(&r.text(a.lang.code()));
                out.push('\n');
            }
        }
        match dag.result() {
            Some(s) => {
                out.push_str(&fill(
                    Msg::FeatResult.tr(a.lang),
                    &[
                        &s.prisms().len().to_string(),
                        &(s.volume_nm3() / 1_000_000_000_000_000_000).to_string(),
                    ],
                ));
                out.push('\n');
            }
            None if rebuild.is_clean() => {
                out.push_str(Msg::FeatNoResult.tr(a.lang));
                out.push('\n');
            }
            None => {}
        }
    } else {
        out.push_str("node,name,state,code,detail\n");
        for r in &rebuild.reports {
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                r.id,
                csv(&r.name),
                state_word(r.state),
                r.code.unwrap_or(""),
                csv(&r.detail)
            ));
        }
    }
    Ok((out, rebuild.is_clean()))
}

/// The limits of the operations between solids, from the settings of the history.
fn lib_limits(dag: &FeatureDag) -> crate::prism::Limits {
    dag.settings().limits()
}

/// `value` divided by ten to the `shift`, with six decimals, rounded half away from zero.
fn fixed(value: i128, shift: u32) -> String {
    let keep = shift.saturating_sub(6);
    let scale = 10i128.pow(keep);
    let half = scale / 2;
    let rounded = if value >= 0 {
        (value + half) / scale
    } else {
        (value - half) / scale
    };
    let sign = if rounded < 0 { "-" } else { "" };
    let magnitude = rounded.unsigned_abs();
    format!(
        "{sign}{}.{:06}",
        magnitude / 1_000_000,
        magnitude % 1_000_000
    )
}

/// A length in nanometres as millimetres with six decimals (exact).
fn mm(nm: i64) -> String {
    fixed(i128::from(nm), 6)
}

const AXES: [&str; 3] = ["x", "y", "z"];

/// The solid of the history of a project, built.
fn built_solid(a: &Args) -> Result<(Solid, crate::prism::Limits), String> {
    let (dag, lib, _) = open(a, a.lang)?;
    let Some(mut dag) = dag else {
        return Err(Msg::FeatNone.tr(a.lang).to_owned());
    };
    let rebuild = dag.rebuild(&lib).map_err(|e| error_text(&e, a.lang))?;
    if !rebuild.is_clean() {
        return Err(Msg::FeatNoResult.tr(a.lang).to_owned());
    }
    let limits = lib_limits(&dag);
    let solid = dag
        .result()
        .ok_or_else(|| Msg::FeatNoResult.tr(a.lang).to_owned())?
        .clone();
    Ok((solid, limits))
}

fn measure(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--config", "--min-wall", "--section"], &["--text"])?;
    let section = match a.value("--section") {
        None => None,
        Some(text) => {
            let (axis, at) = text
                .split_once('=')
                .ok_or_else(|| "--section is AXIS=POSITION, like z=5 mm".to_owned())?;
            let axis = AXES
                .iter()
                .position(|n| n.eq_ignore_ascii_case(axis.trim()))
                .ok_or_else(|| "--section: the axis is x, y or z".to_owned())?;
            let t = at.trim();
            let with_unit = if t.chars().any(char::is_alphabetic) {
                t.to_owned()
            } else {
                format!("{t} mm")
            };
            let l = crate::units::parse_length(&with_unit).map_err(|e| error_text(&e, a.lang))?;
            Some((axis, l.get()))
        }
    };
    let min_wall = match a.value("--min-wall") {
        None => None,
        Some(text) => {
            let t = text.trim();
            let with_unit = if t.chars().any(char::is_alphabetic) {
                t.to_owned()
            } else {
                format!("{t} mm")
            };
            let l = crate::units::parse_length(&with_unit).map_err(|e| error_text(&e, a.lang))?;
            if l.get() <= 0 {
                return Err("--min-wall must be more than zero".to_owned());
            }
            Some(l.get())
        }
    };
    let (solid, limits) = built_solid(&a)?;
    let text = a.has("--text");
    let fail = |e: crate::CadError| error_text(&e, a.lang);
    let mut out = String::new();
    if !text {
        out.push_str("quantity,axis,value,unit\n");
    }
    let Some((lo, hi)) = solid.bounds() else {
        if text {
            out.push_str(Msg::MeasEmpty.tr(a.lang));
            out.push('\n');
        }
        return Ok((out, true));
    };
    let area = solidquery::surface_area_nm2(&solid, &limits).map_err(fail)?;
    let centre = solidquery::centroid_nm(&solid)
        .map_err(fail)?
        .unwrap_or([0; 3]);
    let volume = solid.volume_nm3();
    let mut shadows = Vec::new();
    for (axis, name) in AXES.iter().enumerate() {
        shadows.push((
            *name,
            solidquery::shadow_area_nm2(&solid, axis, &limits).map_err(fail)?,
        ));
    }
    let sectioned = match section {
        Some((axis, at)) => Some((
            AXES[axis],
            at,
            solidquery::section_area_nm2(&solid, axis, at, &limits).map_err(fail)?,
        )),
        None => None,
    };
    let inertia = solidquery::inertia(&solid);
    let mut thin_rows = Vec::new();
    let mut thinnest = Vec::new();
    for (axis, name) in AXES.iter().enumerate() {
        if let Some(t) = solidquery::thinnest(&solid, axis, &limits).map_err(fail)? {
            thinnest.push((*name, t));
        }
        if let Some(limit) = min_wall {
            for w in solidquery::walls(&solid, axis, Some(limit), 1000, &limits).map_err(fail)? {
                thin_rows.push((*name, w));
            }
        }
    }
    if text {
        let lines = [
            fill(Msg::MeasVolume.tr(a.lang), &[&fixed(volume, 18)]),
            fill(Msg::MeasArea.tr(a.lang), &[&fixed(area, 12)]),
            fill(
                Msg::MeasCentre.tr(a.lang),
                &[&mm(centre[0]), &mm(centre[1]), &mm(centre[2])],
            ),
            fill(
                Msg::MeasBox.tr(a.lang),
                &[
                    &mm(lo[0]),
                    &mm(lo[1]),
                    &mm(lo[2]),
                    &mm(hi[0]),
                    &mm(hi[1]),
                    &mm(hi[2]),
                ],
            ),
        ];
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
        for (name, t) in &thinnest {
            out.push_str(&fill(
                Msg::MeasThinnest.tr(a.lang),
                &[name, &mm(t.run.thickness()), &mm(t.run.from), &mm(t.run.to)],
            ));
            out.push('\n');
        }
        for (name, area) in &shadows {
            out.push_str(&fill(
                Msg::MeasShadow.tr(a.lang),
                &[name, &fixed(*area, 12)],
            ));
            out.push('\n');
        }
        if let Some((name, at, area)) = &sectioned {
            out.push_str(&fill(
                Msg::MeasSection.tr(a.lang),
                &[name, &mm(*at), &fixed(*area, 12)],
            ));
            out.push('\n');
        }
        if let Some(i) = &inertia {
            let t = &i.tensor;
            out.push_str(&fill(
                Msg::MeasInertia.tr(a.lang),
                &[
                    &number(t[0][0]),
                    &number(t[1][1]),
                    &number(t[2][2]),
                    &number(t[0][1]),
                    &number(t[1][2]),
                    &number(t[2][0]),
                ],
            ));
            out.push('\n');
        }
        if let Some(limit) = min_wall {
            if thin_rows.is_empty() {
                out.push_str(&fill(Msg::MeasNoWalls.tr(a.lang), &[&mm(limit)]));
                out.push('\n');
            }
            for (name, w) in &thin_rows {
                out.push_str(&fill(
                    Msg::MeasWall.tr(a.lang),
                    &[
                        &mm(limit),
                        name,
                        &mm(w.run.thickness()),
                        &mm(w.run.from),
                        &mm(w.run.to),
                        &mm(w.low[0]),
                        &mm(w.high[0]),
                        &mm(w.low[1]),
                        &mm(w.high[1]),
                    ],
                ));
                out.push('\n');
            }
        }
    } else {
        out.push_str(&format!("volume,,{volume},nm3\n"));
        out.push_str(&format!("surface,,{area},nm2\n"));
        for (i, name) in AXES.iter().enumerate() {
            out.push_str(&format!("centre,{name},{},nm\n", centre[i]));
        }
        for (i, name) in AXES.iter().enumerate() {
            out.push_str(&format!("box_low,{name},{},nm\n", lo[i]));
            out.push_str(&format!("box_high,{name},{},nm\n", hi[i]));
        }
        for (name, t) in &thinnest {
            out.push_str(&format!("thinnest,{name},{},nm\n", t.run.thickness()));
        }
        for (name, area) in &shadows {
            out.push_str(&format!("shadow,{name},{area},nm2\n"));
        }
        if let Some((name, at, area)) = &sectioned {
            out.push_str(&format!("section_at,{name},{at},nm\n"));
            out.push_str(&format!("section,{name},{area},nm2\n"));
        }
        if let Some(i) = &inertia {
            let t = &i.tensor;
            for (key, value) in [
                ("xx", t[0][0]),
                ("yy", t[1][1]),
                ("zz", t[2][2]),
                ("xy", t[0][1]),
                ("yz", t[1][2]),
                ("zx", t[2][0]),
            ] {
                out.push_str(&format!("inertia,{key},{},mm5\n", number(value)));
            }
        }
        for (name, w) in &thin_rows {
            out.push_str(&format!("thin_wall,{name},{},nm\n", w.run.thickness()));
        }
    }
    // a wall under the minimum is a verification that did not pass
    Ok((out, thin_rows.is_empty()))
}

fn points(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--start"], &["--text"])?;
    if a.words.is_empty() {
        return Err("this command needs at least one point".to_owned());
    }
    let start = match a.value("--start") {
        None => [0, 0],
        Some(text) => {
            let p = coordentry::resolve(&[text], [0, 0]).map_err(|e| error_text(&e, a.lang))?;
            p[0]
        }
    };
    let words: Vec<&str> = a.words.iter().map(String::as_str).collect();
    let placed = coordentry::resolve(&words, start).map_err(|e| error_text(&e, a.lang))?;
    let mut out = String::new();
    if !a.has("--text") {
        out.push_str("point,x_nm,y_nm\n");
    }
    for (i, p) in placed.iter().enumerate() {
        if a.has("--text") {
            out.push_str(&fill(
                Msg::PointRow.tr(a.lang),
                &[&(i + 1).to_string(), &mm(p[0]), &mm(p[1])],
            ));
        } else {
            out.push_str(&format!("{},{},{}", i + 1, p[0], p[1]));
        }
        out.push('\n');
    }
    Ok((out, true))
}

fn graph(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--config"], &[])?;
    let (dag, lib, _) = open(&a, a.lang)?;
    match dag {
        Some(d) => Ok((d.to_dot(&lib).map_err(|e| error_text(&e, a.lang))?, true)),
        None => Ok((format!("{}\n", Msg::FeatNone.tr(a.lang)), true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Id;
    use crate::featuredag::Op;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn id(n: u16) -> Id {
        Id::from_parts(1_700_000_000_000, n, u64::from(n) + 3).unwrap()
    }

    /// A project file with a history in a folder of its own for one test.
    fn project(size: &str) -> String {
        let c = Settings::defaults();
        let mut d = FeatureDag::new(HistorySettings::defaults());
        d.add_parameter(id(1), "side", size, &c).unwrap();
        d.add_feature(
            id(2),
            "cube",
            Op::Box,
            &[],
            &[
                ("x", "0 mm"),
                ("y", "0 mm"),
                ("z", "0 mm"),
                ("sx", "side"),
                ("sy", "side"),
                ("sz", "side"),
            ],
            &c,
        )
        .unwrap();
        let mut doc = Document::new();
        doc.set_history(&d).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "cad20_histcli_{}_{}",
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

    #[test]
    fn a_history_that_builds_passes_and_is_listed_node_by_node() {
        let path = project("10 mm");
        let (out, ok) = run("history-check", &s(&[&path])).unwrap();
        assert!(ok, "{out}");
        assert!(out.starts_with("node,name,state,code,detail\n"));
        assert!(
            out.contains(",side,built,,") && out.contains(",cube,built,,"),
            "{out}"
        );
        let (text, ok) = run("history-check", &s(&[&path, "--text"])).unwrap();
        assert!(ok);
        assert!(
            text.starts_with("2 nodes: 2 built, 0 reused, 0 failed, 0 inherit a failure\n"),
            "{text}"
        );
        assert!(text.contains("Result: 1 prisms, volume 1000 mm3"), "{text}");
        let (de, _) = run("history-check", &s(&[&path, "--text", "--lang", "de"])).unwrap();
        assert!(de.contains("Knoten") && de.contains("Ergebnis"), "{de}");
    }

    #[test]
    fn a_history_with_a_node_that_fails_does_not_pass_and_says_which_in_the_language() {
        let path = project("0 mm");
        let (out, ok) = run("history-check", &s(&[&path])).unwrap();
        assert!(!ok);
        assert!(out.contains(",cube,failed,PHOENIX-CAD-052,"), "{out}");
        let (text, ok) = run("history-check", &s(&[&path, "--text", "--lang", "es"])).unwrap();
        assert!(!ok);
        assert!(
            text.contains("cube: PHOENIX-CAD-052 No se pudo construir la operación"),
            "{text}"
        );
        assert!(text.contains("1 con fallo"), "{text}");
    }

    #[test]
    fn the_graph_is_written_and_a_project_without_a_history_is_said_so() {
        let path = project("5 mm");
        let (dot, ok) = run("history-graph", &s(&[&path])).unwrap();
        assert!(
            ok && dot.starts_with("digraph") && dot.contains("cube"),
            "{dot}"
        );
        // a project that is only an envelope
        let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
        let plain = dir.join("plain.phoenixproj");
        fs::write(&plain, Document::new().to_bytes()).unwrap();
        let plain = plain.to_string_lossy().into_owned();
        let (text, ok) = run("history-check", &s(&[&plain, "--text"])).unwrap();
        assert!(ok && text.contains("no history of features"), "{text}");
        let (csv, ok) = run("history-check", &s(&[&plain])).unwrap();
        assert!(ok && csv == "node,name,state,code,detail\n");
    }

    #[test]
    fn the_result_is_written_as_a_glb_file_and_never_over_a_file_that_exists() {
        let path = project("10 mm");
        let out = std::path::Path::new(&path).with_file_name("out.glb");
        let target = out.to_string_lossy().into_owned();
        let (text, ok) = run("history-check", &s(&[&path, "--text", "--glb", &target])).unwrap();
        assert!(ok, "{text}");
        assert!(
            text.starts_with("Wrote 1 solid(s) and 12 triangles to"),
            "{text}"
        );
        let info = crate::glb::check_glb(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(info.triangles, 12);
        assert!((info.volume_m3 - 1.0e-6).abs() < 1.0e-11);
        // a file that exists stays as it was unless --overwrite is given
        let e = run("history-check", &s(&[&path, "--glb", &target])).unwrap_err();
        assert!(e.contains("PHOENIX-CAD-040"), "{e}");
        assert!(run(
            "history-check",
            &s(&[&path, "--glb", &target, "--overwrite"])
        )
        .is_ok());
        assert!(run("history-check", &s(&[&path, "--overwrite"])).is_err());
        // a history with no result writes nothing
        let failing = project("0 mm");
        let none = std::path::Path::new(&failing).with_file_name("none.glb");
        assert!(run(
            "history-check",
            &s(&[&failing, "--glb", &none.to_string_lossy()])
        )
        .is_err());
        assert!(!none.exists());
    }

    #[test]
    fn what_cannot_be_read_is_an_error_and_not_a_verdict() {
        assert!(run("history-check", &s(&[])).is_err());
        assert!(run("history-check", &s(&["no-such-file.phoenixproj"])).is_err());
        assert!(run("history-check", &s(&["a", "b"])).is_err());
        assert!(run("history-check", &s(&["--nothing"])).is_err());
        assert!(run("history-graph", &s(&["a", "--lang", "xx"])).is_err());
        assert!(run("history-other", &s(&[])).is_err());
        let path = project("5 mm");
        let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
        let bad = dir.join("bad.phoenixproj");
        fs::write(&bad, b"not a project").unwrap();
        assert!(run("history-check", &s(&[&bad.to_string_lossy()])).is_err());
        // a settings file that is not one
        let cfg = dir.join("bad.cfg");
        fs::write(&cfg, "[history]\nmax_nodes = 0\n").unwrap();
        let e = run(
            "history-check",
            &s(&[&path, "--config", &cfg.to_string_lossy()]),
        )
        .unwrap_err();
        assert!(e.contains("Settings not valid"), "{e}");
    }

    /// A project whose result is a cube of 10 mm with a cavity of 6 mm whose left wall is
    /// `left` mm thick (and the right one 4 - `left`).
    fn hollow_project(left: &str) -> String {
        let c = Settings::defaults();
        let mut d = FeatureDag::new(HistorySettings::defaults());
        d.add_parameter(id(1), "left", left, &c).unwrap();
        d.add_feature(
            id(2),
            "block",
            Op::Box,
            &[],
            &[
                ("x", "0 mm"),
                ("y", "0 mm"),
                ("z", "0 mm"),
                ("sx", "10 mm"),
                ("sy", "10 mm"),
                ("sz", "10 mm"),
            ],
            &c,
        )
        .unwrap();
        d.add_feature(
            id(3),
            "cavity",
            Op::Box,
            &[],
            &[
                ("x", "left"),
                ("y", "2 mm"),
                ("z", "2 mm"),
                ("sx", "6 mm"),
                ("sy", "6 mm"),
                ("sz", "6 mm"),
            ],
            &c,
        )
        .unwrap();
        d.add_feature(id(4), "hollow", Op::Difference, &[id(2), id(3)], &[], &c)
            .unwrap();
        d.set_tip(Some(id(4)), &c).unwrap();
        let mut doc = Document::new();
        doc.set_history(&d).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "cad24_histcli_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.phoenixproj");
        fs::write(&path, doc.to_bytes()).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn the_measures_of_a_hollow_cube_are_the_ones_worked_out_by_hand() {
        let path = hollow_project("2 mm");
        let (csv, ok) = run("history-measure", &s(&[&path])).unwrap();
        assert!(ok, "{csv}");
        assert!(csv.starts_with("quantity,axis,value,unit\n"));
        // volume 1000 - 216 = 784 mm3, surface 600 + 216 = 816 mm2, centre in the middle
        assert!(
            csv.contains("\nvolume,,784000000000000000000,nm3\n"),
            "{csv}"
        );
        assert!(csv.contains("\nsurface,,816000000000000,nm2\n"), "{csv}");
        assert!(csv.contains("\ncentre,x,5000000,nm\n"), "{csv}");
        assert!(csv.contains("\nthinnest,y,2000000,nm\n"), "{csv}");
        assert!(csv.contains("\nbox_high,z,10000000,nm\n"), "{csv}");
        let (text, ok) = run("history-measure", &s(&[&path, "--text"])).unwrap();
        assert!(ok);
        assert!(
            text.starts_with("Volume: 784.000000 mm3\nSurface: 816.000000 mm2\n"),
            "{text}"
        );
        assert!(
            text.contains("Centre of mass: (5.000000, 5.000000, 5.000000) mm"),
            "{text}"
        );
        assert!(
            text.contains("Thinnest along x: 2.000000 mm, from 0.000000 to 2.000000 mm"),
            "{text}"
        );
        for (lang, word) in [
            ("es", "Superficie"),
            ("de", "Oberfläche"),
            ("fr", "Centre de masse"),
            ("it", "Baricentro"),
            ("ja", "重心"),
            ("zh", "质心"),
            ("pt", "Superfície"),
        ] {
            let (t, _) = run("history-measure", &s(&[&path, "--text", "--lang", lang])).unwrap();
            assert!(t.contains(word), "{lang}: {t}");
        }
    }

    #[test]
    fn a_wall_under_the_minimum_does_not_pass_and_is_reported_with_where() {
        // the left wall is 1 mm thick
        let path = hollow_project("1 mm");
        let (csv, ok) = run("history-measure", &s(&[&path, "--min-wall", "2"])).unwrap();
        assert!(!ok, "{csv}");
        assert!(csv.contains("\nthin_wall,x,1000000,nm\n"), "{csv}");
        let (text, ok) = run(
            "history-measure",
            &s(&[&path, "--text", "--min-wall", "2 mm"]),
        )
        .unwrap();
        assert!(!ok);
        assert!(
            text.contains(
                "Wall under 2.000000 mm along x: 1.000000 mm thick, from 0.000000 to 1.000000 mm, across 2.000000 to 8.000000 by 2.000000 to 8.000000 mm"
            ),
            "{text}"
        );
        let (es, _) = run(
            "history-measure",
            &s(&[&path, "--text", "--min-wall", "2", "--lang", "es"]),
        )
        .unwrap();
        assert!(es.contains("Pared de menos de 2.000000 mm"), "{es}");
        // with the limit at the thickness itself nothing is thinner
        let (text, ok) = run("history-measure", &s(&[&path, "--text", "--min-wall", "1"])).unwrap();
        assert!(ok && text.contains("No wall under 1.000000 mm"), "{text}");
        // a limit that is not a length, or not positive, could not run
        for bad in ["0", "-1", "abc", "2 km"] {
            assert!(
                run("history-measure", &s(&[&path, "--min-wall", bad])).is_err(),
                "{bad}"
            );
        }
        assert!(run("history-measure", &s(&[&path, "--min-wall"])).is_err());
    }

    #[test]
    fn the_solid_is_written_as_an_stl_file_that_reads_back_with_the_exact_volume() {
        let path = hollow_project("2 mm");
        let out = std::path::Path::new(&path).with_file_name("out.stl");
        let target = out.to_string_lossy().into_owned();
        let (text, ok) = run("history-check", &s(&[&path, "--text", "--stl", &target])).unwrap();
        assert!(ok, "{text}");
        // twelve rectangles, twenty-four triangles
        assert!(
            text.starts_with("Wrote 1 solid(s) and 24 triangles to"),
            "{text}"
        );
        let bytes = fs::read(&out).unwrap();
        assert_eq!(bytes.len(), 84 + 24 * 50);
        let tris = crate::mesh::read_stl(&bytes).unwrap();
        assert_eq!(tris.len(), 24);
        let volume: f64 = tris
            .iter()
            .map(|t| {
                (t[0] * (t[4] * t[8] - t[5] * t[7])
                    + t[1] * (t[5] * t[6] - t[3] * t[8])
                    + t[2] * (t[3] * t[7] - t[4] * t[6]))
                    / 6.0
            })
            .sum();
        assert!((volume - 784.0).abs() < 1e-6, "{volume}");
        // nothing is written over a file that exists unless it is ordered
        let e = run("history-check", &s(&[&path, "--stl", &target])).unwrap_err();
        assert!(!e.is_empty());
        assert_eq!(fs::read(&out).unwrap(), bytes);
        run(
            "history-check",
            &s(&[&path, "--stl", &target, "--overwrite"]),
        )
        .unwrap();
        assert!(run("history-check", &s(&[&path, "--overwrite"])).is_err());
    }

    #[test]
    fn the_measures_include_the_shadows_a_section_and_the_inertia() {
        let path = hollow_project("2 mm");
        let (csv, ok) = run("history-measure", &s(&[&path, "--section", "z=5"])).unwrap();
        assert!(ok, "{csv}");
        // the closed cavity does not show in any shadow: 10 x 10 mm
        for axis in ["x", "y", "z"] {
            assert!(
                csv.contains(&format!("\nshadow,{axis},100000000000000,nm2\n")),
                "{csv}"
            );
        }
        // through the middle the square of 10 mm less the cavity of 6 mm: 64 mm2
        assert!(csv.contains("\nsection_at,z,5000000,nm\n"), "{csv}");
        assert!(csv.contains("\nsection,z,64000000000000,nm2\n"), "{csv}");
        // a cube of 10 mm is 1e5 / 6 on the diagonal, the cavity of 6 mm takes 6^5 / 6 = 1296
        assert!(csv.contains("\ninertia,xx,15370.666667,mm5\n"), "{csv}");
        assert!(csv.contains("\ninertia,zz,15370.666667,mm5\n"), "{csv}");
        assert!(csv.contains("\ninertia,xy,0.000000,mm5\n"), "{csv}");
        // no section asked, none given
        let (plain, _) = run("history-measure", &s(&[&path])).unwrap();
        assert!(!plain.contains("section"), "{plain}");
        let (text, _) = run(
            "history-measure",
            &s(&[&path, "--section", "x=3 mm", "--text", "--lang", "de"]),
        )
        .unwrap();
        assert!(
            text.contains("Schatten entlang x: 100.000000 mm2"),
            "{text}"
        );
        assert!(
            text.contains("Schnitt quer zu x bei 3.000000 mm: 64.000000 mm2"),
            "{text}"
        );
        assert!(text.contains("Trägheit um den Schwerpunkt"), "{text}");
        let (es, _) = run("history-measure", &s(&[&path, "--text", "--lang", "es"])).unwrap();
        assert!(es.contains("Sombra a lo largo de z"), "{es}");
        // a section that is not AXIS=POSITION could not run
        for bad in ["", "z", "q=1", "z=", "z=abc", "=1"] {
            assert!(
                run("history-measure", &s(&[&path, "--section", bad])).is_err(),
                "{bad:?}"
            );
        }
        assert!(run("history-measure", &s(&[&path, "--section"])).is_err());
    }

    #[test]
    fn the_solid_is_exported_to_every_mesh_format_and_read_back_with_the_exact_volume() {
        let path = hollow_project("2 mm");
        let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
        for (n, ext) in ["stl", "obj", "3mf", "ply", "off", "ObJ"]
            .into_iter()
            .enumerate()
        {
            let out = dir.join(format!("solid{n}.{ext}"));
            let target = out.to_string_lossy().into_owned();
            let (text, ok) =
                run("history-check", &s(&[&path, "--text", "--export", &target])).unwrap();
            assert!(ok, "{text}");
            assert!(
                text.starts_with("Wrote 1 solid(s) and 24 triangles to"),
                "{ext}: {text}"
            );
            let bodies =
                crate::interchange::read_file_bytes(&target, &fs::read(&out).unwrap()).unwrap();
            assert_eq!(bodies.len(), 1, "{ext}");
            let volume = bodies[0].1.volume_nm3();
            assert!((volume - 784.0e18).abs() < 1.0e12, "{ext}: {volume}");
            // nothing is written over a file that exists unless it is ordered
            let before = fs::read(&out).unwrap();
            assert!(
                run("history-check", &s(&[&path, "--export", &target])).is_err(),
                "{ext}"
            );
            assert_eq!(fs::read(&out).unwrap(), before);
            run(
                "history-check",
                &s(&[&path, "--export", &target, "--overwrite"]),
            )
            .unwrap();
            assert_eq!(
                fs::read(&out).unwrap(),
                before,
                "{ext}: the same bytes again"
            );
        }
        // an extension that is not a mesh format could not run
        for bad in ["solid.step", "solid", "solid.txt"] {
            let target = dir.join(bad).to_string_lossy().into_owned();
            assert!(run("history-check", &s(&[&path, "--export", &target])).is_err());
            assert!(!dir.join(bad).exists());
        }
    }

    #[test]
    fn typed_points_are_placed_one_after_another() {
        let (csv, ok) = run("sketch-points", &s(&["0,0", "@10,0", "@10<90", "@-10,0"])).unwrap();
        assert!(ok);
        assert_eq!(
            csv,
            "point,x_nm,y_nm\n1,0,0\n2,10000000,0\n3,10000000,10000000\n4,0,10000000\n"
        );
        let (text, _) = run(
            "sketch-points",
            &s(&["--start", "1,1", "@5<30", "--text", "--lang", "de"]),
        )
        .unwrap();
        assert_eq!(text, "Punkt 1: (5.330127, 3.500000) mm\n");
        assert!(run("sketch-points", &s(&[])).is_err());
        assert!(run("sketch-points", &s(&["1,2,3"])).is_err());
        assert!(run("sketch-points", &s(&["--start", "@1,1", "0,0"])).is_ok());
    }
}
