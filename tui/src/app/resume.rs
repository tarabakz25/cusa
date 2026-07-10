// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// In-TUI `/resume` orchestration (SPEC-003 / SPEC-051..053).
//
// Bare `/resume` opens a sessions-only picker for the current cwd. Esc
// cancels. `/resume <id|prefix|index>` resumes directly. Mid-session
// resume disposes the current sidecar session, then calls `session/resume`
// and restores approval / skills / MCP from the stored row (SPEC-053).

use crate::app::internal::{AppInternalEvent, SessionResumedPayload};
use crate::app::overlay::{Overlay, ResumeOverlay};
use crate::app::state::{AppState, RunPhase};
use crate::session_store::{now_unix, StoredSession};
use crate::sidecar::SidecarClient;
use cusa_rpc::{ApprovalMode, SessionResumeResult};
use serde_json::Value;
use std::time::{Duration, Instant};

/// Open the `/resume` picker for prior sessions in the current cwd.
pub fn open(state: &mut AppState) {
    if state.phase.is_active() {
        state.overlay = Overlay::Toast {
            message: "cancel the current run before resuming".into(),
            created: Instant::now(),
        };
        return;
    }
    let Some(store) = state.session_store.as_ref() else {
        state.overlay = Overlay::Toast {
            message: "session store unavailable".into(),
            created: Instant::now(),
        };
        return;
    };
    let candidates = store.list_for_cwd(&state.session.cwd);
    if candidates.is_empty() {
        state.overlay = Overlay::Toast {
            message: "no prior sessions for this directory".into(),
            created: Instant::now(),
        };
        return;
    }
    state.overlay = Overlay::Resume(ResumeOverlay::new(
        candidates,
        state.session.cwd.clone(),
    ));
}

/// Resolve `/resume <arg>` and start the dispose → resume flow.
pub fn resume_direct(state: &mut AppState, client: &SidecarClient, arg: &str) {
    if state.phase.is_active() {
        state.overlay = Overlay::Toast {
            message: "cancel the current run before resuming".into(),
            created: Instant::now(),
        };
        return;
    }
    let arg = arg.trim();
    if arg.is_empty() {
        open(state);
        return;
    }
    let Some(store) = state.session_store.as_ref() else {
        state.overlay = Overlay::Toast {
            message: "session store unavailable".into(),
            created: Instant::now(),
        };
        return;
    };
    let candidates = store.list_for_cwd(&state.session.cwd);
    match resolve_arg(&candidates, arg) {
        Some(stored) => start_resume(state, client, stored),
        None => {
            state.overlay = Overlay::Toast {
                message: format!("no session matching '{arg}'"),
                created: Instant::now(),
            };
        }
    }
}

/// Commit the currently selected picker row.
pub fn commit(state: &mut AppState, client: &SidecarClient) {
    if state.phase.is_active() {
        state.overlay = Overlay::Toast {
            message: "cancel the current run before resuming".into(),
            created: Instant::now(),
        };
        return;
    }
    let stored = match &state.overlay {
        Overlay::Resume(overlay) if !overlay.busy => overlay.selected_session().cloned(),
        _ => None,
    };
    let Some(stored) = stored else {
        return;
    };
    start_resume(state, client, stored);
}

/// Resolve `arg` against cwd-filtered candidates: exact agent id, then
/// case-sensitive prefix (including the 8-char short id), then 1-based
/// index into the newest-first list.
pub fn resolve_arg(candidates: &[StoredSession], arg: &str) -> Option<StoredSession> {
    if let Some(exact) = candidates.iter().find(|s| s.agent_id == arg) {
        return Some(exact.clone());
    }
    let prefix_matches: Vec<&StoredSession> = candidates
        .iter()
        .filter(|s| s.agent_id.starts_with(arg))
        .collect();
    if prefix_matches.len() == 1 {
        return Some(prefix_matches[0].clone());
    }
    if let Ok(index) = arg.parse::<usize>() {
        if index >= 1 {
            return candidates.get(index - 1).cloned();
        }
    }
    None
}

