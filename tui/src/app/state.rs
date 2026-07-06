// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Central AppState — the single source of truth the widgets render from and
// the event loop mutates.

use crate::app::internal::AppInternalTx;
use crate::app::overlay::Overlay;
use crate::app::transcript::{TranscriptEntry, TurnState};
use crate::app::usage::UsageAccumulator;
use crate::session_store::SessionStore;
use cusa_rpc::{ApprovalMode, ModelInfo, RouterSource, TokenUsage};
use serde_json::Value;
use std::collections::{HashSet, VecDeque};
use std::time::Instant;

/// Cap for `AppState::input_history` (SPEC-006). Deliberately small so the
/// working set is trivial to scroll and stays well under a KiB in memory.
pub const HISTORY_CAP: usize = 200;

/// Top-level run state, mirroring the state-transition diagram in the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    /// No run in flight.
    Idle,
    /// User submitted; awaiting router decision.
    Routing,
    /// Streaming assistant output.
    Streaming,
    /// A tool call is awaiting user approval.
    AwaitingApproval,
    /// Ctrl-C observed; cancellation request in flight.
    Cancelling,
}

impl RunPhase {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            RunPhase::Routing | RunPhase::Streaming | RunPhase::AwaitingApproval
        )
    }

    /// Short label for the footer / status line.
    pub fn label(self) -> &'static str {
        match self {
            RunPhase::Idle => "idle",
            RunPhase::Routing => "routing",
            RunPhase::Streaming => "streaming",
            RunPhase::AwaitingApproval => "awaiting approval",
            RunPhase::Cancelling => "cancelling",
        }
    }
}

/// Live view over the current session.
#[derive(Debug, Clone)]
pub struct SessionView {
    /// Session id (short hash rendered in the header). `None` until the
    /// sidecar returns the first `session/create` response.
    pub session_id: Option<String>,
    /// Cursor SDK agent id backing this session (SPEC-050/052).
    pub agent_id: Option<String>,
    /// Working directory rendered in the header.
    pub cwd: String,
    /// Current model shown in the status line.
    pub model: String,
    /// Current approval mode.
    pub approval_mode: ApprovalMode,
    /// Enabled skill count (SPEC-032).
    pub skills_count: usize,
    /// Enabled MCP server count (SPEC-042).
    pub mcp_count: usize,
    /// Sidecar health status; drives the "sidecar down" toast + fatal modal.
    pub sidecar_status: SidecarStatusView,
    /// SPEC-016: sticky manual model override. When `Some`, subsequent
    /// `session/send` calls carry a `modelOverride` field and the router is
    /// bypassed on the sidecar side.
    pub manual_model_override: Option<String>,
    /// SPEC-032: ids of skills the user has enabled for this session.
    pub enabled_skill_ids: Vec<String>,
}

impl SessionView {
    pub fn short_id(&self) -> String {
        match &self.session_id {
            Some(id) if id.len() > 8 => id[..8].to_string(),
            Some(id) => id.clone(),
            None => "-".to_string(),
        }
    }
}

/// Copy of `SidecarStatus` for the app layer so widgets don't have to touch
/// sidecar types directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarStatusView {
    Starting,
    Ready,
    Down,
    Reconnected,
}

impl SidecarStatusView {
    pub fn label(self) -> &'static str {
        match self {
            SidecarStatusView::Starting => "starting",
            SidecarStatusView::Ready => "ready",
            SidecarStatusView::Down => "down",
            SidecarStatusView::Reconnected => "reconnected",
        }
    }
}

/// History navigation state (SPEC-006). Present when the user has pressed
/// Up at least once with an empty (or empty-when-entered) input buffer.
#[derive(Debug, Clone, Default)]
pub struct HistoryNav {
    /// Index into `AppState::input_history`, from the newest end.
    /// `0` = newest entry currently loaded, `len-1` = oldest.
    pub cursor: usize,
    /// The unsent draft that was in the buffer when history nav began.
    /// Restored when the user navigates past the newest entry (Down at
    /// cursor 0). Discarded on submit / any other exit.
    pub draft: String,
}

