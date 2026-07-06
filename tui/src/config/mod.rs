// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Static configuration loader (Slice 2 stub).
//
// Slice 2 only cares about (a) discovering `~/.cusa/` paths and (b) shipping
// a stable default set of values that later slices override. The full
// TOML-backed loader (with `router.toml`, MCP overrides, and the API key
// convention from SPEC-101) lives in later slices.

pub mod api_key;
pub mod paths;

pub use api_key::{api_key_configured, parse_api_key_from_toml, resolve_api_key, write_api_key};
pub use paths::{config_path, cusa_home, log_dir, sessions_path};

/// Runtime settings the TUI cares about at boot.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub default_model: String,
    pub approval_mode: cusa_rpc::ApprovalMode,
    pub verbose: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_model: "auto".to_string(),
            approval_mode: cusa_rpc::ApprovalMode::Suggest,
            verbose: false,
        }
    }
}
