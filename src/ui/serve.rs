//! The `--ui` entry point: open a session over the two files, then serve
//! the app to the browser until the process ends.

use std::path::PathBuf;

use crate::ui::http::Server;
use crate::ui::session::Session;

/// Open the diff and serve the UI on `127.0.0.1:port` forever.
pub fn run(
    old: PathBuf,
    new: PathBuf,
    key: Vec<String>,
    hints: Vec<String>,
    port: u16,
) -> Result<(), String> {
    let session = Session::open(&old, &new, key, hints).map_err(|error| error.to_string())?;

    let address = format!("127.0.0.1:{port}");
    let url = format!("http://{address}");
    println!("data-diff serving the UI on {url}");
    open_browser(&url);

    let dist = dist();
    // Development builds rebuild the frontend on change and reload the
    // browser; release binaries serve the built frontend as-is.
    let _watcher = if cfg!(debug_assertions) {
        crate::ui::dev::spawn_frontend_watch(&dist)
    } else {
        None
    };
    let mut server = Server::new(Some(session), dist);
    if _watcher.is_some() {
        server = server.with_live_reload();
    }
    server
        .listen(&address)
        .map_err(|error| format!("cannot serve {address}: {error}"))
}

/// The built frontend: `$DATA_DIFF_UI_DIST`, then `ui/dist` under the working
/// directory, then next to a `target/`-built executable.
fn dist() -> PathBuf {
    if let Ok(path) = std::env::var("DATA_DIFF_UI_DIST") {
        return PathBuf::from(path);
    }
    let local = PathBuf::from("ui/dist");
    if local.join("index.html").exists() {
        return local;
    }
    if let Ok(exe) = std::env::current_exe() {
        // target/{debug,release}/data-diff -> the repository's ui/dist.
        let beside = exe
            .parent()
            .and_then(|dir| dir.parent())
            .and_then(|dir| dir.parent())
            .map(|dir| dir.join("ui/dist"));
        if let Some(path) = beside
            && path.join("index.html").exists()
        {
            return path;
        }
    }
    local
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let opener = ("open", vec![url]);
    #[cfg(target_os = "windows")]
    let opener = ("cmd", vec!["/C", "start", "", url]);
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let opener = ("xdg-open", vec![url]);
    let _ = std::process::Command::new(opener.0).args(&opener.1).spawn();
}
