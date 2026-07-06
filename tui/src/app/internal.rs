// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Cross-task app-internal event channel.
//
// Slash-command overlays such as `/model`, `/skills`, and `/mcp` need to
// spawn one-shot async requests (`models/list`, `skills/list`, `mcp/list`,
// `skills/setEnabled`, `mcp/toggle`) and route the result back into the
// (synchronous) state that widgets render from. Rather than introduce
// shared mutable state under a lock, we push a typed message onto a
// dedicated mpsc channel; the event loop `select!`s on it alongside the
// TUI-event and sidecar-event channels.
//
// The sender is stashed on `AppState::internal_tx` so any code path — key
// handling, slash-command dispatch, notification application — can enqueue
// an event without threading extra arguments through.

use cusa_rpc::{McpServerInfo, ModelInfo, SkillInfo};
use tokio::sync::mpsc;

/// A message the event loop should apply to `AppState`.
#[derive(Debug)]
pub enum AppInternalEvent {
    /// Response of `models/list` for the `/model` picker (SPEC-016).
    ModelsList(Result<Vec<ModelInfo>, String>),
    /// Response of `skills/list` for the `/skills` overlay (SPEC-032).
    SkillsList(Result<SkillsListPayload, String>),
    /// `skills/setEnabled` completed (or failed).
    SkillsSetEnabled(Result<Vec<String>, String>),
    /// Response of `mcp/list` for the `/mcp` overlay (SPEC-042).
    McpList(Result<Vec<McpServerInfo>, String>),
    /// `mcp/toggle` completed. Carries the target `(server_id, enabled)`.
    McpToggle(Result<McpTogglePayload, String>),
    /// Response for a `tool/approvalResponse` request. Purely diagnostic —
    /// the sidecar drives the transition back to `Streaming` via
    /// notifications.
    ApprovalResponseSent(Result<String, String>),
    /// Response of `context/setStrategy` (SPEC-092). Errors are surfaced
    /// as transcript entries unless the method is unknown (sidecar half
    /// not yet landed).
    ContextSetStrategy(Result<(), String>),
}

#[derive(Debug, Clone)]
pub struct SkillsListPayload {
    pub skills: Vec<SkillInfo>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct McpTogglePayload {
    pub server_id: String,
    pub enabled: bool,
    pub pending_until_next_turn: bool,
}

/// Handle stashed on `AppState`. `clone`-able so async tasks can enqueue.
pub type AppInternalTx = mpsc::UnboundedSender<AppInternalEvent>;
pub type AppInternalRx = mpsc::UnboundedReceiver<AppInternalEvent>;

/// Construct the paired channel.
pub fn channel() -> (AppInternalTx, AppInternalRx) {
    mpsc::unbounded_channel::<AppInternalEvent>()
}
