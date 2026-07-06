// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Codex-style session welcome card + startup tips (idle transcript chrome).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::state::SessionView;
use crate::codex_adapter::CusaViewModel;
use crate::codex_ui::history_cell::{HistoryCell, PlainHistoryCell};
use crate::codex_ui::text_formatting::center_truncate_path;
use ratatui::style::{Modifier, Stylize};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

const SESSION_HEADER_MAX_INNER_WIDTH: usize = 56;
const CUSA_VERSION: &str = env!("CARGO_PKG_VERSION");

fn card_inner_width(width: u16) -> Option<usize> {
    if width < 4 {
        return None;
    }
    Some(std::cmp::min(width.saturating_sub(4) as usize, SESSION_HEADER_MAX_INNER_WIDTH))
}

/// Rounded-corner box matching upstream `SessionHeaderHistoryCell` / `with_border`.
fn with_border(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let max_line_width = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let content_width = max_line_width;

    let mut out = Vec::with_capacity(lines.len() + 2);
    let border_inner_width = content_width + 2;
    out.push(Line::from(format!("╭{}╮", "─".repeat(border_inner_width)).dim()));

    for line in lines {
        let used_width: usize = line
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 4);
        spans.push(Span::from("│ ").dim());
        spans.extend(line.spans);
        if used_width < content_width {
            spans.push(Span::from(" ".repeat(content_width - used_width)).dim());
        }
        spans.push(Span::from(" │").dim());
        out.push(Line::from(spans));
    }

    out.push(Line::from(format!("╰{}╯", "─".repeat(border_inner_width)).dim()));
    out
}

fn format_directory(directory: &Path, max_width: Option<usize>) -> String {
    let formatted = if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = directory.strip_prefix(&home) {
            if rel.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display())
            }
        } else {
            directory.display().to_string()
        }
    } else {
        directory.display().to_string()
    };

    if let Some(max_width) = max_width.filter(|w| *w > 0) {
        if UnicodeWidthStr::width(formatted.as_str()) > max_width {
            return center_truncate_path(&formatted, max_width);
        }
    }
    formatted
}

/// `>_ cusa (vX)` bordered session header — Codex `SessionHeaderHistoryCell` parity.
#[derive(Debug)]
pub struct CusaSessionHeaderCell {
    model: String,
    directory: PathBuf,
}

impl CusaSessionHeaderCell {
    pub fn from_session(session: &SessionView) -> Self {
        Self {
            model: session.model.clone(),
            directory: PathBuf::from(&session.cwd),
        }
    }
}

impl HistoryCell for CusaSessionHeaderCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(inner_width) = card_inner_width(width) else {
            return Vec::new();
        };

        const CHANGE_MODEL_HINT_COMMAND: &str = "/model";
        const CHANGE_MODEL_HINT_EXPLANATION: &str = " to change";
        const DIR_LABEL: &str = "directory:";
        let label_width = "model:".len().max(DIR_LABEL.len());

        let title_spans = vec![
            Span::from(">_ ").dim(),
            Span::from("cusa").bold(),
            Span::from(" ").dim(),
            Span::from(format!("(v{CUSA_VERSION})")).dim(),
        ];

        let model_label = format!("{:<label_width$}", "model:", label_width = label_width);
        let model_spans = vec![
            Span::from(format!("{model_label} ")).dim(),
            Span::styled(self.model.clone(), ratatui::style::Style::default()),
            Span::from("   ").dim(),
            Span::styled(CHANGE_MODEL_HINT_COMMAND.to_string(), ratatui::style::Color::Cyan),
            Span::from(CHANGE_MODEL_HINT_EXPLANATION).dim(),
        ];

        let dir_label = format!("{DIR_LABEL:<label_width$}");
        let dir_prefix = format!("{dir_label} ");
        let dir_prefix_width = UnicodeWidthStr::width(dir_prefix.as_str());
        let dir_max_width = inner_width.saturating_sub(dir_prefix_width);
        let dir = format_directory(&self.directory, Some(dir_max_width));
        let dir_spans = vec![Span::from(dir_prefix).dim(), Span::from(dir)];

        with_border(vec![
            Line::from(title_spans),
            Line::from(Vec::<Span>::new()),
            Line::from(model_spans),
            Line::from(dir_spans),
        ])
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        vec![
            Line::from(format!("cusa (v{CUSA_VERSION})")),
            Line::from(format!("model: {}", self.model)),
            Line::from(format!(
                "directory: {}",
                format_directory(&self.directory, None)
            )),
        ]
    }
}

fn welcome_tip_cell() -> PlainHistoryCell {
    PlainHistoryCell::new(vec![
        Line::from(vec![
            Span::styled("Tip:", Modifier::BOLD),
            Span::raw(" Describe a task or paste code — Shift+Enter inserts a newline."),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("· ", ratatui::style::Color::DarkGray),
            Span::styled(
                "Try /help for commands, /model to switch models, Tab to cycle approval mode.",
                ratatui::style::Color::DarkGray,
            ),
        ]),
    ])
}

/// Welcome history cells shown when the transcript is empty (Codex idle screen).
pub fn welcome_cells(session: &SessionView) -> Vec<Arc<dyn HistoryCell>> {
    vec![
        Arc::new(CusaSessionHeaderCell::from_session(session)),
        Arc::new(welcome_tip_cell()),
    ]
}

/// Composer footer: `model · mode · ~/cwd` (Codex bottom-pane status row).
pub fn composer_footer_line(session: &SessionView) -> Line<'static> {
    let mode = CusaViewModel::map_approval_mode(session.approval_mode);
    let dir = format_directory(Path::new(&session.cwd), Some(40));
    Line::from(vec![
        Span::styled(session.model.clone(), ratatui::style::Color::Yellow),
        Span::raw(" · "),
        Span::styled(mode.label().to_string(), ratatui::style::Color::DarkGray),
        Span::raw(" · "),
        Span::styled(dir, ratatui::style::Color::Green),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn spec_codex_welcome_header_uses_cusa_branding() {
        let state = AppState::new("/tmp/project".into());
        let cell = CusaSessionHeaderCell::from_session(&state.session);
        let text: String = cell
            .display_lines(80)
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("cusa"), "{text}");
        assert!(text.contains(">_"), "{text}");
        assert!(text.contains("model:"), "{text}");
        assert!(text.contains("directory:"), "{text}");
        assert!(!text.contains("Codex"), "{text}");
    }

    #[test]
    fn spec_codex_composer_footer_shows_model_and_directory() {
        let state = AppState::new("/tmp/repo".into());
        let text = line_text(&composer_footer_line(&state.session));
        assert!(text.contains("auto"), "{text}");
        assert!(text.contains("suggest"), "{text}");
    }
}
