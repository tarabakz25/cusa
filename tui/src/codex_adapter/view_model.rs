// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// CusaViewModel — pure translations from cusa domain state into adapter
// display types (SPEC-104). Synchronous only; no sidecar or SDK calls.

use crate::app::overlay::Overlay;
use crate::app::state::{AppState, RunPhase};
use crate::app::transcript::{TranscriptEntry, TurnState};
use crate::codex_adapter::types::{
    ApprovalModeView, ComposerView, HistoryCellView, RouterSourceView, RunPhaseView,
};
use cusa_rpc::{ApprovalMode, RouterSource};

/// Adapter translating `AppState` and transcript data into Codex widget views.
pub struct CusaViewModel;

impl CusaViewModel {
    /// Map committed transcript entries plus optional live turn into history cells.
    pub fn history_cells(
        entries: &[TranscriptEntry],
        live: Option<&TurnState>,
    ) -> Vec<HistoryCellView> {
        let mut cells: Vec<HistoryCellView> = entries
            .iter()
            .map(Self::entry_to_cell)
            .collect();

        if let Some(turn) = live {
            if !turn.assistant_text.is_empty() {
                cells.push(HistoryCellView::LiveAssistant {
                    text: turn.assistant_text.clone(),
                });
            }
        }

        cells
    }

    /// Map root app state into bottom-pane composer props.
    pub fn composer_view(state: &AppState) -> ComposerView {
        ComposerView {
            buffer: state.input.clone(),
            cursor_pos: state.cursor_pos,
            active: !overlay_blocks_input(&state.overlay),
            multiline: state.input.contains('\n'),
            history_nav_active: state.history_nav.is_some(),
            phase: map_run_phase(state.phase),
        }
    }

    pub fn map_router_source(source: RouterSource) -> RouterSourceView {
        match source {
            RouterSource::Rule => RouterSourceView::Rule,
            RouterSource::Llm => RouterSourceView::Llm,
            RouterSource::Local => RouterSourceView::Local,
            RouterSource::Override => RouterSourceView::Override,
            RouterSource::Fallback => RouterSourceView::Fallback,
        }
    }

    pub fn map_approval_mode(mode: ApprovalMode) -> ApprovalModeView {
        match mode {
            ApprovalMode::Suggest => ApprovalModeView::Suggest,
            ApprovalMode::AutoEdit => ApprovalModeView::AutoEdit,
            ApprovalMode::FullAuto => ApprovalModeView::FullAuto,
        }
    }

    fn entry_to_cell(entry: &TranscriptEntry) -> HistoryCellView {
        match entry {
            TranscriptEntry::User(text) => HistoryCellView::UserPrompt {
                text: text.clone(),
            },
            TranscriptEntry::RouterDecision {
                model,
                rationale,
                source,
            } => HistoryCellView::RouterDecision {
                model: model.clone(),
                rationale: rationale.clone(),
                source: Self::map_router_source(*source),
            },
            TranscriptEntry::ToolDecision { tool, decision } => HistoryCellView::ToolDecision {
                tool: tool.clone(),
                decision: decision.clone(),
            },
            TranscriptEntry::Assistant { text, model } => HistoryCellView::Assistant {
                text: text.clone(),
                model: model.clone(),
            },
            TranscriptEntry::TurnSummary { summary, model } => HistoryCellView::TurnSummary {
                summary: summary.clone(),
                model: model.clone(),
            },
            TranscriptEntry::ToolCall { name, args_preview } => HistoryCellView::ToolCall {
                name: name.clone(),
                args_preview: args_preview.clone(),
            },
            TranscriptEntry::ToolResult { name, ok, preview } => HistoryCellView::ToolResult {
                name: name.clone(),
                ok: *ok,
                preview: preview.clone(),
            },
            TranscriptEntry::Error(msg) => HistoryCellView::Error {
                message: msg.clone(),
            },
            TranscriptEntry::Note(msg) => HistoryCellView::Note {
                message: msg.clone(),
            },
        }
    }
}

fn map_run_phase(phase: RunPhase) -> RunPhaseView {
    match phase {
        RunPhase::Idle => RunPhaseView::Idle,
        RunPhase::Sending => RunPhaseView::Sending,
        RunPhase::Streaming => RunPhaseView::Streaming,
        RunPhase::AwaitingApproval => RunPhaseView::AwaitingApproval,
        RunPhase::Cancelling => RunPhaseView::Cancelling,
    }
}

