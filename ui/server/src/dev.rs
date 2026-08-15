//! Development-only live reload: rebuild the frontend on change and reload
//! the browser when the build output changes. Compiled into every build but
//! only enabled from `main` under `debug_assertions`, so release binaries
//! never spawn npm or inject the reload script.

use std::path::Path;
use std::time::SystemTime;

/// Start `npm run build:watch` in the frontend directory (the parent of
/// `dist`). `None` — with a warning — when npm is missing.
pub fn spawn_frontend_watch(dist: &Path) -> Option<Watcher> {
    let Some(ui) = dist.parent() else {
        eprintln!("data-diff-ui: cannot locate the frontend directory; live reload off");
        return None;
    };
    match std::process::Command::new("npm")
        .args(["run", "build:watch"])
        .current_dir(ui)
        .spawn()
    {
        Ok(child) => {
            println!("data-diff-ui: watching {} for changes", ui.display());
            Some(Watcher(child))
        }
        Err(error) => {
            eprintln!("data-diff-ui: cannot start npm ({error}); live reload off");
            None
        }
    }
}

/// Keeps the watch build alive for exactly as long as the server runs.
pub struct Watcher(std::process::Child);

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

/// A fingerprint of the built frontend: the newest modification time under
/// `dist` plus the file count, so both edits and added files register.
pub fn dist_stamp(dist: &Path) -> u128 {
    let mut newest = 0_u128;
    let mut count = 0_u128;
    visit(dist, &mut |path| {
        count += 1;
        if let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) {
            if let Ok(since) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                newest = newest.max(since.as_nanos());
            }
        }
    });
    newest + count
}

fn visit(dir: &Path, each: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, each);
        } else {
            each(&path);
        }
    }
}

/// The script injected into served HTML while live reload is on: poll the
/// stamp once a second and reload when it moves.
pub const RELOAD_SCRIPT: &str = "<script>(function(){var v=null;setInterval(function(){fetch('/api/reload').then(function(r){return r.text()}).then(function(t){if(v===null){v=t}else if(t!==v){location.reload()}}).catch(function(){})},1000)})();</script>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_changes_when_a_file_changes() {
        let dir = std::env::temp_dir().join(format!("data-diff-ui-dev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"one").unwrap();
        let before = dist_stamp(&dir);
        std::fs::write(dir.join("b.txt"), b"two").unwrap();
        let after = dist_stamp(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_ne!(before, after);
    }

    #[test]
    fn missing_dist_stamps_to_zero() {
        assert_eq!(dist_stamp(Path::new("/no/such/dir")), 0);
    }
}
