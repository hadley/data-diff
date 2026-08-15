use std::path::PathBuf;

use clap::Parser;
use data_diff_ui::http::Server;
use data_diff_ui::session::Session;

/// Interactive UI for data-diff, served to the browser. With two paths the
/// diff opens directly; without them the app opens on its file form. The
/// positional arguments are what a git difftool passes as `$LOCAL $REMOTE`.
#[derive(Debug, Parser)]
struct Cli {
    /// Original Parquet file.
    old: Option<PathBuf>,
    /// Modified Parquet file.
    new: Option<PathBuf>,
    /// Key columns, comma-separated, or old/new pairs.
    #[arg(long, value_delimiter = ',')]
    key: Vec<String>,
    /// A hint, repeatable: col_drop(old), col_add(new), col_edit(column),
    /// col_rename(old -> new).
    #[arg(long)]
    hint: Vec<String>,
    /// A file of hints, one per line; blanks and # comments skipped.
    #[arg(long)]
    hints: Option<PathBuf>,
    /// The port to serve on.
    #[arg(long, default_value_t = 9471)]
    port: u16,
}

fn main() {
    let cli = Cli::parse();
    let initial = match (cli.old, cli.new) {
        (Some(old), Some(new)) => {
            let mut hints = cli.hints.map(read_hints).unwrap_or_default();
            hints.extend(cli.hint);
            Some(
                Session::open(&old, &new, cli.key, hints).unwrap_or_else(|error| {
                    eprintln!("data-diff-ui: {error}");
                    std::process::exit(1);
                }),
            )
        }
        (None, None) => None,
        _ => {
            eprintln!("data-diff-ui: give both files or neither");
            std::process::exit(2);
        }
    };

    let address = format!("127.0.0.1:{}", cli.port);
    let url = format!("http://{address}");
    println!("data-diff-ui serving on {url}");
    open_browser(&url);

    let dist = dist();
    // Development builds rebuild the frontend on change and reload the
    // browser; release binaries serve the built frontend as-is.
    let _watcher = if cfg!(debug_assertions) {
        data_diff_ui::dev::spawn_frontend_watch(&dist)
    } else {
        None
    };
    let mut server = Server::new(initial, dist);
    if _watcher.is_some() {
        server = server.with_live_reload();
    }
    server.listen(&address).unwrap_or_else(|error| {
        eprintln!("data-diff-ui: cannot serve {address}: {error}");
        std::process::exit(1);
    });
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
        // target/{debug,release}/data-diff-ui -> the repository's ui/dist.
        let beside = exe
            .parent()
            .and_then(|dir| dir.parent())
            .and_then(|dir| dir.parent())
            .map(|dir| dir.join("ui/dist"));
        if let Some(path) = beside {
            if path.join("index.html").exists() {
                return path;
            }
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

fn read_hints(path: PathBuf) -> Vec<String> {
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            eprintln!("data-diff-ui: cannot read {}: {error}", path.display());
            std::process::exit(2);
        })
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}
