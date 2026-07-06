// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// App orchestration: layout, event loop, and glue between the state,
// widgets, and the sidecar client.

pub mod approval;
pub mod context;
pub mod events;
pub mod internal;
pub mod login;
pub mod mcp;
pub mod model_picker;
pub mod overlay;
pub mod skills;
pub mod slash;
pub mod startup;
pub mod state;
pub mod status;
pub mod transcript;
pub mod usage;

use crate::app::events::{spawn_input, TuiEvent};
use crate::codex_adapter::{BottomPaneWidget, ComposerKeyResult, handle_composer_key};
use crate::app::internal::{channel as internal_channel, AppInternalEvent, AppInternalRx};
use crate::app::overlay::{
    cycle_approval_mode, ApprovalPickerOverlay, McpOverlay, ModelPickerOverlay, Overlay,
    OverlayWidget, SkillsOverlay,
};
use crate::app::slash::SlashCommand;
use crate::app::state::{AppState, CtrlCOutcome, RunPhase, SidecarStatusView};
use crate::app::transcript::{TranscriptEntry, TurnState};
use crate::codex_adapter::transcript::CodexTranscriptWidget;
use crate::sidecar::events::{SidecarEvent, SidecarStatus};
use crate::sidecar::SidecarClient;
use anyhow::Result;
use crossterm::event::{Event as CtEvent, KeyCode, KeyModifiers};
use cusa_rpc::{ApprovalMode, RunFinishedParams, ServerNotification};
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Widget;
use ratatui::Terminal;
use std::io;
use std::path::Path;
use std::time::Instant;
use tokio::sync::mpsc;

/// Split the screen into (transcript, bottom pane) — Codex chat layout.
pub fn compute_layout(area: Rect, state: &AppState) -> [Rect; 2] {
    let bottom_height = BottomPaneWidget::desired_height(state, area.width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(bottom_height),
        ])
        .split(area);
    [chunks[0], chunks[1]]
}

/// Trait abstraction so `draw()` works with both ratatui and Codex `custom_terminal` frames.
trait RenderFrame {
    fn area(&self) -> Rect;
    fn render_widget<W: Widget>(&mut self, widget: W, area: Rect);
    fn place_composer_cursor(&mut self, pos: Option<(u16, u16)>);
}

impl RenderFrame for ratatui::Frame<'_> {
    fn area(&self) -> Rect {
        self.area()
    }

    fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self.buffer_mut());
    }

    fn place_composer_cursor(&mut self, _pos: Option<(u16, u16)>) {}
}

impl RenderFrame for crate::codex_ui::custom_terminal::Frame<'_> {
    fn area(&self) -> Rect {
        self.area()
    }

    fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self.buffer_mut());
    }

    fn place_composer_cursor(&mut self, pos: Option<(u16, u16)>) {
        if let Some((x, y)) = pos {
            self.set_cursor_position((x, y));
        }
    }
}

/// Render one frame of the app into any frame type (ratatui or custom_terminal).
fn render_app_ui<F: RenderFrame>(state: &AppState, frame: &mut F) {
    let [transcript, bottom] = compute_layout(frame.area(), state);
    frame.render_widget(
        CodexTranscriptWidget::new(
            &state.transcript,
            state.current_turn.as_ref(),
            Path::new(&state.session.cwd),
        )
        .with_session(&state.session),
        transcript,
    );
    frame.render_widget(BottomPaneWidget::from_state(state), bottom);
    if state.overlay.is_open() {
        frame.render_widget(OverlayWidget::new(&state.overlay), frame.area());
    } else {
        let cursor = crate::codex_adapter::composer::ComposerWidget::terminal_cursor(
            state,
            frame.area(),
        );
        frame.place_composer_cursor(cursor);
    }
}

/// Draw a single frame of the app into `terminal`.
pub fn draw<B: Backend>(state: &AppState, terminal: &mut Terminal<B>) -> Result<()> {
    terminal.draw(|f| render_app_ui(state, f))?;
    Ok(())
}

/// Draw using the vendored Codex `custom_terminal` backend (SPEC-105).
pub fn draw_interactive(
    state: &AppState,
    terminal: &mut crate::terminal::InteractiveTerminal,
) -> Result<()> {
    terminal.draw(|f| render_app_ui(state, f))?;
    Ok(())
}

/// Draw once into a specific backend. Used by tests.
pub fn draw_to_buffer<B: Backend>(state: &AppState, terminal: &mut Terminal<B>) -> Result<()> {
    draw(state, terminal)
}

/// Handle a `SidecarEvent` by mutating state.
pub fn apply_sidecar_event(state: &mut AppState, client: &SidecarClient, event: SidecarEvent) {
    match event {
        SidecarEvent::Notification(n) => apply_notification(state, client, n),
        SidecarEvent::Status(s) => apply_status(state, s),
        SidecarEvent::Log(_line) => {
            // Slice 2: logs land in tracing; overlay debug pane arrives later.
        }
        SidecarEvent::Fatal { message, log_path } => {
            state.overlay = Overlay::Fatal { message, log_path };
        }
        SidecarEvent::OrphanResponseError(err) => {
            state
                .transcript
                .push(TranscriptEntry::Error(format!("sidecar: {}", err.message)));
        }
    }
}

fn apply_notification(state: &mut AppState, client: &SidecarClient, n: ServerNotification) {
    match n {
        ServerNotification::RouterDecision(p) => {
            state.on_router_decision(p.model, p.rationale, p.run_id, p.source);
        }
        ServerNotification::StreamMessage(p) => {
            state.on_stream_message(&p.delta);
        }
        ServerNotification::StreamToolCall(p) => {
            state.transcript.push(TranscriptEntry::ToolCall {
                name: p.name,
                args_preview: preview(&p.args),
            });
        }
        ServerNotification::StreamToolResult(p) => {
            state.transcript.push(TranscriptEntry::ToolResult {
                name: format!("tool#{}", p.call_id),
                ok: p.ok,
                preview: p.output_preview.or(p.error).unwrap_or_default(),
            });
        }
        ServerNotification::StreamUsage(p) => {
            state.on_stream_usage(&p.usage);
        }
        ServerNotification::ToolApprovalRequest(p) => {
            approval::on_approval_request(
                state,
                client,
                p.name,
                preview(&p.args),
                p.request_id,
                p.category,
            );
        }
        ServerNotification::ToolApprovalResult(p) => {
            let decision = match p.decision {
                cusa_rpc::ApprovalResolution::AutoApprove => "auto-approve",
                cusa_rpc::ApprovalResolution::Prompt => "prompt",
            };
            state.transcript.push(TranscriptEntry::ToolDecision {
                tool: p.name,
                decision: decision.into(),
            });
        }
        ServerNotification::RunFinished(RunFinishedParams {
            usage, model, ..
        }) => {
            let model_clone = model.clone();
            state.on_run_finished(model, &usage);
            persist_run_finished(state, model_clone.as_deref());
        }
        ServerNotification::RunError(p) => {
            state.on_run_error(p.error.message);
        }
        ServerNotification::Log(_) => {
            // Slice 2: sidecar logs are captured by the supervisor already.
        }
    }
}

