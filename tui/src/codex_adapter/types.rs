// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Display types consumed by ported Codex widgets. These mirror upstream
// Codex enums without depending on `codex-*` crates (SPEC-104).

/// Router-decision provenance for transcript colorization (SPEC-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterSourceView {
    Rule,
    Llm,
    Override,
    Fallback,
}

impl RouterSourceView {
    /// Short tag rendered beside the router line (`override`, `rule`, …).
    pub fn tag(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::Rule => "rule",
            Self::Llm => "llm",
            Self::Fallback => "fallback",
        }
    }
}

/// Session approval mode shown in status chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalModeView {
    Suggest,
    AutoEdit,
    FullAuto,
}

impl ApprovalModeView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::AutoEdit => "auto-edit",
            Self::FullAuto => "full-auto",
        }
    }
}

/// Top-level run phase for footer hints and composer affordances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhaseView {
    Idle,
    Sending,
    Streaming,
    AwaitingApproval,
    Cancelling,
}

impl RunPhaseView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Sending => "sending",
            Self::Streaming => "streaming",
            Self::AwaitingApproval => "awaiting approval",
            Self::Cancelling => "cancelling",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Sending | Self::Streaming | Self::AwaitingApproval
        )
    }
}

/// One transcript history cell for ported `history_cell` widgets (P2+).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryCellView {
    UserPrompt { text: String },
    RouterDecision {
        model: String,
        rationale: String,
        source: RouterSourceView,
    },
    ToolDecision { tool: String, decision: String },
    Assistant { text: String, model: String },
    TurnSummary { summary: String, model: String },
    ToolCall {
        name: String,
        args_preview: String,
    },
    ToolResult {
        name: String,
        ok: bool,
        preview: String,
    },
    Error { message: String },
    Note { message: String },
    /// In-flight assistant text not yet committed to the transcript.
    LiveAssistant { text: String },
}

/// Bottom-pane composer state for ported `bottom_pane` widgets (P1+).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerView {
    pub buffer: String,
    pub cursor_pos: usize,
    /// True when the composer accepts keystrokes (no overlay blocking).
    pub active: bool,
    /// True when the buffer contains embedded newlines (SPEC-005).
    pub multiline: bool,
    /// True when the user is navigating input history (SPEC-006).
    pub history_nav_active: bool,
    pub phase: RunPhaseView,
}

/// File-level change for vendored `diff_render` (SPEC-108).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChangeView {
    Add { content: String },
    Delete { content: String },
    Update {
        unified_diff: String,
        move_path: Option<std::path::PathBuf>,
    },
}

/// Tool transcript block routed to diff or exec renderers (SPEC-108).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolBlockView {
    Shell {
        command: String,
        output: Option<String>,
        exit_code: Option<i32>,
        active: bool,
    },
    Diff {
        changes: std::collections::HashMap<std::path::PathBuf, FileChangeView>,
    },
    GenericCall {
        name: String,
        args_preview: String,
    },
    GenericResult {
        name: String,
        ok: bool,
        preview: String,
    },
}
