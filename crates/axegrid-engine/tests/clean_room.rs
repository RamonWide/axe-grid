//! Guards the clean-room rules that a machine can check: no unlisted binary
//! assets, and none of the original program's file types, in the tree.

use std::path::{Path, PathBuf};

/// File types that are either binary assets or formats of the original
/// program. Any such file must be accounted for in ATTRIBUTION.md.
const WATCHED: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "icns", "svg", "webp", "ttf", "otf", "woff", "woff2", "stf", "wav",
    "mp3", "ogg", "ir", "syx", "cache", "laxml", "xml", "zip", "exe", "dll", "bin",
];
const SKIPPED_DIRS: &[&str] = &["target", ".git"];

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !SKIPPED_DIRS.iter().any(|skip| name == *skip) {
                collect(&path, found);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| WATCHED.contains(&e.to_ascii_lowercase().as_str()))
        {
            found.push(path);
        }
    }
}

#[test]
fn no_unattributed_assets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).unwrap();
    let mut found = Vec::new();
    collect(&root, &mut found);
    let unlisted: Vec<String> = found
        .iter()
        .filter_map(|path| path.strip_prefix(&root).ok())
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .filter(|relative| !attribution.contains(relative.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "files that need a row in ATTRIBUTION.md, or must be removed (see docs/clean-room.md): {unlisted:?}"
    );
}

#[test]
fn shipped_data_files_are_listed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).unwrap();
    let data = root.join("crates/axegrid-model/data");
    for entry in std::fs::read_dir(data).unwrap().flatten() {
        let listed = format!("crates/axegrid-model/data/{}", entry.file_name().to_string_lossy());
        assert!(attribution.contains(&listed), "{listed} is missing from ATTRIBUTION.md");
    }
}
