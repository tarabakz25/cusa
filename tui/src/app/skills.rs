// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// `/skills` overlay orchestration (SPEC-032).
//
// The overlay lifecycle:
//   1. `/skills` slash-command → `open()` puts a loading overlay onscreen
//      and fires `skills/list { cwd }` in a detached task.
//   2. The task pushes `AppInternalEvent::SkillsList` when the response
//      arrives. `apply_list_response` populates the overlay.
//   3. Space toggles a row; Enter fires `skills/setEnabled` and closes.

use crate::app::internal::{AppInternalEvent, SkillsListPayload};
use crate::app::overlay::{Overlay, SkillsOverlay};
use crate::app::state::AppState;
use crate::sidecar::SidecarClient;
use cusa_rpc::{SkillInfo, SkillsListResult};
use std::time::Duration;

/// Open the `/skills` overlay and dispatch `skills/list`.
pub fn open(state: &mut AppState, client: &SidecarClient) {
    state.overlay = Overlay::Skills(SkillsOverlay::loading());
    spawn_list(state, client);
}

fn spawn_list(state: &AppState, client: &SidecarClient) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let internal_tx = state.internal_tx.clone();
    let client = client.clone();
    let cwd = state.session.cwd.clone();
    tokio::spawn(async move {
        let params = serde_json::json!({ "cwd": cwd });
        let outcome = client
            .call(
                cusa_rpc::method::SKILLS_LIST,
                Some(params),
                Duration::from_secs(20),
            )
            .await;
        let result = to_skills_result(outcome);
        if let Some(tx) = internal_tx {
            let _ = tx.send(AppInternalEvent::SkillsList(result));
        }
    });
}

fn to_skills_result(
    outcome: anyhow::Result<crate::sidecar::CallOutcome>,
) -> Result<SkillsListPayload, String> {
    let outcome = outcome.map_err(|e| e.to_string())?;
    if let Some(err) = outcome.error {
        return Err(err.message);
    }
    let value = outcome.result.ok_or_else(|| "no result".to_string())?;
    let parsed: SkillsListResult = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(SkillsListPayload {
        skills: parsed.skills,
        warnings: parsed.warnings,
    })
}

/// Apply a response from `skills/list` to the current overlay.
pub fn apply_list_response(
    state: &mut AppState,
    result: Result<SkillsListPayload, String>,
) {
    if !matches!(state.overlay, Overlay::Skills(_)) {
        return;
    }
    let enabled = state.session.enabled_skill_ids.clone();
    if let Overlay::Skills(overlay) = &mut state.overlay {
        match result {
            Ok(payload) => overlay.populate(payload.skills, payload.warnings, &enabled),
            Err(err) => {
                overlay.loading = false;
                overlay.error = Some(format!("skills/list failed: {err}"));
            }
        }
    }
}

/// Bind Enter → commit toggled skills to the sidecar.
pub fn commit(state: &mut AppState, client: &SidecarClient) {
    let Overlay::Skills(overlay) = &state.overlay else {
        return;
    };
    let enabled_ids = overlay.enabled_ids();
    state.session.enabled_skill_ids = enabled_ids.clone();
    state.session.skills_count = enabled_ids.len();
    let session_id = state.session.session_id.clone().unwrap_or_default();
    if session_id.is_empty() {
        state.overlay = Overlay::None;
        return;
    }
    spawn_set_enabled(state, client, session_id, enabled_ids);
    state.overlay = Overlay::None;
}

fn spawn_set_enabled(
    state: &AppState,
    client: &SidecarClient,
    session_id: String,
    skill_ids: Vec<String>,
) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let internal_tx = state.internal_tx.clone();
    let client = client.clone();
    tokio::spawn(async move {
        let params = serde_json::json!({
            "sessionId": session_id,
            "skillIds": skill_ids.clone(),
        });
        let outcome = client
            .call(
                cusa_rpc::method::SKILLS_SET_ENABLED,
                Some(params),
                Duration::from_secs(15),
            )
            .await;
        if let Some(tx) = internal_tx {
            let result = match outcome {
                Ok(o) if o.is_ok() => Ok(skill_ids),
                Ok(o) => Err(o
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "unknown".into())),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(AppInternalEvent::SkillsSetEnabled(result));
        }
    });
}

/// Test hook used by unit tests to load a synthetic skills list without a
/// live sidecar.
#[cfg(test)]
pub fn inject_list_for_test(state: &mut AppState, skills: Vec<SkillInfo>) {
    apply_list_response(
        state,
        Ok(SkillsListPayload {
            skills,
            warnings: vec![],
        }),
    );
}
