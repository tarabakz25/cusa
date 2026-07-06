// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// `/context` command orchestration (SPEC-092).
//
// `/context` with no arg opens an info overlay explaining the current
// strategy and the three choices. `/context strategy=<name>` records the
// forced strategy on `AppState` and fires a `context/setStrategy` RPC at
// the sidecar so subsequent turns honour it.
//
// The RPC method name is a placeholder — the sidecar half of Slice 7 owns
// the final schema. Until it lands the request will return
// `METHOD_NOT_FOUND`, which we surface as a transcript error (best-effort).
// See `handoff-phase-e.md` for the reconciliation note.

use crate::app::overlay::{ContextOverlay, Overlay};
use crate::app::state::{AppState, ContextStrategy};
use crate::app::transcript::TranscriptEntry;
use crate::sidecar::SidecarClient;
use std::time::{Duration, Instant};

/// Placeholder RPC method — coordinated with the sidecar subagent. Kept
/// as a string constant so a single edit here lands when the sidecar
/// half chooses a name.
pub const CONTEXT_SET_STRATEGY: &str = "context/setStrategy";

/// Parse a lowercase strategy name into a typed [`ContextStrategy`].
pub fn parse_strategy(name: &str) -> Option<ContextStrategy> {
    match name {
        "auto" => Some(ContextStrategy::Auto),
        "raw" => Some(ContextStrategy::Raw),
        "summary" => Some(ContextStrategy::Summary),
        _ => None,
    }
}

/// Handle `/context` with no argument — open the info overlay.
pub fn open_info(state: &mut AppState) {
    state.overlay = Overlay::Context(ContextOverlay {
        current: state.context_strategy,
    });
}

/// Handle `/context strategy=<name>` — record the strategy and dispatch
/// the RPC. Surfaces a toast on unknown names.
pub fn set_strategy(state: &mut AppState, client: &SidecarClient, name: &str) {
    let Some(strategy) = parse_strategy(name) else {
        state.overlay = Overlay::Toast {
            message: format!(
                "unknown context strategy: {name} (auto/raw/summary)"
            ),
            created: Instant::now(),
        };
        return;
    };
    state.context_strategy = strategy;
    state.overlay = Overlay::Toast {
        message: format!("context strategy: {}", strategy.label()),
        created: Instant::now(),
    };
    dispatch(state, client, strategy);
}

fn dispatch(state: &AppState, client: &SidecarClient, strategy: ContextStrategy) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let client = client.clone();
    let session_id = state.session.session_id.clone().unwrap_or_default();
    let internal_tx = state.internal_tx.clone();
    tokio::spawn(async move {
        let params = serde_json::json!({
            "sessionId": session_id,
            "strategy": strategy.label(),
        });
        let outcome = client
            .call(CONTEXT_SET_STRATEGY, Some(params), Duration::from_secs(15))
            .await;
        if let (Some(tx), Err(err)) = (internal_tx.as_ref(), outcome.as_ref()) {
            let _ = tx.send(crate::app::internal::AppInternalEvent::ContextSetStrategy(
                Err(err.to_string()),
            ));
        }
        // Best-effort: METHOD_NOT_FOUND is common until the sidecar half
        // lands, so we don't loudly surface RPC errors here.
        drop(outcome);
    });
}

/// Push an error into the transcript when the sidecar rejects the
/// context/setStrategy call for a reason other than "not found".
pub fn apply_result(state: &mut AppState, result: Result<(), String>) {
    if let Err(err) = result {
        if err.to_lowercase().contains("methodnotfound")
            || err.contains("-32601")
            || err.to_lowercase().contains("method not found")
        {
            // Sidecar half hasn't shipped — silent no-op.
            return;
        }
        state
            .transcript
            .push(TranscriptEntry::Error(format!("context/setStrategy failed: {err}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_092_parse_strategy_variants() {
        assert_eq!(parse_strategy("auto"), Some(ContextStrategy::Auto));
        assert_eq!(parse_strategy("raw"), Some(ContextStrategy::Raw));
        assert_eq!(parse_strategy("summary"), Some(ContextStrategy::Summary));
        assert_eq!(parse_strategy("bogus"), None);
    }

    #[test]
    fn spec_092_open_info_populates_current_strategy() {
        let mut state = AppState::new("/x".into());
        state.context_strategy = ContextStrategy::Summary;
        open_info(&mut state);
        match &state.overlay {
            Overlay::Context(o) => assert_eq!(o.current, ContextStrategy::Summary),
            _ => panic!("expected context overlay"),
        }
    }
}
