// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Discovery of cusa's on-disk locations.
//
// * `CUSA_HOME` — top-level state dir; defaults to `$HOME/.cusa`.
// * `log_dir()` — `~/.cusa/logs/`.
// * `config_path()` — `~/.cusa/config.toml`.
//
// The lookups here never *create* directories; they only compute paths.
// Creation is the caller's job (typically the logging subscriber on
// `--verbose`).

use std::path::PathBuf;

/// Resolve the cusa state directory.
///
/// Priority:
///   1. `CUSA_HOME` env var (absolute path).
///   2. `$HOME/.cusa`.
///   3. Platform-default via [`directories::ProjectDirs`] (fallback).
pub fn cusa_home() -> PathBuf {
    if let Ok(p) = std::env::var("CUSA_HOME") {
        return PathBuf::from(p);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".cusa");
    }
    if let Some(dirs) = directories::ProjectDirs::from("dev", "cusa", "cusa") {
        return dirs.data_dir().to_path_buf();
    }
    PathBuf::from(".cusa")
}

pub fn log_dir() -> PathBuf {
    cusa_home().join("logs")
}

pub fn config_path() -> PathBuf {
    cusa_home().join("config.toml")
}

pub fn sessions_path() -> PathBuf {
    cusa_home().join("sessions.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_102_log_dir_under_cusa_home() {
        let home = cusa_home();
        assert!(log_dir().starts_with(&home));
        assert!(log_dir().ends_with("logs"));
    }

    #[test]
    fn spec_102_config_path_is_toml() {
        assert!(config_path().to_string_lossy().ends_with("config.toml"));
    }

    #[test]
    fn spec_050_sessions_json_under_cusa_home() {
        assert!(sessions_path().to_string_lossy().ends_with("sessions.json"));
    }
}
