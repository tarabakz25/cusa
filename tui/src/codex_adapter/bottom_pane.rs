// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Codex-style bottom pane: tinted composer + status row (ChatComposer
// layout), plus the slash-command suggestion popup (SPEC-002) rendered
// above the composer.

use crate::app::slash::CommandHint;
use crate::app::state::{AppState, RunPhase};
use crate::codex_adapter::composer::ComposerWidget;
use crate::codex_adapter::welcome::composer_footer_line;
use crate::codex_ui::ui_consts::FOOTER_INDENT_COLS;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

const FOOTER_ROW_HEIGHT: u16 = 1;

/// Braille spinner frames for the activity indicator; one frame per
/// 100 ms of turn elapsed time (redraws are driven by the event-loop
/// tick in `app::run_interactive_loop`).
const SPINNER_FRAMES: [&str; 10] = [
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}",
    "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}",
];

/// Maximum rendered suggestion rows; the window scrolls so the selected
/// row stays visible (SPEC-002).
pub const MAX_POPUP_ROWS: usize = 8;

/// Bottom pane matching Codex `ChatComposer` + footer status row.
#[derive(Debug, Clone)]
pub struct BottomPaneWidget {
    composer: ComposerWidget,
    footer: Line<'static>,
    popup: Vec<Line<'static>>,
    activity: Option<Line<'static>>,
}

impl BottomPaneWidget {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            composer: ComposerWidget::from_state(state),
            footer: composer_footer_line(&state.session),
            popup: popup_lines(state),
            activity: activity_line(state),
        }
    }

    /// Rows the activity indicator occupies for this state (1 while a run
    /// is in flight or cancelling, 0 when idle). Also used by the composer
    /// to offset IME cursor placement.
    pub fn activity_rows(state: &AppState) -> u16 {
        if state.phase == RunPhase::Idle { 0 } else { 1 }
    }

    /// Rows the slash-command suggestion popup occupies for this state.
    pub fn popup_rows(state: &AppState) -> u16 {
        state.slash_suggestions().len().min(MAX_POPUP_ROWS) as u16
    }

    pub fn desired_height(state: &AppState, width: u16) -> u16 {
        Self::activity_rows(state)
            + ComposerWidget::desired_height_for_state(state, width)
            + Self::popup_rows(state)
            + FOOTER_ROW_HEIGHT
    }
}

/// Activity indicator rendered above the composer while a run is in
/// flight: `\u{2839} Working \u{b7} 3s \u{b7} Ctrl-C to interrupt` (SPEC-001/004).
fn activity_line(state: &AppState) -> Option<Line<'static>> {
    let label = match state.phase {
        RunPhase::Idle => return None,
        RunPhase::Routing => "Routing",
        RunPhase::Streaming => "Working",
        RunPhase::AwaitingApproval => "Waiting for approval",
        RunPhase::Cancelling => "Cancelling",
    };
    let elapsed_ms = state
        .current_turn
        .as_ref()
        .and_then(|t| t.started_at)
        .map(|s| s.elapsed().as_millis())
        .unwrap_or(0);
    let frame = SPINNER_FRAMES[(elapsed_ms / 100) as usize % SPINNER_FRAMES.len()];
    Some(Line::from(vec![
        Span::styled(frame.to_string(), Style::default().fg(Color::Magenta)),
        Span::from(" "),
        Span::from(label.to_string()).bold(),
        Span::from(format!(
            " \u{b7} {}s \u{b7} Ctrl-C to interrupt",
            elapsed_ms / 1000
        ))
        .dim(),
    ]))
}

/// Build the popup lines — `› /name  description` — windowed around the
/// selected row so it is always visible.
fn popup_lines(state: &AppState) -> Vec<Line<'static>> {
    let suggestions = state.slash_suggestions();
    if suggestions.is_empty() {
        return Vec::new();
    }
    let selected = state.slash_popup_selected.min(suggestions.len() - 1);
    let start = (selected + 1).saturating_sub(MAX_POPUP_ROWS);
    let name_width = suggestions
        .iter()
        .map(|c| c.label().len())
        .max()
        .unwrap_or(0);
    suggestions
        .iter()
        .enumerate()
        .skip(start)
        .take(MAX_POPUP_ROWS)
        .map(|(idx, hint)| popup_line(hint, idx == selected, name_width))
        .collect()
}

