//! The way to the EH3D server: a project is kept on the server and read back, and the reports of a project are sent to it. The address of the server, the
//! token of this program and the name of the machine are in the settings file (`[server]`), not in the code, and with no address nothing is sent anywhere:
//! the local files are all there is.
//!
//! The link itself is the one client of the suite (`eh3d-serverclient` of EH3D-COMMON): the same keys with the same ranges, a token that cannot be
//! printed by mistake, retries with growing pauses (a `POST` is never sent twice), the state of the link and the refusals of the server by their code in
//! the language of the person. What is this program's own is here: the project store of the server (`/api/v1/apps/phoenix-cad/projects`), the way a project is
//! found by its name, saved as a new version of its file and read back, and the reports written next to it.
//!
//! The server keeps the versions of every file and refuses a write over a file that exists unless it says which version it was made from (`If-Match`), so
//! two people never overwrite each other without knowing: [`Store::save`] reads the tag of the file first and sends it. The client of the suite sends text,
//! so a project is sent as text (the project file of this program is text); a file that holds other bytes is refused before anything is sent.
//!
//! Nothing here changes the local files: the project that is saved to the server is the bytes of a file, and the project that is read back is checked
//! (it must be a project file) before anyone writes it to a disk.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use eh3d_serverclient as sc;
use sc::{Json, Reply, Request, Transport, TransportError};

use crate::config::Config;
use crate::lang::{fill, Lang, Msg};
use crate::project::Document;
use crate::servercad::{self, Answer, CONTRACT_FILE};
use crate::CadError;

/// The name this program gives the server when it logs in, and the folder of its projects in the store.
pub const APP: &str = "phoenix-cad";
/// The file of the store that holds the project.
pub const PROJECT_FILE: &str = "project.phoenixproj";
/// The folder of the store where the reports of a project are written.
pub const REPORTS_FOLDER: &str = "reports/";
/// The longest name of a project or of a report.
pub const NAME_MAX: usize = 120;
/// The keys of the link, in the order of the settings file (all of them in the section `[server]`).
pub const KEYS: [&str; 7] = [
    "server_address",
    "server_token",
    "server_machine",
    "server_timeout_s",
    "server_retries",
    "server_mdns",
    "server_max_answer_mib",
];

/// The settings of the link from the keys `server.server_address`, `server.server_token`... of a settings file; every key must be there, and each one
/// is checked against its range by the client of the suite.
pub fn from_config(c: &Config) -> Result<sc::Settings, CadError> {
    let mut link = sc::Settings::default();
    for key in KEYS {
        let full = format!("server.{key}");
        let value = c
            .get(&full)
            .ok_or_else(|| CadError::Config(format!("{full} is missing")))?;
        link.set(key, value)
            .map_err(|e| CadError::Config(format!("server.{e}")))?;
    }
    Ok(link)
}

/// A failure of a call: what to tell the person, in their language, and whether the cure is a new login (the token is gone or was refused).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError {
    /// What to tell the person, in their language.
    pub text: String,
    /// The token is gone or was refused: the cure is a new login.
    pub needs_login: bool,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::error::Error for ServerError {}

/// A project of the server, as the list of the store gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// The id the server gave the project (letters, digits, `_` and `-`).
    pub id: String,
    /// The name of the project.
    pub name: String,
    /// How many files it holds.
    pub files: u64,
}

/// A file of a project of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    /// The name of the file in the project, with its folders.
    pub name: String,
    /// The number of its current version.
    pub version: u64,
    /// The tag of the file: the checksum of its current version, which the server wants back to write over it.
    pub tag: String,
    /// Its size in bytes.
    pub size: u64,
}

/// What a write to the store did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The id of the project on the server.
    pub project: String,
    /// The name of the file in the project.
    pub file: String,
    /// The number of its current version.
    pub version: u64,
    /// The bytes were those of the current version: nothing was written.
    pub unchanged: bool,
    /// The project did not exist and was made.
    pub created: bool,
}

/// Whether a name can go to the server as the name of a project or of a report: 1 to [`NAME_MAX`] characters and none of them a control character.
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= NAME_MAX
        && !name.chars().any(char::is_control)
}

/// The name of a report as a file of the folder of reports: letters, digits, `.`, `_` and `-` only, 1 to [`NAME_MAX`] characters, not starting with a
/// dot. `None` when the name is none of that.
pub fn report_file(name: &str) -> Option<String> {
    let ok = !name.is_empty()
        && name.len() <= NAME_MAX
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    ok.then(|| format!("{REPORTS_FOLDER}{name}"))
}