fn apply_status(state: &mut AppState, s: SidecarStatus) {
    let view = match s {
        SidecarStatus::Starting => SidecarStatusView::Starting,
        SidecarStatus::Ready => SidecarStatusView::Ready,
        SidecarStatus::Down => SidecarStatusView::Down,
        SidecarStatus::Reconnected => SidecarStatusView::Reconnected,
    };
    state.session.sidecar_status = view;
    if matches!(s, SidecarStatus::Reconnected) {
        state.overlay = Overlay::Toast {
            message: "sidecar reconnected".into(),
            created: Instant::now(),
        };
    }
}

/// Apply an app-internal event (response of `/model` / `/skills` / `/mcp`
/// list-and-toggle RPCs) to state.
pub fn apply_internal_event(state: &mut AppState, event: AppInternalEvent) {
    match event {
        AppInternalEvent::ModelsList(result) => model_picker::apply_list_response(state, result),
        AppInternalEvent::SkillsList(result) => skills::apply_list_response(state, result),
        AppInternalEvent::SkillsSetEnabled(result) => match result {
            Ok(ids) => {
                state.session.enabled_skill_ids = ids.clone();
                state.session.skills_count = ids.len();
                state.overlay = Overlay::Toast {
                    message: format!("skills updated ({} enabled)", ids.len()),
                    created: Instant::now(),
                };
            }
            Err(err) => {
                state.transcript.push(TranscriptEntry::Error(format!(
                    "skills/setEnabled failed: {err}"
                )));
            }
        },
        AppInternalEvent::McpList(result) => mcp::apply_list_response(state, result),
        AppInternalEvent::McpToggle(result) => match result {
            Ok(payload) => {
                let mut msg = format!(
                    "mcp: {} {}",
                    payload.server_id,
                    if payload.enabled { "enabled" } else { "disabled" }
                );
                if payload.pending_until_next_turn {
                    msg.push_str(" (pending until next turn)");
                }
                state.overlay = Overlay::Toast {
                    message: msg,
                    created: Instant::now(),
                };
            }
            Err(err) => {
                state
                    .transcript
                    .push(TranscriptEntry::Error(format!("mcp/toggle failed: {err}")));
            }
        },
        AppInternalEvent::ApprovalResponseSent(result) => {
            if let Err(err) = result {
                state.transcript.push(TranscriptEntry::Error(format!(
                    "tool/approvalResponse failed: {err}"
                )));
            }
        }
        AppInternalEvent::ContextSetStrategy(result) => {
            context::apply_result(state, result);
        }
    }
}

/// SPEC-050: after a `run/finished` lands, bump the session row's
/// `last_used_at` + turn counter. Errors are logged (best-effort).
pub fn persist_run_finished(state: &AppState, model: Option<&str>) {
    let Some(store) = state.session_store.as_ref() else {
        return;
    };
    let Some(agent_id) = state.session.agent_id.as_ref() else {
        return;
    };
    let mut delta = crate::session_store::SessionDelta::new()
        .with_last_used(crate::session_store::now_unix())
        .bump_turn();
    if let Some(m) = model {
        delta = delta.with_model(m);
    }
    if let Err(err) = store.update(agent_id, delta) {
        tracing::warn!(target: "session_store", ?err, "update on run/finished failed");
    }
}

/// SPEC-050: called by `/reset` — removes the session row so the next
/// launch's chooser does not offer it.
pub fn persist_session_removed(state: &AppState, agent_id: &str) {
    if let Some(store) = state.session_store.as_ref() {
        if let Err(err) = store.remove(agent_id) {
            tracing::warn!(target: "session_store", ?err, "remove failed");
        }
    }
}

/// SPEC-050: called on quit — bumps `last_used_at` so the row appears
/// newest in the chooser.
pub fn persist_on_dispose(state: &AppState) {
    let Some(store) = state.session_store.as_ref() else {
        return;
    };
    let Some(agent_id) = state.session.agent_id.as_ref() else {
        return;
    };
    let delta = crate::session_store::SessionDelta::new()
        .with_last_used(crate::session_store::now_unix())
        .with_approval_mode(state.session.approval_mode)
        .with_skills(state.session.enabled_skill_ids.clone())
        .with_model(state.session.model.clone())
        .with_mcp_overrides(state.mcp_overrides.clone());
    if let Err(err) = store.update(agent_id, delta) {
        tracing::warn!(target: "session_store", ?err, "update on dispose failed");
    }
}

fn preview(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 80 {
        format!("{}…", &s[..80])
    } else {
        s
    }
}

/// Handle a key event when no overlay is blocking input.
pub fn handle_key(
    state: &mut AppState,
    client: &SidecarClient,
    code: KeyCode,
    mods: KeyModifiers,
) -> KeyOutcome {
    // Ctrl-C first, regardless of overlay.
    if mods.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
        let outcome = state.on_ctrl_c(Instant::now());
        match outcome {
            CtrlCOutcome::CancelRun => {
                let session_id = state.session.session_id.clone().unwrap_or_default();
                let run_id = state
                    .current_turn
                    .as_ref()
                    .and_then(|t| t.run_id.clone())
                    .unwrap_or_default();
                if !session_id.is_empty() && !run_id.is_empty() {
                    spawn_send_cancel(client.clone(), session_id, run_id);
                }
                state.footer_override = Some("cancelling… press Ctrl-C again to quit".into());
                return KeyOutcome::Handled;
            }
            CtrlCOutcome::HintExit => {
                state.footer_override = Some("press Ctrl-C again to quit".into());
                return KeyOutcome::Handled;
            }
            CtrlCOutcome::Exit => {
                state.should_quit = true;
                return KeyOutcome::Quit;
            }
        }
    }

    // Overlay handling.
    if state.overlay.is_open() {
        return handle_overlay_key(state, client, code);
    }

    // Reset any Ctrl-C hint on the first non-Ctrl-C key.
    state.footer_override = None;

    // SPEC-021: Tab cycles the approval mode when no overlay is open.
    // The new mode is reflected in the status line immediately; the
    // footer hint updates on the next draw, so we intentionally skip the
    // toast here to preserve fast successive cycling.
    if matches!(code, KeyCode::Tab) {
        state.session.approval_mode = cycle_approval_mode(state.session.approval_mode);
        state.footer_override = Some(format!(
            "approval: {}",
            approval_label(state.session.approval_mode)
        ));
        return KeyOutcome::Handled;
    }

    match handle_composer_key(state, code, mods) {
        ComposerKeyResult::Submit => submit_input(state, client),
        ComposerKeyResult::Handled => KeyOutcome::Handled,
    }
}

