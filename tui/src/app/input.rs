// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Input pane (Slice 2: single-line).
//
// Slice 2 keeps the input pane deliberately minimal — one line of text with
// a visible cursor. Multiline (SPEC-005) and history navigation (SPEC-006)
// arrive in a later slice.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// Input widget. Renders `> <buffer>` bordered on top.
#[derive(Debug)]
pub struct InputWidget<'a> {
    buffer: &'a str,
    cursor_pos: usize,
    active: bool,
}

impl<'a> InputWidget<'a> {
    pub fn new(buffer: &'a str, cursor_pos: usize, active: bool) -> Self {
        Self {
            buffer,
            cursor_pos,
            active,
        }
    }

    /// Row of visible content the input widget renders (used by widget tests).
    pub fn display_line(&self) -> Line<'static> {
        let cursor_style = if self.active {
            Style::default()
                .bg(Color::White)
                .fg(Color::Black)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let mut spans = vec![Span::styled("› ", Style::default().fg(Color::Magenta))];
        let (left, right) = split_at_char(self.buffer, self.cursor_pos);
        spans.push(Span::raw(left.to_string()));
        if right.is_empty() {
            spans.push(Span::styled(" ", cursor_style));
        } else {
            let mut iter = right.chars();
            if let Some(first) = iter.next() {
                let mut s = String::new();
                s.push(first);
                spans.push(Span::styled(s, cursor_style));
                spans.push(Span::raw(iter.collect::<String>()));
            }
        }
        Line::from(spans)
    }
}

fn split_at_char(s: &str, char_idx: usize) -> (&str, &str) {
    for (i, (b, _)) in s.char_indices().enumerate() {
        if i == char_idx {
            return s.split_at(b);
        }
    }
    (s, "")
}

impl<'a> Widget for InputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(self.display_line()).render(inner, buf);
    }
}

/// In-place editor operations, tested independently of the widget.
pub fn insert_char(text: &mut String, cursor: &mut usize, c: char) {
    let byte_idx = char_index_to_byte(text, *cursor);
    text.insert(byte_idx, c);
    *cursor += 1;
}

pub fn backspace(text: &mut String, cursor: &mut usize) {
    if *cursor == 0 || text.is_empty() {
        return;
    }
    let prev = *cursor - 1;
    let byte_idx = char_index_to_byte(text, prev);
    let ch_len = text[byte_idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(0);
    text.replace_range(byte_idx..byte_idx + ch_len, "");
    *cursor = prev;
}

pub fn move_left(cursor: &mut usize) {
    if *cursor > 0 {
        *cursor -= 1;
    }
}

pub fn move_right(text: &str, cursor: &mut usize) {
    let len = text.chars().count();
    if *cursor < len {
        *cursor += 1;
    }
}

pub fn move_home(cursor: &mut usize) {
    *cursor = 0;
}

pub fn move_end(text: &str, cursor: &mut usize) {
    *cursor = text.chars().count();
}

fn char_index_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or_else(|| s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_001_insert_and_backspace_manipulate_buffer() {
        let mut s = String::new();
        let mut c = 0;
        insert_char(&mut s, &mut c, 'h');
        insert_char(&mut s, &mut c, 'i');
        assert_eq!(s, "hi");
        assert_eq!(c, 2);
        backspace(&mut s, &mut c);
        assert_eq!(s, "h");
        assert_eq!(c, 1);
    }

    #[test]
    fn spec_001_cursor_navigation() {
        let s = String::from("abc");
        let mut c = 3;
        move_left(&mut c);
        move_left(&mut c);
        assert_eq!(c, 1);
        move_home(&mut c);
        assert_eq!(c, 0);
        move_end(&s, &mut c);
        assert_eq!(c, 3);
        move_right(&s, &mut c);
        assert_eq!(c, 3);
    }

    #[test]
    fn spec_001_backspace_at_zero_is_noop() {
        let mut s = String::from("");
        let mut c = 0;
        backspace(&mut s, &mut c);
        assert_eq!(s, "");
        assert_eq!(c, 0);
    }

    #[test]
    fn spec_001_input_widget_renders_prompt_prefix() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let w = InputWidget::new("hi", 2, true);
        let backend = TestBackend::new(20, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(w, f.area());
            })
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(content.contains("› "), "prefix missing: {content:?}");
        assert!(content.contains("hi"));
    }
}
