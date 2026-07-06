// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// `/model` picker overlay orchestration (SPEC-016).

use crate::app::internal::AppInternalEvent;
use crate::app::overlay::{ModelPickerOverlay, Overlay};
use crate::app::state::AppState;
use crate::sidecar::SidecarClient;
use cusa_rpc::{ModelInfo, ModelsListResult};
use std::time::Duration;

/// Open the `/model` picker. If the models list is cached, populate the
/// overlay directly; otherwise show a "loading…" state and dispatch a
/// `models/list` request.
pub fn open(state: &mut AppState, client: &SidecarClient) {
    if let Some(models) = state.models_cache.clone() {
        state.overlay = Overlay::ModelPicker(ModelPickerOverlay::populated(models));
        return;
    }
    state.overlay = Overlay::ModelPicker(ModelPickerOverlay::loading());
    spawn_list(state, client);
}

fn spawn_list(state: &AppState, client: &SidecarClient) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let internal_tx = state.internal_tx.clone();
    let client = client.clone();
    tokio::spawn(async move {
        let outcome = client
            .call(cusa_rpc::method::MODELS_LIST, None, Duration::from_secs(20))
            .await;
        let result = to_models_result(outcome);
        if let Some(tx) = internal_tx {
            let _ = tx.send(AppInternalEvent::ModelsList(result));
        }
    });
}

fn to_models_result(
    outcome: anyhow::Result<crate::sidecar::CallOutcome>,
) -> Result<Vec<ModelInfo>, String> {
    let outcome = outcome.map_err(|e| e.to_string())?;
    if let Some(err) = outcome.error {
        return Err(err.message);
    }
    let value = outcome.result.ok_or_else(|| "no result".to_string())?;
    let parsed: ModelsListResult = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(parsed.models)
}

/// Apply a `models/list` response to the current overlay.
pub fn apply_list_response(state: &mut AppState, result: Result<Vec<ModelInfo>, String>) {
    match result {
        Ok(models) => {
            state.models_cache = Some(models.clone());
            if matches!(state.overlay, Overlay::ModelPicker(_)) {
                state.overlay = Overlay::ModelPicker(ModelPickerOverlay::populated(models));
            }
        }
        Err(err) => {
            if let Overlay::ModelPicker(overlay) = &mut state.overlay {
                overlay.loading = false;
                overlay.error = Some(format!("models/list failed: {err}"));
            }
        }
    }
}
