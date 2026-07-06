// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Transcript domain types (SPEC-001). Rendering lives in
// `codex_adapter::CodexTranscriptWidget` (SPEC-107); legacy `TranscriptWidget`
// removed per SPEC-113.

use cusa_rpc::RouterSource;

/// One item in the transcript.
#[derive(Debug, Clone)]
pub enum TranscriptEntry {
    /// User prompt.
    User(String),
    /// Router decision (`→ <model> · <rationale>`). SPEC-012: color-coded
    /// by `source` so overrides and LLM decisions are visually distinct.
    RouterDecision {
        model: String,
        rationale: String,
        source: RouterSource,
    },
    /// Observational tool decision entry (approve / deny / always).
    ToolDecision { tool: String, decision: String },
    /// Assistant text (may span multiple lines).
    Assistant { text: String, model: String },
    /// Per-turn usage summary line.
    TurnSummary { summary: String, model: String },
    /// Tool call block.
    ToolCall {
        name: String,
        args_preview: String,
    },
    /// Tool result block.
    ToolResult {
        name: String,
        ok: bool,
        preview: String,
    },
    /// Error line.
    Error(String),
    /// Informational note (e.g. "reset session", "reconnected").
    Note(String),
}

/// In-flight turn state. Held separately from the transcript so the streaming
/// text can be rendered mid-flight before the turn commits into the log.
#[derive(Debug, Clone, Default)]
pub struct TurnState {
    pub prompt: String,
    pub assistant_text: String,
    pub model: Option<String>,
    /// Sidecar-assigned run id captured from `router/decision`. Used by
    /// `session/cancel` to target the correct run (SPEC-004).
    pub run_id: Option<String>,
    /// Wall-clock start of this turn. Drives the activity indicator's
    /// elapsed-seconds readout and spinner animation (SPEC-001/004).
    pub started_at: Option<std::time::Instant>,
}

impl TurnState {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            assistant_text: String::new(),
            model: None,
            run_id: None,
            started_at: Some(std::time::Instant::now()),
        }
    }
}
