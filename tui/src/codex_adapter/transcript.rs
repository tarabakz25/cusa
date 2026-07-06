// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Transcript rendering via vendored Codex history_cell pipeline (SPEC-107).
// Maps `HistoryCellView` from `CusaViewModel` into vendored `HistoryCell`
// implementations and renders them into the transcript pane.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::transcript::{TranscriptEntry, TurnState};
use crate::codex_adapter::tool_display;
use crate::codex_adapter::types::{HistoryCellView, RouterSourceView};
use crate::codex_adapter::view_model::CusaViewModel;
use crate::codex_ui::history_cell::{
    self, tool_call_cell, tool_decision_cell, tool_result_cell, turn_summary_cell,
    AgentMarkdownCell, HistoryCell, PlainAssistantCell, RouterSourceStyle, StreamingAgentTailCell,
    UserHistoryCell, error_cell, note_cell, router_decision_cell,
};
use crate::codex_ui::markdown;
use crate::codex_ui::terminal_hyperlinks::HyperlinkLine;
use crate::codex_ui::thread_transcript::TranscriptCells;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

/// Renders tool call/result entries through vendored diff/exec widgets (SPEC-108).
#[derive(Debug)]
struct ToolDisplayCell {
    entry: TranscriptEntry,
    cwd: PathBuf,
}

impl ToolDisplayCell {
    fn new(entry: TranscriptEntry, cwd: &Path) -> Self {
        Self {
            entry,
            cwd: cwd.to_path_buf(),
        }
    }
}

impl HistoryCell for ToolDisplayCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let width = width.max(1);
        if let Some(lines) = tool_display::render_tool_entry(&self.entry, &self.cwd, width) {
            return lines;
        }
        match &self.entry {
            TranscriptEntry::ToolCall { name, args_preview } => {
                tool_call_cell(name.clone(), args_preview.clone()).display_lines(width)
            }
            TranscriptEntry::ToolResult { name, ok, preview } => {
                tool_result_cell(name.clone(), *ok, preview.clone()).display_lines(width)
            }
            _ => Vec::new(),
        }
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.display_lines(u16::MAX)
    }
}

fn tool_display_cell(entry: TranscriptEntry, cwd: &Path) -> Arc<dyn HistoryCell> {
    Arc::new(ToolDisplayCell::new(entry, cwd))
}

/// Build vendored transcript cells from adapter views.
pub fn views_to_transcript_cells(views: &[HistoryCellView], cwd: &Path) -> TranscriptCells {
    views
        .iter()
        .filter_map(|view| view_to_cell(view, cwd))
        .collect()
}

fn map_router_source(source: RouterSourceView) -> RouterSourceStyle {
    match source {
        RouterSourceView::Rule => RouterSourceStyle::Rule,
        RouterSourceView::Llm => RouterSourceStyle::Llm,
        RouterSourceView::Override => RouterSourceStyle::Override,
        RouterSourceView::Fallback => RouterSourceStyle::Fallback,
    }
}

fn view_to_cell(view: &HistoryCellView, cwd: &Path) -> Option<Arc<dyn HistoryCell>> {
    let cell: Arc<dyn HistoryCell> = match view {
        HistoryCellView::UserPrompt { text } => Arc::new(UserHistoryCell {
            message: text.clone(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }),
        HistoryCellView::RouterDecision {
            model,
            rationale,
            source,
        } => Arc::new(router_decision_cell(
            model.clone(),
            rationale.clone(),
            map_router_source(*source),
        )),
        HistoryCellView::ToolDecision { tool, decision } => {
            Arc::new(tool_decision_cell(tool.clone(), decision.clone()))
        }
        HistoryCellView::Assistant { text, .. } => {
            if text.trim().is_empty() {
                return None;
            }
            Arc::new(AgentMarkdownCell::new(text.clone(), cwd))
        }
        HistoryCellView::TurnSummary { summary, model } => Arc::new(turn_summary_cell(
            summary.clone(),
            model.clone(),
        )),
        HistoryCellView::ToolCall { name, args_preview } => tool_display_cell(
            TranscriptEntry::ToolCall {
                name: name.clone(),
                args_preview: args_preview.clone(),
            },
            cwd,
        ),
        HistoryCellView::ToolResult { name, ok, preview } => tool_display_cell(
            TranscriptEntry::ToolResult {
                name: name.clone(),
                ok: *ok,
                preview: preview.clone(),
            },
            cwd,
        ),
        HistoryCellView::Error { message } => Arc::new(error_cell(message.clone())),
        HistoryCellView::Note { message } => Arc::new(note_cell(message.clone())),
        HistoryCellView::LiveAssistant { text } => {
            if text.is_empty() {
                return None;
            }
            let wrap_width = 78usize;
            let lines = markdown::render_markdown_agent_with_links_and_cwd(
                text,
                Some(wrap_width),
                Some(cwd),
            );
            if lines.is_empty() {
                return Some(Arc::new(PlainAssistantCell::new(text.clone())));
            }
            Arc::new(StreamingAgentTailCell::new(lines, true))
        }
    };
    Some(cell)
}

/// Collect display lines for all transcript cells at the given width.
pub fn render_transcript_lines(cells: &[Arc<dyn HistoryCell>], width: u16) -> Vec<Line<'static>> {
    let inner_width = width.saturating_sub(2).max(1);
    let mut out: Vec<Line<'static>> = Vec::new();
    for cell in cells {
        let mut lines = cell.display_lines(inner_width);
        if lines.is_empty() {
            continue;
        }
        if !out.is_empty() && !cell.is_stream_continuation() {
            // Preserve visual separation between committed entries.
            if out.last().is_some_and(|l| !l.spans.is_empty()) {
                out.push(Line::from(""));
            }
        }
        out.append(&mut lines);
    }
    out
}

