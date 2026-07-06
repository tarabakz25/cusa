// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Vendored from OpenAI `codex-rs/tui` (Apache-2.0). See `UPSTREAM` for the pin
// SHA and `README.md` for the cherry-pick procedure (SPEC-103, SPEC-105, SPEC-107).

#![allow(dead_code, clippy::all)]

// P0 foundation
pub mod color;
pub mod custom_terminal;
pub mod render;
pub mod style;
pub mod terminal_palette;
pub mod text_formatting;
pub mod ui_consts;
pub mod width;
pub mod wrapping;

// P1 bottom pane / composer (SPEC-106)
pub mod key_hint;
pub mod keymap;
pub mod bottom_pane;

// P2 transcript pipeline (SPEC-107)
pub mod table_detect;
pub mod terminal_hyperlinks;
pub mod markdown_text_merge;
pub mod markdown_render;
pub mod markdown;
pub mod markdown_stream;
pub mod insert_history;
pub mod streaming;
pub mod transcript_reflow;
pub mod history_cell;
pub mod thread_transcript;

// P3 tool display (SPEC-108)
pub mod diff_model;
pub mod diff_render;
pub mod exec_cell;
pub mod exec_command;

// P4 status chrome (SPEC-109)
pub mod motion;
pub mod shimmer;
pub mod status_chrome;
