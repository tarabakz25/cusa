// Vendored from openai/codex codex-rs/tui — see UPSTREAM

// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Cusa-specific history cells mapped from `HistoryCellView` (SPEC-107).

use ratatui::prelude::*;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

use super::super::render::line_utils::prefix_lines;
use super::super::wrapping::RtOptions;
use super::super::wrapping::adaptive_wrap_lines;
use super::HistoryCell;
use super::PlainHistoryCell;

/// Router-decision provenance tag colors (SPEC-012 parity with legacy transcript).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterSourceStyle {
    Rule,
    Llm,
    Local,
    Override,
    Fallback,
}

impl RouterSourceStyle {
    pub fn color(self) -> Color {
        match self {
            Self::Override => Color::Yellow,
            Self::Rule => Color::Cyan,
            Self::Llm => Color::Magenta,
            Self::Local => Color::Green,
            Self::Fallback => Color::DarkGray,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::Rule => "rule",
            Self::Llm => "llm",
            Self::Local => "local",
            Self::Fallback => "fallback",
        }
    }
}

pub fn router_decision_cell(
    model: String,
    rationale: String,
    source: RouterSourceStyle,
) -> PlainHistoryCell {
    let color = source.color();
    let tag = source.tag();
    PlainHistoryCell::new(vec![Line::from(vec![
        Span::styled("→ ", Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(model, Style::default().fg(color)),
        Span::raw(" · "),
        Span::styled(
            tag.to_string(),
            Style::default().fg(color).add_modifier(Modifier::DIM),
        ),
        Span::raw(" · "),
        Span::styled(rationale, Style::default().fg(Color::DarkGray)),
    ])])
}

pub fn tool_decision_cell(tool: String, decision: String) -> PlainHistoryCell {
    PlainHistoryCell::new(vec![Line::from(vec![
        Span::styled("· ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{tool}: {decision}"),
            Style::default().fg(Color::DarkGray),
        ),
    ])])
}

pub fn turn_summary_cell(summary: String, model: String) -> PlainHistoryCell {
    let mut spans = vec![Span::styled(
        summary,
        Style::default().fg(Color::DarkGray),
    )];
    if !model.is_empty() {
        spans.push(Span::raw(" · "));
        spans.push(Span::styled(model, Style::default().fg(Color::DarkGray)));
    }
    PlainHistoryCell::new(vec![Line::from(spans), Line::from("")])
}

pub fn tool_call_cell(name: String, args_preview: String) -> PlainHistoryCell {
    PlainHistoryCell::new(vec![Line::from(vec![
        Span::styled("⚙ ", Style::default().fg(Color::Yellow)),
        Span::styled(name, Style::default().fg(Color::Yellow)),
        Span::raw(" "),
        Span::styled(args_preview, Style::default().fg(Color::DarkGray)),
    ])])
}

pub fn tool_result_cell(name: String, ok: bool, preview: String) -> PlainHistoryCell {
    let color = if ok { Color::Green } else { Color::Red };
    let symbol = if ok { "✓" } else { "✗" };
    PlainHistoryCell::new(vec![Line::from(vec![
        Span::styled(format!("{symbol} "), Style::default().fg(color)),
        Span::styled(name, Style::default().fg(color)),
        Span::raw(" "),
        Span::raw(preview),
    ])])
}

pub fn error_cell(message: String) -> PlainHistoryCell {
    PlainHistoryCell::new(vec![Line::from(vec![
        Span::styled("✗ ", Style::default().fg(Color::Red)),
        Span::styled(message, Style::default().fg(Color::Red)),
    ])])
}

pub fn note_cell(message: String) -> PlainHistoryCell {
    PlainHistoryCell::new(vec![Line::from(vec![
        Span::styled("· ", Style::default().fg(Color::DarkGray)),
        Span::styled(message, Style::default().fg(Color::DarkGray)),
    ])])
}

/// Wrap plain assistant fallback text when markdown rendering is unavailable.
#[derive(Debug)]
pub struct PlainAssistantCell {
    text: String,
}

impl PlainAssistantCell {
    pub fn new(text: String) -> Self {
        Self { text }
    }
}

impl HistoryCell for PlainAssistantCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.text.is_empty() {
            return Vec::new();
        }
        let wrap_width = width.saturating_sub(2).max(1) as usize;
        let mut lines: Vec<Line<'static>> = self
            .text
            .lines()
            .flat_map(|line| {
                adaptive_wrap_lines(
                    [Line::from(line.to_string())],
                    RtOptions::new(wrap_width),
                )
            })
            .collect();
        lines.push(Line::from(""));
        prefix_lines(lines, "• ".dim(), "  ".into())
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        super::raw_lines_from_source(&self.text)
    }
}
