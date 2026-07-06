// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Tracing setup (SPEC-102).
//
// When `--verbose` is set, install a rolling file writer under
// `~/.cusa/logs/`. Otherwise, install a null subscriber so `tracing::*!`
// macros compile but produce no output — the TUI cannot afford stdout
// interference during rendering.

use crate::config::log_dir;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

/// Initialize tracing. Returns the resolved log-file path when `--verbose`
/// enables file logging; returns `None` when logging is silenced.
pub fn init(verbose: bool) -> Result<Option<PathBuf>> {
    if !verbose {
        // Silence tracing entirely so it never touches the terminal.
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_env_filter(EnvFilter::new("off"))
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        return Ok(None);
    }

    let dir = log_dir();
    fs::create_dir_all(&dir).with_context(|| format!("create log dir {}", dir.display()))?;
    let path = dir.join(format!("cusa-tui-{}.log", std::process::id()));
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open log file {}", path.display()))?;
    let subscriber = tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("install tracing subscriber")?;
    Ok(Some(path))
}
