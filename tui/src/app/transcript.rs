// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Transcript pane (SPEC-001).
//
// The transcript is a scrollable list of `TranscriptEntry` values. It
// renders:
//   * user prompts,
//   * router-decision lines (`→ <model> · <rationale>`),
//   * assistant text (which streams live from the in-flight `TurnState`),
//   * tool-call blocks,
//   * per-turn usage summaries.

use cusa_rpc::RouterSource;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// One item in the transcript.
#[derive(Debug, Clone)]
pub enum TranscriptEntry {
    /// User prompt.
    User(String),
    /// Router decision (`→ <model> · <rationale>`). SPEC-012: color-coded
    /// by `source` so overrides and LLM decisions are visually distinct.
    RouterDecision {
        model: String,
        rationale: String,
        source: RouterSource,
    },
    /// Observational tool decision entry (approve / deny / always). Rendered
    /// as a dim, single-line note in the transcript.
    ToolDecision { tool: String, decision: String },
    /// Assistant text (may span multiple lines).
    Assistant { text: String, model: String },
    /// Per-turn usage summary line.
    TurnSummary { summary: String, model: String },
    /// Tool call block.
    ToolCall {
        name: String,
        args_preview: String,
    },
    /// Tool result block.
    ToolResult {
        name: String,
        ok: bool,
        preview: String,
    },
    /// Error line.
    Error(String),
    /// Informational note (e.g. "reset session", "reconnected").
    Note(String),
}

/// In-flight turn state. Held separately from the transcript so the streaming
/// text can be rendered mid-flight before the turn commits into the log.
#[derive(Debug, Clone, Default)]
pub struct TurnState {
    pub prompt: String,
    pub assistant_text: String,
    pub model: Option<String>,
    /// Sidecar-assigned run id captured from `router/decision`. Used by
    /// `session/cancel` to target the correct run (SPEC-004).
    pub run_id: Option<String>,
}

impl TurnState {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            assistant_text: String::new(),
            model: None,
            run_id: None,
        }
    }
}

/// The transcript widget. Borrows the app's transcript slice and the
/// optional in-flight turn.
#[derive(Debug)]
pub struct TranscriptWidget<'a> {
    entries: &'a [TranscriptEntry],
    live_turn: Option<&'a TurnState>,
    /// Number of scrollback rows to skip when rendering.
    scroll: u16,
}

impl<'a> TranscriptWidget<'a> {
    pub fn new(entries: &'a [TranscriptEntry], live_turn: Option<&'a TurnState>) -> Self {
        Self {
            entries,
            live_turn,
            scroll: 0,
        }
    }

    pub fn with_scroll(mut self, scroll: u16) -> Self {
        self.scroll = scroll;
        self
    }