fn popup_line(hint: &CommandHint, selected: bool, name_width: usize) -> Line<'static> {
    let marker = if selected { "\u{203a} " } else { "  " };
    // Display label includes aliases — `/clear (new)` — but Tab/Enter
    // always complete/execute the canonical `hint.name`.
    let name = format!("/{:<width$}", hint.label(), width = name_width);
    let name_span = if selected {
        Span::styled(name, Style::default().fg(Color::Cyan)).bold()
    } else {
        Span::styled(name, Style::default().fg(Color::Cyan))
    };
    Line::from(vec![
        Span::styled(marker.to_string(), Style::default().fg(Color::Cyan)),
        name_span,
        Span::from("  "),
        Span::from(hint.description.to_string()).dim(),
    ])
}

impl Widget for BottomPaneWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let popup_height = (self.popup.len() as u16).min(area.height);
        let activity_height = u16::from(self.activity.is_some()).min(area.height);
        // Top to bottom: activity status, suggestion popup (adjacent to the
        // input it filters), composer, footer.
        let [activity_rect, popup_rect, composer_rect, footer_rect] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(activity_height),
                Constraint::Length(popup_height),
                Constraint::Min(1),
                Constraint::Length(FOOTER_ROW_HEIGHT.min(area.height)),
            ])
            .areas(area);

        if let Some(line) = self.activity {
            if activity_rect.height > 0 && activity_rect.width > FOOTER_INDENT_COLS as u16 {
                let indented = Rect {
                    x: activity_rect.x.saturating_add(FOOTER_INDENT_COLS as u16),
                    y: activity_rect.y,
                    width: activity_rect.width.saturating_sub(FOOTER_INDENT_COLS as u16),
                    height: activity_rect.height,
                };
                Paragraph::new(line).render(indented, buf);
            }
        }

        self.composer.render_composer_surface(composer_rect, buf);

        if popup_rect.height > 0 && popup_rect.width > FOOTER_INDENT_COLS as u16 {
            for (i, line) in self
                .popup
                .into_iter()
                .take(popup_rect.height as usize)
                .enumerate()
            {
                let row = Rect {
                    x: popup_rect.x.saturating_add(FOOTER_INDENT_COLS as u16),
                    y: popup_rect.y + i as u16,
                    width: popup_rect.width.saturating_sub(FOOTER_INDENT_COLS as u16),
                    height: 1,
                };
                Paragraph::new(line).render(row, buf);
            }
        }

        if footer_rect.height > 0 && footer_rect.width > FOOTER_INDENT_COLS as u16 {
            let indented = Rect {
                x: footer_rect.x.saturating_add(FOOTER_INDENT_COLS as u16),
                y: footer_rect.y,
                width: footer_rect.width.saturating_sub(FOOTER_INDENT_COLS as u16),
                height: footer_rect.height,
            };
            Paragraph::new(self.footer).render(indented, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AppState, RunPhase};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_to_string(state: &AppState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(BottomPaneWidget::from_state(state), f.area()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    #[test]
    fn spec_002_slash_input_renders_suggestion_popup() {
        let mut state = AppState::new("/tmp".into());
        state.input = "/mo".into();
        state.cursor_pos = 3;
        let out = render_to_string(&state, 100, 10);
        assert!(out.contains("/model"), "popup must list /model: {out}");
        assert!(out.contains("/mode"), "popup must list /mode: {out}");
        assert!(out.contains("\u{203a}"), "selected row marker missing: {out}");
    }

    fn render_rows(state: &AppState, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(BottomPaneWidget::from_state(state), f.area()))
            .unwrap();
        let symbols: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        symbols
            .chunks(width as usize)
            .map(|row| row.concat())
            .collect()
    }

    #[test]
    fn spec_002_popup_renders_above_composer() {
        let mut state = AppState::new("/tmp".into());
        state.input = "/mo".into();
        state.cursor_pos = 3;
        let rows = render_rows(&state, 100, 10);
        let popup_row = rows
            .iter()
            .position(|r| r.contains("/model"))
            .expect("popup row with /model");
        let composer_row = rows
            .iter()
            .position(|r| r.contains("/mo") && !r.contains("/model") && !r.contains("/mode"))
            .expect("composer row with raw input");
        assert!(
            popup_row < composer_row,
            "popup must render above the composer (popup row {popup_row}, composer row {composer_row}): {rows:?}"
        );
    }

    #[test]
    fn spec_002_popup_shows_alias_label_for_clear() {
        let mut state = AppState::new("/tmp".into());
        state.input = "/new".into();
        state.cursor_pos = 4;
        let out = render_to_string(&state, 100, 10);
        assert!(
            out.contains("/clear (new)"),
            "typing /new must surface the aliased /clear row: {out}"
        );
    }

    #[test]
    fn spec_002_plain_prompt_renders_no_popup() {
        let mut state = AppState::new("/tmp".into());
        state.input = "hello".into();
        state.cursor_pos = 5;
        let out = render_to_string(&state, 100, 10);
        assert!(!out.contains("/model"), "no popup for plain prompts: {out}");
    }

    #[test]
    fn spec_002_popup_grows_desired_height_capped() {
        let mut plain = AppState::new("/tmp".into());
        plain.input = "x".into();
        let base = BottomPaneWidget::desired_height(&plain, 80);

        let mut slash = AppState::new("/tmp".into());
        slash.input = "/".into();
        let with_popup = BottomPaneWidget::desired_height(&slash, 80);
        // 12 suggestions capped at MAX_POPUP_ROWS.
        assert_eq!(with_popup, base + MAX_POPUP_ROWS as u16);

        let mut narrow = AppState::new("/tmp".into());
        narrow.input = "/sk".into();
        assert_eq!(BottomPaneWidget::desired_height(&narrow, 80), base + 1);
    }

    #[test]
    fn spec_002_popup_hidden_while_overlay_open() {
        let mut state = AppState::new("/tmp".into());
        state.input = "/mo".into();
        state.overlay = crate::app::overlay::Overlay::Help;
        assert_eq!(BottomPaneWidget::popup_rows(&state), 0);
    }

    // ------------------------------------------------------------------
    // SPEC-001/004 — activity indicator
    // ------------------------------------------------------------------

    fn streaming_state() -> AppState {
        let mut state = AppState::new("/tmp".into());
        state.begin_user_turn("explain this".into());
        state.on_router_decision(
            "composer-2.5".into(),
            "rule".into(),
            "run-1".into(),
            cusa_rpc::RouterSource::Rule,
        );
        state
    }

    #[test]
    fn spec_001_streaming_shows_activity_indicator() {
        let state = streaming_state();
        assert_eq!(state.phase, RunPhase::Streaming);
        let out = render_to_string(&state, 100, 10);
        assert!(out.contains("Working"), "streaming must show Working: {out}");
        assert!(
            out.contains("Ctrl-C to interrupt"),
            "interrupt hint missing: {out}"
        );
    }

    #[test]
    fn spec_001_idle_has_no_activity_indicator() {
        let state = AppState::new("/tmp".into());
        let out = render_to_string(&state, 100, 10);
        assert!(!out.contains("Working"), "idle must not show Working: {out}");
        assert!(!out.contains("Ctrl-C to interrupt"), "{out}");
        assert_eq!(BottomPaneWidget::activity_rows(&state), 0);
    }

    #[test]
    fn spec_001_phase_labels_cover_all_active_states() {
        let mut state = streaming_state();
        state.phase = RunPhase::Routing;
        assert!(render_to_string(&state, 100, 10).contains("Routing"));
        state.phase = RunPhase::AwaitingApproval;
        assert!(render_to_string(&state, 100, 10).contains("Waiting for approval"));
        state.phase = RunPhase::Cancelling;
        assert!(render_to_string(&state, 100, 10).contains("Cancelling"));
    }

    #[test]
    fn spec_001_activity_row_grows_desired_height() {
        let idle = AppState::new("/tmp".into());
        let streaming = streaming_state();
        assert_eq!(
            BottomPaneWidget::desired_height(&streaming, 80),
            BottomPaneWidget::desired_height(&idle, 80) + 1
        );
    }
}
