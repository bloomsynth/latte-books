//! Loads the Frappe Books JSON schemas vendored in `vendor/books/schemas`.
//!
//! Port of `schemas/` @ v0.36.0. See `vendor/books/schemas/README.md` for how regional,
//! abstract and subclass schemas combine into the final schema.

use std::path::PathBuf;

/// Path to the vendored Frappe Books schema directory.
pub fn vendored_schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/books/schemas")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn json_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                json_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "json") {
                out.push(path);
            }
        }
    }

    #[test]
    fn every_vendored_schema_is_valid_json() {
        let mut files = Vec::new();
        json_files(&vendored_schema_dir(), &mut files);
        assert!(files.len() >= 70, "found only {} schema files", files.len());
        for path in files {
            let text = fs::read_to_string(&path).unwrap();
            serde_json::from_str::<serde_json::Value>(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}
