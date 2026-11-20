//! A project store in memory that answers like the store of the EH3D server (`/api/v1/apps/cad/projects`), for the tests of this program and of the people
//! who write a program that talks to it. It is not a server: it has no accounts, one token is the only login, and the checksum that tags a version is
//! not a SHA-256. What it does keep from the real one is the rules that the client has to follow: a write over a file that exists says which version it
//! was made from (`428` without it, `412` when it is not the current one), the same bytes make no version, and a project that is not the caller's does
//! not exist.
//!
//! [`MemoryStore`] is a `Transport` of the client of the suite, and [`serve`] puts it behind a socket of this computer so that the plain HTTP of the
//! program (and its command line) is tried too.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use eh3d_serverclient::json::quote;
use eh3d_serverclient::{Reply, Request, Transport, TransportError};

use crate::server::split_if_match;

/// What the store saw of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The method of the request.
    pub method: String,
    /// The path without the query.
    pub path: String,
    /// The tag of `If-Match`, with the quotes taken off.
    pub if_match: Option<String>,
    /// The `Content-Type` of a request that came through a socket (nothing for the one that came through the transport in memory).
    pub content_type: Option<String>,
}

struct Project {
    id: String,
    name: String,
    /// The versions of each file, the first one first.
    files: BTreeMap<String, Vec<String>>,
}

struct State {
    token: String,
    require_if_match: bool,
    projects: Vec<Project>,
    next: u32,
    seen: Vec<Seen>,
}

/// The store. Cloning it gives another handle to the same projects.
#[derive(Clone)]
pub struct MemoryStore(Arc<Mutex<State>>);

fn tag_of(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}{:016x}", h.rotate_left(29) ^ text.len() as u64)
}

fn error(status: u16, code: &str) -> (u16, String) {
    (
        status,
        format!("{{\"error\":{},\"detail\":\"\"}}", quote(code)),
    )
}

fn decode(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|s| u8::from_str_radix(s, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn query_get(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| decode(v))
    })
}

fn file_json(name: &str, version: usize, text: &str) -> String {
    format!(
        "{{\"name\":{},\"version\":{version},\"sha256\":{},\"size\":{}}}",
        quote(name),
        quote(&tag_of(text)),
        text.len()
    )
}

fn project_json(p: &Project) -> String {
    let files: Vec<String> = p
        .files
        .iter()
        .filter_map(|(name, versions)| {
            versions
                .last()
                .map(|text| file_json(name, versions.len(), text))
        })
        .collect();
    format!(
        "{{\"id\":{},\"app\":\"cad\",\"name\":{},\"owner\":\"token\",\"etag\":\"{}\",\"files\":[{}]}}",
        quote(&p.id),
        quote(&p.name),
        p.files.len(),
        files.join(",")
    )
}

fn summary_json(p: &Project) -> String {
    format!(
        "{{\"id\":{},\"app\":\"cad\",\"name\":{},\"owner\":\"token\",\"etag\":\"{}\",\"files\":{}}}",
        quote(&p.id),
        quote(&p.name),
        p.files.len(),
        p.files.len()
    )
}

fn bad_file_name(name: &str) -> bool {
    name.is_empty()
        || name.len() > 240
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains(':')
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || name.chars().any(char::is_control)
}

