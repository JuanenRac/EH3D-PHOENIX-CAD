//! The commands that talk to the EH3D server: `server-status`, `server-list`, `server-save`, `server-load` and `server-report`. They read the `[server]`
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
use crate::server::Store;
use crate::store;

/// The variable that holds the password for `--user`.
pub const PASSWORD_VARIABLE: &str = "PHOENIX_CAD_SERVER_PASSWORD";

/// The commands of this module, as the line of the command gives them.
pub const COMMANDS: [&str; 5] = [
    "server-status",
    "server-list",
    "server-save",
    "server-load",
    "server-report",
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
    comment: String,
    overwrite: bool,
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
            "--comment" => o.comment = value("a text")?,
            "--overwrite" => o.overwrite = true,
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
    let mut store = connect(&o, &settings, lang)?;
    let failed = |e: crate::server::ServerError| e.text;
    match command {
        "server-status" => {
            if !o.words.is_empty() {
                return Err("server-status takes no arguments".to_string());
            }
            let h = store.health().map_err(failed)?;
            Ok(format!(
                "{}\n",
                fill(Msg::ServerLink.tr(lang), &[&h.name, &h.version, &h.status])
            ))
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
        other => Err(format!("{other}: not a command of the server")),
    }
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
            for command in COMMANDS {
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
        assert_eq!(list, "p-0002  Box two  (1 files)\np-0001  box  (1 files)\n");
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
}
