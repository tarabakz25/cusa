// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Integration test that mirrors `scripts/check-headers.sh` in-process. This
// gives the compliance requirement a Rust-visible SPEC-tagged test so
// `grep 'SPEC-[0-9]+' tui/src` catches SPEC-083 without shelling out.

use std::fs;
use std::path::{Path, PathBuf};

const MARKERS: &[&str] = &[
    "SPDX-License-Identifier: Apache-2.0",
    "Licensed under the Apache License, Version 2.0",
];

fn walk_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            walk_rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn workspace_src() -> PathBuf {
    // CARGO_MANIFEST_DIR = tui/, so src is one step down.
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("src")
}

#[test]
fn spec_083_every_rust_file_carries_apache_header() {
    let mut files = Vec::new();
    walk_rust_files(&workspace_src(), &mut files);
    assert!(
        !files.is_empty(),
        "no .rs files found under {}",
        workspace_src().display()
    );
    let mut missing = Vec::new();
    for f in &files {
        let contents = fs::read_to_string(f).unwrap();
        let head: String = contents.lines().take(40).collect::<Vec<_>>().join("\n");
        if !MARKERS.iter().any(|m| head.contains(m)) {
            missing.push(f.clone());
        }
    }
    assert!(missing.is_empty(), "missing Apache-2.0 header in: {missing:?}");
}
