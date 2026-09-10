//! The frontend bundled at compile time by `build.rs`: `(path, bytes)`
//! pairs for every file under `ui/dist`, empty when the frontend was not
//! built. Serving prefers the on-disk `dist`, so this is the fallback that
//! lets an installed binary stand alone.

include!(concat!(env!("OUT_DIR"), "/embedded_dist.rs"));

/// The bundled file at `path` (a URL path like `/assets/index.js`), if any.
pub fn get(path: &str) -> Option<&'static [u8]> {
    let path = path.trim_start_matches('/');
    EMBEDDED_DIST
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn lookups_ignore_the_leading_slash() {
        for (name, bytes) in super::EMBEDDED_DIST {
            assert_eq!(super::get(name), Some(*bytes));
            assert_eq!(super::get(&format!("/{name}")), Some(*bytes));
        }
    }
}
