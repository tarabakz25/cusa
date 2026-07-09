// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// `/model` picker overlay orchestration (SPEC-016).

use crate::app::internal::AppInternalEvent;
use crate::app::overlay::{ModelPickerOverlay, Overlay};
use crate::app::state::AppState;
use crate::sidecar::SidecarClient;
use cusa_rpc::{ModelInfo, ModelSelection, ModelsListResult};
use std::time::Duration;

/// Human-readable model label for status chrome and toasts.
pub fn format_model_label(selection: &ModelSelection, models: Option<&[ModelInfo]>) -> String {
    let model_meta = models.and_then(|ms| ms.iter().find(|m| m.id == selection.id));
    let base = model_meta
        .and_then(|m| m.display_name.as_deref())
        .unwrap_or(selection.id.as_str());
    if selection.params.is_empty() {
        return base.to_string();
    }
    let mut parts = vec![base.to_string()];
    for param in &selection.params {
        let label = model_meta
            .and_then(|m| m.parameters.iter().find(|d| d.id == param.id))
            .and_then(|def| {
                def.values
                    .iter()
                    .find(|v| v.value == param.value)
                    .and_then(|v| v.display_name.as_deref())
            })
            .unwrap_or(param.value.as_str());
        parts.push(label.to_string());
    }
    parts.join(" ")
}

fn populated_picker(state: &AppState, models: Vec<ModelInfo>) -> ModelPickerOverlay {
    let mut picker = ModelPickerOverlay::populated(models);
    if let Some(existing) = &state.session.manual_model_override {
        picker.restore_selection(existing);
    }
    picker
}

/// Open the `/model` picker. If the models list is cached, populate the
/// overlay directly; otherwise show a "loading…" state and dispatch a
/// `models/list` request.
pub fn open(state: &mut AppState, client: &SidecarClient) {
    if let Some(models) = state.models_cache.clone() {
        state.overlay = Overlay::ModelPicker(populated_picker(state, models));
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
                state.overlay = Overlay::ModelPicker(populated_picker(state, models));
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

#[cfg(test)]
mod tests {
    use super::*;
    use cusa_rpc::{ModelParameterDefinition, ModelParameterValue, ModelParameterValueOption};

    #[test]
    fn format_model_label_includes_param_display_names() {
        let models = [ModelInfo {
            id: "composer-2.5".into(),
            display_name: Some("Composer 2.5".into()),
            provider: None,
            supports_thinking: false,
            parameters: vec![ModelParameterDefinition {
                id: "effort".into(),
                display_name: Some("Effort".into()),
                values: vec![
                    ModelParameterValueOption {
                        value: "high".into(),
                        display_name: Some("High".into()),
                    },
                    ModelParameterValueOption {
                        value: "low".into(),
                        display_name: None,
                    },
                ],
            }],
        }];
        let sel = ModelSelection {
            id: "composer-2.5".into(),
            params: vec![ModelParameterValue {
                id: "effort".into(),
                value: "high".into(),
            }],
        };
        assert_eq!(
            format_model_label(&sel, Some(&models)),
            "Composer 2.5 High"
        );
    }
}