fn safe_segment(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A tag the server gave (a checksum): letters and digits, so that it cannot break a header line.
fn valid_tag(tag: &str) -> bool {
    (1..=128).contains(&tag.len()) && tag.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The mark that a path carries for [`HttpTransport`] to turn into the `If-Match` header; the server never sees it.
const TAG_MARK: &str = "&x_if_match=";

/// The path with the tag of the version a write is made from.
pub fn with_if_match(path: &str, tag: &str) -> String {
    format!("{path}{TAG_MARK}{tag}")
}

/// A path without the mark, and the tag that was in it.
pub fn split_if_match(path: &str) -> (&str, Option<&str>) {
    match path.rsplit_once(TAG_MARK) {
        Some((clean, tag)) => (clean, Some(tag)),
        None => (path, None),
    }
}

/// Plain HTTP/1.1, one request per connection, as the transport of the suite, with the two things the store needs and that one does not do: the
/// `If-Match` header of a write over a file (from the mark of [`with_if_match`]) and the type `application/octet-stream` for the bytes of a file.
pub struct HttpTransport {
    address: String,
}

impl HttpTransport {
    /// A transport to `host:port` (a leading `http://` is taken off).
    pub fn new(address: impl Into<String>) -> HttpTransport {
        HttpTransport {
            address: address.into(),
        }
    }
}

impl Transport for HttpTransport {
    fn send(&self, request: &Request) -> Result<Reply, TransportError> {
        let (path, tag) = split_if_match(&request.path);
        sc::transport::check_request_part("the path", path).map_err(TransportError::Connect)?;
        sc::transport::check_request_part("the address of the server", &self.address)
            .map_err(TransportError::Connect)?;
        if let Some(t) = request.bearer() {
            sc::transport::check_request_part("the token", t).map_err(TransportError::Connect)?;
        }
        if let Some(t) = tag {
            if !valid_tag(t) {
                return Err(TransportError::Connect(
                    "the tag of the version has a character that cannot be sent".to_string(),
                ));
            }
        }
        let authority = self
            .address
            .strip_prefix("http://")
            .unwrap_or(&self.address)
            .trim_end_matches('/')
            .to_string();
        let target = authority
            .to_socket_addrs()
            .map_err(|e| TransportError::Connect(format!("{authority}: {e}")))?
            .next()
            .ok_or_else(|| TransportError::Connect(format!("{authority}: no address")))?;
        let connect_limit = request.timeout.min(Duration::from_secs(5));
        let mut stream = TcpStream::connect_timeout(&target, connect_limit)
            .map_err(|e| TransportError::Connect(format!("{authority}: {e}")))?;
        let io = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                TransportError::Timeout
            }
            _ => TransportError::Io(e.to_string()),
        };
        stream.set_read_timeout(Some(request.timeout)).map_err(io)?;
        stream
            .set_write_timeout(Some(request.timeout))
            .map_err(io)?;
        let payload = request.body.clone().unwrap_or_default();
        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n",
            request.method, path, authority
        );
        if let Some(t) = request.bearer() {
            head.push_str(&format!("Authorization: Bearer {t}\r\n"));
        }
        if let Some(t) = tag {
            head.push_str(&format!("If-Match: \"{t}\"\r\n"));
        }
        if request.body.is_some() {
            let kind = if path.contains("/files/content") {
                "application/octet-stream"
            } else {
                "application/json"
            };
            head.push_str(&format!(
                "Content-Type: {kind}\r\nContent-Length: {}\r\n",
                payload.len()
            ));
        } else if request.method == "POST" || request.method == "PUT" {
            head.push_str("Content-Length: 0\r\n");
        }
        head.push_str("\r\n");
        head.push_str(&payload);
        stream.write_all(head.as_bytes()).map_err(io)?;
        let limit = request.max_answer_bytes;
        let mut raw = Vec::new();
        (&mut stream)
            .take(limit.saturating_add(1))
            .read_to_end(&mut raw)
            .map_err(io)?;
        if raw.len() as u64 > limit {
            return Err(TransportError::TooLarge);
        }
        sc::transport::parse_response(&raw)
    }
}

/// The language of the suite for the language of the window.
fn link_lang(lang: Lang) -> sc::Lang {
    sc::Lang::from_tag(lang.code()).unwrap_or(sc::Lang::En)
}

/// The project store of the server, through one client.
pub struct Store {
    client: sc::Client,
    lang: Lang,
    max_bytes: usize,
}

impl Store {
    /// A store over a client already made (a server in memory for the tests, a tunnel). `max_bytes` is the most a project file may hold.
    pub fn over(client: sc::Client, lang: Lang, max_bytes: usize) -> Store {
        Store {
            client,
            lang,
            max_bytes,
        }
    }

    /// A store over plain HTTP to the address of the settings; refused with a sentence in the language of the person while there is no address.
    pub fn connect(
        link: &sc::Settings,
        lang: Lang,
        max_bytes: usize,
    ) -> Result<Store, ServerError> {
        if !link.is_configured() {
            return Err(ServerError {
                text: Msg::ServerNotSet.tr(lang).to_string(),
                needs_login: false,
            });
        }
        let transport = HttpTransport::new(link.server_address.clone());
        Ok(Store::over(
            sc::Client::new(link.clone(), Box::new(transport)),
            lang,
            max_bytes,
        ))
    }

    /// The state of the link, for a line of the log.
    pub fn link(&self) -> sc::Link {
        self.client.link()
    }

    /// A sentence when the token would travel in the clear to a network that is not the person's own.
    pub fn plain_token_warning(&self) -> Option<String> {
        let s = self.client.settings();
        (!s.server_token.is_empty() && sc::plain_token_warning(&s.server_address))
            .then(|| fill(Msg::ServerPlainToken.tr(self.lang), &[&s.server_address]))
    }

    fn fail(&self, e: &sc::ClientError) -> ServerError {
        ServerError {
            text: e.text(link_lang(self.lang)),
            needs_login: e.needs_login(),
        }
    }

    fn own(&self, msg: Msg, values: &[&str]) -> ServerError {
        ServerError {
            text: fill(msg.tr(self.lang), values),
            needs_login: false,
        }
    }

    fn bad_answer(&self, what: &str) -> ServerError {
        self.own(Msg::ServerBadAnswer, &[what])
    }

    fn call(&mut self, method: &str, path: &str, body: Option<&str>) -> Result<Reply, ServerError> {
        self.client
            .request(method, path, body)
            .map_err(|e| self.fail(&e))
    }

    fn json_of(&self, reply: &Reply) -> Result<Json, ServerError> {
        Json::parse(&reply.body).map_err(|e| self.bad_answer(&e))
    }

    /// `GET /health` (no login needed): the name, the version and the state the server says of itself.
    pub fn health(&mut self) -> Result<sc::Health, ServerError> {
        self.client.health().map_err(|e| self.fail(&e))
    }

    /// Logs in for this program with a name and a password, when there is no token yet (a program token in `server_token` is used as it is); `false`
    /// when no login was needed.
    pub fn login_if_needed(&mut self, name: &str, password: &str) -> Result<bool, ServerError> {
        if self.client.has_token() {
            return Ok(false);
        }
        self.client
            .login(name, password, APP)
            .map(|_| true)
            .map_err(|e| self.fail(&e))
    }

    fn base() -> String {
        format!("/api/v1/apps/{APP}/projects")
    }

    /// The projects of the application `cad` whose name has the text (any letter case); all of them with no text. At most 500, the last changed first.
    pub fn projects(&mut self, query: &str) -> Result<Vec<Remote>, ServerError> {
        let mut path = format!("{}?limit=500", Store::base());
        if !query.is_empty() {
            path.push_str(&format!("&q={}", sc::path_segment(query)));
        }
        let reply = self.call("GET", &path, None)?;
        let json = self.json_of(&reply)?;
        let items = json
            .get("items")
            .and_then(Json::as_array)
            .ok_or_else(|| self.bad_answer("items"))?;
        let mut out = Vec::new();
        for item in items {
            if item.get("deleted").and_then(Json::as_bool) == Some(true) {
                continue;
            }
            out.push(self.remote_of(item)?);
        }
        Ok(out)
    }

