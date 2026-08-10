//! End-to-end tests: the real server, real TCP requests, real Parquet
//! files. Every route the frontend's buttons and toggles call is exercised,
//! error paths included.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use arrow_array::RecordBatch;
use data_diff::{diff_tables, DiffOptions};
use data_diff_ui::http::Server;
use data_diff_ui::session::Session;
use parquet::arrow::ArrowWriter;
use test_support::table;

/// The same shape as the command tests' fixture: edits, a rename, a drop,
/// an add, an added row, and a dropped row.
fn tables() -> (RecordBatch, RecordBatch) {
    let old = table! {
        "id" => [1, 2, 3, 4, 6],
        "price" => [9, 14, 20, 7, 1],
        "name" => ["a", "b", "c", "d", "x"],
        "qty" => [1, 2, 3, 4, 5],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5],
        "price" => [9, 16, 21, 8, 2],
        "label" => ["a", "b", "c", "d", "w"],
        "sku" => ["x", "y", "z", "w", "v"],
    };
    (old, new)
}

fn session() -> Session {
    let (old, new) = tables();
    let diff = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: vec!["id".to_owned()],
            ..DiffOptions::default()
        },
    )
    .unwrap();
    Session {
        old_path: PathBuf::from("old.parquet"),
        new_path: PathBuf::from("new.parquet"),
        key: vec!["id".to_owned()],
        hints: Vec::new(),
        old,
        new,
        diff,
    }
}

fn start(initial: Option<Session>, dist: PathBuf) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || Server::new(initial, dist).serve(listener));
    address
}

/// One request, returning the status and the body.
fn request(address: SocketAddr, method: &str, target: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(address).unwrap();
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\nhost: localhost\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let raw = String::from_utf8(raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap();
    (status, body.to_owned())
}

fn get(address: SocketAddr, target: &str) -> (u16, serde_json::Value) {
    let (status, body) = request(address, "GET", target, None);
    (
        status,
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null),
    )
}

fn post(address: SocketAddr, target: &str, body: &str) -> (u16, serde_json::Value) {
    let (status, body) = request(address, "POST", target, Some(body));
    (
        status,
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null),
    )
}

fn write_parquet(path: &Path, batch: &RecordBatch) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
    writer.write(batch).unwrap();
    writer.close().unwrap();
}

static TEMP: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "data-diff-ui-http-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn every_route_serves_an_open_session() {
    let address = start(Some(session()), PathBuf::from("/nonexistent"));

    // The summary the app opens with.
    let (status, summary) = get(address, "/api/session");
    assert_eq!(status, 200);
    assert_eq!(summary["cells"], 3);
    assert_eq!(summary["edited_rows"], 3);
    assert_eq!(summary["added_rows"], 1);
    assert_eq!(summary["dropped_rows"], 1);
    assert!(summary["schema"].as_array().unwrap().len() >= 3);

    // The schema panel, both toggle states.
    let (status, changed) = get(address, "/api/schema?changed_only=true");
    assert_eq!(status, 200);
    let (status, all) = get(address, "/api/schema?changed_only=false");
    assert_eq!(status, 200);
    assert!(all.as_array().unwrap().len() > changed.as_array().unwrap().len());

    // The cell view: default page, both sorts, both filters, second page.
    let (status, cells) = get(address, "/api/cells?page=0&page_size=2");
    assert_eq!(status, 200);
    assert_eq!(cells["total"], 3);
    assert_eq!(cells["items"].as_array().unwrap().len(), 2);
    for target in [
        "/api/cells?sort=column",
        "/api/cells?column=2",
        "/api/cells?row=2",
        "/api/cells?page=1&page_size=2",
    ] {
        let (status, _) = get(address, target);
        assert_eq!(status, 200, "{target}");
    }

    // The column view, every toggle combination.
    for target in [
        "/api/column-view",
        "/api/column-view?all_columns=true",
        "/api/column-view?all_rows=true",
        "/api/column-view?added_dropped=true",
        "/api/column-view?all_columns=true&all_rows=true&added_dropped=true&page=0&page_size=2",
    ] {
        let (status, view) = get(address, target);
        assert_eq!(status, 200, "{target}");
        // The fixture edits one column; context toggles only add more.
        assert!(!view["columns"].as_array().unwrap().is_empty(), "{target}");
    }

    // Every row-view section.
    for kind in ["edited", "added", "dropped", "moved", "fanout"] {
        let (status, _) = get(address, &format!("/api/row-view?kind={kind}"));
        assert_eq!(status, 200, "{kind}");
        let (status, _) = get(
            address,
            &format!("/api/row-view?kind={kind}&all_columns=true"),
        );
        assert_eq!(status, 200, "{kind} all_columns");
    }
}

