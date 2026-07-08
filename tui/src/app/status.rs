// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Header row + status line (SPEC-060, SPEC-109).
//
// Row 0 and row 1 render through vendored Codex status chrome with `cusa`
// branding (magenta), not `Codex`.

use crate::app::state::AppState;
use crate::codex_adapter::status_chrome;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

/// Header widget for row 0.
#[derive(Debug)]
pub struct HeaderWidget<'a> {
    state: &'a AppState,
}

impl<'a> HeaderWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub fn line(&self) -> Line<'static> {
        status_chrome::header_line(self.state)
    }
}

impl<'a> Widget for HeaderWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line()).render(area, buf);
    }
}

/// Status line widget for row 1.
#[derive(Debug)]
pub struct StatusWidget<'a> {
    state: &'a AppState,
}

impl<'a> StatusWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub fn line(&self) -> Line<'static> {
        status_chrome::status_line(self.state)
    }
}

impl<'a> Widget for StatusWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line()).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use cusa_rpc::ModelSelection;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_row(w: impl Widget, width: u16) -> String {
        let backend = TestBackend::new(width, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(w, f.area());
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn spec_070_header_contains_cusa_and_cwd() {
        let s = AppState::new("/repo/here".into());
        let out = render_row(HeaderWidget::new(&s), 60);
        assert!(out.contains("cusa"));
        assert!(out.contains("/repo/here"));
        assert!(!out.contains("Codex"));
    }

    #[test]
    fn spec_060_status_line_contains_tokens_and_model() {
        let s = AppState::new("/x".into());
        let out = render_row(StatusWidget::new(&s), 80);
        assert!(out.contains("auto"));
        assert!(out.contains("suggest"));
        assert!(out.contains("tokens in 0"), "got {out}");
        assert!(out.contains("skills(0)"));
        assert!(out.contains("mcp(0)"));
    }

    #[test]
    fn spec_016_status_line_shows_override_marker() {
        let mut s = AppState::new("/x".into());
        s.session.model = "claude-sonnet-4".into();
        s.session.manual_model_override = Some(ModelSelection::id_only("claude-sonnet-4"));
        let out = render_row(StatusWidget::new(&s), 100);
        assert!(out.contains("claude-sonnet-4"));
        assert!(out.contains("[override]"), "override marker missing: {out}");
    }

    #[test]
    fn spec_109_header_branding_is_magenta_cusa() {
        let s = AppState::new("/tmp".into());
        let line = HeaderWidget::new(&s).line();
        let brand = line
            .spans
            .first()
            .expect("header has spans");
        assert_eq!(brand.content, "cusa");
    }
}