fn handle_overlay_key(state: &mut AppState, client: &SidecarClient, code: KeyCode) -> KeyOutcome {
    // Toasts auto-dismiss on any key.
    if state.overlay.is_toast() {
        state.overlay = Overlay::None;
        return KeyOutcome::Handled;
    }
    match &state.overlay {
        Overlay::Help => {
            if matches!(code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                state.overlay = Overlay::None;
            }
            KeyOutcome::Handled
        }
        Overlay::Fatal { .. } => match code {
            KeyCode::Enter => {
                state.should_quit = true;
                KeyOutcome::Quit
            }
            KeyCode::Esc => {
                state.overlay = Overlay::None;
                KeyOutcome::Handled
            }
            _ => KeyOutcome::Handled,
        },
        Overlay::Approval(_) => {
            let consumed = approval::on_key(state, client, code);
            if !consumed && matches!(code, KeyCode::Esc) {
                state.overlay = Overlay::None;
                state.phase = RunPhase::Streaming;
            }
            KeyOutcome::Handled
        }
        Overlay::ModelPicker(_) => handle_model_picker_key(state, code),
        Overlay::ApprovalPicker(_) => handle_approval_picker_key(state, code),
        Overlay::Skills(_) => handle_skills_key(state, client, code),
        Overlay::Mcp(_) => handle_mcp_key(state, client, code),
        _ => KeyOutcome::Handled,
    }
}

fn handle_model_picker_key(state: &mut AppState, code: KeyCode) -> KeyOutcome {
    match code {
        KeyCode::Esc => {
            state.overlay = Overlay::None;
        }
        KeyCode::Up => {
            if let Overlay::ModelPicker(overlay) = &mut state.overlay {
                overlay.move_up();
            }
        }
        KeyCode::Down => {
            if let Overlay::ModelPicker(overlay) = &mut state.overlay {
                overlay.move_down();
            }
        }
        KeyCode::Enter => {
            let selected_id = {
                if let Overlay::ModelPicker(overlay) = &state.overlay {
                    overlay.selected().map(|m| m.id.clone())
                } else {
                    None
                }
            };
            if let Some(id) = selected_id {
                if id == "auto" {
                    clear_model_override(state);
                } else {
                    set_model_override(state, id);
                }
                state.overlay = Overlay::None;
            }
        }
        _ => {}
    }
    KeyOutcome::Handled
}

fn handle_approval_picker_key(state: &mut AppState, code: KeyCode) -> KeyOutcome {
    match code {
        KeyCode::Esc => {
            state.overlay = Overlay::None;
        }
        KeyCode::Up => {
            if let Overlay::ApprovalPicker(overlay) = &mut state.overlay {
                overlay.move_up();
            }
        }
        KeyCode::Down => {
            if let Overlay::ApprovalPicker(overlay) = &mut state.overlay {
                overlay.move_down();
            }
        }
        KeyCode::Char('1') => set_approval_mode(state, ApprovalMode::Suggest, true),
        KeyCode::Char('2') => set_approval_mode(state, ApprovalMode::AutoEdit, true),
        KeyCode::Char('3') => set_approval_mode(state, ApprovalMode::FullAuto, true),
        KeyCode::Enter => {
            let mode = if let Overlay::ApprovalPicker(overlay) = &state.overlay {
                Some(overlay.selected())
            } else {
                None
            };
            if let Some(m) = mode {
                set_approval_mode(state, m, true);
            }
        }
        _ => {}
    }
    KeyOutcome::Handled
}

fn handle_skills_key(state: &mut AppState, client: &SidecarClient, code: KeyCode) -> KeyOutcome {
    match code {
        KeyCode::Esc => {
            state.overlay = Overlay::None;
        }
        KeyCode::Up => {
            if let Overlay::Skills(overlay) = &mut state.overlay {
                overlay.move_up();
            }
        }
        KeyCode::Down => {
            if let Overlay::Skills(overlay) = &mut state.overlay {
                overlay.move_down();
            }
        }
        KeyCode::Char(' ') => {
            if let Overlay::Skills(overlay) = &mut state.overlay {
                overlay.toggle_cursor();
            }
        }
        KeyCode::Enter => {
            skills::commit(state, client);
        }
        _ => {}
    }
    KeyOutcome::Handled
}

fn handle_mcp_key(state: &mut AppState, client: &SidecarClient, code: KeyCode) -> KeyOutcome {
    match code {
        KeyCode::Esc => {
            state.overlay = Overlay::None;
        }
        KeyCode::Up => {
            if let Overlay::Mcp(overlay) = &mut state.overlay {
                overlay.move_up();
            }
        }
        KeyCode::Down => {
            if let Overlay::Mcp(overlay) = &mut state.overlay {
                overlay.move_down();
            }
        }
        KeyCode::Right | KeyCode::Enter => {
            if let Overlay::Mcp(overlay) = &mut state.overlay {
                overlay.toggle_cursor_expansion();
            }
        }
        KeyCode::Char(' ') => {
            mcp::toggle_cursor(state, client);
        }
        _ => {}
    }
    KeyOutcome::Handled
}

fn submit_input(state: &mut AppState, client: &SidecarClient) -> KeyOutcome {
    let text = state.input.trim().to_string();
    if text.is_empty() {
        return KeyOutcome::Handled;
    }
    state.input.clear();
    state.cursor_pos = 0;
    state.composer_input_active = false;

    if let Some(cmd) = slash::parse(&text) {
        return dispatch_slash(state, client, cmd);
    }
    // Guard against sending while a run is in flight.
    if state.phase.is_active() {
        state.overlay = Overlay::Toast {
            message: "run in progress — press Ctrl-C to cancel".into(),
            created: Instant::now(),
        };
        state.input = text;
        state.cursor_pos = text_char_count(&state.input);
        return KeyOutcome::Handled;
    }
    let session_id = state.session.session_id.clone().unwrap_or_default();
    if session_id.is_empty() {
        state.overlay = Overlay::Toast {
            message: "session not ready yet — waiting for sidecar…".into(),
            created: Instant::now(),
        };
        state.input = text;
        state.cursor_pos = text_char_count(&state.input);
        return KeyOutcome::Handled;
    }
    let override_ = state.session.manual_model_override.clone();
    state.begin_user_turn(text.clone());
    spawn_send_prompt(client.clone(), session_id, text, override_);
    KeyOutcome::Handled
}

