// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Bridges `AppState` input fields ↔ vendored Codex `TextArea` composer (SPEC-106).

use crate::app::state::AppState;
use crate::codex_ui::keymap::{composer_submit_keys, RuntimeKeymap};
use crate::codex_adapter::types::ComposerView;
use crate::codex_adapter::view_model::CusaViewModel;
use crate::codex_ui::bottom_pane::textarea::TextArea;
use crate::codex_ui::key_hint::KeyBindingListExt;
use crate::codex_ui::style::user_message_style;
use crate::codex_ui::ui_consts::LIVE_PREFIX_COLS;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Widget, WidgetRef};

/// Result of feeding a key into the composer while the input pane is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerKeyResult {
    Handled,
    Submit,
}

/// Ratatui widget rendering the vendored multi-line composer for one frame.
#[derive(Debug, Clone)]
pub struct ComposerWidget {
    buffer: String,
    cursor_byte: usize,
    active: bool,
}

impl ComposerWidget {
    pub fn from_state(state: &AppState) -> Self {
        let view = CusaViewModel::composer_view(state);
        Self::from_view(&view)
    }

    pub fn from_view(view: &ComposerView) -> Self {
        Self {
            buffer: view.buffer.clone(),
            cursor_byte: char_index_to_byte(&view.buffer, view.cursor_pos),
            active: view.active,
        }
    }

    /// Rows required for the composer region (top border + wrapped textarea).
    pub fn desired_height(text: &str, width: u16) -> u16 {
        let inner_width = inner_text_width(width);
        let mut textarea = TextArea::new();
        textarea.set_text_clearing_elements(text);
        (textarea.desired_height(inner_width) + 1).max(2)
    }

    pub fn desired_height_for_state(state: &AppState, width: u16) -> u16 {
        Self::desired_height(&state.input, width)
    }
}

impl Widget for ComposerWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = user_message_style();
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(style);
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.is_empty() {
            return;
        }

        let textarea_width = inner_text_width(inner.width);
        let textarea_rect = Rect {
            x: inner.x,
            y: inner.y,
            width: textarea_width.min(inner.width),
            height: inner.height,
        };

        let mut textarea = TextArea::new();
        textarea.set_text_clearing_elements(&self.buffer);
        textarea.set_cursor(self.cursor_byte);

        let prompt = if self.active {
            Span::from("›").bold()
        } else {
            Span::from("›").dim()
        };
        if textarea_rect.width > 0 {
            buf.set_span(
                textarea_rect.x.saturating_sub(LIVE_PREFIX_COLS),
                textarea_rect.y,
                &prompt,
                textarea_rect.width.saturating_add(LIVE_PREFIX_COLS),
            );
        }

        (&textarea).render_ref(textarea_rect, buf);
    }
}

/// Handle a key destined for the composer when no overlay is open.
pub fn handle_composer_key(
    state: &mut AppState,
    code: KeyCode,
    mods: KeyModifiers,
) -> ComposerKeyResult {
    let event = KeyEvent {
        code,
        modifiers: mods,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    };

    let submit_keys = composer_submit_keys();
    if submit_keys.is_pressed(event) {
        return ComposerKeyResult::Submit;
    }

    let view = CusaViewModel::composer_view(state);
    let mut textarea = textarea_from_view(&view);
    textarea.input(event);
    apply_textarea_to_state(&mut textarea, state);
    ComposerKeyResult::Handled
}

fn textarea_from_view(view: &ComposerView) -> TextArea {
    let mut textarea = TextArea::new();
    textarea.set_keymap_bindings(&RuntimeKeymap::default_composer());
    textarea.set_text_clearing_elements(&view.buffer);
    textarea.set_cursor(char_index_to_byte(&view.buffer, view.cursor_pos));
    textarea
}

fn apply_textarea_to_state(textarea: &TextArea, state: &mut AppState) {
    state.input = textarea.text().to_string();
    state.cursor_pos = byte_index_to_char(state.input.as_str(), textarea.cursor());
}

fn inner_text_width(total_width: u16) -> u16 {
    total_width.saturating_sub(LIVE_PREFIX_COLS + 1)
}

fn char_index_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or_else(|| s.len())
}

fn byte_index_to_char(s: &str, byte_idx: usize) -> usize {
    s[..byte_idx.min(s.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn spec_106_shift_enter_inserts_newline() {
        let mut state = AppState::new("/tmp".into());
        handle_composer_key(&mut state, KeyCode::Char('a'), KeyModifiers::empty());
        handle_composer_key(
            &mut state,
            KeyCode::Enter,
            KeyModifiers::SHIFT,
        );
        handle_composer_key(&mut state, KeyCode::Char('b'), KeyModifiers::empty());
        assert_eq!(state.input, "a\nb");
        assert_eq!(state.cursor_pos, 3);
    }

    #[test]
    fn spec_106_plain_enter_submits_not_newline() {
        let mut state = AppState::new("/tmp".into());
        state.input = "hello".into();
        state.cursor_pos = 5;
        let result = handle_composer_key(&mut state, KeyCode::Enter, KeyModifiers::empty());
        assert_eq!(result, ComposerKeyResult::Submit);
        assert_eq!(state.input, "hello");
    }

    #[test]
    fn spec_106_composer_widget_expands_for_multiline_buffer() {
        let mut state = AppState::new("/tmp".into());
        state.input = "line one\nline two\nline three".into();
        let h = ComposerWidget::desired_height_for_state(&state, 80);
        assert!(h >= 4, "expected multiline composer height, got {h}");
    }

    #[test]
    fn spec_106_composer_widget_renders_codex_prefix() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let state = AppState::new("/tmp".into());
        let widget = ComposerWidget::from_state(&state);
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(widget, f.area());
            })
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(content.contains('›'), "composer prefix missing: {content:?}");
    }
}