/// SPEC-092: forced history-injection strategy. `Auto` (default) lets the
/// sidecar decide based on byte budget; `Raw` / `Summary` are user
/// overrides supplied via `/context strategy=...`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextStrategy {
    #[default]
    Auto,
    Raw,
    Summary,
}

impl ContextStrategy {
    pub fn label(self) -> &'static str {
        match self {
            ContextStrategy::Auto => "auto",
            ContextStrategy::Raw => "raw",
            ContextStrategy::Summary => "summary",
        }
    }
}

/// Root state.
#[derive(Debug)]
pub struct AppState {
    pub session: SessionView,
    pub transcript: Vec<TranscriptEntry>,
    /// Free-form input buffer. May contain embedded `\n` characters when
    /// multi-line mode is engaged (SPEC-005). `cursor_pos` is a char index
    /// into this buffer, counting `\n` as a single character.
    pub input: String,
    pub cursor_pos: usize,
    pub phase: RunPhase,
    pub current_turn: Option<TurnState>,
    pub usage: UsageAccumulator,
    pub overlay: Overlay,
    /// Last Ctrl-C timestamp; used for the double-tap-exit test in SPEC-004.
    pub last_ctrl_c: Option<Instant>,
    /// True once the event loop is asked to shut down.
    pub should_quit: bool,
    /// Optional footer hint that overrides the default keys.
    pub footer_override: Option<String>,
    /// SPEC-022/023: tool names the user has approved as "always allow" for
    /// the lifetime of this session. Cleared on `/reset` / session dispose.
    pub always_approved_tools: HashSet<String>,
    /// SPEC-016: cached response of `models/list`. Populated on first
    /// `/model` overlay open.
    pub models_cache: Option<Vec<ModelInfo>>,
    /// Sender for cross-task app-internal events. Populated by the event
    /// loop; `None` in unit tests that only exercise state transitions.
    pub internal_tx: Option<AppInternalTx>,
    /// SPEC-050: handle to `~/.cusa/sessions.json`. `None` in unit tests
    /// that don't touch disk.
    pub session_store: Option<SessionStore>,
    /// SPEC-041: parsed `--mcp <path>` document, forwarded on
    /// `session/create` / `session/resume`.
    pub mcp_overrides: Option<Value>,
    /// SPEC-006: bounded ring of previously-submitted prompts. Newest
    /// entries live at the *back* of the deque.
    pub input_history: VecDeque<String>,
    /// SPEC-006: active up/down navigation state, if any.
    pub history_nav: Option<HistoryNav>,
    /// SPEC-092: forced history strategy.
    pub context_strategy: ContextStrategy,
    /// True after the user begins interacting with the composer (including IME
    /// composition before text is committed). Hides the placeholder so preedit
    /// does not overlap the dim hint string.
    pub composer_input_active: bool,
}

impl AppState {
    pub fn new(cwd: String) -> Self {
        Self {
            session: SessionView {
                session_id: None,
                agent_id: None,
                cwd,
                model: "auto".to_string(),
                approval_mode: ApprovalMode::Suggest,
                skills_count: 0,
                mcp_count: 0,
                sidecar_status: SidecarStatusView::Starting,
                manual_model_override: None,
                enabled_skill_ids: Vec::new(),
            },
            transcript: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            phase: RunPhase::Idle,
            current_turn: None,
            usage: UsageAccumulator::new(),
            overlay: Overlay::None,
            last_ctrl_c: None,
            should_quit: false,
            footer_override: None,
            always_approved_tools: HashSet::new(),
            models_cache: None,
            internal_tx: None,
            session_store: None,
            mcp_overrides: None,
            input_history: VecDeque::new(),
            history_nav: None,
            context_strategy: ContextStrategy::Auto,
            composer_input_active: false,
        }
    }