fn start_resume(state: &mut AppState, client: &SidecarClient, stored: StoredSession) {
    if let Overlay::Resume(overlay) = &mut state.overlay {
        overlay.busy = true;
    } else {
        state.overlay = Overlay::Toast {
            message: format!("resuming {}…", stored.short_agent_id()),
            created: Instant::now(),
        };
    }

    if tokio::runtime::Handle::try_current().is_err() {
        // Unit tests without a runtime apply synchronously via helpers.
        return;
    }

    let internal_tx = state.internal_tx.clone();
    let client = client.clone();
    let current_session_id = state.session.session_id.clone();
    let cwd = state.session.cwd.clone();
    tokio::spawn(async move {
        let result = dispose_and_resume(&client, current_session_id.as_deref(), &cwd, &stored).await;
        if let Some(tx) = internal_tx {
            let _ = tx.send(AppInternalEvent::SessionResumed(result));
        }
    });
}

async fn dispose_and_resume(
    client: &SidecarClient,
    current_session_id: Option<&str>,
    cwd: &str,
    stored: &StoredSession,
) -> Result<SessionResumedPayload, String> {
    if let Some(session_id) = current_session_id {
        let dispose = client
            .call(
                cusa_rpc::method::SESSION_DISPOSE,
                Some(serde_json::json!({ "sessionId": session_id })),
                Duration::from_secs(20),
            )
            .await
            .map_err(|e| e.to_string())?;
        if let Some(err) = dispose.error {
            return Err(format!("session/dispose failed: {}", err.message));
        }
    }

    let approval_mode_str = match stored.approval_mode {
        ApprovalMode::Suggest => "suggest",
        ApprovalMode::AutoEdit => "auto-edit",
        ApprovalMode::FullAuto => "full-auto",
    };
    let mut params = serde_json::json!({
        "agentId": stored.agent_id,
        "cwd": cwd,
        "approvalMode": approval_mode_str,
    });
    if !stored.enabled_skill_ids.is_empty() {
        params["enabledSkillIds"] = serde_json::to_value(&stored.enabled_skill_ids)
            .unwrap_or(Value::Array(vec![]));
    }
    if let Some(mcp) = &stored.mcp_overrides {
        params["mcpOverrides"] = mcp.clone();
    }

    let outcome = client
        .call(
            cusa_rpc::method::SESSION_RESUME,
            Some(params),
            Duration::from_secs(30),
        )
        .await
        .map_err(|e| e.to_string())?;
    if let Some(err) = outcome.error {
        return Err(format!("session/resume failed: {}", err.message));
    }
    let value = outcome
        .result
        .ok_or_else(|| "session/resume returned no result".to_string())?;
    let parsed: SessionResumeResult =
        serde_json::from_value(value).map_err(|e| e.to_string())?;
    let model = parsed
        .model
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| stored.model.clone());

    Ok(SessionResumedPayload {
        session_id: parsed.session_id,
        agent_id: stored.agent_id.clone(),
        model,
        approval_mode: stored.approval_mode,
        enabled_skill_ids: stored.enabled_skill_ids.clone(),
        mcp_overrides: stored.mcp_overrides.clone(),
        stored: stored.clone(),
    })
}

/// Apply a completed resume (or surface the error).
pub fn apply_result(state: &mut AppState, result: Result<SessionResumedPayload, String>) {
    match result {
        Ok(payload) => {
            state.session.session_id = Some(payload.session_id);
            state.session.agent_id = Some(payload.agent_id.clone());
            state.session.model = payload.model.clone();
            state.session.approval_mode = payload.approval_mode;
            state.session.enabled_skill_ids = payload.enabled_skill_ids.clone();
            state.session.skills_count = payload.enabled_skill_ids.len();
            state.session.manual_model_override = None;
            state.mcp_overrides = payload.mcp_overrides;
            state.always_approved_tools.clear();
            state.transcript.clear();
            state.usage.reset();
            state.current_turn = None;
            state.phase = RunPhase::Idle;
            state.transcript_scroll = 0;

            persist_resumed(state, &payload.stored, &payload.model);

            state.overlay = Overlay::Toast {
                message: format!("resumed {}", payload.stored.short_agent_id()),
                created: Instant::now(),
            };
        }
        Err(err) => {
            // Dispose may have already cleared the sidecar session.
            state.session.session_id = None;
            state.phase = RunPhase::Idle;
            state.current_turn = None;
            state.overlay = Overlay::Toast {
                message: err.clone(),
                created: Instant::now(),
            };
            state
                .transcript
                .push(crate::app::transcript::TranscriptEntry::Error(err));
        }
    }
}

