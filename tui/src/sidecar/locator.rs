// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Sidecar entry-point resolution.
//
// Search order (SPEC-071):
//   1. `--sidecar <path>` CLI flag (highest priority).
//   2. `CUSA_SIDECAR` environment variable (set by the npm shim in prod).
//   3. Fallback: search `../sidecar/dist/index.js` and
//      `../../sidecar/dist/index.js` relative to the running binary.
//
// The `node` executable is taken from `CUSA_NODE` if set, otherwise `node`
// from `$PATH`.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// A resolved sidecar location.
#[derive(Debug, Clone)]
pub struct SidecarLocator {
    /// Node executable to invoke.
    pub node: PathBuf,
    /// JS entry to pass to `node`.
    pub entry: PathBuf,
}

/// Resolve the sidecar entry using CLI, env, and fallback locations.
///
/// `cli_arg` corresponds to the `--sidecar` flag. `env` is a snapshot of the
/// process environment (accepting a HashMap allows tests to drive the
/// function without mutating global state).
pub fn resolve_sidecar_entry(
    cli_arg: Option<&str>,
    env: &dyn EnvLookup,
    exe_path: Option<&Path>,
) -> Result<SidecarLocator> {
    let node = env
        .get("CUSA_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("node"));

    if let Some(cli) = cli_arg {
        return Ok(SidecarLocator {
            node,
            entry: PathBuf::from(cli),
        });
    }
    if let Some(v) = env.get("CUSA_SIDECAR") {
        return Ok(SidecarLocator {
            node,
            entry: PathBuf::from(v),
        });
    }

    // Fallback: look next to the running binary.
    if let Some(exe) = exe_path {
        let candidates = fallback_candidates(exe);
        for cand in &candidates {
            if cand.exists() {
                return Ok(SidecarLocator {
                    node,
                    entry: cand.clone(),
                });
            }
        }
    }

    Err(anyhow!(
        "could not locate sidecar entry (tried --sidecar, CUSA_SIDECAR, and fallback paths)"
    ))
}

/// Build the list of fallback candidate paths for the given executable.
pub fn fallback_candidates(exe: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dir = exe.parent().map(PathBuf::from);
    for _ in 0..3 {
        if let Some(d) = dir.as_ref() {
            out.push(d.join("sidecar/dist/index.js"));
            out.push(d.join("sidecar/index.js"));
        }
        dir = dir.as_ref().and_then(|d| d.parent().map(PathBuf::from));
    }
    out
}

/// Trait so tests can inject a fake environment lookup without touching the
/// real process env. In production `StdEnv` reads from `std::env::var`.
pub trait EnvLookup {
    fn get(&self, key: &str) -> Option<String>;
}

pub struct StdEnv;

impl EnvLookup for StdEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// Trivial in-memory env used by tests.
#[derive(Debug, Default, Clone)]
pub struct StaticEnv {
    map: std::collections::HashMap<String, String>,
}

impl StaticEnv {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(mut self, key: &str, value: &str) -> Self {
        self.map.insert(key.to_string(), value.to_string());
        self
    }
}

impl EnvLookup for StaticEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_071_cli_arg_wins() {
        let env = StaticEnv::new().set("CUSA_SIDECAR", "/env/path.js");
        let loc = resolve_sidecar_entry(Some("/cli/path.js"), &env, None).unwrap();
        assert_eq!(loc.entry, PathBuf::from("/cli/path.js"));
    }

    #[test]
    fn spec_071_env_used_when_cli_absent() {
        let env = StaticEnv::new().set("CUSA_SIDECAR", "/env/path.js");
        let loc = resolve_sidecar_entry(None, &env, None).unwrap();
        assert_eq!(loc.entry, PathBuf::from("/env/path.js"));
    }

    #[test]
    fn spec_071_node_defaults_to_node() {
        let env = StaticEnv::new();
        let loc = resolve_sidecar_entry(Some("/x.js"), &env, None).unwrap();
        assert_eq!(loc.node, PathBuf::from("node"));
    }

    #[test]
    fn spec_071_node_env_override() {
        let env = StaticEnv::new().set("CUSA_NODE", "/opt/node/bin/node");
        let loc = resolve_sidecar_entry(Some("/x.js"), &env, None).unwrap();
        assert_eq!(loc.node, PathBuf::from("/opt/node/bin/node"));
    }

    #[test]
    fn spec_071_fallback_candidates_include_parents() {
        let exe = PathBuf::from("/opt/cusa/target/debug/cusa-tui");
        let cands = fallback_candidates(&exe);
        assert!(cands
            .iter()
            .any(|p| p.ends_with("sidecar/dist/index.js")));
    }

    #[test]
    fn spec_071_missing_all_errors() {
        let env = StaticEnv::new();
        let err = resolve_sidecar_entry(None, &env, None).unwrap_err();
        assert!(err.to_string().contains("could not locate"));
    }
}
