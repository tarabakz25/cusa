// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-110: Insta snapshot integration tests for UI parity gates (phased
// Codex TUI cherry-pick). Snapshots live under `tui/tests/snapshots/`.

use cusa_tui::app::draw_to_buffer;
use cusa_tui::codex_adapter::{BottomPaneWidget, CodexTranscriptWidget};
use cusa_tui::app::state::{AppState, RunPhase};
use cusa_tui::app::transcript::{TranscriptEntry, TurnState};
use cusa_tui::codex_adapter::tool_display;
use cusa_tui::codex_adapter::welcome::composer_footer_line;
use cusa_rpc::RouterSource;
use ratatui::backend::TestBackend;
use ratatui::widgets::Widget;
use ratatui::Terminal;
use std::path::Path;

/// Flatten a `TestBackend` buffer into a plain string (one char per cell).
fn buffer_string(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol().to_string())
        .collect()
}

/// Render a full `AppState` frame into a snapshot string.
fn render_app_snapshot(state: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    draw_to_buffer(state, &mut terminal).unwrap();
    buffer_string(&terminal)
}

/// Render the vendored bottom pane (composer + footer status).
fn render_bottom_pane_snapshot(state: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let widget = BottomPaneWidget::from_state(state);
    terminal
        .draw(|f| {
            f.render_widget(widget, f.area());
        })
        .unwrap();
    buffer_string(&terminal)
}

fn render_composer_footer_snapshot(state: &AppState, width: u16) -> String {
    let backend = TestBackend::new(width, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    let line = composer_footer_line(&state.session);
    terminal
        .draw(|f| {
            use ratatui::widgets::Paragraph;
            Paragraph::new(line).render(f.area(), f.buffer_mut());
        })
        .unwrap();
    buffer_string(&terminal)
}

/// Render transcript pane alone via the P2 Codex history_cell pipeline (SPEC-107/110).
fn render_transcript_snapshot(state: &AppState, width: u16, height: u16) -> String {
    use cusa_tui::codex_adapter::CodexTranscriptWidget;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let widget = CodexTranscriptWidget::new(
        &state.transcript,
        state.current_turn.as_ref(),
        Path::new(&state.session.cwd),
    )
    .with_session(&state.session);
    terminal
        .draw(|f| {
            f.render_widget(widget, f.area());
        })
        .unwrap();
    buffer_string(&terminal)
}

fn render_tool_block_snapshot(entry: &TranscriptEntry, cwd: &Path, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let lines = tool_display::render_tool_entry(entry, cwd, width).expect("tool entry");
    terminal
        .draw(|f| {
            use ratatui::widgets::Paragraph;
            Paragraph::new(lines).render(f.area(), f.buffer_mut());
        })
        .unwrap();
    buffer_string(&terminal)
}

fn render_status_chrome_snapshot(state: &AppState, width: u16) -> String {
    render_composer_footer_snapshot(state, width)
}

#[test]
fn spec_110_p0_foundation_idle_frame_80x24() {
    let state = AppState::new("/tmp/repo".into());
    insta::assert_snapshot!(render_app_snapshot(&state, 80, 24));
}

#[test]
fn spec_110_p0_foundation_idle_frame_120x40() {
    let state = AppState::new("/tmp/repo".into());
    insta::assert_snapshot!(render_app_snapshot(&state, 120, 40));
}

#[test]
fn spec_110_p1_composer_idle() {
    let state = AppState::new("/tmp/repo".into());
    insta::assert_snapshot!(render_bottom_pane_snapshot(&state, 80, 4));
}

#[test]
fn spec_110_p2_transcript_mixed() {
    let mut state = AppState::new("/tmp/repo".into());
    state.transcript = vec![
        TranscriptEntry::User("explain this module".into()),
        TranscriptEntry::RouterDecision {
            model: "composer-2.5".into(),
            rationale: "fast rule".into(),
            source: RouterSource::Rule,
        },
        TranscriptEntry::ToolCall {
            name: "read_file".into(),
            args_preview: "{\"path\":\"lib.rs\"}".into(),
        },
    ];
    let mut turn = TurnState::new("explain this module".into());
    turn.assistant_text = "Streaming **assistant** output…".into();
    state.current_turn = Some(turn);
    state.phase = RunPhase::Streaming;
    insta::assert_snapshot!(render_transcript_snapshot(&state, 80, 16));
}

#[test]
fn spec_108_tool_exec_block() {
    let entry = TranscriptEntry::ToolCall {
        name: "shell_exec".into(),
        args_preview: r#"{"cmd":"echo ok"}"#.into(),
    };
    insta::assert_snapshot!(render_tool_block_snapshot(
        &entry,
        Path::new("/tmp/repo"),
        80,
        4
    ));
}

#[test]
fn spec_108_tool_diff_block() {
    let entry = TranscriptEntry::ToolResult {
        name: "apply_patch".into(),
        ok: true,
        preview: r#"{"src/main.rs":{"content":"fn main() {}\n"}}"#.into(),
    };
    insta::assert_snapshot!(render_tool_block_snapshot(
        &entry,
        Path::new("/tmp/repo"),
        80,
        10
    ));
}

#[test]
fn spec_109_status_chrome_idle_80x24() {
    let state = AppState::new("/tmp/repo".into());
    insta::assert_snapshot!(render_status_chrome_snapshot(&state, 80));
}
