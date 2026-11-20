//! The commands that talk to the EH3D server: `server-status`, `server-list`, `server-save`, `server-load`, `server-report` and `server-check`. They read the `[server]`
//! section of the settings file given with `--config` (with no address, or with no file, nothing is sent anywhere), print their answer in the language of
//! `--lang` (English without it) and never touch a local file unless they were asked to: `server-load` writes the project it read only to the file given
//! with `--out`, and not over a file that exists unless `--overwrite` is given.
//!
//! The token is the one of the settings file; without one, `--user NAME` logs in with the password in the variable [`PASSWORD_VARIABLE`] (a password is
//! never taken from the command line, where other programs can read it).

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::lang::{fill, Lang, Msg};
use crate::project::Document;
use crate::server::Store;
use crate::servercad::{self, Answer, Issue};
use crate::store;

/// The variable that holds the password for `--user`.
pub const PASSWORD_VARIABLE: &str = "PHOENIX_CAD_SERVER_PASSWORD";

/// The commands of this module, as the line of the command gives them.
pub const COMMANDS: [&str; 7] = [
    "server-status",
    "server-list",
    "server-save",
    "server-load",
    "server-report",
    "server-check",
    "server-contract",
];

#[derive(Default)]
struct Options {
    lang: Option<Lang>,
    config: Option<PathBuf>,
    user: Option<String>,
    name: Option<String>,
    out: Option<PathBuf>,
    report: Option<String>,
    from: Option<PathBuf>,
    stored: Option<String>,
    kind: Option<String>,
    comment: String,
    overwrite: bool,
    metrics: bool,
    words: Vec<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |what: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs {what}"))
        };
        match arg.as_str() {
            "--lang" => {
                let code = value("a language code")?;
                o.lang = Some(Lang::from_code(&code).ok_or("unknown language code")?);
            }
            "--config" => o.config = Some(PathBuf::from(value("a settings file")?)),
            "--user" => o.user = Some(value("a user name")?),
            "--name" => o.name = Some(value("a project name")?),
            "--out" => o.out = Some(PathBuf::from(value("a file")?)),
            "--report" => o.report = Some(value("the name of the report")?),
            "--from" => o.from = Some(PathBuf::from(value("a file")?)),
            "--stored" => o.stored = Some(value("the name of a project of the server")?),
            "--kind" => o.kind = Some(value("check or report")?),
            "--comment" => o.comment = value("a text")?,
            "--overwrite" => o.overwrite = true,
            "--metrics" => o.metrics = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => o.words.push(other.to_string()),
        }
    }
    Ok(o)
}

fn settings_of(config: Option<&Path>) -> Result<Settings, String> {
    let Some(path) = config else {
        return Ok(Settings::defaults());
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Settings::with_user_config(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn connect(o: &Options, s: &Settings, lang: Lang) -> Result<Store, String> {
    let mut store = Store::connect(&s.server, lang, s.mechanical.max_bytes).map_err(|e| e.text)?;
    if let Some(warning) = store.plain_token_warning() {
        eprintln!("note: {warning}");
    }
    if let Some(user) = &o.user {
        let password = std::env::var(PASSWORD_VARIABLE)
            .map_err(|_| format!("{PASSWORD_VARIABLE} holds the password of {user}"))?;
        store.login_if_needed(user, &password).map_err(|e| e.text)?;
    }
    Ok(store)
}

fn one_word<'a>(o: &'a Options, what: &str) -> Result<&'a str, String> {
    match o.words.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("this command needs {what}")),
        _ => Err(format!("only one {what} can be given")),
    }
}

fn stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

