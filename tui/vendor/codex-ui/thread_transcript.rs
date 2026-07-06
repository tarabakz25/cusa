// Vendored from openai/codex codex-rs/tui — see UPSTREAM

// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Decoupled transcript cell list types for cusa (SPEC-107).
// Upstream `thread_transcript.rs` loaded app-server threads; cusa builds cells
// from `HistoryCellView` in `codex_adapter::transcript` instead.

use std::sync::Arc;

use super::history_cell::HistoryCell;

/// Ordered transcript cells ready for viewport rendering.
pub type TranscriptCells = Vec<Arc<dyn HistoryCell>>;