    fn remote_of(&self, item: &Json) -> Result<Remote, ServerError> {
        let text = |k: &str| item.get(k).and_then(Json::as_str).map(str::to_string);
        let id = text("id")
            .filter(|i| safe_segment(i))
            .ok_or_else(|| self.bad_answer("id"))?;
        let name = text("name").ok_or_else(|| self.bad_answer("name"))?;
        let files = match item.get("files") {
            Some(Json::Arr(a)) => a.len() as u64,
            Some(n) => n.as_u64().unwrap_or(0),
            None => 0,
        };
        Ok(Remote { id, name, files })
    }

    /// The project with exactly this name, if the server has one.
    pub fn find(&mut self, name: &str) -> Result<Option<Remote>, ServerError> {
        if !valid_name(name) {
            return Err(self.own(Msg::ServerBadName, &[name]));
        }
        Ok(self.projects(name)?.into_iter().find(|p| p.name == name))
    }

    fn create(&mut self, name: &str) -> Result<Remote, ServerError> {
        let body = format!("{{\"name\":{}}}", sc::json::quote(name));
        let reply = self.call("POST", &Store::base(), Some(&body))?;
        let json = self.json_of(&reply)?;
        self.remote_of(&json)
    }

    /// The files of a project with their tags.
    pub fn files(&mut self, project: &str) -> Result<Vec<RemoteFile>, ServerError> {
        if !safe_segment(project) {
            return Err(self.bad_answer("id"));
        }
        let reply = self.call("GET", &format!("{}/{project}", Store::base()), None)?;
        let json = self.json_of(&reply)?;
        let files = json
            .get("files")
            .and_then(Json::as_array)
            .ok_or_else(|| self.bad_answer("files"))?;
        let mut out = Vec::new();
        for f in files {
            let name = f
                .get("name")
                .and_then(Json::as_str)
                .ok_or_else(|| self.bad_answer("name"))?
                .to_string();
            let tag = f
                .get("sha256")
                .and_then(Json::as_str)
                .filter(|t| valid_tag(t))
                .ok_or_else(|| self.bad_answer("sha256"))?
                .to_string();
            out.push(RemoteFile {
                name,
                version: f.get("version").and_then(Json::as_u64).unwrap_or(0),
                tag,
                size: f.get("size").and_then(Json::as_u64).unwrap_or(0),
            });
        }
        Ok(out)
    }

    /// Writes a file of a project as a new version, made from the version whose tag is given (none for a file that is new). The text is the file.
    pub fn write_file(
        &mut self,
        project: &str,
        file: &str,
        text: &str,
        comment: &str,
        tag: Option<&str>,
    ) -> Result<(u64, bool), ServerError> {
        if !safe_segment(project) {
            return Err(self.bad_answer("id"));
        }
        let mut path = format!(
            "{}/{project}/files/content?name={}",
            Store::base(),
            sc::path_segment(file)
        );
        if !comment.is_empty() {
            path.push_str(&format!("&comment={}", sc::path_segment(comment)));
        }
        if let Some(t) = tag {
            if !valid_tag(t) {
                return Err(self.bad_answer("sha256"));
            }
            path = with_if_match(&path, t);
        }
        let reply = self.call("PUT", &path, Some(text))?;
        let json = self.json_of(&reply)?;
        let version = json
            .get("version")
            .and_then(Json::as_u64)
            .ok_or_else(|| self.bad_answer("version"))?;
        let unchanged = json.get("unchanged").and_then(Json::as_bool) == Some(true);
        Ok((version, unchanged))
    }

    /// Reads the bytes of a file of a project (its current version).
    pub fn read_file(&mut self, project: &str, file: &str) -> Result<String, ServerError> {
        if !safe_segment(project) {
            return Err(self.bad_answer("id"));
        }
        let path = format!(
            "{}/{project}/files/content?name={}",
            Store::base(),
            sc::path_segment(file)
        );
        Ok(self.call("GET", &path, None)?.body)
    }

    fn project_for_write(&mut self, name: &str) -> Result<(Remote, bool), ServerError> {
        match self.find(name)? {
            Some(p) => Ok((p, false)),
            None => Ok((self.create(name)?, true)),
        }
    }

    fn write_in_project(
        &mut self,
        name: &str,
        file: &str,
        text: &str,
        comment: &str,
    ) -> Result<Written, ServerError> {
        let (project, created) = self.project_for_write(name)?;
        let tag = if created {
            None
        } else {
            self.files(&project.id)?
                .into_iter()
                .find(|f| f.name == file)
                .map(|f| f.tag)
        };
        let (version, unchanged) =
            self.write_file(&project.id, file, text, comment, tag.as_deref())?;
        Ok(Written {
            project: project.id,
            file: file.to_string(),
            version,
            unchanged,
            created,
        })
    }