/// Runs one of the [`COMMANDS`] with its arguments and gives the text to print.
pub fn run(command: &str, args: &[String]) -> Result<String, String> {
    let o = parse(args)?;
    let lang = o.lang.unwrap_or(Lang::En);
    let settings = settings_of(o.config.as_deref())?;
    if command == "server-contract" {
        return contract_text(&o, &settings);
    }
    let mut store = connect(&o, &settings, lang)?;
    store.set_contract(servercad::ContractOptions::from_settings(&settings));
    let failed = |e: crate::server::ServerError| e.text;
    match command {
        "server-status" => {
            if !o.words.is_empty() {
                return Err("server-status takes no arguments".to_string());
            }
            let h = store.health().map_err(failed)?;
            let mut out = format!(
                "{}\n",
                fill(Msg::ServerLink.tr(lang), &[&h.name, &h.version, &h.status])
            );
            if o.metrics {
                out.push_str(Msg::ServerMetricsHead.tr(lang));
                out.push('\n');
                out.push_str(&store.link_metrics());
            }
            Ok(out)
        }
        "server-list" => {
            let query = match o.words.as_slice() {
                [] => "",
                [q] => q,
                _ => return Err("only one text to look for can be given".to_string()),
            };
            let mut out = String::new();
            for p in store.projects(query).map_err(failed)? {
                out.push_str(&fill(
                    Msg::ServerProjectLine.tr(lang),
                    &[&p.id, &p.name, &p.files.to_string()],
                ));
                out.push('\n');
            }
            Ok(out)
        }
        "server-save" => {
            let file = PathBuf::from(one_word(&o, "project file")?);
            let name = o
                .name
                .clone()
                .or_else(|| stem(&file))
                .ok_or("the project needs a name: give --name")?;
            let bytes = store::load_limited(&file, settings.mechanical.max_bytes as u64)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let w = store.save(&name, &bytes, &o.comment).map_err(failed)?;
            let msg = if w.unchanged {
                Msg::ServerUnchanged
            } else {
                Msg::ServerSaved
            };
            Ok(format!(
                "{}\n",
                fill(msg.tr(lang), &[&name, &w.version.to_string()])
            ))
        }
        "server-load" => {
            let name = one_word(&o, "project name")?;
            let out = o.out.clone().ok_or("server-load needs --out FILE")?;
            if out.exists() && !o.overwrite {
                return Err(format!(
                    "{} exists: give --overwrite to write over it",
                    out.display()
                ));
            }
            let bytes = store.load(name).map_err(failed)?;
            store::save_atomic(&out, &bytes, settings.backups).map_err(|e| e.to_string())?;
            Ok(format!(
                "{}\n",
                fill(
                    Msg::ServerLoaded.tr(lang),
                    &[name, &bytes.len().to_string()]
                )
            ))
        }
        "server-report" => {
            let name = one_word(&o, "project name")?;
            let from = o.from.clone().ok_or("server-report needs --from FILE")?;
            let report = match (&o.report, stem_with_extension(&from)) {
                (Some(r), _) => r.clone(),
                (None, Some(r)) => r,
                (None, None) => return Err("the report needs a name: give --report".to_string()),
            };
            let size = fs::metadata(&from)
                .map_err(|e| format!("{}: {e}", from.display()))?
                .len();
            if size > settings.mechanical.max_bytes as u64 {
                return Err(format!(
                    "{}: larger than {} bytes",
                    from.display(),
                    settings.mechanical.max_bytes
                ));
            }
            let text = fs::read_to_string(&from).map_err(|e| format!("{}: {e}", from.display()))?;
            let w = store
                .send_report(name, &report, &text, &o.comment)
                .map_err(failed)?;
            Ok(format!(
                "{}\n",
                fill(
                    Msg::ServerReportSent.tr(lang),
                    &[&w.file, name, &w.version.to_string()]
                )
            ))
        }
        "server-check" => {
            let kind = o.kind.clone().unwrap_or_else(|| "check".to_string());
            if !matches!(kind.as_str(), "check" | "report") {
                return Err("--kind is check or report".to_string());
            }
            let (name, project) = match (&o.stored, o.words.as_slice()) {
                (Some(name), []) => (name.clone(), store.load_contract(name).map_err(failed)?),
                (Some(_), _) => {
                    return Err("give a project file or --stored NAME, not both".to_string())
                }
                (None, _) => {
                    let file = PathBuf::from(one_word(&o, "project file")?);
                    let name = o
                        .name
                        .clone()
                        .or_else(|| stem(&file))
                        .ok_or("the project needs a name: give --name")?;
                    let bytes = store::load_limited(&file, settings.mechanical.max_bytes as u64)
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    let doc = Document::from_bytes(&bytes, settings.mechanical.max_bytes)
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    let options = servercad::ContractOptions::from_settings(&settings);
                    let text = servercad::project_json_with(&doc, &name, &options)
                        .map_err(|e| e.to_string())?;
                    (name, text)
                }
            };
            let answer = store.station(&kind, &name, &project).map_err(failed)?;
            Ok(describe(lang, &name, &answer))
        }
        other => Err(format!("{other}: not a command of the server")),
    }
}