/// Codex-style transcript pane replacing legacy `TranscriptWidget` (SPEC-107).
#[derive(Debug)]
pub struct CodexTranscriptWidget<'a> {
    entries: &'a [TranscriptEntry],
    live_turn: Option<&'a TurnState>,
    cwd: &'a Path,
    scroll: u16,
}

impl<'a> CodexTranscriptWidget<'a> {
    pub fn new(
        entries: &'a [TranscriptEntry],
        live_turn: Option<&'a TurnState>,
        cwd: &'a Path,
    ) -> Self {
        Self {
            entries,
            live_turn,
            cwd,
            scroll: 0,
        }
    }

    pub fn with_scroll(mut self, scroll: u16) -> Self {
        self.scroll = scroll;
        self
    }

    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let views = CusaViewModel::history_cells(self.entries, self.live_turn);
        let cells = views_to_transcript_cells(&views, self.cwd);
        render_transcript_lines(&cells, width)
    }
}

impl<'a> Widget for CodexTranscriptWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = self.lines(area.width);
        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::transcript::TranscriptEntry;
    use crate::codex_adapter::types::HistoryCellView;
    use cusa_rpc::RouterSource;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_widget(w: CodexTranscriptWidget<'_>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| w.render(f.area(), f.buffer_mut()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    fn all_variants_fixture() -> Vec<TranscriptEntry> {
        vec![
            TranscriptEntry::User("explain this".into()),
            TranscriptEntry::RouterDecision {
                model: "composer-2.5".into(),
                rationale: "fast rule".into(),
                source: RouterSource::Rule,
            },
            TranscriptEntry::Assistant {
                text: "Here is **markdown** output.".into(),
                model: "composer-2.5".into(),
            },
            TranscriptEntry::ToolCall {
                name: "read_file".into(),
                args_preview: "{\"path\":\"main.rs\"}".into(),
            },
            TranscriptEntry::ToolResult {
                name: "read_file".into(),
                ok: true,
                preview: "fn main() {}".into(),
            },
            TranscriptEntry::ToolDecision {
                tool: "shell_exec".into(),
                decision: "auto-approve".into(),
            },
            TranscriptEntry::TurnSummary {
                summary: "turn Δ in+10 out+20".into(),
                model: "composer-2.5".into(),
            },
            TranscriptEntry::Error("something failed".into()),
            TranscriptEntry::Note("session note".into()),
        ]
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
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 60, 8);
        let router_idx = out.find("composer-2.5").expect("router model appears");
        let assistant_idx = out.find("the answer").expect("assistant text appears");
        assert!(router_idx < assistant_idx, "router line must come first");
    }

    #[test]
    fn spec_012_router_decision_line_uses_source_color_for_rule() {
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
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 80, 6);
        assert!(out.contains("override"), "override tag missing: {out}");
        assert!(out.contains("rule"), "rule tag missing: {out}");
    }

    #[test]
    fn spec_001_renders_stream_message_delta() {
        let entries: Vec<TranscriptEntry> = vec![TranscriptEntry::User("hi".into())];
        let mut turn = TurnState::new("hi".into());
        turn.assistant_text = "streaming reply".into();
        let w = CodexTranscriptWidget::new(&entries, Some(&turn), Path::new("/tmp"));
        let out = render_widget(w, 40, 6);
        assert!(out.contains("streaming reply"), "got: {out}");
    }

    #[test]
    fn spec_113_legacy_transcript_widget_removed() {
        // SPEC-113: rendering path is CodexTranscriptWidget only.
        let entries = vec![TranscriptEntry::User("ok".into())];
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 40, 4);
        assert!(out.contains("ok"));
    }

    #[test]
    fn spec_107_user_prompt_renders_chevron_prefix() {
        let entries = vec![TranscriptEntry::User("hello".into())];
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 60, 6);
        assert!(out.contains('›'), "chevron missing: {out}");
        assert!(out.contains("hello"));
    }

    #[test]
    fn spec_107_router_decision_shows_source_tag() {
        let entries = vec![TranscriptEntry::RouterDecision {
            model: "m".into(),
            rationale: "why".into(),
            source: RouterSource::Override,
        }];
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 70, 4);
        assert!(out.contains("override"), "tag missing: {out}");
        assert!(out.contains('→'), "arrow missing: {out}");
    }

    #[test]
    fn spec_107_assistant_markdown_renders_body() {
        let entries = vec![TranscriptEntry::Assistant {
            text: "plain assistant text".into(),
            model: "m".into(),
        }];
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 60, 8);
        assert!(out.contains("plain assistant text"), "body missing: {out}");
    }

    #[test]
    fn spec_107_tool_call_and_result_render() {
        let entries = vec![
            TranscriptEntry::ToolCall {
                name: "grep".into(),
                args_preview: "{\"pattern\":\"foo\"}".into(),
            },
            TranscriptEntry::ToolResult {
                name: "grep".into(),
                ok: false,
                preview: "not found".into(),
            },
        ];
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 70, 6);
        assert!(out.contains("grep"), "tool name missing: {out}");
        assert!(out.contains("not found"), "preview missing: {out}");
    }

    #[test]
    fn spec_107_tool_decision_turn_summary_error_note_render() {
        let entries = vec![
            TranscriptEntry::ToolDecision {
                tool: "shell".into(),
                decision: "deny".into(),
            },
            TranscriptEntry::TurnSummary {
                summary: "turn Δ".into(),
                model: "m".into(),
            },
            TranscriptEntry::Error("boom".into()),
            TranscriptEntry::Note("reconnected".into()),
        ];
        let w = CodexTranscriptWidget::new(&entries, None, Path::new("/tmp"));
        let out = render_widget(w, 70, 12);
        assert!(out.contains("shell: deny"), "{out}");
        assert!(out.contains("turn Δ"), "{out}");
        assert!(out.contains("boom"), "{out}");
        assert!(out.contains("reconnected"), "{out}");
    }

    #[test]
    fn spec_107_live_assistant_streaming_text() {
        let entries = vec![TranscriptEntry::User("hi".into())];
        let mut turn = TurnState::new("hi".into());
        turn.assistant_text = "streaming partial".into();
        let w = CodexTranscriptWidget::new(&entries, Some(&turn), Path::new("/tmp"));
        let out = render_widget(w, 60, 8);
        assert!(out.contains("streaming partial"), "{out}");
    }

    #[test]
    fn spec_107_all_transcript_entry_variants_map_to_cells() {
        let entries = all_variants_fixture();
        let views = CusaViewModel::history_cells(&entries, None);
        assert_eq!(views.len(), entries.len());
        let cells = views_to_transcript_cells(&views, Path::new("/tmp/repo"));
        assert_eq!(cells.len(), entries.len());
        for cell in &cells {
            assert!(
                !cell.display_lines(60).is_empty(),
                "cell produced no lines: {cell:?}"
            );
        }
    }

    #[test]
    fn spec_107_views_to_cells_covers_every_history_cell_view_variant() {
        let views = vec![
            HistoryCellView::UserPrompt { text: "u".into() },
            HistoryCellView::RouterDecision {
                model: "m".into(),
                rationale: "r".into(),
                source: RouterSourceView::Rule,
            },
            HistoryCellView::ToolDecision {
                tool: "t".into(),
                decision: "d".into(),
            },
            HistoryCellView::Assistant {
                text: "a".into(),
                model: "m".into(),
            },
            HistoryCellView::TurnSummary {
                summary: "s".into(),
                model: "m".into(),
            },
            HistoryCellView::ToolCall {
                name: "n".into(),
                args_preview: "{}".into(),
            },
            HistoryCellView::ToolResult {
                name: "n".into(),
                ok: true,
                preview: "p".into(),
            },
            HistoryCellView::Error {
                message: "e".into(),
            },
            HistoryCellView::Note {
                message: "n".into(),
            },
            HistoryCellView::LiveAssistant {
                text: "live".into(),
            },
        ];
        let cells = views_to_transcript_cells(&views, Path::new("/tmp"));
        assert_eq!(cells.len(), views.len());
    }
}