    /// SPEC-006: push a submitted prompt onto the history ring, capped at
    /// [`HISTORY_CAP`] entries. No-op for whitespace-only strings.
    pub fn push_history(&mut self, entry: &str) {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(last) = self.input_history.back() {
            if last == trimmed {
                return;
            }
        }
        while self.input_history.len() >= HISTORY_CAP {
            self.input_history.pop_front();
        }
        self.input_history.push_back(trimmed.to_string());
    }

    /// Clear session-scoped caches (SPEC-022 always-cache, model cache, etc.)
    /// Called on `/reset` and on session dispose.
    pub fn clear_session_caches(&mut self) {
        self.always_approved_tools.clear();
        self.session.manual_model_override = None;
        self.session.enabled_skill_ids.clear();
        self.session.skills_count = 0;
    }

    /// Push a user prompt turn into the transcript and set `current_turn`.
    pub fn begin_user_turn(&mut self, prompt: String) {
        self.transcript
            .push(TranscriptEntry::User(prompt.clone()));
        self.current_turn = Some(TurnState::new(prompt));
        self.phase = RunPhase::Routing;
    }

    /// Called when `router/decision` arrives. Captures the sidecar-assigned
    /// `run_id` so a subsequent Ctrl-C can target the correct run
    /// (SPEC-004). The `source` drives the transcript colorization
    /// (SPEC-012).
    pub fn on_router_decision(
        &mut self,
        model: String,
        rationale: String,
        run_id: String,
        source: RouterSource,
    ) {
        self.session.model = model.clone();
        if let Some(turn) = self.current_turn.as_mut() {
            turn.run_id = Some(run_id);
        }
        self.transcript.push(TranscriptEntry::RouterDecision {
            model,
            rationale,
            source,
        });
        if self.phase == RunPhase::Routing {
            self.phase = RunPhase::Streaming;
        }
    }

    /// Called on every `stream/message` assistant delta.
    pub fn on_stream_message(&mut self, delta: &str) {
        let turn = self.current_turn.get_or_insert_with(|| TurnState::new(String::new()));
        turn.assistant_text.push_str(delta);
        if self.phase != RunPhase::Cancelling {
            self.phase = RunPhase::Streaming;
        }
    }

    /// Called on `stream/usage`.
    pub fn on_stream_usage(&mut self, usage: &TokenUsage) {
        self.usage.ingest_stream(usage);
    }

    /// Called on `run/finished`. Flushes the current turn into the transcript
    /// and records the per-turn delta.
    pub fn on_run_finished(&mut self, model: Option<String>, usage: &TokenUsage) {
        let effective_model = model.clone().unwrap_or_else(|| self.session.model.clone());
        self.usage
            .finish_turn_with_model(usage, Some(&effective_model));
        if let Some(mut turn) = self.current_turn.take() {
            turn.model = model.or(Some(self.session.model.clone()));
            let text = std::mem::take(&mut turn.assistant_text);
            let model_label = turn.model.clone().unwrap_or_default();
            self.transcript.push(TranscriptEntry::Assistant {
                text,
                model: model_label.clone(),
            });
            self.transcript.push(TranscriptEntry::TurnSummary {
                summary: self.usage.snapshot().turn_summary(),
                model: model_label,
            });
        }
        self.phase = RunPhase::Idle;
    }

    /// Called on `run/error`.
    pub fn on_run_error(&mut self, message: String) {
        if let Some(turn) = self.current_turn.take() {
            let _ = turn;
        }
        self.transcript
            .push(TranscriptEntry::Error(message));
        self.phase = RunPhase::Idle;
    }

    /// Called on Ctrl-C. Returns `true` if this Ctrl-C should terminate the
    /// app (second within 500 ms, or first while idle).
    pub fn on_ctrl_c(&mut self, now: Instant) -> CtrlCOutcome {
        let within_window = self
            .last_ctrl_c
            .is_some_and(|prev| now.duration_since(prev).as_millis() < 500);
        self.last_ctrl_c = Some(now);

        if within_window {
            return CtrlCOutcome::Exit;
        }

        if self.phase.is_active() {
            self.phase = RunPhase::Cancelling;
            return CtrlCOutcome::CancelRun;
        }

        // First Ctrl-C while idle → schedule exit-if-tapped-again toast.
        CtrlCOutcome::HintExit
    }
}