impl MemoryStore {
    /// A store that lets in whoever sends this token.
    pub fn new(token: &str) -> MemoryStore {
        MemoryStore(Arc::new(Mutex::new(State {
            token: token.to_string(),
            require_if_match: true,
            projects: Vec::new(),
            next: 1,
            seen: Vec::new(),
        })))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Turns the rule of `If-Match` off, as `project_require_if_match = false` does on the server.
    pub fn require_if_match(&self, on: bool) {
        self.state().require_if_match = on;
    }

    /// What the store has seen, in order.
    pub fn seen(&self) -> Vec<Seen> {
        self.state().seen.clone()
    }

    /// The names of the projects, in the order they were made.
    pub fn project_names(&self) -> Vec<String> {
        self.state()
            .projects
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    /// The text of the current version of a file of the project of this name.
    pub fn file(&self, project: &str, file: &str) -> Option<String> {
        let st = self.state();
        let p = st.projects.iter().find(|p| p.name == project)?;
        p.files.get(file)?.last().cloned()
    }

    /// How many versions a file of the project of this name has.
    pub fn versions(&self, project: &str, file: &str) -> usize {
        let st = self.state();
        st.projects
            .iter()
            .find(|p| p.name == project)
            .and_then(|p| p.files.get(file))
            .map_or(0, Vec::len)
    }

    /// Writes a file as the server would for another person (a change that the client did not see coming).
    pub fn write_behind(&self, project: &str, file: &str, text: &str) {
        let mut st = self.state();
        if let Some(p) = st.projects.iter_mut().find(|p| p.name == project) {
            p.files
                .entry(file.to_string())
                .or_default()
                .push(text.to_string());
        }
    }

    /// Answers one request: the method, the target (path and query), the token of `Authorization`, the tag of `If-Match`, the type of the content and
    /// the body.
    pub fn handle(
        &self,
        method: &str,
        target: &str,
        bearer: Option<&str>,
        if_match: Option<&str>,
        content_type: Option<&str>,
        body: &str,
    ) -> (u16, String) {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let mut st = self.state();
        st.seen.push(Seen {
            method: method.to_string(),
            path: path.to_string(),
            if_match: if_match.map(str::to_string),
            content_type: content_type.map(str::to_string),
        });
        if method == "GET" && path == "/api/v1/health" {
            return (
                200,
                "{\"name\":\"eh3d-server\",\"version\":\"0.0.0-memory\",\"api_revision\":1,\"status\":\"ok\"}".to_string(),
            );
        }
        if bearer != Some(st.token.as_str()) {
            return error(401, "unauthorized");
        }
        let Some(rest) = path.strip_prefix("/api/v1/apps/cad/projects") else {
            return error(404, "not_found");
        };
        if rest.is_empty() {
            return match method {
                "GET" => list(&st, query),
                "POST" => make(&mut st, body),
                _ => error(404, "not_found"),
            };
        }
        let Some(rest) = rest.strip_prefix('/') else {
            return error(404, "not_found");
        };
        let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let Some(index) = st.projects.iter().position(|p| p.id == id) else {
            return error(404, "unknown_project");
        };
        match (method, tail) {
            ("GET", "") => (200, project_json(&st.projects[index])),
            ("PUT", "files/content") => {
                let require = st.require_if_match;
                write(&mut st.projects[index], query, if_match, body, require)
            }
            ("GET", "files/content") => read(&st.projects[index], query),
            _ => error(404, "not_found"),
        }
    }
}

fn list(st: &State, query: &str) -> (u16, String) {
    let q = query_get(query, "q").unwrap_or_default().to_lowercase();
    let limit = query_get(query, "limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, 500);
    let offset = query_get(query, "offset")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let found: Vec<&Project> = st
        .projects
        .iter()
        .rev()
        .filter(|p| p.name.to_lowercase().contains(&q))
        .collect();
    let items: Vec<String> = found
        .iter()
        .skip(offset)
        .take(limit)
        .map(|p| summary_json(p))
        .collect();
    (
        200,
        format!(
            "{{\"total\":{},\"offset\":{offset},\"limit\":{limit},\"items\":[{}]}}",
            found.len(),
            items.join(",")
        ),
    )
}

fn make(st: &mut State, body: &str) -> (u16, String) {
    let name = eh3d_serverclient::Json::parse(body)
        .ok()
        .and_then(|j| j.get("name").and_then(|n| n.as_str().map(str::to_string)));
    let Some(name) = name else {
        return error(400, "bad_request");
    };
    if name.trim().is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return error(422, "bad_name");
    }
    let id = format!("p-{:04}", st.next);
    st.next += 1;
    st.projects.push(Project {
        id,
        name,
        files: BTreeMap::new(),
    });
    let made = st.projects.last().map(project_json).unwrap_or_default();
    (201, made)
}

fn write(
    p: &mut Project,
    query: &str,
    if_match: Option<&str>,
    body: &str,
    require: bool,
) -> (u16, String) {
    let Some(name) = query_get(query, "name") else {
        return error(400, "bad_request");
    };
    if bad_file_name(&name) {
        return error(422, "bad_filename");
    }
    let versions = p.files.entry(name.clone()).or_default();
    if let Some(current) = versions.last() {
        let tag = tag_of(current);
        match if_match {
            None if require => return error(428, "precondition_required"),
            Some(t) if t != tag => return error(412, "precondition_failed"),
            _ => {}
        }
        if current == body {
            return (
                200,
                format!(
                    "{{\"name\":{},\"version\":{},\"sha256\":{},\"size\":{},\"unchanged\":true}}",
                    quote(&name),
                    versions.len(),
                    quote(&tag),
                    body.len()
                ),
            );
        }
    }
    versions.push(body.to_string());
    (
        201,
        format!(
            "{{\"name\":{},\"version\":{},\"sha256\":{},\"size\":{},\"unchanged\":false}}",
            quote(&name),
            versions.len(),
            quote(&tag_of(body)),
            body.len()
        ),
    )
}

fn read(p: &Project, query: &str) -> (u16, String) {
    let Some(name) = query_get(query, "name") else {
        return error(400, "bad_request");
    };
    let Some(versions) = p.files.get(&name) else {
        return error(404, "unknown_file");
    };
    let number = query_get(query, "version").and_then(|v| v.parse::<usize>().ok());
    let chosen = match number {
        Some(n) => n.checked_sub(1).and_then(|i| versions.get(i)),
        None => versions.last(),
    };
    match chosen {
        Some(text) => (200, text.clone()),
        None => error(404, "unknown_file"),
    }
}

impl Transport for MemoryStore {
    fn send(&self, request: &Request) -> Result<Reply, TransportError> {
        let (path, tag) = split_if_match(&request.path);
        let (status, body) = self.handle(
            &request.method,
            path,
            request.bearer(),
            tag,
            None,
            request.body.as_deref().unwrap_or(""),
        );
        Ok(Reply { status, body })
    }
}

/// A store behind a socket of this computer. It stops (and its thread ends) when this value is dropped.
pub struct Served {
    /// `127.0.0.1:PORT`, for `server_address`.
    pub address: String,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Serves the store over plain HTTP/1.1 on a free port of this computer.
pub fn serve(store: &MemoryStore) -> std::io::Result<Served> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?.to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let (flag, store) = (stop.clone(), store.clone());
    let join = std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = answer(stream, &store);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
    });
    Ok(Served {
        address,
        stop,
        join: Some(join),
    })
}

fn answer(mut stream: TcpStream, store: &MemoryStore) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    let split = loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        raw.extend_from_slice(&chunk[..n]);
        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if raw.len() > 1 << 20 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (mut bearer, mut if_match, mut kind, mut length) = (None, None, None, 0usize);
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "authorization" => bearer = v.strip_prefix("Bearer ").map(str::to_string),
            "if-match" => if_match = Some(v.trim_matches('"').to_string()),
            "content-type" => kind = Some(v.to_string()),
            "content-length" => length = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    let mut body = raw[split + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&body).to_string();
    let (status, out) = store.handle(
        method,
        target,
        bearer.as_deref(),
        if_match.as_deref(),
        kind.as_deref(),
        &text,
    );
    let reason = match status {
        200 => "OK",
        201 => "Created",
        _ => "Error",
    };
    let reply = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    stream.write_all(reply.as_bytes())?;
    stream.flush()
}
