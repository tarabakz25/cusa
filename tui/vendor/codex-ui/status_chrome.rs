// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//
// Header + status line chrome (SPEC-109). Codex `status_line_style` accents with
// cusa branding (magenta agent identity, not "Codex").

use ratatui::style::{Color, Modifier, Style};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use super::text_formatting::center_truncate_path;

const STATUS_LINE_SEPARATOR: &str = " · ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusAccent {
    Brand,
    Model,
    Mode,
    Count,
    Usage,
    Meta,
}

impl StatusAccent {
    fn style(self) -> Style {
        match self {
            Self::Brand => Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            Self::Model => Style::default().fg(Color::Cyan),
            Self::Mode => Style::default().fg(Color::Yellow),
            Self::Count => Style::default().fg(Color::DarkGray),
            Self::Usage => Style::default().fg(Color::Green),
            Self::Meta => Style::default().fg(Color::DarkGray),
        }
    }
}

fn push_segment(spans: &mut Vec<Span<'static>>, accent: StatusAccent, text: impl Into<String>) {
    if !spans.is_empty() {
        spans.push(STATUS_LINE_SEPARATOR.dim());
    }
    spans.push(Span::styled(text.into(), accent.style()));
}

/// Render row-0 header: `cusa · <session-short> · <cwd>`.
pub fn render_header_line(short_id: &str, cwd: &str, max_cwd_width: usize) -> Line<'static> {
    let cwd_display = if cwd.chars().count() <= max_cwd_width {
        cwd.to_string()
    } else {
        center_truncate_path(cwd, max_cwd_width)
    };
    Line::from(vec![
        Span::styled("cusa", StatusAccent::Brand.style()),
        Span::raw(" · "),
        Span::styled(short_id.to_string(), StatusAccent::Model.style()),
        Span::raw(" · "),
        Span::styled(cwd_display, StatusAccent::Meta.style()),
    ])
}

/// One segment for the configurable status line.
pub struct StatusSegment {
    pub accent: StatusSegmentKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusSegmentKind {
    Model,
    Mode,
    Count,
    Usage,
    Meta,
}

impl StatusSegmentKind {
    fn accent(self) -> StatusAccent {
        match self {
            Self::Model => StatusAccent::Model,
            Self::Mode => StatusAccent::Mode,
            Self::Count => StatusAccent::Count,
            Self::Usage => StatusAccent::Usage,
            Self::Meta => StatusAccent::Meta,
        }
    }
}

/// Render row-1 status line from pre-built segments (Codex ` · ` rhythm).
pub fn render_status_line(segments: &[StatusSegment]) -> Line<'static> {
    let mut spans = Vec::new();
    for segment in segments {
        push_segment(
            &mut spans,
            segment.accent.accent(),
            segment.text.clone(),
        );
    }
    Line::from(spans)
}

#[cfg(all(test, feature = "vendor-tests"))]
mod tests {
    use super::*;

    #[test]
    fn header_uses_cusa_branding() {
        let line = render_header_line("abcd1234", "/tmp/repo", 48);
        let text: String = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("cusa"));
        assert!(!text.contains("Codex"));
    }
}