/// Result of processing a Ctrl-C key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlCOutcome {
    /// A run was active; a cancel was requested but the app stays alive.
    CancelRun,
    /// Idle Ctrl-C — show a "press again to exit" hint.
    HintExit,
    /// Second Ctrl-C within 500 ms → tear down.
    Exit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_004_first_ctrl_c_when_idle_hints() {
        let mut s = AppState::new("/tmp".into());
        let outcome = s.on_ctrl_c(Instant::now());
        assert_eq!(outcome, CtrlCOutcome::HintExit);
        assert_eq!(s.phase, RunPhase::Idle);
    }

    #[test]
    fn spec_004_second_ctrl_c_within_500ms_exits() {
        let mut s = AppState::new("/tmp".into());
        let t0 = Instant::now();
        s.on_ctrl_c(t0);
        let outcome = s.on_ctrl_c(t0 + std::time::Duration::from_millis(200));
        assert_eq!(outcome, CtrlCOutcome::Exit);
    }

    #[test]
    fn spec_004_ctrl_c_during_run_cancels() {
        let mut s = AppState::new("/tmp".into());
        s.begin_user_turn("hi".into());
        assert!(s.phase.is_active());
        let outcome = s.on_ctrl_c(Instant::now());
        assert_eq!(outcome, CtrlCOutcome::CancelRun);
        assert_eq!(s.phase, RunPhase::Cancelling);
    }

    #[test]
    fn spec_004_ctrl_c_outside_500ms_hints_again() {
        let mut s = AppState::new("/tmp".into());
        let t0 = Instant::now();
        s.on_ctrl_c(t0);
        let later = t0 + std::time::Duration::from_millis(800);
        let outcome = s.on_ctrl_c(later);
        assert_eq!(outcome, CtrlCOutcome::HintExit);
    }

    #[test]
    fn spec_001_on_router_decision_records_line() {
        let mut s = AppState::new("/tmp".into());
        s.begin_user_turn("hi".into());
        s.on_router_decision(
            "composer-2.5".into(),
            "explain code".into(),
            "r0".into(),
            RouterSource::Rule,
        );
        assert!(s
            .transcript
            .iter()
            .any(|e| matches!(e, TranscriptEntry::RouterDecision { model, .. } if model == "composer-2.5")));
        assert_eq!(s.session.model, "composer-2.5");
        assert_eq!(s.phase, RunPhase::Streaming);
        assert_eq!(
            s.current_turn.as_ref().and_then(|t| t.run_id.as_deref()),
            Some("r0")
        );
    }

    #[test]
    fn spec_001_stream_message_accumulates_into_current_turn() {
        let mut s = AppState::new("/tmp".into());
        s.begin_user_turn("hi".into());
        s.on_router_decision("m".into(), "r".into(), "run-1".into(), RouterSource::Rule);
        s.on_stream_message("Hel");
        s.on_stream_message("lo");
        assert_eq!(s.current_turn.as_ref().unwrap().assistant_text, "Hello");
    }

    #[test]
    fn spec_061_run_finished_pushes_turn_summary() {
        let mut s = AppState::new("/tmp".into());
        s.begin_user_turn("hi".into());
        s.on_router_decision("m".into(), "r".into(), "run-2".into(), RouterSource::Rule);
        s.on_stream_message("out");
        let final_usage = TokenUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            ..Default::default()
        };
        s.on_run_finished(Some("m".into()), &final_usage);
        assert_eq!(s.phase, RunPhase::Idle);
        let has_summary = s
            .transcript
            .iter()
            .any(|e| matches!(e, TranscriptEntry::TurnSummary { .. }));
        assert!(has_summary, "expected a turn summary in transcript");
    }
}
