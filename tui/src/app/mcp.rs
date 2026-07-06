// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// `/mcp` overlay orchestration (SPEC-042).

use crate::app::internal::{AppInternalEvent, McpTogglePayload};
use crate::app::overlay::{McpOverlay, Overlay};
use crate::app::state::AppState;
use crate::sidecar::SidecarClient;
use cusa_rpc::{McpListResult, McpServerInfo, McpToggleResult};
use std::time::Duration;

/// Open the `/mcp` overlay and dispatch `mcp/list`.
pub fn open(state: &mut AppState, client: &SidecarClient) {
    state.overlay = Overlay::Mcp(McpOverlay::loading());
    spawn_list(state, client);
}

fn spawn_list(state: &AppState, client: &SidecarClient) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let internal_tx = state.internal_tx.clone();
    let client = client.clone();
    let session_id = state.session.session_id.clone().unwrap_or_default();
    tokio::spawn(async move {
        let params = serde_json::json!({ "sessionId": session_id });
        let outcome = client
            .call(
                cusa_rpc::method::MCP_LIST,
                Some(params),
                Duration::from_secs(20),
            )
            .await;
        let result = to_mcp_result(outcome);
        if let Some(tx) = internal_tx {
            let _ = tx.send(AppInternalEvent::McpList(result));
        }
    });
}

fn to_mcp_result(
    outcome: anyhow::Result<crate::sidecar::CallOutcome>,
) -> Result<Vec<McpServerInfo>, String> {
    let outcome = outcome.map_err(|e| e.to_string())?;
    if let Some(err) = outcome.error {
        return Err(err.message);
    }
    let value = outcome.result.ok_or_else(|| "no result".to_string())?;
    let parsed: McpListResult = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(parsed.servers)
}

/// Apply a response from `mcp/list` to the current overlay.
pub fn apply_list_response(state: &mut AppState, result: Result<Vec<McpServerInfo>, String>) {
    if !matches!(state.overlay, Overlay::Mcp(_)) {
        return;
    }
    if let Overlay::Mcp(overlay) = &mut state.overlay {
        match result {
            Ok(servers) => {
                let enabled_count = servers.iter().filter(|s| s.enabled).count();
                overlay.populate(servers);
                state.session.mcp_count = enabled_count;
            }
            Err(err) => {
                overlay.loading = false;
                overlay.error = Some(format!("mcp/list failed: {err}"));
            }
        }
    }
}

/// Toggle the enabled flag on the cursor's server. Fires `mcp/toggle`.
pub fn toggle_cursor(state: &mut AppState, client: &SidecarClient) {
    let (server_id, next_enabled) = {
        let Overlay::Mcp(overlay) = &state.overlay else {
            return;
        };
        let Some(row) = overlay.servers.get(overlay.cursor) else {
            return;
        };
        (row.id.clone(), !row.enabled)
    };
    // Optimistic UI: flip the flag locally.
    if let Overlay::Mcp(overlay) = &mut state.overlay {
        if let Some(row) = overlay.servers.get_mut(overlay.cursor) {
            row.enabled = next_enabled;
        }
    }
    let session_id = state.session.session_id.clone().unwrap_or_default();
    if session_id.is_empty() {
        return;
    }
    spawn_toggle(state, client, session_id, server_id, next_enabled);
    // Keep mcp_count in sync locally.
    if let Overlay::Mcp(overlay) = &state.overlay {
        state.session.mcp_count = overlay.servers.iter().filter(|s| s.enabled).count();
    }
}

fn spawn_toggle(
    state: &AppState,
    client: &SidecarClient,
    session_id: String,
    server_id: String,
    enabled: bool,
) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let internal_tx = state.internal_tx.clone();
    let client = client.clone();
    tokio::spawn(async move {
        let params = serde_json::json!({
            "sessionId": session_id,
            "serverId": server_id.clone(),
            "enabled": enabled,
        });
        let outcome = client
            .call(
                cusa_rpc::method::MCP_TOGGLE,
                Some(params),
                Duration::from_secs(15),
            )
            .await;
        if let Some(tx) = internal_tx {
            let result = match outcome {
                Ok(o) if o.is_ok() => {
                    let pending_until_next_turn = o
                        .result
                        .as_ref()
                        .and_then(|v| serde_json::from_value::<McpToggleResult>(v.clone()).ok())
                        .map(|r| r.pending_until_next_turn)
                        .unwrap_or(false);
                    Ok(McpTogglePayload {
                        server_id,
                        enabled,
                        pending_until_next_turn,
                    })
                }
                Ok(o) => Err(o
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "unknown".into())),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(AppInternalEvent::McpToggle(result));
        }
    });
}