    /// Checks the bytes of a project file: they must be text, within the limit, and a project file of this program.
    fn project_text<'a>(&self, label: &str, bytes: &'a [u8]) -> Result<&'a str, ServerError> {
        if bytes.len() > self.max_bytes {
            return Err(self.own(Msg::ServerTooBig, &[label, &self.max_bytes.to_string()]));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| self.own(Msg::ServerNotText, &[]))?;
        Document::from_bytes(bytes, self.max_bytes)
            .map_err(|e| self.own(Msg::ServerBadFile, &[label, &e.to_string()]))?;
        Ok(text)
    }

    /// Saves the bytes of a project file on the server under a name: the project is made when the server has none of that name, and the file is a new
    /// version of `project.phoenixproj` (nothing is written when the bytes are those of the current version).
    pub fn save(
        &mut self,
        name: &str,
        bytes: &[u8],
        comment: &str,
    ) -> Result<Written, ServerError> {
        if !valid_name(name) {
            return Err(self.own(Msg::ServerBadName, &[name]));
        }
        let text = self.project_text(name, bytes)?;
        let written = self.write_in_project(name, PROJECT_FILE, text, comment)?;
        // the project as the station of the server reads it, next to the file of the program
        let doc = Document::from_bytes(bytes, self.max_bytes)
            .map_err(|e| self.own(Msg::ServerBadFile, &[name, &e.to_string()]))?;
        let contract = servercad::project_json(&doc, name)
            .map_err(|e| self.own(Msg::ServerBadFile, &[name, &e.to_string()]))?;
        self.write_in_project(name, CONTRACT_FILE, &contract, comment)?;
        Ok(written)
    }

    /// The project of this name as the station reads it (`project.json` of the store): the text the
    /// last save from this program wrote, checked to be JSON.
    pub fn load_contract(&mut self, name: &str) -> Result<String, ServerError> {
        let project = self
            .find(name)?
            .ok_or_else(|| self.own(Msg::ServerNoProject, &[name]))?;
        let has = self
            .files(&project.id)?
            .iter()
            .any(|f| f.name == CONTRACT_FILE);
        if !has {
            return Err(self.own(Msg::ServerNoContract, &[name]));
        }
        let text = self.read_file(&project.id, CONTRACT_FILE)?;
        Json::parse(&text).map_err(|e| self.bad_answer(&e))?;
        Ok(text)
    }

    /// Asks the station `cad` of the server (the machine of `server_machine`) for a `check` or a
    /// `report` of a project, given as the text of the contract (`servercad::project_json`): makes
    /// the job, starts it and reads it until it ends, for at most `server_timeout_s`. The answer is
    /// checked to have the keys of the contract.
    pub fn station(
        &mut self,
        kind: &str,
        name: &str,
        project: &str,
    ) -> Result<Answer, ServerError> {
        let settings = self.client.settings();
        let machine = settings.server_machine.clone();
        let limit = Duration::from_secs(settings.server_timeout_s.max(1));
        if machine.is_empty() {
            return Err(self.own(Msg::ServerNoMachine, &[]));
        }
        if !matches!(kind, "check" | "report") {
            return Err(self.bad_answer("kind"));
        }
        Json::parse(project).map_err(|e| self.bad_answer(&e))?;
        let body = format!(
            "{{\"machine\":{},\"name\":{},\"app\":{},\"request\":{{\"kind\":{},\"project\":{}}}}}",
            sc::json::quote(&machine),
            sc::json::quote(&format!("{kind} {name}")),
            sc::json::quote(APP),
            sc::json::quote(kind),
            project
        );
        let made = self.call("POST", "/api/v1/work/jobs", Some(&body))?;
        let made = self.json_of(&made)?;
        let id = made
            .get("id")
            .and_then(Json::as_str)
            .filter(|i| safe_segment(i))
            .ok_or_else(|| self.bad_answer("id"))?
            .to_string();
        self.call("POST", &format!("/api/v1/work/jobs/{id}/start"), None)?;
        let started = std::time::Instant::now();
        loop {
            let reply = self.call("GET", &format!("/api/v1/work/jobs/{id}"), None)?;
            let job = self.json_of(&reply)?;
            match job.get("state").and_then(Json::as_str) {
                Some("done") => {
                    let result = job
                        .get("view")
                        .and_then(|v| v.get("result"))
                        .ok_or_else(|| self.bad_answer("view.result"))?;
                    return servercad::parse_answer(result).map_err(|e| self.bad_answer(&e));
                }
                Some("failed" | "cancelled") => {
                    let why = job.get("message").and_then(Json::as_str).unwrap_or("");
                    return Err(self.own(Msg::ServerJobFailed, &[kind, why]));
                }
                Some(_) => {}
                None => return Err(self.bad_answer("state")),
            }
            if started.elapsed() >= limit {
                return Err(self.own(Msg::ServerJobTimeout, &[&limit.as_secs().to_string()]));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Reads the project of this name from the server: the bytes of its file, checked to be a project file.
    pub fn load(&mut self, name: &str) -> Result<Vec<u8>, ServerError> {
        let project = self
            .find(name)?
            .ok_or_else(|| self.own(Msg::ServerNoProject, &[name]))?;
        let text = self.read_file(&project.id, PROJECT_FILE)?;
        let bytes = text.into_bytes();
        // a byte that was not text came back as U+FFFD and changed the lengths of the domains, which the reading of the project file notices
        self.project_text(name, &bytes)?;
        Ok(bytes)
    }

    /// Writes a report (a text) into the folder of reports of the project of this name, which is made when it does not exist; the report is a new
    /// version of its file.
    pub fn send_report(
        &mut self,
        name: &str,
        report: &str,
        text: &str,
        comment: &str,
    ) -> Result<Written, ServerError> {
        if !valid_name(name) {
            return Err(self.own(Msg::ServerBadName, &[name]));
        }
        let file = report_file(report).ok_or_else(|| self.own(Msg::ServerBadName, &[report]))?;
        if text.len() > self.max_bytes {
            return Err(self.own(Msg::ServerTooBig, &[report, &self.max_bytes.to_string()]));
        }
        self.write_in_project(name, &file, text, comment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Settings, DEFAULT_CONFIG};
    use crate::serverfake::{serve, MemoryStore};

    const TOKEN: &str = "t0ken-of-the-tests";

    fn project_bytes(steps: usize) -> Vec<u8> {
        let mut doc = Document::new();
        doc.set("mechanical", true, format!("steps {steps}\n").into_bytes())
            .unwrap();
        doc.to_bytes()
    }

    fn link_to(address: &str) -> sc::Settings {
        let mut link = sc::Settings::default();
        link.set("server_address", address).unwrap();
        link.set("server_token", TOKEN).unwrap();
        link.set("server_retries", "0").unwrap();
        link
    }

    fn store_over(mem: &MemoryStore, lang: Lang) -> Store {
        let client = sc::Client::new(link_to("memory:1"), Box::new(mem.clone()));
        Store::over(client, lang, 1 << 20)
    }

    #[test]
    fn the_link_is_read_from_the_section_server_of_the_settings() {
        let d = Settings::defaults().server;
        assert_eq!(d, sc::Settings::default());
        assert!(!d.is_configured());
        let s = Settings::with_user_config(
            "[server]
server_address = 192.168.0.180:18200
server_token = abc123
server_machine = bench
server_timeout_s = 30
server_retries = 4
server_mdns = on
server_max_answer_mib = 16
",
        )
        .unwrap()
        .server;
        assert_eq!(s.server_address, "192.168.0.180:18200");
        assert_eq!(s.server_token.expose(), "abc123");
        assert_eq!(s.server_machine, "bench");
        assert_eq!(
            (s.server_timeout_s, s.server_retries, s.server_mdns),
            (30, 4, true)
        );
        assert_eq!(s.server_max_answer_mib, 16);
        // the token cannot be printed by mistake
        assert!(!format!("{s:?}").contains("abc123"));
    }

    #[test]
    fn a_value_of_the_link_out_of_its_range_is_refused_with_its_key() {
        for (key, value) in [
            ("server_timeout_s", "0"),
            ("server_timeout_s", "121"),
            ("server_retries", "6"),
            ("server_mdns", "maybe"),
            ("server_max_answer_mib", "0"),
            ("server_max_answer_mib", "257"),
            ("server_address", "a b"),
            ("server_token", "a b"),
        ] {
            let text = format!("[server]\n{key} = {value}\n");
            let e = Settings::with_user_config(&text).unwrap_err().to_string();
            assert!(e.contains(&format!("server.{key}")), "{key} = {value}: {e}");
        }
        // the edges are accepted
        for (key, value) in [
            ("server_timeout_s", "1"),
            ("server_timeout_s", "120"),
            ("server_retries", "0"),
            ("server_retries", "5"),
            ("server_max_answer_mib", "1"),
            ("server_max_answer_mib", "256"),
        ] {
            let text = format!("[server]\n{key} = {value}\n");
            assert!(Settings::with_user_config(&text).is_ok(), "{key} = {value}");
        }
    }

    #[test]
    fn every_key_of_the_link_is_in_the_shipped_file_with_the_default_of_the_client() {
        let c = Config::parse(DEFAULT_CONFIG).unwrap();
        for key in KEYS {
            assert!(c.get(&format!("server.{key}")).is_some(), "{key}");
        }
        assert_eq!(KEYS.len(), sc::KEYS.len());
        for (mine, theirs) in KEYS.iter().zip(sc::KEYS.iter()) {
            assert_eq!(*mine, theirs.key);
            let shipped = c.get(&format!("server.{mine}")).unwrap();
            assert_eq!(shipped, theirs.default, "{mine}");
        }
    }

    #[test]
    fn without_an_address_nothing_is_connected_and_the_sentence_is_in_the_language() {
        let link = sc::Settings::default();
        let mut seen = Vec::new();
        for lang in Lang::ALL {
            let e = Store::connect(&link, lang, 1000).err().unwrap();
            assert_eq!(e.text, Msg::ServerNotSet.tr(lang));
            assert!(!e.needs_login);
            assert!(!seen.contains(&e.text));
            seen.push(e.text);
        }
    }

    #[test]
    fn a_project_saved_on_the_server_is_read_back_byte_for_byte() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        let bytes = project_bytes(3);
        let w = store.save("front panel", &bytes, "first").unwrap();
        assert!(w.created && !w.unchanged);
        assert_eq!((w.version, w.file.as_str()), (1, PROJECT_FILE));
        assert_eq!(store.load("front panel").unwrap(), bytes);
        assert_eq!(mem.project_names(), vec!["front panel".to_string()]);
        let found = store.find("front panel").unwrap().unwrap();
        // the file of the program and the project as the station reads it
        assert_eq!((found.id, found.files), (w.project, 2));
        assert!(store.find("front").unwrap().is_none());
        let e = store.load("front").unwrap_err();
        assert_eq!(e.text, "The server has no project named front");
    }

    #[test]
    fn a_second_save_says_which_version_it_is_made_from_and_the_same_bytes_make_none() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        store.save("p", &project_bytes(1), "").unwrap();
        let w2 = store.save("p", &project_bytes(2), "second").unwrap();
        assert_eq!((w2.version, w2.unchanged, w2.created), (2, false, false));
        let puts: Vec<_> = mem
            .seen()
            .into_iter()
            .filter(|s| s.method == "PUT")
            .collect();
        // each save writes two files: the project of the program and the one of the station
        assert_eq!(puts.len(), 4);
        assert_eq!((&puts[0].if_match, &puts[1].if_match), (&None, &None));
        for p in &puts[2..] {
            assert!(p.if_match.as_deref().is_some_and(|t| t.len() == 32));
        }
        let w3 = store.save("p", &project_bytes(2), "").unwrap();
        assert_eq!((w3.version, w3.unchanged), (2, true));
        assert_eq!(mem.versions("p", PROJECT_FILE), 2);
        assert_eq!(store.load("p").unwrap(), project_bytes(2));
    }

    #[test]
    fn a_write_over_a_file_without_its_tag_or_with_an_old_one_is_refused_in_the_language() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::Es);
        let w = store.save("p", &project_bytes(1), "").unwrap();
        let text = String::from_utf8(project_bytes(2)).unwrap();
        let e = store
            .write_file(&w.project, PROJECT_FILE, &text, "", None)
            .unwrap_err();
        assert_eq!(e.text, sc::text_for("precondition_required", sc::Lang::Es));
        let e = store
            .write_file(&w.project, PROJECT_FILE, &text, "", Some("abc123"))
            .unwrap_err();
        assert_eq!(e.text, sc::text_for("precondition_failed", sc::Lang::Es));
        assert!(!e.needs_login);
        // a tag with a character that does not belong in a header is not sent
        let e = store
            .write_file(&w.project, PROJECT_FILE, &text, "", Some("a\r\nb"))
            .unwrap_err();
        assert!(e.text.contains("sha256"));
        // nothing was written by any of those
        assert_eq!(mem.versions("p", PROJECT_FILE), 1);
        // with the rule off on the server the old way works too
        mem.require_if_match(false);
        assert!(store
            .write_file(&w.project, PROJECT_FILE, &text, "", None)
            .is_ok());
    }

    #[test]
    fn a_change_made_by_somebody_else_in_the_meantime_is_not_overwritten_blindly() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        let w = store.save("p", &project_bytes(1), "").unwrap();
        let tag = store.files(&w.project).unwrap()[0].tag.clone();
        mem.write_behind("p", PROJECT_FILE, "another version");
        let text = String::from_utf8(project_bytes(5)).unwrap();
        let e = store
            .write_file(&w.project, PROJECT_FILE, &text, "", Some(&tag))
            .unwrap_err();
        assert_eq!(e.text, sc::text_for("precondition_failed", sc::Lang::En));
        // a save reads the tag again, so it goes over the new version knowingly
        let again = store.save("p", &project_bytes(5), "").unwrap();
        assert_eq!(again.version, 3);
    }

    #[test]
    fn names_that_cannot_go_to_the_server_and_files_that_are_not_projects_are_refused_before_sending(
    ) {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        let ok = project_bytes(1);
        for name in ["", "   ", "a\nb", "tab\there"] {
            let e = store.save(name, &ok, "").unwrap_err();
            assert_eq!(e.text, fill(Msg::ServerBadName.tr(Lang::En), &[name]));
        }
        let long = "x".repeat(NAME_MAX + 1);
        assert!(store.save(&long, &ok, "").is_err());
        assert!(store.find("").is_err());
        let e = store.save("p", &[0xff, 0xfe, 0x00], "").unwrap_err();
        assert_eq!(e.text, Msg::ServerNotText.tr(Lang::En));
        let e = store.save("p", b"this is not a project", "").unwrap_err();
        assert!(e.text.starts_with("p is not a project of this program"));
        let mut small = store_over(&mem, Lang::En);
        small.max_bytes = ok.len() - 1;
        let e = small.save("p", &ok, "").unwrap_err();
        assert!(e.text.contains("larger than"));
        // none of those reached the server
        assert!(mem.seen().is_empty());
        assert!(store.save(&"x".repeat(NAME_MAX), &ok, "").is_ok());
    }

    #[test]
    fn a_file_that_comes_back_damaged_is_not_given_as_a_project() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        store.save("p", &project_bytes(1), "").unwrap();
        mem.write_behind("p", PROJECT_FILE, "not a project at all");
        let e = store.load("p").unwrap_err();
        assert!(e.text.starts_with("p is not a project of this program"));
        // the domain was cut: its length says more than what there is
        mem.write_behind(
            "p",
            PROJECT_FILE,
            "PHOENIX 1\ndomain mechanical critical 99\nshort\n",
        );
        assert!(store.load("p").is_err());
    }

    #[test]
    fn a_report_goes_into_the_folder_of_reports_of_the_project_as_versions_of_its_file() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        let w = store
            .send_report("box", "bom.csv", "a,b\n1,2\n", "first")
            .unwrap();
        assert!(w.created);
        assert_eq!((w.file.as_str(), w.version), ("reports/bom.csv", 1));
        assert_eq!(mem.file("box", "reports/bom.csv").unwrap(), "a,b\n1,2\n");
        let w = store
            .send_report("box", "bom.csv", "a,b\n1,3\n", "")
            .unwrap();
        assert!(!w.created);
        assert_eq!(w.version, 2);
        let same = store
            .send_report("box", "bom.csv", "a,b\n1,3\n", "")
            .unwrap();
        assert!(same.unchanged);
        // the project and its reports live together: saving the project does not touch the reports
        store.save("box", &project_bytes(1), "").unwrap();
        assert_eq!(mem.versions("box", "reports/bom.csv"), 2);
        assert_eq!(mem.project_names().len(), 1);
        let long = "r".repeat(NAME_MAX + 1);
        for bad in [
            "",
            ".hidden",
            "../x",
            "a b",
            "dir/file",
            "a\\b",
            long.as_str(),
        ] {
            assert!(store.send_report("box", bad, "x", "").is_err(), "{bad}");
        }
        assert_eq!(report_file("parts.txt").unwrap(), "reports/parts.txt");
        assert!(report_file("\u{e9}.txt").is_none());
    }

    #[test]
    fn projects_are_listed_last_changed_first_and_found_by_part_of_the_name_in_any_case() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        for n in ["Front Panel", "rear panel", "lid"] {
            store.save(n, &project_bytes(1), "").unwrap();
        }
        let names = |v: Vec<Remote>| v.into_iter().map(|r| r.name).collect::<Vec<_>>();
        assert_eq!(
            names(store.projects("").unwrap()),
            vec!["lid", "rear panel", "Front Panel"]
        );
        assert_eq!(
            names(store.projects("PANEL").unwrap()),
            vec!["rear panel", "Front Panel"]
        );
        assert!(store.projects("nothing").unwrap().is_empty());
        // a name with characters of a query comes back whole
        store
            .save("a&b=c %d+\u{e9}", &project_bytes(1), "")
            .unwrap();
        assert!(store.find("a&b=c %d+\u{e9}").unwrap().is_some());
        assert_eq!(
            names(store.projects("&b=c").unwrap()),
            vec!["a&b=c %d+\u{e9}"]
        );
    }

    #[test]
    fn a_token_the_server_does_not_know_asks_for_a_new_login_in_each_language() {
        let mem = MemoryStore::new(TOKEN);
        let mut texts = Vec::new();
        for lang in Lang::ALL {
            let mut link = link_to("memory:1");
            link.set("server_token", "wrong").unwrap();
            let mut store = Store::over(sc::Client::new(link, Box::new(mem.clone())), lang, 1000);
            let e = store.projects("").unwrap_err();
            assert!(e.needs_login);
            assert!(!e.text.is_empty());
            texts.push(e.text);
        }
        let mut sorted = texts.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 8, "{texts:?}");
        // and the health of the server needs no login
        let mut store = store_over(&mem, Lang::En);
        assert_eq!(store.health().unwrap().status, "ok");
    }

    #[test]
    fn the_token_in_the_clear_is_warned_about_only_on_a_network_that_is_not_the_persons_own() {
        let mem = MemoryStore::new(TOKEN);
        let at = |address: &str| {
            Store::over(
                sc::Client::new(link_to(address), Box::new(mem.clone())),
                Lang::En,
                10,
            )
        };
        assert!(at("127.0.0.1:18200").plain_token_warning().is_none());
        assert!(at("192.168.0.180:18200").plain_token_warning().is_none());
        let w = at("203.0.113.9:18200").plain_token_warning().unwrap();
        assert!(w.contains("203.0.113.9:18200"));
    }

    #[test]
    fn over_plain_http_the_file_goes_as_bytes_with_its_tag_in_the_header() {
        let mem = MemoryStore::new(TOKEN);
        let served = serve(&mem).unwrap();
        let mut store = Store::connect(&link_to(&served.address), Lang::En, 1 << 20).unwrap();
        assert_eq!(store.health().unwrap().name, "eh3d-server");
        let first = project_bytes(1);
        let second = project_bytes(2);
        assert_eq!(store.save("wire", &first, "one").unwrap().version, 1);
        assert_eq!(store.save("wire", &second, "two").unwrap().version, 2);
        assert_eq!(store.load("wire").unwrap(), second);
        let puts: Vec<_> = mem
            .seen()
            .into_iter()
            .filter(|s| s.method == "PUT")
            .collect();
        assert_eq!(puts.len(), 4);
        for p in &puts {
            assert_eq!(p.content_type.as_deref(), Some("application/octet-stream"));
        }
        assert_eq!((&puts[0].if_match, &puts[1].if_match), (&None, &None));
        assert!(puts[2].if_match.is_some() && puts[3].if_match.is_some());
        // the creation of the project is JSON
        let post = mem.seen().into_iter().find(|s| s.method == "POST").unwrap();
        assert_eq!(post.content_type.as_deref(), Some("application/json"));
        // a wrong token over the wire is a login to make
        let mut link = link_to(&served.address);
        link.set("server_token", "nope").unwrap();
        let mut bad = Store::connect(&link, Lang::De, 100).unwrap();
        assert!(bad.projects("").unwrap_err().needs_login);
    }

    #[test]
    fn a_server_that_is_not_there_is_told_in_the_language_and_the_link_is_marked_lost() {
        let mem = MemoryStore::new(TOKEN);
        let served = serve(&mem).unwrap();
        let address = served.address.clone();
        drop(served);
        let mut link = link_to(&address);
        link.set("server_timeout_s", "1").unwrap();
        let mut store = Store::connect(&link, Lang::En, 1 << 20).unwrap();
        let e = store.save("p", &project_bytes(1), "").unwrap_err();
        assert!(!e.needs_login);
        assert!(!e.text.is_empty());
        assert_eq!(store.link(), sc::Link::Disconnected);
    }

    /// A small generator of numbers with a fixed seed (xorshift), so that the tests below do the same every time.
    struct Seed(u64);

    impl Seed {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn pick(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    #[test]
    fn two_hundred_random_projects_with_odd_names_and_texts_come_back_the_same_with_a_fixed_seed() {
        const PIECES: [&str; 14] = [
            "a", "Z", "7", " ", "&", "=", "%", "+", "?", "\u{e9}", "\u{6f22}", "\"", "\\", "#",
        ];
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        let mut seed = Seed(0x9e37_79b9_7f4a_7c15);
        let mut kept: Vec<(String, Vec<u8>)> = Vec::new();
        for n in 0..200 {
            let mut name = format!("p{n}-");
            for _ in 0..1 + seed.pick(12) {
                name.push_str(PIECES[seed.pick(PIECES.len())]);
            }
            let name = name.trim_end().to_string();
            let mut data = String::new();
            for _ in 0..seed.pick(40) {
                data.push_str(PIECES[seed.pick(PIECES.len())]);
                if seed.pick(5) == 0 {
                    data.push('\n');
                }
            }
            let mut doc = Document::new();
            doc.set("mechanical", seed.pick(2) == 0, data.into_bytes())
                .unwrap();
            let bytes = doc.to_bytes();
            let w = store.save(&name, &bytes, "").unwrap();
            assert!(w.created, "{name}");
            kept.push((name, bytes));
        }
        for (name, bytes) in &kept {
            assert_eq!(&store.load(name).unwrap(), bytes, "{name}");
        }
        assert_eq!(mem.project_names().len(), 200);
        // a second pass of the same bytes writes nothing
        for (name, bytes) in kept.iter().step_by(7) {
            assert!(store.save(name, bytes, "").unwrap().unchanged, "{name}");
        }
    }

    #[test]
    fn random_saves_and_writes_made_behind_never_lose_a_version_or_overwrite_blindly_with_a_fixed_seed(
    ) {
        use std::collections::BTreeMap;
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        let mut seed = Seed(0x0123_4567_89ab_cdef);
        // what the server must hold for each project: the text of the file and its versions
        let mut model: BTreeMap<String, (String, usize)> = BTreeMap::new();
        let (mut refused, mut unchanged) = (0, 0);
        for n in 0..300 {
            let name = format!("p{}", seed.pick(5));
            let bytes = project_bytes(1 + seed.pick(6));
            let text = String::from_utf8(bytes.clone()).unwrap();
            match seed.pick(3) {
                0 | 2 => {
                    let w = store.save(&name, &bytes, "").unwrap();
                    match model.get_mut(&name) {
                        None => {
                            assert!(w.created, "step {n}");
                            model.insert(name, (text, 1));
                        }
                        Some((kept, versions)) if *kept == text => {
                            assert!(w.unchanged, "step {n}");
                            assert_eq!(w.version as usize, *versions, "step {n}");
                            unchanged += 1;
                        }
                        Some((kept, versions)) => {
                            *versions += 1;
                            *kept = text;
                            assert_eq!(w.version as usize, *versions, "step {n}");
                        }
                    }
                }
                _ => {
                    // somebody else writes while this program holds the tag it read: its write is
                    // refused and what the other one wrote stays
                    if let Some((kept, versions)) = model.get_mut(&name) {
                        let project = store.find(&name).unwrap().unwrap().id;
                        let tag = store.files(&project).unwrap()[0].tag.clone();
                        let other = format!("written by somebody else at step {n}");
                        mem.write_behind(&name, PROJECT_FILE, &other);
                        *kept = other;
                        *versions += 1;
                        let e = store
                            .write_file(&project, PROJECT_FILE, &text, "", Some(&tag))
                            .unwrap_err();
                        assert_eq!(e.text, sc::text_for("precondition_failed", sc::Lang::En));
                        refused += 1;
                    }
                }
            }
            for (name, (kept, versions)) in &model {
                assert_eq!(
                    &mem.file(name, PROJECT_FILE).unwrap(),
                    kept,
                    "{name} step {n}"
                );
                assert_eq!(
                    mem.versions(name, PROJECT_FILE),
                    *versions,
                    "{name} step {n}"
                );
            }
        }
        assert!(refused > 20 && unchanged > 5, "{refused} {unchanged}");
    }

    const PLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.phoenixproj");
    const ASSEMBLY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/assembly.phoenixproj");

    fn station_over(mem: &MemoryStore, lang: Lang, machine: &str, timeout: &str) -> Store {
        let mut link = link_to("memory:1");
        link.set("server_machine", machine).unwrap();
        link.set("server_timeout_s", timeout).unwrap();
        Store::over(sc::Client::new(link, Box::new(mem.clone())), lang, 1 << 22)
    }

    fn contract_of(path: &str, name: &str) -> String {
        let doc = Document::from_bytes(&std::fs::read(path).unwrap(), 1 << 22).unwrap();
        servercad::project_json(&doc, name).unwrap()
    }

    #[test]
    fn a_save_also_writes_the_project_as_the_station_reads_it_and_the_same_save_makes_no_version() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = station_over(&mem, Lang::En, "cad1", "5");
        let bytes = std::fs::read(PLATE).unwrap();
        let w = store.save("plate", &bytes, "first").unwrap();
        assert_eq!((w.version, w.unchanged, w.created), (1, false, true));
        // both files are in the project: the one of the program and the one of the station
        assert_eq!(mem.versions("plate", PROJECT_FILE), 1);
        assert_eq!(mem.versions("plate", CONTRACT_FILE), 1);
        let contract = mem.file("plate", CONTRACT_FILE).unwrap();
        assert_eq!(contract, contract_of(PLATE, "plate"));
        let json = Json::parse(&contract).unwrap();
        assert!(servercad::check_json(&json).pass);
        // the same bytes again make no version of either file
        let again = store.save("plate", &bytes, "again").unwrap();
        assert!(again.unchanged);
        assert_eq!(mem.versions("plate", PROJECT_FILE), 1);
        assert_eq!(mem.versions("plate", CONTRACT_FILE), 1);
        // and the store is the one of the application of the server
        assert!(mem.seen().iter().all(|s| s.path == "/api/v1/health"
            || s.path.starts_with("/api/v1/apps/phoenix-cad/projects")));
        // what was stored comes back as the text the station is given
        assert_eq!(store.load_contract("plate").unwrap(), contract);
    }

    #[test]
    fn a_project_with_no_file_for_the_station_is_told_so_in_the_language() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = station_over(&mem, Lang::Es, "cad1", "5");
        // a project that another program made, with only the file of the program
        let id = store.create("old").unwrap().id;
        store.write_file(&id, PROJECT_FILE, "x", "", None).unwrap();
        let e = store.load_contract("old").unwrap_err();
        assert_eq!(e.text, fill(Msg::ServerNoContract.tr(Lang::Es), &["old"]));
        let e = store.load_contract("none").unwrap_err();
        assert_eq!(e.text, fill(Msg::ServerNoProject.tr(Lang::Es), &["none"]));
        // a file that is not JSON is refused before it is sent
        store
            .write_file(&id, CONTRACT_FILE, "not json", "", None)
            .unwrap();
        let e = store.load_contract("old").unwrap_err();
        assert!(
            e.text.starts_with("La respuesta del servidor"),
            "{}",
            e.text
        );
    }

    #[test]
    fn the_station_checks_and_reports_a_project_and_the_job_goes_through_its_routes() {
        let mem = MemoryStore::new(TOKEN);
        let mut store = station_over(&mem, Lang::En, "cad1", "5");
        let contract = contract_of(PLATE, "plate");
        let check = store.station("check", "plate", &contract).unwrap();
        assert_eq!(check.kind, "check");
        assert!(check.verdict.pass && check.counts.is_none());
        assert_eq!(check.evidence, "simulator");
        let report = store.station("report", "plate", &contract).unwrap();
        let counts = report.counts.unwrap();
        assert_eq!((counts.parts, counts.sketches, counts.features), (1, 1, 1));
        let routes: Vec<String> = mem
            .seen()
            .iter()
            .map(|s| format!("{} {}", s.method, s.path))
            .collect();
        assert_eq!(
            routes,
            vec![
                "POST /api/v1/work/jobs",
                "POST /api/v1/work/jobs/j-0001/start",
                "GET /api/v1/work/jobs/j-0001",
                "POST /api/v1/work/jobs",
                "POST /api/v1/work/jobs/j-0002/start",
                "GET /api/v1/work/jobs/j-0002",
            ]
        );
        // a project with a structure that is wrong fails the check, and the issues come back
        let broken = contract.replace("\"sketch\":\"plate\"", "\"sketch\":\"gone\"");
        let answer = store.station("check", "plate", &broken).unwrap();
        assert!(!answer.verdict.pass);
        assert_eq!(answer.verdict.errors, 1);
        assert_eq!(answer.verdict.issues[0].code, "missing_ref");
        // the assembly has parts with no feature: a pass with warnings
        let assembly = contract_of(ASSEMBLY, "assembly");
        let answer = store.station("check", "assembly", &assembly).unwrap();
        assert!(answer.verdict.pass);
        assert_eq!((answer.verdict.errors, answer.verdict.warnings), (0, 6));
    }

    #[test]
    fn a_station_that_is_not_set_that_fails_or_that_never_answers_is_told_in_each_language() {
        let mem = MemoryStore::new(TOKEN);
        let contract = contract_of(PLATE, "plate");
        for lang in Lang::ALL {
            let mut none = station_over(&mem, lang, "", "5");
            let e = none.station("check", "plate", &contract).unwrap_err();
            assert_eq!(e.text, Msg::ServerNoMachine.tr(lang));
            assert!(!e.needs_login);
            let mut broken = station_over(&mem, lang, "broken", "5");
            let e = broken.station("report", "plate", &contract).unwrap_err();
            assert_eq!(
                e.text,
                fill(
                    Msg::ServerJobFailed.tr(lang),
                    &["report", "the station is out of order"]
                )
            );
        }
        // a machine the server does not have, a kind it does not know, a project that is not JSON
        let mut missing = station_over(&mem, Lang::En, "missing", "5");
        assert!(missing.station("check", "plate", &contract).is_err());
        let mut ok = station_over(&mem, Lang::En, "cad1", "5");
        assert!(ok.station("export", "plate", &contract).is_err());
        assert!(ok.station("check", "plate", "not json").is_err());
        // a station that never ends is given up after the time of the link
        let mut slow = station_over(&mem, Lang::Pt, "slow", "1");
        let started = std::time::Instant::now();
        let e = slow.station("check", "plate", &contract).unwrap_err();
        assert_eq!(e.text, fill(Msg::ServerJobTimeout.tr(Lang::Pt), &["1"]));
        assert!(started.elapsed() >= Duration::from_secs(1));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_jobs_also_go_over_plain_http_with_the_token() {
        let mem = MemoryStore::new(TOKEN);
        let served = serve(&mem).unwrap();
        let mut link = link_to(&served.address);
        link.set("server_machine", "cad1").unwrap();
        let mut store = Store::connect(&link, Lang::En, 1 << 22).unwrap();
        let answer = store
            .station("report", "plate", &contract_of(PLATE, "plate"))
            .unwrap();
        assert!(answer.verdict.pass);
        assert_eq!(answer.counts.unwrap().sketches, 1);
        // a wrong token is a login to do, not a job
        link.set("server_token", "other").unwrap();
        let mut wrong = Store::connect(&link, Lang::En, 1 << 22).unwrap();
        let e = wrong
            .station("check", "plate", &contract_of(PLATE, "plate"))
            .unwrap_err();
        assert!(e.needs_login, "{}", e.text);
    }
}
