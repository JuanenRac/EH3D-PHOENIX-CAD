// SPDX-License-Identifier: GPL-3.0-or-later
//! The commands that look at the history of features of a project: `history-check` builds it and
//! reports every node, `history-graph` writes its graph for Graphviz.
//!
//! The exit code of `history-check` is the one of every check of the suite: 0 when every node was
//! built, 2 when the verification did not pass (a node failed or inherits a failure), and 1 when
//! the command could not run (a file that cannot be read, a history that is not one). The reports
//! use the language of `--lang` (English without it); the comma separated values never change with
//! the language.

use std::fs;
use std::path::Path;

use crate::config::Settings;
use crate::export::write_bytes;
use crate::featuredag::{BuildState, FeatureDag, HistorySettings};
use crate::glb;
use crate::lang::{fill, Lang, Msg};
use crate::project::Document;
use crate::tools_cli::Args;
use crate::uiconfig::UiSettings;
use crate::window::error_text;

/// The commands of this module, for the program to know which words are theirs.
pub const COMMANDS: [&str; 2] = ["history-check", "history-graph"];

/// The largest project file that these commands read.
const MAX_PROJECT_BYTES: usize = 256 << 20;

/// Runs one of the commands of [`COMMANDS`] and returns what it prints and whether the check
/// passed.
pub fn run(command: &str, args: &[String]) -> Result<(String, bool), String> {
    match command {
        "history-check" => check(args),
        "history-graph" => graph(args),
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
    let a = Args::parse(args, &["--config", "--glb"], &["--text", "--overwrite"])?;
    let (dag, lib, ui) = open(&a, a.lang)?;
    if a.has("--overwrite") && a.value("--glb").is_none() {
        return Err("--overwrite is for --glb".to_owned());
    }
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
}
