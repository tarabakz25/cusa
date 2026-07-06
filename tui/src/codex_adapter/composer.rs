// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Bridges `AppState` input fields ↔ vendored Codex `TextArea` composer (SPEC-106).

use crate::app::state::AppState;
use crate::codex_ui::keymap::{composer_submit_keys, RuntimeKeymap};
use crate::codex_adapter::types::ComposerView;
use crate::codex_adapter::view_model::CusaViewModel;
use crate::codex_ui::bottom_pane::textarea::{TextArea, TextAreaState};
use crate::codex_ui::key_hint::KeyBindingListExt;
use crate::codex_ui::render::{Insets, RectExt};
use crate::codex_ui::style::user_message_style;
use crate::codex_ui::ui_consts::LIVE_PREFIX_COLS;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Span;
use ratatui::widgets::{Block, StatefulWidgetRef, Widget, WidgetRef};

/// Codex default placeholder (`Ask Codex to do anything` → cusa branding).
pub const COMPOSER_PLACEHOLDER: &str = "Ask cusa to do anything";

const COMPOSER_FOOTER_ROWS: u16 = 1;

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
    /// When true, omit the dim placeholder so IME preedit can render cleanly.
    hide_placeholder: bool,
}

impl ComposerWidget {
    pub fn from_state(state: &AppState) -> Self {
        let view = CusaViewModel::composer_view(state);
        Self::from_view(&view, state.composer_input_active)
    }

    pub fn from_view(view: &ComposerView, hide_placeholder: bool) -> Self {
        Self {
            buffer: view.buffer.clone(),
            cursor_byte: char_index_to_byte(&view.buffer, view.cursor_pos),
            active: view.active,
            hide_placeholder,
        }
    }

    /// Rows for the tinted composer surface (padding only — footer is separate).
    pub fn desired_height(text: &str, width: u16) -> u16 {
        let inner_width = inner_text_width(width);
        let mut textarea = TextArea::new();
        textarea.set_text_clearing_elements(text);
        textarea.desired_height(inner_width).saturating_add(2).max(3)
    }

    pub fn desired_height_for_state(state: &AppState, width: u16) -> u16 {
        Self::desired_height(&state.input, width)
    }

    /// Inner textarea rect inside a bottom-pane composer region (Codex insets).
    pub fn textarea_rect(composer_area: Rect) -> Rect {
        composer_area.inset(Insets::tlbr(1, LIVE_PREFIX_COLS, 1, 1))
    }

    /// Composer region within the bottom pane (excludes the status footer row).
    pub fn composer_area_in_bottom_pane(bottom: Rect) -> Rect {
        let composer_height = bottom
            .height
            .saturating_sub(COMPOSER_FOOTER_ROWS.min(bottom.height));
        Rect {
            x: bottom.x,
            y: bottom.y,
            width: bottom.width,
            height: composer_height,
        }
    }

    /// Terminal cursor for IME preedit — matches upstream `ChatComposer::cursor_pos`.
    pub fn terminal_cursor(state: &AppState, screen: Rect) -> Option<(u16, u16)> {
        if state.overlay.is_open() || !CusaViewModel::composer_view(state).active {
            return None;
        }
        let bottom = composer_bottom_rect(screen, state)?;
        let composer_area = Self::composer_area_in_bottom_pane(bottom);
        let textarea_rect = Self::textarea_rect(composer_area);
        if textarea_rect.is_empty() {
            return None;
        }

        let mut textarea = TextArea::new();
        textarea.set_text_clearing_elements(&state.input);
        textarea.set_cursor(char_index_to_byte(&state.input, state.cursor_pos));
        let ta_state = TextAreaState::default();

        textarea
            .cursor_pos_with_state(textarea_rect, ta_state)
            .or(Some((textarea_rect.x, textarea_rect.y)))
    }

