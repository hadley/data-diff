//! A minimal HTTP/1.1 server over `std::net`, deliberately dependency-free:
//! the app is a local single-user tool, and its whole traffic is small JSON
//! pages and the bundled frontend. One thread per connection is far past
//! need.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::commands;
use crate::session::Session;

pub struct Server {
    session: Mutex<Option<Session>>,
    dist: PathBuf,
    live_reload: bool,
}

impl Server {
    pub fn new(initial: Option<Session>, dist: PathBuf) -> Self {
        Self {
            session: Mutex::new(initial),
            dist,
            live_reload: false,
        }
    }

    /// Serve `/api/reload` and inject the reload script into HTML — the
    /// development mode `main` switches on alongside the watch build.
    pub fn with_live_reload(mut self) -> Self {
        self.live_reload = true;
        self
    }

    /// Serve until the process ends.
    pub fn listen(self, address: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(address)?;
        self.serve(listener)
    }

    /// Serve on an already-bound listener — how tests get an ephemeral port.
    pub fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        let server = Arc::new(self);
        for connection in listener.incoming() {
            match connection {
                Ok(stream) => {
                    let server = Arc::clone(&server);
                    std::thread::spawn(move || server.handle(stream));
                }
                Err(error) => eprintln!("data-diff-ui: connection failed: {error}"),
            }
        }
        Ok(())
    }

    fn handle(&self, mut stream: std::net::TcpStream) {
        if let Err(error) = self.respond(&mut stream) {
            eprintln!("data-diff-ui: request failed: {error}");
        }
    }

    fn respond(&self, stream: &mut std::net::TcpStream) -> std::io::Result<()> {
        let request = read_request(stream)?;
        let Some(request) = request else {
            return Ok(());
        };
        let (status, content_type, body) = self.route(&request);
        let head = format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(&body)
    }

    fn route(&self, request: &Request) -> (u16, &'static str, Vec<u8>) {
        let (path, query) = request
            .target
            .split_once('?')
            .unwrap_or((request.target.as_str(), ""));
        let query = Query(query);
        match (request.method.as_str(), path) {
            ("GET", "/api/reload") if self.live_reload => (
                200,
                "text/plain",
                crate::dev::dist_stamp(&self.dist).to_string().into_bytes(),
            ),
            ("GET", "/api/session") => self.json(|| Ok(self.summary())),
            ("POST", "/api/open") => self.json(|| {
                let body: OpenRequest =
                    serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
                let session = Session::open(
                    Path::new(&body.old),
                    Path::new(&body.new),
                    body.key,
                    body.hints,
                )
                .map_err(|error| error.to_string())?;
                let summary = commands::session_summary(&session);
                *self.session.lock().map_err(|error| error.to_string())? = Some(session);
                Ok(Some(summary))
            }),
            ("GET", "/api/schema") => self.json(|| {
                self.with(|session| {
                    commands::schema_panel(session, query.flag("changed_only", true))
                })
            }),
            ("GET", "/api/cells") => self.json(|| {
                self.with(|session| {
                    commands::cells_page(
                        session,
                        query.text("sort", "key"),
                        query.flag("added_dropped", false),
                        query.number("page").unwrap_or(0) as usize,
                        query.number("page_size").unwrap_or(50) as usize,
                    )
                })
            }),
            ("GET", "/api/column-view") => self.json(|| {
                self.with(|session| {
                    commands::column_view(
                        session,
                        query.flag("all_columns", false),
                        query.flag("all_rows", false),
                        query.flag("added_dropped", false),
                        query.number("group").map(|group| group as usize),
                        query.number("page").unwrap_or(0) as usize,
                        query.number("page_size").unwrap_or(50) as usize,
                    )
                })
            }),
            ("GET", "/api/edited-groups") => self.json(|| self.with(commands::edited_groups)),
            ("GET", "/api/column-groups") => {
                self.json(|| self.with(commands::edited_column_groups))
            }
            ("GET", "/api/row-view") => self.json(|| {
                self.with(|session| {
                    commands::row_view_section(
                        session,
                        query.text("kind", "edited"),
                        query.flag("all_columns", false),
                        query.number("group").map(|group| group as usize),
                        match query.text("side", "") {
                            "old" => Some(data_diff::Side::Old),
                            "new" => Some(data_diff::Side::New),
                            _ => None,
                        },
                        query.number("page").unwrap_or(0) as usize,
                        query.number("page_size").unwrap_or(50) as usize,
                    )
                })
            }),
            // An unknown API route is an error, not the app shell: returning
            // HTML here would surface as a JSON parse failure in the UI.
            (_, p) if p.starts_with("/api/") => (
                400,
                "application/json",
                serde_json::to_vec(&ErrorBody {
                    error: format!("unknown route {p}"),
                })
                .expect("the error body serializes"),
            ),
            ("GET", _) => self.static_file(path),
            _ => (405, "text/plain", b"method not allowed".to_vec()),
        }
    }

    fn summary(&self) -> Option<crate::dto::SessionSummaryDto> {
        self.session
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(commands::session_summary))
    }

    /// Run `command` against the open session, or the one error every
    /// session-less route shares.
    fn with<T>(&self, command: impl FnOnce(&Session) -> T) -> Result<T, String> {
        let guard = self.session.lock().map_err(|error| error.to_string())?;
        let session = guard.as_ref().ok_or("no session is open")?;
        Ok(command(session))
    }

    /// Serialize a handler's result, mapping its error to a 400.
    fn json<T: Serialize>(
        &self,
        handler: impl FnOnce() -> Result<T, String>,
    ) -> (u16, &'static str, Vec<u8>) {
        match handler() {
            Ok(value) => (
                200,
                "application/json",
                serde_json::to_vec(&value).expect("DTOs serialize"),
            ),
            Err(error) => (
                400,
                "application/json",
                serde_json::to_vec(&ErrorBody { error }).expect("the error body serializes"),
            ),
        }
    }

    fn static_file(&self, path: &str) -> (u16, &'static str, Vec<u8>) {
        let path = if path == "/" { "/index.html" } else { path };
        // No traversal: every component must be an ordinary path segment.
        if path
            .split('/')
            .any(|part| part == ".." || part.contains('\\'))
        {
            return (403, "text/plain", b"forbidden".to_vec());
        }
        let file = self.dist.join(path.trim_start_matches('/'));
        match std::fs::read(&file) {
            Ok(body) => (200, content_type(path), self.inject(body, path)),
            // The on-disk build wins; the bundled copy is the fallback for
            // installed binaries with no `dist` beside them.
            Err(_) => match crate::embedded::get(path) {
                Some(body) => (200, content_type(path), self.inject(body.to_vec(), path)),
                // A client-side route or a missing file gets the app shell.
                None if !path.contains('.') => self.app_shell(),
                None => (404, "text/plain", b"not found".to_vec()),
            },
        }
    }

    fn app_shell(&self) -> (u16, &'static str, Vec<u8>) {
        if let Ok(body) = std::fs::read(self.dist.join("index.html")) {
            return (200, "text/html", self.inject(body, "/index.html"));
        }
        match crate::embedded::get("/index.html") {
            Some(body) => (200, "text/html", self.inject(body.to_vec(), "/index.html")),
            None => (404, "text/plain", b"frontend not built".to_vec()),
        }
    }

    /// Append the reload script to HTML pages while live reload is on.
    fn inject(&self, body: Vec<u8>, path: &str) -> Vec<u8> {
        if self.live_reload && path.ends_with(".html") {
            let mut body = body;
            body.extend_from_slice(crate::dev::RELOAD_SCRIPT.as_bytes());
            body
        } else {
            body
        }
    }
}

#[derive(serde::Deserialize)]
struct OpenRequest {
    old: String,
    new: String,
    #[serde(default)]
    key: Vec<String>,
    #[serde(default)]
    hints: Vec<String>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

struct Request {
    method: String,
    target: String,
    body: Vec<u8>,
}

/// Read one request; `None` on an empty connection, which browsers open
/// speculatively.
fn read_request(stream: &mut std::net::TcpStream) -> std::io::Result<Option<Request>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 && buffer.is_empty() {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            break end;
        }
        if read == 0 || buffer.len() > 1 << 20 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "malformed request head",
            ));
        }
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Ok(Some(Request {
        method,
        target,
        body,
    }))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Query-string access: flags as `=true`, numbers as bare integers.
struct Query<'a>(&'a str);

impl Query<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    }

    fn flag(&self, name: &str, default: bool) -> bool {
        self.get(name)
            .map(|value| value == "true")
            .unwrap_or(default)
    }

    fn number(&self, name: &str) -> Option<u32> {
        self.get(name).and_then(|value| value.parse().ok())
    }

    fn text<'a>(&'a self, name: &str, default: &'a str) -> &'a str {
        self.get(name).unwrap_or(default)
    }
}