    /// Materialize the widget's lines. Public for snapshot tests.
    pub fn lines(&self) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::with_capacity(self.entries.len() * 2 + 4);
        for entry in self.entries {
            match entry {
                TranscriptEntry::User(text) => {
                    out.push(Line::from(vec![
                        Span::styled("› ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                        Span::styled(text.clone(), Style::default().add_modifier(Modifier::BOLD)),
                    ]));
                }
                TranscriptEntry::RouterDecision {
                    model,
                    rationale,
                    source,
                } => {
                    let (color, tag) = router_source_style(*source);
                    out.push(Line::from(vec![
                        Span::styled("→ ", Style::default().fg(color).add_modifier(Modifier::BOLD)),
                        Span::styled(model.clone(), Style::default().fg(color)),
                        Span::raw(" · "),
                        Span::styled(tag.to_string(), Style::default().fg(color).add_modifier(Modifier::DIM)),
                        Span::raw(" · "),
                        Span::styled(rationale.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
                TranscriptEntry::ToolDecision { tool, decision } => {
                    out.push(Line::from(vec![
                        Span::styled("· ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{tool}: {decision}"),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }
                TranscriptEntry::Assistant { text, .. } => {
                    for line in text.lines() {
                        out.push(Line::from(line.to_string()));
                    }
                    // Preserve an intentional trailing blank line so streamed
                    // messages are visually separated from the next entry.
                    out.push(Line::from(""));
                }
                TranscriptEntry::TurnSummary { summary, model } => {
                    let mut spans = vec![Span::styled(
                        summary.clone(),
                        Style::default().fg(Color::DarkGray),
                    )];
                    if !model.is_empty() {
                        spans.push(Span::raw(" · "));
                        spans.push(Span::styled(
                            model.clone(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    out.push(Line::from(spans));
                    out.push(Line::from(""));
                }
                TranscriptEntry::ToolCall { name, args_preview } => {
                    out.push(Line::from(vec![
                        Span::styled("⚙ ", Style::default().fg(Color::Yellow)),
                        Span::styled(name.clone(), Style::default().fg(Color::Yellow)),
                        Span::raw(" "),
                        Span::styled(args_preview.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
                TranscriptEntry::ToolResult { name, ok, preview } => {
                    let color = if *ok { Color::Green } else { Color::Red };
                    let symbol = if *ok { "✓" } else { "✗" };
                    out.push(Line::from(vec![
                        Span::styled(format!("{symbol} "), Style::default().fg(color)),
                        Span::styled(name.clone(), Style::default().fg(color)),
                        Span::raw(" "),
                        Span::raw(preview.clone()),
                    ]));
                }
                TranscriptEntry::Error(msg) => {
                    out.push(Line::from(vec![
                        Span::styled("✗ ", Style::default().fg(Color::Red)),
                        Span::styled(msg.clone(), Style::default().fg(Color::Red)),
                    ]));
                }
                TranscriptEntry::Note(msg) => {
                    out.push(Line::from(vec![
                        Span::styled("· ", Style::default().fg(Color::DarkGray)),
                        Span::styled(msg.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        }
        // In-flight assistant text (streamed).
        if let Some(turn) = self.live_turn {
            if !turn.assistant_text.is_empty() {
                for line in turn.assistant_text.lines() {
                    out.push(Line::from(line.to_string()));
                }
                // No trailing blank — the run is still in progress.
            }
        }
        out
    }
}

/// SPEC-012: map a router-decision `source` to a (color, short tag) pair.
/// yellow = override, cyan = rule, magenta = llm, dim gray = fallback.
pub fn router_source_style(source: RouterSource) -> (Color, &'static str) {
    match source {
        RouterSource::Override => (Color::Yellow, "override"),
        RouterSource::Rule => (Color::Cyan, "rule"),
        RouterSource::Llm => (Color::Magenta, "llm"),
        RouterSource::Fallback => (Color::DarkGray, "fallback"),
    }
}

impl<'a> Widget for TranscriptWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = self.lines();
        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        block.render(area, buf);
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        paragraph.render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(w: TranscriptWidget<'_>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                f.render_widget(w, area);
            })
            .unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol().to_string()).collect::<Vec<_>>().join("")
    }

    #[test]
    fn spec_001_renders_stream_message_delta() {
        let entries: Vec<TranscriptEntry> = vec![TranscriptEntry::User("hi".into())];
        let mut turn = TurnState::new("hi".into());
        turn.assistant_text = "streaming reply".into();
        let widget = TranscriptWidget::new(&entries, Some(&turn));
        let out = render(widget, 40, 6);
        assert!(out.contains("streaming reply"), "got: {out}");
    }

    #[test]
    fn spec_012_renders_router_decision_before_assistant() {
        let entries: Vec<TranscriptEntry> = vec![
            TranscriptEntry::User("explain".into()),
            TranscriptEntry::RouterDecision {
                model: "composer-2.5".into(),
                rationale: "fast rule".into(),
                source: RouterSource::Rule,
            },
            TranscriptEntry::Assistant {
                text: "the answer".into(),
                model: "composer-2.5".into(),
            },
        ];
        let widget = TranscriptWidget::new(&entries, None);
        let out = render(widget, 60, 8);
        let router_idx = out.find("composer-2.5").expect("router model appears");
        let assistant_idx = out.find("the answer").expect("assistant text appears");
        assert!(router_idx < assistant_idx, "router line must come first");
    }

    #[test]
    fn spec_012_router_decision_line_uses_source_color_for_rule() {
        // Rule → cyan tag; Override → yellow tag. This snapshot asserts
        // the source-derived short tag renders inside the transcript.
        let entries: Vec<TranscriptEntry> = vec![
            TranscriptEntry::RouterDecision {
                model: "claude-sonnet-4".into(),
                rationale: "explicit override".into(),
                source: RouterSource::Override,
            },
            TranscriptEntry::RouterDecision {
                model: "composer-2.5".into(),
                rationale: "keyword match".into(),
                source: RouterSource::Rule,
            },
        ];
        let widget = TranscriptWidget::new(&entries, None);
        let out = render(widget, 80, 6);
        assert!(out.contains("override"), "override tag missing: {out}");
        assert!(out.contains("rule"), "rule tag missing: {out}");
    }

    #[test]
    fn spec_001_user_prompt_prefix_is_chevron() {
        let entries: Vec<TranscriptEntry> = vec![TranscriptEntry::User("hello".into())];
        let widget = TranscriptWidget::new(&entries, None);
        let out = render(widget, 40, 4);
        assert!(out.contains("› "), "prompt chevron missing: {out}");
        assert!(out.contains("hello"));
    }
}