/// `server-contract PROJECT [--name NAME]`: the project file as the station reads it (`project.json`), with the features of the enclosure and the title
/// block the settings ask for (`[contract]`), printed as it would be sent. It asks nothing of the network, so it works with no address and no server.
fn contract_text(o: &Options, settings: &Settings) -> Result<String, String> {
    let file = PathBuf::from(one_word(o, "project file")?);
    let name = o
        .name
        .clone()
        .or_else(|| stem(&file))
        .ok_or("the project needs a name: give --name")?;
    let bytes = store::load_limited(&file, settings.mechanical.max_bytes as u64)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let doc = Document::from_bytes(&bytes, settings.mechanical.max_bytes)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let options = servercad::ContractOptions::from_settings(settings);
    let text = servercad::project_json_with(&doc, &name, &options).map_err(|e| e.to_string())?;
    Ok(format!("{text}\n"))
}

/// The sentence of an issue in the language: the text of its code with the id it is about, or the
/// sentence the station gave when the code is one this program does not know.
pub fn issue_text(lang: Lang, issue: &Issue) -> String {
    let known = match issue.code.as_str() {
        "bad_schema" => Some(Msg::ServerIssueBadSchema),
        "unknown_units" => Some(Msg::ServerIssueUnknownUnits),
        "missing_id" => Some(Msg::ServerIssueMissingId),
        "duplicate_id" => Some(Msg::ServerIssueDuplicateId),
        "missing_ref" => Some(Msg::ServerIssueMissingRef),
        "unknown_kind" => Some(Msg::ServerIssueUnknownKind),
        "empty_part" => Some(Msg::ServerIssueEmptyPart),
        "bad_titleblock" => Some(Msg::ServerIssueBadTitleBlock),
        _ => None,
    };
    match known {
        Some(m) => fill(m.tr(lang), &[&issue.id]),
        None => issue.message.clone(),
    }
}

/// What the station answered, in the language: the verdict with its counts, one line for each
/// thing it found, the counts of a report and a line that says the evidence is a simulator.
pub fn describe(lang: Lang, name: &str, answer: &Answer) -> String {
    let v = &answer.verdict;
    let word = if v.pass {
        Msg::ServerPass
    } else {
        Msg::ServerFail
    };
    let mut out = format!(
        "{}\n",
        fill(
            Msg::ServerCheckHead.tr(lang),
            &[
                name,
                word.tr(lang),
                &v.errors.to_string(),
                &v.warnings.to_string()
            ]
        )
    );
    for i in &v.issues {
        let severity = if i.severity == "error" {
            Msg::ServerSeverityError
        } else {
            Msg::ServerSeverityWarning
        };
        out.push_str(&format!(
            "  {}: {}\n",
            severity.tr(lang),
            issue_text(lang, i)
        ));
    }
    if let Some(c) = &answer.counts {
        out.push_str(&fill(
            Msg::ServerReportCounts.tr(lang),
            &[
                &c.parts.to_string(),
                &c.sketches.to_string(),
                &c.features.to_string(),
            ],
        ));
        out.push('\n');
    }
    if answer.evidence == "simulator" {
        out.push_str(Msg::ServerSimulator.tr(lang));
        out.push('\n');
    }
    out
}

