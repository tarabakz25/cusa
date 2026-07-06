// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// cusa-tui library crate — shared by the binary and integration tests.
//
// The binary entry point lives in `main.rs`; this crate exposes the app,
// sidecar, and supporting modules for `cargo test -p cusa-tui` integration
// tests (SPEC-110 snapshot harness).

#![allow(dead_code, unused_imports)]

pub mod app;
pub mod codex_adapter;
pub mod codex_ui;
pub mod config;
pub mod logging;
pub mod session_store;
pub mod sidecar;
pub mod terminal;