fn overlay_blocks_input(overlay: &Overlay) -> bool {
    // Toasts are transient notices; keep composer text visible while they show.
    overlay.is_open() && !overlay.is_toast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;

    #[test]
    fn spec_104_history_cells_maps_user_prompt() {
        let entries = vec![TranscriptEntry::User("hello world".into())];
        let cells = CusaViewModel::history_cells(&entries, None);
        assert_eq!(cells.len(), 1);
        assert_eq!(
            cells[0],
            HistoryCellView::UserPrompt {
                text: "hello world".into()
            }
        );
    }

    #[test]
    fn spec_104_history_cells_appends_live_assistant() {
        let entries = vec![TranscriptEntry::User("hi".into())];
        let mut turn = TurnState::new("hi".into());
        turn.assistant_text = "partial".into();
        let cells = CusaViewModel::history_cells(&entries, Some(&turn));
        assert_eq!(cells.len(), 2);
        assert_eq!(
            cells[1],
            HistoryCellView::LiveAssistant {
                text: "partial".into()
            }
        );
    }

    #[test]
    fn spec_104_composer_view_reflects_input_state() {
        let mut state = AppState::new("/tmp".into());
        state.input = "draft text".into();
        state.cursor_pos = 6;
        state.begin_user_turn("other".into());

        let view = CusaViewModel::composer_view(&state);
        assert_eq!(view.buffer, "draft text");
        assert_eq!(view.cursor_pos, 6);
        assert!(view.active);
        assert!(!view.multiline);
        assert!(!view.history_nav_active);
        assert_eq!(view.phase, RunPhaseView::Sending);
    }

    #[test]
    fn spec_104_composer_view_inactive_when_overlay_open() {
        let mut state = AppState::new("/tmp".into());
        state.overlay = Overlay::Help;
        let view = CusaViewModel::composer_view(&state);
        assert!(!view.active);
    }

    #[test]
    fn spec_104_composer_view_active_during_toast() {
        use std::time::Instant;
        let mut state = AppState::new("/tmp".into());
        state.input = "hello".into();
        state.overlay = Overlay::Toast {
            message: "copied \"x\"".into(),
            created: Instant::now(),
        };
        let view = CusaViewModel::composer_view(&state);
        assert!(view.active, "toast must not hide composer text");
        assert_eq!(view.buffer, "hello");
    }

    #[test]
    fn spec_104_composer_view_multiline_when_buffer_has_newline() {
        let mut state = AppState::new("/tmp".into());
        state.input = "line one\nline two".into();
        let view = CusaViewModel::composer_view(&state);
        assert!(view.multiline);
    }

    #[test]
    fn spec_104_router_source_maps_to_display_enum() {
        assert_eq!(
            CusaViewModel::map_router_source(RouterSource::Override),
            RouterSourceView::Override
        );
        assert_eq!(
            CusaViewModel::map_router_source(RouterSource::Rule),
            RouterSourceView::Rule
        );
        assert_eq!(
            CusaViewModel::map_router_source(RouterSource::Llm),
            RouterSourceView::Llm
        );
        assert_eq!(
            CusaViewModel::map_router_source(RouterSource::Fallback),
            RouterSourceView::Fallback
        );

        let entries = vec![TranscriptEntry::RouterDecision {
            model: "m".into(),
            rationale: "r".into(),
            source: RouterSource::Rule,
        }];
        let cells = CusaViewModel::history_cells(&entries, None);
        match &cells[0] {
            HistoryCellView::RouterDecision { source, .. } => {
                assert_eq!(*source, RouterSourceView::Rule);
                assert_eq!(source.tag(), "rule");
            }
            other => panic!("expected RouterDecision, got {other:?}"),
        }
    }

    #[test]
    fn spec_104_approval_mode_maps_to_display_enum() {
        assert_eq!(
            CusaViewModel::map_approval_mode(ApprovalMode::Suggest),
            ApprovalModeView::Suggest
        );
        assert_eq!(
            CusaViewModel::map_approval_mode(ApprovalMode::AutoEdit),
            ApprovalModeView::AutoEdit
        );
        assert_eq!(
            CusaViewModel::map_approval_mode(ApprovalMode::FullAuto),
            ApprovalModeView::FullAuto
        );
    }
}