/// Fire a `session/send` request in a detached task (SPEC-016 injects the
/// optional model override).
pub fn spawn_send_prompt(
    client: SidecarClient,
    session_id: String,
    text: String,
    model_override: Option<String>,
) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        let mut params = serde_json::json!({ "sessionId": session_id, "text": text });
        if let Some(m) = model_override {
            params["modelOverride"] = serde_json::Value::String(m);
        }
        let _ = client
            .call(
                cusa_rpc::method::SESSION_SEND,
                Some(params),
                std::time::Duration::from_secs(600),
            )
            .await;
    });
}

/// Fire a `session/cancel` request in a detached task (SPEC-004).
fn spawn_send_cancel(client: SidecarClient, session_id: String, run_id: String) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        let params = serde_json::json!({ "sessionId": session_id, "runId": run_id });
        let _ = client
            .call(
                cusa_rpc::method::SESSION_CANCEL,
                Some(params),
                std::time::Duration::from_secs(10),
            )
            .await;
    });
}

fn text_char_count(s: &str) -> usize {
    s.chars().count()
}

/// Dispatch a parsed slash command.
pub fn dispatch_slash(state: &mut AppState, client: &SidecarClient, cmd: SlashCommand) -> KeyOutcome {
    match cmd {
        SlashCommand::Help => {
            state.overlay = Overlay::Help;
            KeyOutcome::Handled
        }
        SlashCommand::Clear => {
            state.transcript.clear();
            state.current_turn = None;
            state.overlay = Overlay::Toast {
                message: "transcript cleared".into(),
                created: Instant::now(),
            };
            KeyOutcome::Handled
        }
        SlashCommand::Reset => {
            if let Some(sid) = state.session.session_id.take() {
                let _ = client.notify(
                    cusa_rpc::method::SESSION_DISPOSE,
                    Some(serde_json::json!({ "sessionId": sid })),
                );
            }
            if let Some(agent_id) = state.session.agent_id.take() {
                persist_session_removed(state, &agent_id);
            }
            state.transcript.clear();
            state.usage.reset();
            state.current_turn = None;
            state.phase = RunPhase::Idle;
            state.clear_session_caches();
            state.transcript.push(TranscriptEntry::Note(
                "session reset — a new session will be created on next turn".into(),
            ));
            KeyOutcome::Handled
        }
        SlashCommand::Quit => {
            state.should_quit = true;
            KeyOutcome::Quit
        }
        SlashCommand::Model(arg) => dispatch_model(state, client, arg),
        SlashCommand::Approval(arg) | SlashCommand::Mode(arg) => {
            dispatch_approval(state, arg)
        }
        SlashCommand::Skills => {
            skills::open(state, client);
            KeyOutcome::Handled
        }
        SlashCommand::Mcp => {
            mcp::open(state, client);
            KeyOutcome::Handled
        }
        SlashCommand::Cost => {
            state.overlay = Overlay::Cost(overlay::CostOverlay::new(
                state.usage.snapshot().clone(),
            ));
            KeyOutcome::Handled
        }
        SlashCommand::Context(arg) => {
            match arg {
                None => context::open_info(state),
                Some(name) => context::set_strategy(state, client, &name),
            }
            KeyOutcome::Handled
        }
        SlashCommand::Unknown(name) => {
            state.overlay = Overlay::Toast {
                message: format!("unknown command: /{name}"),
                created: Instant::now(),
            };
            KeyOutcome::Handled
        }
        stub if stub.is_stub() => {
            state.overlay = Overlay::Toast {
                message: format!("{stub} — not yet implemented in this slice"),
                created: Instant::now(),
            };
            KeyOutcome::Handled
        }
        _ => KeyOutcome::Handled,
    }
}

fn dispatch_model(
    state: &mut AppState,
    client: &SidecarClient,
    arg: Option<String>,
) -> KeyOutcome {
    match arg.as_deref() {
        None => {
            model_picker::open(state, client);
        }
        Some("auto") => {
            clear_model_override(state);
        }
        Some(id) => {
            set_model_override(state, id.to_string());
        }
    }
    KeyOutcome::Handled
}

fn dispatch_approval(state: &mut AppState, arg: Option<String>) -> KeyOutcome {
    match arg.as_deref() {
        None => {
            // No arg: cycle through modes (Codex-TUI-style hotkey semantics)
            // and pop a toast so the user sees the transition. Users who
            // want to *pick* can use `/mode` from the overlay entry (or
            // hit 1/2/3 in the picker). The picker is opened when there's
            // a shift-hotkey path — but the primary UX is cycling.
            state.session.approval_mode = cycle_approval_mode(state.session.approval_mode);
            state.overlay = Overlay::ApprovalPicker(ApprovalPickerOverlay::new(
                state.session.approval_mode,
            ));
        }
        Some(name) => {
            let mode = match name.to_ascii_lowercase().as_str() {
                "suggest" => Some(ApprovalMode::Suggest),
                "auto-edit" | "autoedit" | "auto" => Some(ApprovalMode::AutoEdit),
                "full-auto" | "fullauto" | "full" => Some(ApprovalMode::FullAuto),
                _ => None,
            };
            match mode {
                Some(m) => set_approval_mode(state, m, false),
                None => {
                    state.overlay = Overlay::Toast {
                        message: format!(
                            "unknown approval mode: {name} (suggest/auto-edit/full-auto)"
                        ),
                        created: Instant::now(),
                    };
                }
            }
        }
    }
    KeyOutcome::Handled
}

fn set_model_override(state: &mut AppState, id: String) {
    state.session.manual_model_override = Some(id.clone());
    state.session.model = id.clone();
    state.overlay = Overlay::Toast {
        message: format!("model override: {id}"),
        created: Instant::now(),
    };
}

fn clear_model_override(state: &mut AppState) {
    state.session.manual_model_override = None;
    state.overlay = Overlay::Toast {
        message: "auto mode restored".into(),
        created: Instant::now(),
    };
}

fn set_approval_mode(state: &mut AppState, mode: ApprovalMode, _close_overlay: bool) {
    state.session.approval_mode = mode;
    state.overlay = Overlay::Toast {
        message: format!("approval: {}", approval_label(mode)),
        created: Instant::now(),
    };
}

