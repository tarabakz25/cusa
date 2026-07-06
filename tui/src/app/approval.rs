// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Approval-modal logic (SPEC-021, SPEC-022, SPEC-023).
//
// Slice 2 stubbed the overlay: it rendered but did not issue a real
// `tool/approvalResponse`. Phase D wires the full flow:
//   * Y approves, N denies, A approves + adds to the session's "always" set.
//   * Before showing the modal on a subsequent tool call, the app checks
//     the always-set and auto-approves without prompting.
//   * The event loop then transitions state back to `Streaming`; the
//     sidecar drives the next run/finished notification.

use crate::app::internal::AppInternalEvent;
use crate::app::overlay::{ApprovalOverlay, Overlay};
use crate::app::state::{AppState, RunPhase};
use crate::app::transcript::TranscriptEntry;
use crate::sidecar::SidecarClient;
use cusa_rpc::{ApprovalDecision, ToolCategory};
use std::time::Duration;

/// Called when the sidecar sends `tool/approvalRequest`. If the tool has
/// already been "always"-approved, dispatch immediately and skip the
/// overlay. Otherwise, open the approval modal.
pub fn on_approval_request(
    state: &mut AppState,
    client: &SidecarClient,
    tool_name: String,
    args_preview: String,
    request_id: String,
    category: ToolCategory,
) {
    if state.always_approved_tools.contains(&tool_name) {
        dispatch_response(state, client, request_id.clone(), ApprovalDecision::Approve);
        state.transcript.push(TranscriptEntry::ToolDecision {
            tool: tool_name,
            decision: "always (auto)".into(),
        });
        state.phase = RunPhase::Streaming;
        return;
    }
    state.overlay = Overlay::Approval(ApprovalOverlay {
        tool_name,
        args_preview,
        request_id,
        category,
    });
    state.phase = RunPhase::AwaitingApproval;
}

/// Handle a key press while the approval overlay is open. Returns `true`
/// if the key consumed the modal.
pub fn on_key(
    state: &mut AppState,
    client: &SidecarClient,
    key: crossterm::event::KeyCode,
) -> bool {
    use crossterm::event::KeyCode;
    let Overlay::Approval(overlay) = state.overlay.clone() else {
        return false;
    };
    match key {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            complete(state, client, &overlay, ApprovalDecision::Approve, "approve");
            true
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            complete(state, client, &overlay, ApprovalDecision::Deny, "deny");
            true
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            state
                .always_approved_tools
                .insert(overlay.tool_name.clone());
            complete(state, client, &overlay, ApprovalDecision::Always, "always");
            true
        }
        _ => false,
    }
}

fn complete(
    state: &mut AppState,
    client: &SidecarClient,
    overlay: &ApprovalOverlay,
    decision: ApprovalDecision,
    label: &str,
) {
    dispatch_response(state, client, overlay.request_id.clone(), decision);
    state.transcript.push(TranscriptEntry::ToolDecision {
        tool: overlay.tool_name.clone(),
        decision: label.into(),
    });
    state.overlay = Overlay::None;
    state.phase = RunPhase::Streaming;
}

/// Fire `tool/approvalResponse` in a detached task. See
/// `app::spawn_send_prompt` for the tokio-runtime guard.
pub fn dispatch_response(
    state: &AppState,
    client: &SidecarClient,
    request_id: String,
    decision: ApprovalDecision,
) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let internal_tx = state.internal_tx.clone();
    let client = client.clone();
    tokio::spawn(async move {
        let decision_str = match decision {
            ApprovalDecision::Approve => "approve",
            ApprovalDecision::Deny => "deny",
            ApprovalDecision::Always => "always",
        };
        let params = serde_json::json!({
            "requestId": request_id.clone(),
            "decision": decision_str,
        });
        let outcome = client
            .call(
                cusa_rpc::method::TOOL_APPROVAL_RESPONSE,
                Some(params),
                Duration::from_secs(30),
            )
            .await;
        if let Some(tx) = internal_tx {
            let result = match outcome {
                Ok(o) if o.is_ok() => Ok(request_id),
                Ok(o) => Err(o
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "unknown".into())),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(AppInternalEvent::ApprovalResponseSent(result));
        }
    });
}
