// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Adapter boundary between cusa domain state and vendored Codex UI modules
// (SPEC-104, SPEC-105, SPEC-106). Vendor code must not import `crate::app` or sidecar.

pub mod composer;
pub mod shim;
pub mod status_chrome;
pub mod terminal_probe;
pub mod tool_display;
pub mod transcript;
pub mod types;
pub mod view_model;

pub use composer::{ComposerKeyResult, ComposerWidget, handle_composer_key};
pub use types::{
    ApprovalModeView, ComposerView, HistoryCellView, RouterSourceView, RunPhaseView,
};
pub use transcript::{CodexTranscriptWidget, render_transcript_lines, views_to_transcript_cells};
pub use view_model::CusaViewModel;