fn approval_label(mode: ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::Suggest => "suggest",
        ApprovalMode::AutoEdit => "auto-edit",
        ApprovalMode::FullAuto => "full-auto",
    }
}

/// Outcome of processing a key: continue the event loop or quit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    Handled,
    Quit,
}

/// The main event loop. Owns the terminal and drives it until quit.
pub async fn run_event_loop(
    state: &mut AppState,
    client: SidecarClient,
    mut events: mpsc::UnboundedReceiver<SidecarEvent>,
) -> Result<()> {
    let session = crate::terminal::TerminalSession::open()?;
    let mut terminal = session.terminal;

    let (tui_tx, mut tui_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let _input_task = spawn_input(tui_tx);

    let (internal_tx, mut internal_rx) = internal_channel();
    state.internal_tx = Some(internal_tx);

    let result = run_interactive_loop(
        state,
        &client,
        &mut terminal,
        &mut tui_rx,
        &mut events,
        &mut internal_rx,
    )
    .await;

    crate::terminal::TerminalSession { terminal }.teardown();

    // SPEC-050: bump the last-used timestamp so the next launch's chooser
    // lists this session at the top. Best-effort; ignored on failure.
    persist_on_dispose(state);
    if let Some(sid) = state.session.session_id.clone() {
        let _ = client.notify(
            cusa_rpc::method::SESSION_DISPOSE,
            Some(serde_json::json!({ "sessionId": sid })),
        );
    }

    // Best-effort shutdown notification.
    let _ = client.notify(cusa_rpc::method::SHUTDOWN, None);
    client.request_shutdown();

    result
}

async fn run_interactive_loop(
    state: &mut AppState,
    client: &SidecarClient,
    terminal: &mut crate::terminal::InteractiveTerminal,
    tui_rx: &mut mpsc::UnboundedReceiver<TuiEvent>,
    events: &mut mpsc::UnboundedReceiver<SidecarEvent>,
    internal_rx: &mut AppInternalRx,
) -> Result<()> {
    draw_interactive(state, terminal)?;
    loop {
        tokio::select! {
            evt = tui_rx.recv() => {
                match evt {
                    Some(TuiEvent::Term(CtEvent::Key(k))) => {
                        if let KeyOutcome::Quit = handle_key(state, client, k.code, k.modifiers) {
                            break;
                        }
                    }
                    Some(TuiEvent::Term(CtEvent::Resize(_, _))) => {
                        let _ = crate::terminal::TerminalSession::sync_viewport(terminal);
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            evt = events.recv() => {
                match evt {
                    Some(e) => apply_sidecar_event(state, client, e),
                    None => break,
                }
            }
            evt = internal_rx.recv() => {
                match evt {
                    Some(e) => apply_internal_event(state, e),
                    None => break,
                }
            }
        }
        // Toast auto-expiry: 2.5 seconds.
        if let Overlay::Toast { created, .. } = state.overlay {
            if created.elapsed().as_millis() > 2500 {
                state.overlay = Overlay::None;
            }
        }
        draw_interactive(state, terminal)?;
        if state.should_quit {
            break;
        }
    }
    Ok(())
}

#[allow(dead_code)]
async fn run_loop<B: Backend>(
    state: &mut AppState,
    client: &SidecarClient,
    terminal: &mut Terminal<B>,
    tui_rx: &mut mpsc::UnboundedReceiver<TuiEvent>,
    events: &mut mpsc::UnboundedReceiver<SidecarEvent>,
    internal_rx: &mut AppInternalRx,
) -> Result<()> {
    draw(state, terminal)?;
    loop {
        tokio::select! {
            evt = tui_rx.recv() => {
                match evt {
                    Some(TuiEvent::Term(CtEvent::Key(k))) => {
                        if let KeyOutcome::Quit = handle_key(state, client, k.code, k.modifiers) {
                            break;
                        }
                    }
                    Some(TuiEvent::Term(CtEvent::Resize(_, _))) => {
                        // Redraw on resize.
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            evt = events.recv() => {
                match evt {
                    Some(e) => apply_sidecar_event(state, client, e),
                    None => break,
                }
            }
            evt = internal_rx.recv() => {
                match evt {
                    Some(e) => apply_internal_event(state, e),
                    None => break,
                }
            }
        }
        // Toast auto-expiry: 2.5 seconds.
        if let Overlay::Toast { created, .. } = state.overlay {
            if created.elapsed().as_millis() > 2500 {
                state.overlay = Overlay::None;
            }
        }
        draw(state, terminal)?;
        if state.should_quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::SidecarClient;
    use cusa_rpc::{
        McpServerInfo, McpServerStatus, ModelInfo, RouterDecisionParams, RouterSource, SkillInfo,
        SkillSource, StreamMessageParams, StreamTextKind, ToolApprovalRequestParams, ToolCategory,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_string(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    #[test]
    fn spec_001_empty_transcript_layout_renders_codex_idle_screen() {
        let state = AppState::new("/tmp/repo".into());
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        draw_to_buffer(&state, &mut terminal).unwrap();
        let s = buffer_string(&terminal);
        assert!(s.contains("cusa"), "welcome header missing: {s:?}");
        assert!(s.contains("/tmp/repo"), "directory missing: {s:?}");
        assert!(s.contains("model:"), "session card missing: {s:?}");
        assert!(s.contains("Ask cusa"), "composer placeholder missing: {s:?}");
        assert!(s.contains("suggest"), "composer footer mode missing: {s:?}");
    }

    #[tokio::test]
    async fn spec_001_stubbed_stream_produces_expected_transcript() {
        let mut state = AppState::new("/tmp".into());
        let (_client, peer) = SidecarClient::in_memory();

        state.begin_user_turn("explain".into());
        let _ = peer;
        state.on_router_decision(
            "composer-2.5".into(),
            "fast rule".into(),
            "run-t".into(),
            RouterSource::Rule,
        );
        for delta in ["Hel", "lo, ", "world."] {
            state.on_stream_message(delta);
        }
        let usage = cusa_rpc::TokenUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            ..Default::default()
        };
        state.on_run_finished(Some("composer-2.5".into()), &usage);

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        draw_to_buffer(&state, &mut terminal).unwrap();
        let s = buffer_string(&terminal);
        assert!(s.contains("explain"));
        assert!(s.contains("composer-2.5"));
        assert!(s.contains("Hello, world."));
        assert!(s.contains("turn Δ"));
    }

    #[tokio::test]
    async fn spec_002_help_slash_opens_overlay() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        let outcome = dispatch_slash(&mut state, &client, SlashCommand::Help);
        assert_eq!(outcome, KeyOutcome::Handled);
        assert!(state.overlay.is_open());
        assert!(matches!(state.overlay, Overlay::Help));
    }

    #[tokio::test]
    async fn spec_002_clear_slash_wipes_transcript_keeps_session() {
        let mut state = AppState::new("/tmp".into());
        state.session.session_id = Some("keep-me".into());
        state.transcript.push(TranscriptEntry::User("gone".into()));
        let (client, _peer) = SidecarClient::in_memory();
        dispatch_slash(&mut state, &client, SlashCommand::Clear);
        assert!(state.transcript.is_empty());
        assert_eq!(state.session.session_id.as_deref(), Some("keep-me"));
    }

    #[tokio::test]
    async fn spec_002_reset_slash_disposes_session_and_records_note() {
        let mut state = AppState::new("/tmp".into());
        state.session.session_id = Some("gone".into());
        state.session.manual_model_override = Some("claude-sonnet-4".into());
        state.always_approved_tools.insert("shell".into());
        let (client, mut peer) = SidecarClient::in_memory();
        dispatch_slash(&mut state, &client, SlashCommand::Reset);
        assert!(state.session.session_id.is_none());
        assert!(state.session.manual_model_override.is_none());
        assert!(state.always_approved_tools.is_empty());
        assert!(
            state
                .transcript
                .iter()
                .any(|e| matches!(e, TranscriptEntry::Note(_))),
            "reset should append a note"
        );
        let frame = peer.try_recv_outbound().expect("session/dispose sent");
        if let crate::sidecar::OutboundFrame::Value(v) = frame {
            assert_eq!(v["method"], "session/dispose");
        } else {
            panic!("unexpected frame type");
        }
    }

    #[tokio::test]
    async fn spec_002_quit_slash_asks_event_loop_to_quit() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        let outcome = dispatch_slash(&mut state, &client, SlashCommand::Quit);
        assert_eq!(outcome, KeyOutcome::Quit);
        assert!(state.should_quit);
    }

    #[tokio::test]
    async fn spec_002_remaining_stub_slash_shows_toast() {
        // `/cost` and `/context` graduated from stubs to real overlays in
        // Phase E; only `/resume` remains as a stub until the resume-flow
        // UI is wired end-to-end. Update this test if that changes.
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        let stub = SlashCommand::Resume(String::new());
        dispatch_slash(&mut state, &client, stub);
        assert!(state.overlay.is_toast(), "stub should show toast");
        state.overlay = Overlay::None;
    }

    #[tokio::test]
    async fn spec_073_status_transitions_update_state() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        apply_sidecar_event(&mut state, &client, SidecarEvent::Status(SidecarStatus::Ready));
        assert_eq!(state.session.sidecar_status, SidecarStatusView::Ready);
        apply_sidecar_event(&mut state, &client, SidecarEvent::Status(SidecarStatus::Down));
        assert_eq!(state.session.sidecar_status, SidecarStatusView::Down);
        apply_sidecar_event(
            &mut state,
            &client,
            SidecarEvent::Status(SidecarStatus::Reconnected),
        );
        assert_eq!(state.session.sidecar_status, SidecarStatusView::Reconnected);
        assert!(state.overlay.is_toast(), "expected reconnect toast");
    }

    #[tokio::test]
    async fn spec_074_fatal_event_pushes_modal() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        apply_sidecar_event(
            &mut state,
            &client,
            SidecarEvent::Fatal {
                message: "sidecar died".into(),
                log_path: Some(std::path::PathBuf::from("/tmp/x.log")),
            },
        );
        match &state.overlay {
            Overlay::Fatal { message, log_path } => {
                assert!(message.contains("sidecar died"));
                assert_eq!(log_path.as_deref(), Some(std::path::Path::new("/tmp/x.log")));
            }
            _ => panic!("expected fatal overlay"),
        }
    }

    #[tokio::test]
    async fn spec_001_stream_message_notification_appears_in_transcript() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        state.begin_user_turn("hi".into());
        apply_sidecar_event(
            &mut state,
            &client,
            SidecarEvent::Notification(ServerNotification::RouterDecision(
                RouterDecisionParams {
                    session_id: "s".into(),
                    run_id: "r".into(),
                    model: "m".into(),
                    rationale: "why".into(),
                    source: RouterSource::Rule,
                },
            )),
        );
        for delta in ["a", "b", "c"] {
            apply_sidecar_event(
                &mut state,
                &client,
                SidecarEvent::Notification(ServerNotification::StreamMessage(
                    StreamMessageParams {
                        run_id: "r".into(),
                        delta: delta.into(),
                        kind: StreamTextKind::Assistant,
                    },
                )),
            );
        }
        assert_eq!(state.current_turn.as_ref().unwrap().assistant_text, "abc");
    }

    #[tokio::test]
    async fn spec_004_handle_key_ctrl_c_cancels_during_run() {
        let mut state = AppState::new("/tmp".into());
        state.session.session_id = Some("s0".into());
        state.begin_user_turn("hi".into());
        state.on_router_decision("m".into(), "r".into(), "run-abc".into(), RouterSource::Rule);
        let (client, mut peer) = SidecarClient::in_memory();
        let outcome = handle_key(&mut state, &client, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(outcome, KeyOutcome::Handled);
        assert_eq!(state.phase, RunPhase::Cancelling);
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let frame = peer.try_recv_outbound().expect("cancel request sent");
        if let crate::sidecar::OutboundFrame::Value(v) = frame {
            assert_eq!(v["method"], "session/cancel");
            assert_eq!(v["params"]["sessionId"], "s0");
            assert_eq!(v["params"]["runId"], "run-abc");
        } else {
            panic!("unexpected frame type");
        }
    }

    #[tokio::test]
    async fn spec_004_double_ctrl_c_quits() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        handle_key(&mut state, &client, KeyCode::Char('c'), KeyModifiers::CONTROL);
        let outcome = handle_key(&mut state, &client, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(outcome, KeyOutcome::Quit);
        assert!(state.should_quit);
    }

    // ----- Slice 3 -------------------------------------------------------

    #[tokio::test]
    async fn spec_016_slash_model_id_sets_override_and_next_send_carries_it() {
        let mut state = AppState::new("/tmp".into());
        state.session.session_id = Some("s0".into());
        let (client, mut peer) = SidecarClient::in_memory();
        dispatch_slash(
            &mut state,
            &client,
            SlashCommand::Model(Some("claude-sonnet-4".into())),
        );
        assert_eq!(
            state.session.manual_model_override.as_deref(),
            Some("claude-sonnet-4")
        );
        state.overlay = Overlay::None;

        // Submit a plain prompt through the same code path Enter would.
        state.input = "hello".into();
        state.cursor_pos = 5;
        submit_input(&mut state, &client);

        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let frame = peer.try_recv_outbound().expect("session/send fired");
        if let crate::sidecar::OutboundFrame::Value(v) = frame {
            assert_eq!(v["method"], "session/send");
            assert_eq!(v["params"]["modelOverride"], "claude-sonnet-4");
            assert_eq!(v["params"]["text"], "hello");
        } else {
            panic!("unexpected frame type");
        }
    }

    #[tokio::test]
    async fn spec_016_slash_model_auto_clears_override() {
        let mut state = AppState::new("/tmp".into());
        state.session.manual_model_override = Some("claude-sonnet-4".into());
        let (client, _peer) = SidecarClient::in_memory();
        dispatch_slash(
            &mut state,
            &client,
            SlashCommand::Model(Some("auto".into())),
        );
        assert!(state.session.manual_model_override.is_none());
        assert!(state.overlay.is_toast());
    }

    #[tokio::test]
    async fn spec_016_model_picker_populates_from_internal_event() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        // Open the picker (empty cache → loading state).
        dispatch_slash(&mut state, &client, SlashCommand::Model(None));
        assert!(matches!(state.overlay, Overlay::ModelPicker(_)));

        apply_internal_event(
            &mut state,
            AppInternalEvent::ModelsList(Ok(vec![ModelInfo {
                id: "composer-2.5".into(),
                display_name: None,
                provider: None,
                supports_thinking: false,
            }])),
        );
        if let Overlay::ModelPicker(overlay) = &state.overlay {
            assert!(!overlay.loading);
            assert_eq!(overlay.models.len(), 1);
        } else {
            panic!("expected ModelPicker overlay");
        }
    }

    // ----- Slice 4 -------------------------------------------------------

    fn open_approval(state: &mut AppState) {
        state.overlay = Overlay::Approval(overlay::ApprovalOverlay {
            tool_name: "shell_exec".into(),
            args_preview: "{\"cmd\":\"ls\"}".into(),
            request_id: "req-1".into(),
            category: ToolCategory::Shell,
        });
        state.phase = RunPhase::AwaitingApproval;
    }

    #[tokio::test]
    async fn spec_022_approval_prompt_sends_approve_on_y() {
        let mut state = AppState::new("/tmp".into());
        open_approval(&mut state);
        let (client, mut peer) = SidecarClient::in_memory();
        let outcome = handle_key(&mut state, &client, KeyCode::Char('y'), KeyModifiers::empty());
        assert_eq!(outcome, KeyOutcome::Handled);
        assert!(matches!(state.overlay, Overlay::None));
        assert_eq!(state.phase, RunPhase::Streaming);
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let frame = peer.try_recv_outbound().expect("approval response sent");
        if let crate::sidecar::OutboundFrame::Value(v) = frame {
            assert_eq!(v["method"], "tool/approvalResponse");
            assert_eq!(v["params"]["decision"], "approve");
            assert_eq!(v["params"]["requestId"], "req-1");
        }
    }

    #[tokio::test]
    async fn spec_022_approval_prompt_sends_deny_on_n() {
        let mut state = AppState::new("/tmp".into());
        open_approval(&mut state);
        let (client, mut peer) = SidecarClient::in_memory();
        handle_key(&mut state, &client, KeyCode::Char('n'), KeyModifiers::empty());
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let frame = peer.try_recv_outbound().expect("approval response sent");
        if let crate::sidecar::OutboundFrame::Value(v) = frame {
            assert_eq!(v["params"]["decision"], "deny");
        }
    }

    #[tokio::test]
    async fn spec_022_approval_always_marks_tool_and_auto_approves_next_call() {
        let mut state = AppState::new("/tmp".into());
        open_approval(&mut state);
        let (client, mut peer) = SidecarClient::in_memory();
        handle_key(&mut state, &client, KeyCode::Char('a'), KeyModifiers::empty());
        assert!(state.always_approved_tools.contains("shell_exec"));
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let _first = peer.try_recv_outbound().expect("first approval sent");

        // Now the sidecar sends a second approvalRequest for the same tool.
        apply_sidecar_event(
            &mut state,
            &client,
            SidecarEvent::Notification(ServerNotification::ToolApprovalRequest(
                ToolApprovalRequestParams {
                    request_id: "req-2".into(),
                    run_id: "r".into(),
                    name: "shell_exec".into(),
                    args: serde_json::json!({"cmd":"pwd"}),
                    category: ToolCategory::Shell,
                },
            )),
        );
        // Overlay should NOT open — auto-approval fires instead.
        assert!(matches!(state.overlay, Overlay::None));
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let frame = peer.try_recv_outbound().expect("auto-approval response");
        if let crate::sidecar::OutboundFrame::Value(v) = frame {
            assert_eq!(v["method"], "tool/approvalResponse");
            assert_eq!(v["params"]["decision"], "approve");
            assert_eq!(v["params"]["requestId"], "req-2");
        }
    }

    #[tokio::test]
    async fn spec_023_auto_edit_prompts_on_shell_tool_category() {
        // In `auto-edit`, the sidecar decides whether a category needs
        // approval. From the TUI's perspective, when the sidecar *does*
        // ask, we render the modal with the correct category. This test
        // guards that mapping.
        let mut state = AppState::new("/tmp".into());
        state.session.approval_mode = ApprovalMode::AutoEdit;
        let (client, _peer) = SidecarClient::in_memory();
        apply_sidecar_event(
            &mut state,
            &client,
            SidecarEvent::Notification(ServerNotification::ToolApprovalRequest(
                ToolApprovalRequestParams {
                    request_id: "req-3".into(),
                    run_id: "r".into(),
                    name: "shell_exec".into(),
                    args: serde_json::json!({"cmd":"rm -rf ~"}),
                    category: ToolCategory::Shell,
                },
            )),
        );
        match &state.overlay {
            Overlay::Approval(a) => {
                assert_eq!(a.category, ToolCategory::Shell);
                assert_eq!(a.request_id, "req-3");
            }
            _ => panic!("expected approval overlay"),
        }
    }

    #[tokio::test]
    async fn spec_021_slash_approval_cycles_modes() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        assert_eq!(state.session.approval_mode, ApprovalMode::Suggest);
        dispatch_slash(&mut state, &client, SlashCommand::Approval(None));
        assert_eq!(state.session.approval_mode, ApprovalMode::AutoEdit);
        state.overlay = Overlay::None;
        dispatch_slash(&mut state, &client, SlashCommand::Approval(None));
        assert_eq!(state.session.approval_mode, ApprovalMode::FullAuto);
        state.overlay = Overlay::None;
        dispatch_slash(&mut state, &client, SlashCommand::Approval(None));
        assert_eq!(state.session.approval_mode, ApprovalMode::Suggest);
    }

    #[tokio::test]
    async fn spec_021_tab_key_cycles_modes_when_no_overlay() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        assert_eq!(state.session.approval_mode, ApprovalMode::Suggest);
        handle_key(&mut state, &client, KeyCode::Tab, KeyModifiers::empty());
        assert_eq!(state.session.approval_mode, ApprovalMode::AutoEdit);
        handle_key(&mut state, &client, KeyCode::Tab, KeyModifiers::empty());
        assert_eq!(state.session.approval_mode, ApprovalMode::FullAuto);
        handle_key(&mut state, &client, KeyCode::Tab, KeyModifiers::empty());
        assert_eq!(state.session.approval_mode, ApprovalMode::Suggest);
    }

    // ----- Slice 5 -------------------------------------------------------

    fn make_skill(id: &str, name: &str) -> SkillInfo {
        SkillInfo {
            id: id.into(),
            name: name.into(),
            description: format!("does {id}"),
            path: format!("/tmp/{id}/SKILL.md"),
            size_bytes: 0,
            source: SkillSource::User,
        }
    }

    #[tokio::test]
    async fn spec_032_slash_skills_calls_list_and_renders_toggles() {
        let mut state = AppState::new("/tmp".into());
        let (client, _peer) = SidecarClient::in_memory();
        dispatch_slash(&mut state, &client, SlashCommand::Skills);
        assert!(matches!(state.overlay, Overlay::Skills(_)));

        // Simulate skills/list response landing.
        apply_internal_event(
            &mut state,
            AppInternalEvent::SkillsList(Ok(internal::SkillsListPayload {
                skills: vec![make_skill("foo", "Foo"), make_skill("bar", "Bar")],
                warnings: vec![],
            })),
        );
        if let Overlay::Skills(overlay) = &state.overlay {
            assert!(!overlay.loading);
            assert_eq!(overlay.items.len(), 2);
        } else {
            panic!("expected skills overlay");
        }
    }

    #[tokio::test]
    async fn spec_032_toggling_a_skill_and_confirming_persists_via_set_enabled() {
        let mut state = AppState::new("/tmp".into());
        state.session.session_id = Some("s-1".into());
        let (client, mut peer) = SidecarClient::in_memory();
        dispatch_slash(&mut state, &client, SlashCommand::Skills);
        apply_internal_event(
            &mut state,
            AppInternalEvent::SkillsList(Ok(internal::SkillsListPayload {
                skills: vec![make_skill("foo", "Foo"), make_skill("bar", "Bar")],
                warnings: vec![],
            })),
        );
        // Space toggles the cursor row (foo).
        handle_key(&mut state, &client, KeyCode::Char(' '), KeyModifiers::empty());
        // Move down and toggle bar too.
        handle_key(&mut state, &client, KeyCode::Down, KeyModifiers::empty());
        handle_key(&mut state, &client, KeyCode::Char(' '), KeyModifiers::empty());
        // Enter commits.
        handle_key(&mut state, &client, KeyCode::Enter, KeyModifiers::empty());
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert!(matches!(state.overlay, Overlay::None));
        assert_eq!(state.session.enabled_skill_ids.len(), 2);
        assert_eq!(state.session.skills_count, 2);
        // Drain outbound frames — the open() call fires skills/list first;
        // the commit fires skills/setEnabled.
        let mut methods: Vec<String> = Vec::new();
        while let Some(frame) = peer.try_recv_outbound() {
            if let crate::sidecar::OutboundFrame::Value(v) = frame {
                if let Some(m) = v["method"].as_str() {
                    methods.push(m.to_string());
                    if m == "skills/setEnabled" {
                        assert_eq!(v["params"]["sessionId"], "s-1");
                        let ids = v["params"]["skillIds"].as_array().expect("array");
                        assert_eq!(ids.len(), 2);
                    }
                }
            }
        }
        assert!(
            methods.iter().any(|m| m == "skills/setEnabled"),
            "expected skills/setEnabled among {methods:?}"
        );
    }

    #[tokio::test]
    async fn spec_042_slash_mcp_calls_list_and_renders_servers_with_transport() {
        let mut state = AppState::new("/tmp".into());
        state.session.session_id = Some("s-1".into());
        let (client, _peer) = SidecarClient::in_memory();
        dispatch_slash(&mut state, &client, SlashCommand::Mcp);
        assert!(matches!(state.overlay, Overlay::Mcp(_)));
        apply_internal_event(
            &mut state,
            AppInternalEvent::McpList(Ok(vec![McpServerInfo {
                id: "fs".into(),
                transport: "stdio".into(),
                status: McpServerStatus::Ready,
                tools: vec![],
                enabled: true,
                error: None,
            }])),
        );
        if let Overlay::Mcp(overlay) = &state.overlay {
            assert!(!overlay.loading);
            assert_eq!(overlay.servers.len(), 1);
            assert_eq!(overlay.servers[0].transport, "stdio");
        } else {
            panic!("expected mcp overlay");
        }
    }

    #[tokio::test]
    async fn spec_042_expanded_row_shows_tool_list() {
        let mut state = AppState::new("/tmp".into());
        state.session.session_id = Some("s-1".into());
        let (client, _peer) = SidecarClient::in_memory();
        dispatch_slash(&mut state, &client, SlashCommand::Mcp);
        apply_internal_event(
            &mut state,
            AppInternalEvent::McpList(Ok(vec![McpServerInfo {
                id: "fs".into(),
                transport: "stdio".into(),
                status: McpServerStatus::Ready,
                tools: vec![cusa_rpc::McpToolInfo {
                    name: "read".into(),
                    description: "read file".into(),
                }],
                enabled: true,
                error: None,
            }])),
        );
        handle_key(&mut state, &client, KeyCode::Right, KeyModifiers::empty());
        if let Overlay::Mcp(overlay) = &state.overlay {
            assert_eq!(overlay.expanded, Some(0));
        } else {
            panic!("expected mcp overlay");
        }
        // Render sanity: the tool list should be visible.
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        draw_to_buffer(&state, &mut terminal).unwrap();
        let s = buffer_string(&terminal);
        assert!(s.contains("read"), "tool row missing: {s}");
    }

    // Silence "unused" warnings for helper structs re-exported for tests.
    #[allow(dead_code)]
    fn _touch_types() {
        let _ = SkillsOverlay::loading();
        let _ = McpOverlay::loading();
        let _ = ModelPickerOverlay::loading();
    }
}
