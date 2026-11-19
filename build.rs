//! Bundle the built frontend (`ui/dist`) into the binary, so a
//! `cargo install`ed `data-diff --ui` serves the app with no files on disk.
//! The server still prefers the on-disk `dist` when it exists, so the
//! development watch build keeps working. With no `dist` present the
//! embedded table is empty and the server reports "frontend not built".

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("ui/dist");
    let dist = dist.canonicalize().unwrap_or(dist);
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut files = Vec::new();
    visit(&dist, &mut files);
    files.sort();
    // A release binary has no watch build to recreate `dist` (debug
    // `cargo run` does), so an empty embedded frontend is only a problem
    // there — e.g. `cargo install --path .` from a fresh checkout.
    if files.is_empty() && std::env::var("PROFILE").as_deref() == Ok("release") {
        println!(
            "cargo:warning=ui/dist is missing; --ui will report \"frontend not built\". \
             Build the frontend first with `(cd ui && npm install && npm run build)`."
        );
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded_dist.rs");
    let mut source = String::from("pub static EMBEDDED_DIST: &[(&str, &[u8])] = &[\n");
    for file in files {
        let relative = file.strip_prefix(&dist).unwrap();
        let name = relative.to_str().unwrap().replace('\\', "/");
        source.push_str(&format!(
            "    ({name:?}, include_bytes!({file:?})),\n",
            name = name,
            file = file.display().to_string(),
        ));
    }
    source.push_str("];\n");
    std::fs::write(out, source).unwrap();
}

fn visit(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, files);
        } else {
            files.push(path);
        }
    }
}