fn persist_resumed(state: &AppState, stored: &StoredSession, model: &str) {
    let Some(store) = state.session_store.as_ref() else {
        return;
    };
    let now = now_unix();
    let mut updated = stored.clone();
    updated.last_used_at = now;
    updated.model = model.to_string();
    updated.approval_mode = state.session.approval_mode;
    updated.enabled_skill_ids = state.session.enabled_skill_ids.clone();
    updated.mcp_overrides = state.mcp_overrides.clone();
    if let Err(err) = store.record_new(updated) {
        tracing::warn!(target: "session_store", ?err, "record_new (in-tui resume) failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::internal::channel as internal_channel;
    use crate::app::transcript::TranscriptEntry;
    use crate::session_store::SessionStore;
    use cusa_rpc::RequestId;
    use std::sync::atomic::{AtomicU64, Ordering};

    static STORE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_stored(id: &str, cwd: &str, last: i64) -> StoredSession {
        StoredSession {
            agent_id: id.into(),
            cwd: cwd.into(),
            model: "composer-2.5".into(),
            approval_mode: ApprovalMode::AutoEdit,
            enabled_skill_ids: vec!["skill-a".into()],
            mcp_overrides: Some(serde_json::json!({"servers": {"demo": {}}})),
            created_at: last,
            last_used_at: last,
            turns: 2,
        }
    }

    fn temp_store(entries: &[StoredSession]) -> SessionStore {
        let id = STORE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "cusa-resume-test-{}-{}",
            std::process::id(),
            id
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.json");
        let store = SessionStore::new(path);
        for entry in entries {
            store.record_new(entry.clone()).unwrap();
        }
        store
    }

    #[test]
    fn resolve_arg_exact_prefix_and_index() {
        let candidates = vec![
            make_stored("abcdef123456", "/tmp/repo", 300),
            make_stored("zzzzzzzz9999", "/tmp/repo", 200),
        ];
        assert_eq!(
            resolve_arg(&candidates, "abcdef123456")
                .unwrap()
                .agent_id,
            "abcdef123456"
        );
        assert_eq!(
            resolve_arg(&candidates, "abcdef12").unwrap().agent_id,
            "abcdef123456"
        );
        assert_eq!(
            resolve_arg(&candidates, "1").unwrap().agent_id,
            "abcdef123456"
        );
        assert_eq!(
            resolve_arg(&candidates, "2").unwrap().agent_id,
            "zzzzzzzz9999"
        );
        assert!(resolve_arg(&candidates, "nope").is_none());
        assert!(resolve_arg(&candidates, "0").is_none());
    }

    #[test]
    fn open_shows_toast_when_no_sessions() {
        let mut state = AppState::new("/tmp/empty".into());
        state.session_store = Some(temp_store(&[]));
        open(&mut state);
        assert!(state.overlay.is_toast());
    }

    #[test]
    fn open_shows_picker_for_cwd_sessions() {
        let mut state = AppState::new("/tmp/repo".into());
        state.session_store = Some(temp_store(&[
            make_stored("agent-one-aaaaaaaa", "/tmp/repo", 10),
            make_stored("other-cwd", "/elsewhere", 20),
        ]));
        open(&mut state);
        match &state.overlay {
            Overlay::Resume(overlay) => {
                assert_eq!(overlay.candidates.len(), 1);
                assert_eq!(overlay.candidates[0].agent_id, "agent-one-aaaaaaaa");
            }
            other => panic!("expected Resume overlay, got {other:?}"),
        }
    }

    #[test]
    fn open_blocked_while_run_active() {
        let mut state = AppState::new("/tmp/repo".into());
        state.session_store = Some(temp_store(&[make_stored("a", "/tmp/repo", 1)]));
        state.phase = RunPhase::Streaming;
        open(&mut state);
        assert!(state.overlay.is_toast());
        assert!(!matches!(state.overlay, Overlay::Resume(_)));
    }

    #[test]
    fn apply_result_restores_spec_053_and_clears_transcript() {
        let mut state = AppState::new("/tmp/repo".into());
        state.session.session_id = Some("old-sess".into());
        state.session.agent_id = Some("old-agent".into());
        state.session.manual_model_override =
            Some(cusa_rpc::ModelSelection::id_only("sticky"));
        state.always_approved_tools.insert("shell".into());
        state.transcript.push(TranscriptEntry::User("hi".into()));
        state.transcript_scroll = 12;
        state.session_store = Some(temp_store(&[]));

        let stored = make_stored("new-agent-bbbbbbbb", "/tmp/repo", 50);
        apply_result(
            &mut state,
            Ok(SessionResumedPayload {
                session_id: "sess_new".into(),
                agent_id: stored.agent_id.clone(),
                model: "composer-2.5".into(),
                approval_mode: stored.approval_mode,
                enabled_skill_ids: stored.enabled_skill_ids.clone(),
                mcp_overrides: stored.mcp_overrides.clone(),
                stored: stored.clone(),
            }),
        );

        assert_eq!(state.session.session_id.as_deref(), Some("sess_new"));
        assert_eq!(state.session.agent_id.as_deref(), Some("new-agent-bbbbbbbb"));
        assert_eq!(state.session.approval_mode, ApprovalMode::AutoEdit);
        assert_eq!(state.session.enabled_skill_ids, vec!["skill-a".to_string()]);
        assert!(state.session.manual_model_override.is_none());
        assert!(state.always_approved_tools.is_empty());
        assert!(state.transcript.is_empty());
        assert_eq!(state.transcript_scroll, 0);
        assert_eq!(state.phase, RunPhase::Idle);
        assert!(state.overlay.is_toast());

        let listed = state
            .session_store
            .as_ref()
            .unwrap()
            .list_for_cwd("/tmp/repo");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].agent_id, "new-agent-bbbbbbbb");
    }

    #[tokio::test]
    async fn dispose_then_resume_emits_internal_event() {
        let mut state = AppState::new("/tmp/repo".into());
        let (tx, mut rx) = internal_channel();
        state.internal_tx = Some(tx);
        state.session.session_id = Some("sess_old".into());
        state.session_store = Some(temp_store(&[make_stored(
            "agent-resume-cccccccc",
            "/tmp/repo",
            1,
        )]));

        let (client, mut peer) = SidecarClient::in_memory();
        resume_direct(&mut state, &client, "agent-resume-cccccccc");

        // dispose
        let frame = peer.expect_frame().await;
        let crate::sidecar::OutboundFrame::Value(v) = frame else {
            panic!("expected dispose request");
        };
        assert_eq!(v["method"], "session/dispose");
        assert_eq!(v["params"]["sessionId"], "sess_old");
        let id = RequestId::Num(v["id"].as_i64().unwrap());
        peer.respond_ok(id, serde_json::json!({ "ok": true }));

        // resume
        let frame = peer.expect_frame().await;
        let crate::sidecar::OutboundFrame::Value(v) = frame else {
            panic!("expected resume request");
        };
        assert_eq!(v["method"], "session/resume");
        assert_eq!(v["params"]["agentId"], "agent-resume-cccccccc");
        assert_eq!(v["params"]["approvalMode"], "auto-edit");
        assert_eq!(v["params"]["enabledSkillIds"], serde_json::json!(["skill-a"]));
        let id = RequestId::Num(v["id"].as_i64().unwrap());
        peer.respond_ok(
            id,
            serde_json::json!({ "sessionId": "sess_resumed", "model": "composer-2.5" }),
        );

        let event = rx.recv().await.expect("SessionResumed event");
        let AppInternalEvent::SessionResumed(Ok(payload)) = event else {
            panic!("expected Ok SessionResumed, got {event:?}");
        };
        assert_eq!(payload.session_id, "sess_resumed");
        assert_eq!(payload.agent_id, "agent-resume-cccccccc");
        assert_eq!(payload.approval_mode, ApprovalMode::AutoEdit);
    }
}