    /// Render the Codex `ChatComposer` tinted input surface (no top rule).
    pub fn render_composer_surface(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let style = user_message_style();
        Block::default().style(style).render_ref(area, buf);

        let textarea_rect = Self::textarea_rect(area);
        if textarea_rect.is_empty() {
            return;
        }

        let mut textarea = TextArea::new();
        textarea.set_text_clearing_elements(&self.buffer);
        textarea.set_cursor(self.cursor_byte);

        let prompt = if self.active {
            Span::from("›").bold()
        } else {
            Span::from("›").dim()
        };
        buf.set_span(
            textarea_rect.x.saturating_sub(LIVE_PREFIX_COLS),
            textarea_rect.y,
            &prompt,
            textarea_rect.width.saturating_add(LIVE_PREFIX_COLS),
        );

        let mut state = TextAreaState::default();
        if self.active {
            StatefulWidgetRef::render_ref(&(&textarea), textarea_rect, buf, &mut state);
        }

        let show_placeholder = self.active
            && self.buffer.is_empty()
            && !self.hide_placeholder;
        if show_placeholder {
            // Draw placeholder without advancing the terminal cursor (IME targets
            // `textarea_rect.x` via `terminal_cursor`, not the end of this string).
            buf.set_string(
                textarea_rect.x,
                textarea_rect.y,
                COMPOSER_PLACEHOLDER,
                ratatui::style::Style::default().dim(),
            );
        }
    }
}

impl Widget for ComposerWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_composer_surface(area, buf);
    }
}

/// Handle a key destined for the composer when no overlay is open.
pub fn handle_composer_key(
    state: &mut AppState,
    code: KeyCode,
    mods: KeyModifiers,
) -> ComposerKeyResult {
    if matches!(code, KeyCode::Esc) {
        if state.input.is_empty() {
            state.composer_input_active = false;
        }
    }

    let event = KeyEvent {
        code,
        modifiers: mods,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    };

    let submit_keys = composer_submit_keys();
    if submit_keys.is_pressed(event) {
        state.composer_input_active = false;
        return ComposerKeyResult::Submit;
    }

    if let KeyCode::Char(_) = code {
        if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            state.composer_input_active = true;
        }
    }

    let view = CusaViewModel::composer_view(state);
    let mut textarea = textarea_from_view(&view);
    textarea.input(event);
    apply_textarea_to_state(&mut textarea, state);

    ComposerKeyResult::Handled
}

fn composer_bottom_rect(screen: Rect, state: &AppState) -> Option<Rect> {
    use ratatui::layout::{Constraint, Direction, Layout};
    let bottom_height =
        crate::codex_adapter::bottom_pane::BottomPaneWidget::desired_height(state, screen.width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(bottom_height),
        ])
        .split(screen);
    Some(chunks[1])
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
    fn spec_106_composer_widget_renders_codex_prefix_and_placeholder() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let state = AppState::new("/tmp".into());
        let widget = ComposerWidget::from_state(&state);
        let backend = TestBackend::new(80, 4);
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
        assert!(
            content.contains(COMPOSER_PLACEHOLDER),
            "placeholder missing: {content:?}"
        );
    }

    #[test]
    fn spec_ime_empty_buffer_cursor_at_textarea_origin_not_after_placeholder() {
        let state = AppState::new("/tmp".into());
        let screen = Rect::new(0, 0, 80, 10);
        let (x, y) = ComposerWidget::terminal_cursor(&state, screen).expect("cursor");
        let bottom = composer_bottom_rect(screen, &state).unwrap();
        let textarea = ComposerWidget::textarea_rect(ComposerWidget::composer_area_in_bottom_pane(bottom));
        assert_eq!((x, y), (textarea.x, textarea.y));
    }

    #[test]
    fn spec_ime_hides_placeholder_after_input_begins() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut state = AppState::new("/tmp".into());
        state.composer_input_active = true;
        let widget = ComposerWidget::from_state(&state);
        let backend = TestBackend::new(80, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(widget, f.area()))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            !content.contains(COMPOSER_PLACEHOLDER),
            "placeholder should hide during IME: {content:?}"
        );
    }

    #[test]
    fn spec_ime_any_char_key_hides_placeholder_even_before_commit() {
        let mut state = AppState::new("/tmp".into());
        handle_composer_key(&mut state, KeyCode::Char('a'), KeyModifiers::empty());
        assert!(state.composer_input_active);
        assert_eq!(state.input, "a");
    }
}
