// Vendored from openai/codex codex-rs/tui — see UPSTREAM

// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Decoupled history-cell trait and message cells for cusa P2 (SPEC-107).
// Upstream `history_cell/mod.rs` is trimmed to remove `codex-*` dependencies.

use std::any::Any;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

use ratatui::prelude::*;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use super::insert_history;
use super::markdown;
use super::render::line_utils::prefix_lines;
use super::style::user_message_style;
use super::terminal_hyperlinks::HyperlinkLine;
use super::terminal_hyperlinks::plain_hyperlink_lines;
use super::terminal_hyperlinks::prefix_hyperlink_lines;
use super::terminal_hyperlinks::visible_lines;
use super::terminal_hyperlinks::adaptive_wrap_hyperlink_lines;
use super::ui_consts::LIVE_PREFIX_COLS;
use super::width;
use super::wrapping::RtOptions;
use super::wrapping::adaptive_wrap_lines;

mod base;
pub mod cusa_cells;
mod messages;

pub(crate) use base::*;
pub(crate) use cusa_cells::{
    error_cell, note_cell, router_decision_cell, tool_call_cell, tool_decision_cell,
    tool_result_cell, turn_summary_cell, PlainAssistantCell, RouterSourceStyle,
};
pub(crate) use messages::*;

/// Styled text element within a user message (subset of upstream protocol type).
#[derive(Debug, Clone)]
pub(crate) struct TextElement {
    pub byte_range: Range<usize>,
}

pub(crate) fn local_image_label_text(index: usize) -> String {
    format!("[image {index}]")
}

pub(crate) fn raw_lines_from_source(source: &str) -> Vec<Line<'static>> {
    if source.is_empty() {
        return Vec::new();
    }
    let mut parts = source.split('\n').collect::<Vec<_>>();
    if source.ends_with('\n') {
        parts.pop();
    }
    parts
        .into_iter()
        .map(|line| Line::from(line.to_string()))
        .collect()
}

pub(crate) fn plain_lines(lines: impl IntoIterator<Item = Line<'static>>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let text = line
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>();
            Line::from(text)
        })
        .collect()
}

/// A single renderable unit of conversation history.
pub trait HistoryCell: std::fmt::Debug + Send + Sync + Any {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;
    fn raw_lines(&self) -> Vec<Line<'static>>;
    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        plain_hyperlink_lines(self.display_lines(width))
    }
    fn is_stream_continuation(&self) -> bool {
        false
    }
}

pub(crate) fn display_height(lines: &[Line<'static>], width: u16) -> u16 {
    if width == 0 || lines.is_empty() {
        return 0;
    }
    Paragraph::new(lines.to_vec())
        .wrap(Wrap { trim: false })
        .line_count(width)
        .try_into()
        .unwrap_or(0)
}