#[test]
fn hints_round_trip_and_bad_hints_are_errors() {
    let address = start(Some(session()), PathBuf::from("/nonexistent"));

    // Split the name -> label rename into drop + add.
    let (status, summary) = post(
        address,
        "/api/hints",
        r#"{"hints": ["col_drop(name)", "col_add(label)"]}"#,
    );
    assert_eq!(status, 200);
    let schema = summary["schema"].as_array().unwrap();
    assert!(schema.iter().all(|row| row["basis"].is_null()));
    assert!(schema.iter().any(|row| row["status"] == "dropped"));

    // Join them back into a rename.
    let (status, summary) = post(
        address,
        "/api/hints",
        r#"{"hints": ["col_rename(name -> label)"]}"#,
    );
    assert_eq!(status, 200);
    let schema = summary["schema"].as_array().unwrap();
    assert!(schema.iter().any(|row| row["basis"] == "hinted"));

    // A malformed hint is a 400 with a readable error, not a dropped
    // connection.
    let (status, error) = post(address, "/api/hints", r#"{"hints": ["bogus"]}"#);
    assert_eq!(status, 400);
    assert!(error["error"].as_str().unwrap().contains("bogus"));
}

#[test]
fn opening_files_over_http_replaces_the_session() {
    let dir = temp_dir();
    let (old, new) = tables();
    let old_path = dir.join("old.parquet");
    let new_path = dir.join("new.parquet");
    write_parquet(&old_path, &old);
    write_parquet(&new_path, &new);

    // The server starts empty: every data route is a clean 400.
    let address = start(None, PathBuf::from("/nonexistent"));
    let (status, session) = get(address, "/api/session");
    assert_eq!(status, 200);
    assert!(session.is_null());
    let (status, error) = get(address, "/api/cells");
    assert_eq!(status, 400);
    assert_eq!(error["error"], "no session is open");

    // The open form's button.
    let body = format!(
        r#"{{"old": "{}", "new": "{}", "key": ["id"], "hints": []}}"#,
        old_path.display(),
        new_path.display()
    );
    let (status, summary) = post(address, "/api/open", &body);
    assert_eq!(status, 200);
    assert_eq!(summary["cells"], 3);
    let (status, cells) = get(address, "/api/cells");
    assert_eq!(status, 200);
    assert_eq!(cells["total"], 3);

    // A missing file is a 400 naming it, not a panic.
    let (status, error) = post(
        address,
        "/api/open",
        r#"{"old": "/nonexistent.parquet", "new": "/nonexistent.parquet"}"#,
    );
    assert_eq!(status, 400);
    assert!(error["error"].as_str().unwrap().contains("nonexistent"));
}

#[test]
fn static_routes_and_unknown_apis_behave() {
    let dir = temp_dir();
    std::fs::write(dir.join("index.html"), "<html>app</html>").unwrap();
    std::fs::write(dir.join("app.js"), "console.log(1)").unwrap();
    let address = start(Some(session()), dir);

    let (status, body) = request(address, "GET", "/", None);
    assert_eq!(status, 200);
    assert!(body.contains("app"));
    let (status, _) = request(address, "GET", "/app.js", None);
    assert_eq!(status, 200);
    let (status, _) = request(address, "GET", "/missing.png", None);
    assert_eq!(status, 404);
    let (status, _) = request(address, "GET", "/../Cargo.toml", None);
    assert!(status == 403 || status == 404);

    // An unknown API route is a JSON error, not the app shell.
    let (status, error) = get(address, "/api/bogus");
    assert_eq!(status, 400);
    assert!(error["error"].as_str().unwrap().contains("bogus"));

    let (status, _) = request(address, "DELETE", "/api/cells", None);
    assert_eq!(status, 400);
}

#[test]
fn malformed_requests_get_errors_not_crashes() {
    let address = start(Some(session()), PathBuf::from("/nonexistent"));

    let (status, _) = post(address, "/api/hints", "not json");
    assert_eq!(status, 400);
    let (status, _) = post(address, "/api/open", r#"{"old": 1}"#);
    assert_eq!(status, 400);

    // The server is still up after all of it.
    let (status, _) = get(address, "/api/session");
    assert_eq!(status, 200);
}
