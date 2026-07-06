// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Footer row (hint keys).

use crate::app::state::{AppState, RunPhase};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

#[derive(Debug)]
pub struct FooterWidget<'a> {
    state: &'a AppState,
}

impl<'a> FooterWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub fn line(&self) -> Line<'static> {
        if let Some(o) = &self.state.footer_override {
            return Line::from(Span::styled(
                o.clone(),
                Style::default().fg(Color::Cyan),
            ));
        }
        let hint = match self.state.phase {
            RunPhase::Idle => "Enter send · /help commands · Ctrl-C exit",
            RunPhase::Streaming | RunPhase::Routing => "Ctrl-C cancel · streaming…",
            RunPhase::AwaitingApproval => "y approve · n deny · a always",
            RunPhase::Cancelling => "cancelling…",
        };
        Line::from(Span::styled(
            hint.to_string(),
            Style::default().fg(Color::Cyan),
        ))
    }
}

impl<'a> Widget for FooterWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line()).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(state: &AppState) -> String {
        let backend = TestBackend::new(60, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(FooterWidget::new(state), f.area());
            })
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
    fn spec_001_footer_idle_hints() {
        let s = AppState::new("/x".into());
        let out = render(&s);
        assert!(out.contains("Ctrl-C exit"), "got {out}");
    }

    #[test]
    fn spec_004_footer_streaming_shows_cancel_hint() {
        let mut s = AppState::new("/x".into());
        s.begin_user_turn("hi".into());
        s.on_router_decision(
            "m".into(),
            "r".into(),
            "run-1".into(),
            cusa_rpc::RouterSource::Rule,
        );
        let out = render(&s);
        assert!(out.contains("Ctrl-C cancel"), "got {out}");
    }
}
