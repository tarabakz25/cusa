// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Header row + status line (SPEC-060 for the tokens portion).
//
// Row 0 renders `cusa · <session-id-short> · <cwd-truncated>`.
// Row 1 renders `<model> · <approval-mode> · skills(N) · mcp(N) · <tokens>`.

use crate::app::state::AppState;
use cusa_rpc::ApprovalMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
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
        let short = self.state.session.short_id();
        let cwd = truncate_cwd(&self.state.session.cwd, 48);
        Line::from(vec![
            Span::styled(
                "cusa",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" · "),
            Span::styled(short, Style::default().fg(Color::Cyan)),
            Span::raw(" · "),
            Span::styled(cwd, Style::default().fg(Color::DarkGray)),
        ])
    }
}

impl<'a> Widget for HeaderWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let line = self.line();
        Paragraph::new(line).render(area, buf);
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
        let s = self.state;
        let mode_label = approval_label(s.session.approval_mode);
        let sidecar = s.session.sidecar_status.label();
        let tokens = s.usage.snapshot().status_line();
        let mut spans = vec![Span::styled(
            s.session.model.clone(),
            Style::default().fg(Color::Cyan),
        )];
        if s.session.manual_model_override.is_some() {
            spans.push(Span::styled(
                " [override]".to_string(),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
        spans.extend([
            Span::raw(" · "),
            Span::styled(mode_label.to_string(), Style::default().fg(Color::Yellow)),
            Span::raw(" · "),
            Span::styled(
                format!("skills({})", s.session.skills_count),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(" · "),
            Span::styled(
                format!("mcp({})", s.session.mcp_count),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(" · "),
            Span::styled(tokens, Style::default().fg(Color::Green)),
            Span::raw(" · "),
            Span::styled(
                format!("sidecar:{sidecar}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        Line::from(spans)
    }
}

impl<'a> Widget for StatusWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line()).render(area, buf);
    }
}

fn approval_label(mode: ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::Suggest => "suggest",
        ApprovalMode::AutoEdit => "auto-edit",
        ApprovalMode::FullAuto => "full-auto",
    }
}

fn truncate_cwd(cwd: &str, max: usize) -> String {
    if cwd.chars().count() <= max {
        return cwd.to_string();
    }
    let tail: String = cwd.chars().rev().take(max - 1).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
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
        s.session.manual_model_override = Some("claude-sonnet-4".into());
        let out = render_row(StatusWidget::new(&s), 100);
        assert!(out.contains("claude-sonnet-4"));
        assert!(out.contains("[override]"), "override marker missing: {out}");
    }

    #[test]
    fn spec_060_truncate_cwd_prepends_ellipsis() {
        let long = "/very/very/very/very/very/very/very/deep/path/segment/finalfilehere";
        let t = truncate_cwd(long, 20);
        assert!(t.starts_with('…'), "got {t}");
        assert_eq!(t.chars().count(), 20);
    }
}
