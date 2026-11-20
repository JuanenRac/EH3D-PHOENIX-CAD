//! The commands that talk to the EH3D server, run as the program: the store is the one in memory of the library behind a socket of this computer, so
//! the plain HTTP of the program is tried too.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use eh3d_phoenix_cad::serverfake::{serve, MemoryStore};

const BIN: &str = env!("CARGO_BIN_EXE_eh3d-phoenix-cad");
const BOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixproj");

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .env_remove("PHOENIX_CAD_SERVER_PASSWORD")
        .output()
        .expect("the program runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "phoenix-cad-server-bin-{name}-{}",
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

#[test]
fn the_help_lists_the_commands_of_the_server_and_the_keys_of_its_section() {
    let (code, out, _) = run(&["help"]);
    assert_eq!(code, 0);
    for word in [
        "server-status",
        "server-list",
        "server-save",
        "server-load",
        "server-report",
        "server-check",
        "server-contract",
        "--metrics",
        "server_address",
        "PHOENIX_CAD_SERVER_PASSWORD",
    ] {
        assert!(out.contains(word), "{word}");
    }
    let (_, config, _) = run(&["config"]);
    assert!(config.contains("[server]"));
    assert!(config.contains("server_address ="));
    assert!(config.contains("server_token ="));
}

#[test]
fn without_a_server_in_the_settings_the_commands_fail_with_a_sentence_and_send_nothing() {
    let (code, out, err) = run(&["server-status"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(
        err.contains("The address of the server is not set"),
        "{err}"
    );
    let (code, _, err) = run(&["server-status", "--lang", "pt"]);
    assert_eq!(code, 1);
    assert!(
        err.contains("O endere\u{e7}o do servidor n\u{e3}o est\u{e1} definido"),
        "{err}"
    );
}

#[test]
fn a_project_is_saved_on_the_server_listed_and_read_back_by_the_program() {
    let mem = MemoryStore::new("bin-token");
    let served = serve(&mem).unwrap();
    let dir = Scratch::new("cycle");
    let cfg = dir.file("settings.cfg");
    fs::write(
        &cfg,
        format!(
            "[server]\nserver_address = {}\nserver_token = bin-token\nserver_retries = 0\n",
            served.address
        ),
    )
    .unwrap();
    let (code, out, err) = run(&["server-status", "--config", &cfg]);
    assert_eq!(
        (code, out.as_str()),
        (0, "Server eh3d-server 0.0.0-memory: ok\n"),
        "{err}"
    );
    let (code, out, err) = run(&["server-save", BOX, "--name", "bench box", "--config", &cfg]);
    assert_eq!(
        (code, out.as_str()),
        (0, "bench box saved on the server (version 1)\n"),
        "{err}"
    );
    let (code, out, _) = run(&["server-list", "--config", &cfg]);
    assert_eq!(code, 0);
    assert_eq!(out, "p-0001  bench box  (2 files)\n");
    let target = dir.file("back.phoenixproj");
    let (code, _, err) = run(&[
        "server-load",
        "bench box",
        "--out",
        &target,
        "--config",
        &cfg,
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fs::read(&target).unwrap(), fs::read(BOX).unwrap());
    // the token never appears in what the program prints
    assert!(!err.contains("bin-token"));
    // the program that reads the copy from the server opens it
    let (code, out, _) = run(&["check", &target]);
    assert!(code == 0 || code == 1);
    assert!(!out.is_empty());
    // a wrong token is a refusal with the text of the server's code, and exit 1
    fs::write(
        &cfg,
        format!(
            "[server]\nserver_address = {}\nserver_token = wrong\nserver_retries = 0\n",
            served.address
        ),
    )
    .unwrap();
    let (code, out, err) = run(&["server-list", "--config", &cfg]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(!err.contains("wrong"), "{err}");
    assert!(!err.is_empty());
}

const PLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.phoenixproj");

#[test]
fn the_station_checks_a_project_as_the_program_over_a_socket_and_tells_it_in_the_language_asked() {
    let mem = MemoryStore::new("tok");
    let served = serve(&mem).unwrap();
    let dir = Scratch::new("station");
    let cfg = dir.file("settings.cfg");
    fs::write(
        &cfg,
        format!(
            "[server]\nserver_address = {}\nserver_token = tok\nserver_machine = cad1\nserver_retries = 0\nserver_timeout_s = 5\n",
            served.address
        ),
    )
    .unwrap();
    let (code, out, err) = run(&["server-check", PLATE, "--config", cfg.as_str()]);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.starts_with("plate: pass (0 errors, 0 warnings)\n"),
        "{out}"
    );
    let (code, out, _) = run(&[
        "server-check",
        PLATE,
        "--kind",
        "report",
        "--lang",
        "fr",
        "--config",
        cfg.as_str(),
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("1 pièces, 1 esquisses, 1 fonctions"), "{out}");
    assert!(out.contains("Preuve : simulateur"), "{out}");
    // the project was not saved by checking it; saving it makes the second way possible
    let (code, _, err) = run(&[
        "server-check",
        "--stored",
        "plate",
        "--config",
        cfg.as_str(),
    ]);
    assert_eq!(code, 1);
    assert!(
        err.contains("The server has no project named plate"),
        "{err}"
    );
    let (code, _, _) = run(&["server-save", PLATE, "--config", cfg.as_str()]);
    assert_eq!(code, 0);
    assert!(mem.file("plate", "project.json").is_some());
    let (code, out, _) = run(&[
        "server-check",
        "--stored",
        "plate",
        "--lang",
        "it",
        "--config",
        cfg.as_str(),
    ]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with("plate: superato (0 errori, 0 avvisi)\n"),
        "{out}"
    );
    // no address, no machine
    let (code, _, err) = run(&["server-check", PLATE]);
    assert_eq!(code, 1);
    assert!(
        err.contains("The address of the server is not set"),
        "{err}"
    );
}