fn stem_with_extension(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverfake::{serve, MemoryStore};

    const BOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixproj");

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "phoenix-cad-server-cli-{name}-{}",
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

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    fn config_for(dir: &Scratch, address: &str, token: &str) -> String {
        let path = dir.file("settings.cfg");
        fs::write(
            &path,
            format!(
                "[server]\nserver_address = {address}\nserver_token = {token}\nserver_retries = 0\nserver_timeout_s = 5\n"
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn with_no_address_every_command_says_so_in_the_language_asked_for() {
        for lang in Lang::ALL {
            // the command that prints the contract asks nothing of the network, so it has no address to miss
            for command in COMMANDS.into_iter().filter(|c| *c != "server-contract") {
                let e = run(command, &words(&["--lang", lang.code()])).unwrap_err();
                assert_eq!(
                    e,
                    Msg::ServerNotSet.tr(lang),
                    "{command} in {}",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn a_project_goes_to_the_server_and_comes_back_through_the_commands() {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("cycle");
        let cfg = config_for(&dir, &served.address, "tok");
        let with = |list: Vec<&str>| {
            let mut all = words(&list);
            all.extend(words(&["--config", cfg.as_str()]));
            all
        };
        let out = run("server-status", &with(vec![])).unwrap();
        assert_eq!(out, "Server eh3d-server 0.0.0-memory: ok\n");
        assert_eq!(run("server-list", &with(vec![])).unwrap(), "");
        let saved = run("server-save", &with(vec![BOX, "--comment", "first"])).unwrap();
        assert_eq!(saved, "box saved on the server (version 1)\n");
        let again = run("server-save", &with(vec![BOX])).unwrap();
        assert_eq!(
            again,
            "box is already on the server as version 1; nothing was written\n"
        );
        let renamed = run("server-save", &with(vec![BOX, "--name", "Box two"])).unwrap();
        assert_eq!(renamed, "Box two saved on the server (version 1)\n");
        let list = run("server-list", &with(vec!["BOX"])).unwrap();
        assert_eq!(list, "p-0002  Box two  (2 files)\np-0001  box  (2 files)\n");
        let target = dir.file("back.phoenixproj");
        let loaded = run("server-load", &with(vec!["box", "--out", target.as_str()])).unwrap();
        let size = fs::read(BOX).unwrap().len();
        assert_eq!(loaded, format!("box read from the server ({size} bytes)\n"));
        assert_eq!(fs::read(&target).unwrap(), fs::read(BOX).unwrap());
        // a file that exists is not written over unless it was ordered
        let e = run("server-load", &with(vec!["box", "--out", target.as_str()])).unwrap_err();
        assert!(e.contains("--overwrite"), "{e}");
        assert!(run(
            "server-load",
            &with(vec!["box", "--out", target.as_str(), "--overwrite"])
        )
        .is_ok());
        let e = run(
            "server-load",
            &with(vec!["nothing", "--out", dir.file("n").as_str()]),
        )
        .unwrap_err();
        assert_eq!(e, "The server has no project named nothing");
        assert!(!Path::new(&dir.file("n")).exists());
    }

    #[test]
    fn a_report_is_sent_with_the_name_of_its_file_or_the_one_given() {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("report");
        let cfg = config_for(&dir, &served.address, "tok");
        let report = dir.file("parts.csv");
        fs::write(&report, "name,mass\nbar,12\n").unwrap();
        let args = words(&["box", "--from", report.as_str(), "--config", cfg.as_str()]);
        let sent = run("server-report", &args).unwrap();
        assert_eq!(
            sent,
            "reports/parts.csv sent to the project box of the server (version 1)\n"
        );
        assert_eq!(
            mem.file("box", "reports/parts.csv").unwrap(),
            "name,mass\nbar,12\n"
        );
        fs::write(&report, "name,mass\nbar,13\n").unwrap();
        let args = words(&[
            "box",
            "--from",
            report.as_str(),
            "--report",
            "mass.csv",
            "--lang",
            "es",
            "--config",
            cfg.as_str(),
        ]);
        let sent = run("server-report", &args).unwrap();
        assert_eq!(
            sent,
            "reports/mass.csv enviado al proyecto box del servidor (versi\u{f3}n 1)\n"
        );
        let args = words(&[
            "box",
            "--from",
            report.as_str(),
            "--report",
            "../x",
            "--config",
            cfg.as_str(),
        ]);
        assert!(run("server-report", &args).is_err());
        assert_eq!(mem.versions("box", "reports/parts.csv"), 1);
    }

    #[test]
    fn a_wrong_token_asks_for_a_login_and_the_password_never_comes_from_the_command_line() {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("login");
        let cfg = config_for(&dir, &served.address, "other");
        let e = run("server-list", &words(&["--config", cfg.as_str()])).unwrap_err();
        assert_eq!(e, sc_text("unauthorized"));
        // there is no option for the password
        let e = run(
            "server-list",
            &words(&["--password", "x", "--config", cfg.as_str()]),
        )
        .unwrap_err();
        assert_eq!(e, "unknown option --password");
        if std::env::var(PASSWORD_VARIABLE).is_err() {
            let cfg = config_for(&dir, &served.address, "");
            let e = run(
                "server-list",
                &words(&["--user", "juan", "--config", cfg.as_str()]),
            )
            .unwrap_err();
            assert!(e.contains(PASSWORD_VARIABLE), "{e}");
        }
    }

    fn sc_text(code: &str) -> String {
        eh3d_serverclient::text_for(code, eh3d_serverclient::Lang::En)
    }

    #[test]
    fn the_arguments_are_checked_before_anything_is_sent() {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("args");
        let cfg = config_for(&dir, &served.address, "tok");
        let with = |list: Vec<&str>| {
            let mut all = words(&list);
            all.extend(words(&["--config", cfg.as_str()]));
            all
        };
        for (command, list, why) in [
            ("server-save", vec![], "needs project file"),
            ("server-save", vec!["a", "b"], "only one project file"),
            ("server-load", vec!["x"], "--out"),
            ("server-load", vec!["--out", "f"], "needs project name"),
            ("server-report", vec!["x"], "--from"),
            ("server-status", vec!["extra"], "no arguments"),
            ("server-list", vec!["a", "b"], "only one text"),
            ("server-status", vec!["--lang", "xx"], "unknown language"),
            ("server-status", vec!["--lang"], "unknown language"),
            ("server-status", vec!["--wat"], "unknown option --wat"),
        ] {
            let e = run(command, &with(list)).unwrap_err();
            assert!(e.contains(why), "{command}: {e}");
        }
        assert!(mem.seen().is_empty());
        let e = run(
            "server-save",
            &with(vec![dir.file("missing.phoenixproj").as_str()]),
        )
        .unwrap_err();
        assert!(e.contains("missing.phoenixproj"), "{e}");
        let text = dir.file("not-a-project.phoenixproj");
        fs::write(&text, "hello").unwrap();
        let e = run("server-save", &with(vec![text.as_str()])).unwrap_err();
        assert!(e.contains("is not a project of this program"), "{e}");
        assert!(mem.project_names().is_empty());
    }

    fn config_with_station(dir: &Scratch, address: &str, machine: &str) -> String {
        let path = dir.file("station.cfg");
        fs::write(
            &path,
            format!(
                "[server]\nserver_address = {address}\nserver_token = tok\nserver_machine = {machine}\nserver_retries = 0\nserver_timeout_s = 5\n"
            ),
        )
        .unwrap();
        path
    }

    const PLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.phoenixproj");
    const ASSEMBLY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/assembly.phoenixproj");

    #[test]
    fn server_check_asks_the_station_and_tells_the_answer_in_the_language() {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("check");
        let cfg = config_with_station(&dir, &served.address, "cad1");
        let with = |list: Vec<&str>| {
            let mut all = words(&list);
            all.extend(words(&["--config", cfg.as_str()]));
            all
        };
        let out = run("server-check", &with(vec![PLATE])).unwrap();
        assert_eq!(
            out,
            "plate: pass (0 errors, 0 warnings)\nEvidence: simulator (the station checks the structure of the file; it has no geometry kernel)\n"
        );
        let out = run("server-check", &with(vec![PLATE, "--kind", "report"])).unwrap();
        assert!(out.contains("1 parts, 1 sketches, 1 features\n"), "{out}");
        // the assembly passes with a warning for each part that has no feature, told in Spanish
        let out = run("server-check", &with(vec![ASSEMBLY, "--lang", "es"])).unwrap();
        assert!(
            out.starts_with("assembly: correcto (0 errores, 6 avisos)\n"),
            "{out}"
        );
        assert!(
            out.contains(
                "  aviso: la pieza 018bcfe5-6800-7000-8000-000000001000 no tiene operaciones
"
            ),
            "{out}"
        );
    }

    #[test]
    fn server_check_can_take_the_project_the_last_save_wrote_and_refuses_what_does_not_fit() {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("stored");
        let cfg = config_with_station(&dir, &served.address, "cad1");
        let with = |list: Vec<&str>| {
            let mut all = words(&list);
            all.extend(words(&["--config", cfg.as_str()]));
            all
        };
        // nothing was saved yet
        let e = run("server-check", &with(vec!["--stored", "plate"])).unwrap_err();
        assert_eq!(e, fill(Msg::ServerNoProject.tr(Lang::En), &["plate"]));
        run("server-save", &with(vec![PLATE])).unwrap();
        let out = run(
            "server-check",
            &with(vec!["--stored", "plate", "--lang", "de"]),
        )
        .unwrap();
        assert!(
            out.starts_with("plate: bestanden (0 Fehler, 0 Warnungen)\n"),
            "{out}"
        );
        // a file and a name together, a kind that is not one, a file that is not a project
        assert!(run("server-check", &with(vec![PLATE, "--stored", "plate"])).is_err());
        let e = run("server-check", &with(vec![PLATE, "--kind", "export"])).unwrap_err();
        assert_eq!(e, "--kind is check or report");
        let e = run("server-check", &with(vec![])).unwrap_err();
        assert!(e.contains("project file"), "{e}");
        let junk = dir.file("junk.phoenixproj");
        fs::write(&junk, "not a project").unwrap();
        assert!(run("server-check", &with(vec![junk.as_str()])).is_err());
        // a station that is not set is told in the language
        let none = config_with_station(&dir, &served.address, "");
        for lang in Lang::ALL {
            let e = run(
                "server-check",
                &words(&[PLATE, "--config", none.as_str(), "--lang", lang.code()]),
            )
            .unwrap_err();
            assert_eq!(e, Msg::ServerNoMachine.tr(lang));
        }
    }

    #[test]
    fn every_code_of_the_station_has_its_sentence_in_the_eight_languages() {
        for lang in Lang::ALL {
            for code in [
                "bad_schema",
                "unknown_units",
                "missing_id",
                "duplicate_id",
                "missing_ref",
                "unknown_kind",
                "empty_part",
                "bad_titleblock",
            ] {
                let issue = Issue {
                    severity: "error".to_string(),
                    code: code.to_string(),
                    id: "THE-ID".to_string(),
                    message: "english from the server".to_string(),
                };
                let text = issue_text(lang, &issue);
                assert!(
                    !text.is_empty() && text != "english from the server",
                    "{code}"
                );
                if matches!(
                    code,
                    "duplicate_id" | "missing_ref" | "unknown_kind" | "empty_part"
                ) {
                    assert!(text.contains("THE-ID"), "{code} in {}: {text}", lang.code());
                }
            }
            // a code this program does not know keeps the sentence of the station
            let other = Issue {
                severity: "warning".to_string(),
                code: "new_code".to_string(),
                id: String::new(),
                message: "a new thing".to_string(),
            };
            assert_eq!(issue_text(lang, &other), "a new thing");
            for m in [
                Msg::ServerNoMachine,
                Msg::ServerJobFailed,
                Msg::ServerJobTimeout,
                Msg::ServerCheckHead,
                Msg::ServerPass,
                Msg::ServerFail,
                Msg::ServerSeverityError,
                Msg::ServerSeverityWarning,
                Msg::ServerReportCounts,
                Msg::ServerSimulator,
                Msg::ServerNoContract,
            ] {
                assert!(!m.tr(lang).is_empty());
            }
        }
    }

    fn contract_config(dir: &Scratch, address: &str, contract: &str) -> String {
        let path = dir.file("contract.cfg");
        fs::write(
            &path,
            format!(
                "[server]\nserver_address = {address}\nserver_token = tok\nserver_machine = cad1\nserver_retries = 0\nserver_timeout_s = 5\n[contract]\n{contract}\n[title_block]\nnumber = P-9\nauthor = Ana\n"
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn server_contract_prints_the_project_as_it_would_be_sent_with_no_server_at_all() {
        let dir = Scratch::new("contract");
        let kinds = |text: &str| -> Vec<String> {
            let json = eh3d_serverclient::Json::parse(text.trim_end()).unwrap();
            json.get("features")
                .and_then(eh3d_serverclient::Json::as_array)
                .unwrap()
                .iter()
                .filter_map(|f| f.get("kind").and_then(eh3d_serverclient::Json::as_str))
                .map(str::to_string)
                .collect()
        };
        // the shipped settings: the structure the station reads today, and no address needed
        let plain = run("server-contract", &words(&[BOX])).unwrap();
        assert!(plain.starts_with("{\"schema\":\"eh3d.cad.project/1\",\"name\":\"box\","));
        assert!(plain.ends_with("}\n") && !plain.contains("titleblock"));
        assert!(kinds(&plain).is_empty());
        // the section [contract] adds the features of the enclosure and the title block
        let cfg = contract_config(&dir, "", "enclosure_features = 1\ntitle_block = 1");
        let full = run("server-contract", &words(&[BOX, "--config", cfg.as_str()])).unwrap();
        let mut kinds = kinds(&full);
        kinds.sort();
        assert_eq!(
            kinds,
            [
                "edging",
                "face_hole",
                "face_hole",
                "lid",
                "post",
                "post",
                "post",
                "post"
            ]
        );
        assert!(full.contains("\"titleblock\":{\"organisation\":\"\",\"title\":\"box\",\"number\":\"P-9\",\"revision\":\"A\",\"author\":\"Ana\"}"), "{full}");
        // the name of the project is the one asked for
        let named = run(
            "server-contract",
            &words(&[BOX, "--name", "Front", "--config", cfg.as_str()]),
        )
        .unwrap();
        assert!(named.contains("\"name\":\"Front\"") && named.contains("\"title\":\"Front\""));
        // what is wrong with the arguments is told
        assert!(run("server-contract", &words(&[]))
            .unwrap_err()
            .contains("needs project file"));
        assert!(run("server-contract", &words(&[BOX, BOX]))
            .unwrap_err()
            .contains("only one"));
        let missing = dir.file("nothing.phoenixproj");
        assert!(run("server-contract", &words(&[missing.as_str()])).is_err());
    }

    #[test]
    fn the_settings_of_the_contract_decide_what_a_save_writes_and_the_station_warns_of_the_new_kinds(
    ) {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("contract-save");
        let off = config_with_station(&dir, &served.address, "cad1");
        run(
            "server-save",
            &words(&[BOX, "--name", "plain", "--config", off.as_str()]),
        )
        .unwrap();
        let stored = mem.file("plain", "project.json").unwrap();
        assert!(!stored.contains("face_hole") && !stored.contains("titleblock"));
        let on = contract_config(
            &dir,
            &served.address,
            "enclosure_features = 1\ntitle_block = 1",
        );
        run(
            "server-save",
            &words(&[BOX, "--name", "rich", "--config", on.as_str()]),
        )
        .unwrap();
        let stored = mem.file("rich", "project.json").unwrap();
        assert_eq!(stored.matches("\"kind\":\"face_hole\"").count(), 2);
        assert_eq!(stored.matches("\"kind\":\"post\"").count(), 4);
        assert!(stored.contains("\"number\":\"P-9\""));
        // the station passes it and says, once for each, which kinds it does not know yet
        let out = run(
            "server-check",
            &words(&["--stored", "rich", "--config", on.as_str()]),
        )
        .unwrap();
        assert!(
            out.starts_with("rich: pass (0 errors, 8 warnings)\n"),
            "{out}"
        );
        assert_eq!(
            out.matches("is of a kind the station does not know")
                .count(),
            8
        );
        let out = run(
            "server-check",
            &words(&["--stored", "plain", "--config", on.as_str()]),
        )
        .unwrap();
        assert!(
            out.starts_with("plain: pass (0 errors, 0 warnings)\n"),
            "{out}"
        );
        // the same file checked directly follows the settings too
        let out = run("server-check", &words(&[BOX, "--config", on.as_str()])).unwrap();
        assert!(
            out.starts_with("box: pass (0 errors, 8 warnings)\n"),
            "{out}"
        );
        let out = run("server-check", &words(&[BOX, "--config", off.as_str()])).unwrap();
        assert!(
            out.starts_with("box: pass (0 errors, 0 warnings)\n"),
            "{out}"
        );
    }

    #[test]
    fn server_status_tells_what_the_link_has_done_when_it_is_asked_to() {
        let mem = MemoryStore::new("tok");
        let served = serve(&mem).unwrap();
        let dir = Scratch::new("metrics");
        let cfg = config_for(&dir, &served.address, "tok");
        let plain = run("server-status", &words(&["--config", cfg.as_str()])).unwrap();
        assert_eq!(plain, "Server eh3d-server 0.0.0-memory: ok\n");
        let out = run(
            "server-status",
            &words(&["--metrics", "--config", cfg.as_str()]),
        )
        .unwrap();
        let (first, rest) = out.split_once('\n').unwrap();
        assert_eq!(first, "Server eh3d-server 0.0.0-memory: ok");
        let mut lines = rest.lines();
        assert_eq!(
            lines.next(),
            Some("What the link has done (times in microseconds):")
        );
        let metrics: Vec<(&str, &str)> = lines.filter_map(|l| l.split_once(" = ")).collect();
        let get = |k: &str| metrics.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
        // one request, one try, one answer, nothing lost or repeated
        for (key, value) in [
            ("requests", "1"),
            ("attempts", "1"),
            ("retries", "0"),
            ("replies", "1"),
            ("server_errors", "0"),
            ("failures", "0"),
            ("disconnects", "0"),
        ] {
            assert_eq!(get(key), Some(value), "{key}");
        }
        assert!(get("latency_p95_us").is_some_and(|v| v.parse::<u64>().is_ok()));
        // it holds no token and no address, and every language has the head
        assert!(!out.contains("tok") && !out.contains(&served.address));
        for lang in Lang::ALL {
            let out = run(
                "server-status",
                &words(&["--metrics", "--lang", lang.code(), "--config", cfg.as_str()]),
            )
            .unwrap();
            assert!(
                out.contains(Msg::ServerMetricsHead.tr(lang)),
                "{}",
                lang.code()
            );
        }
    }
}
