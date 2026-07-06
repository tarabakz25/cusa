// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Minimal editor/composer keymap defaults (SPEC-106). Full upstream `keymap.rs`
// is not vendored; only bindings required by `TextArea` and the composer bridge.

use super::key_hint::{self, KeyBinding};
use crossterm::event::{KeyCode, KeyModifiers};

/// Editor bindings consumed by vendored `TextArea`.
#[derive(Clone, Debug)]
pub struct EditorKeymap {
    pub insert_newline: Vec<KeyBinding>,
    pub move_left: Vec<KeyBinding>,
    pub move_right: Vec<KeyBinding>,
    pub move_up: Vec<KeyBinding>,
    pub move_down: Vec<KeyBinding>,
    pub move_word_left: Vec<KeyBinding>,
    pub move_word_right: Vec<KeyBinding>,
    pub move_line_start: Vec<KeyBinding>,
    pub move_line_end: Vec<KeyBinding>,
    pub delete_backward: Vec<KeyBinding>,
    pub delete_forward: Vec<KeyBinding>,
    pub delete_backward_word: Vec<KeyBinding>,
    pub delete_forward_word: Vec<KeyBinding>,
    pub kill_line_start: Vec<KeyBinding>,
    pub kill_whole_line: Vec<KeyBinding>,
    pub kill_line_end: Vec<KeyBinding>,
    pub yank: Vec<KeyBinding>,
}

impl EditorKeymap {
    pub fn default_composer() -> Self {
        Self {
            insert_newline: vec![
                key_hint::ctrl(KeyCode::Char('j')),
                key_hint::ctrl(KeyCode::Char('m')),
                key_hint::shift(KeyCode::Enter),
                key_hint::alt(KeyCode::Enter),
            ],
            move_left: vec![key_hint::plain(KeyCode::Left), key_hint::ctrl(KeyCode::Char('b'))],
            move_right: vec![key_hint::plain(KeyCode::Right), key_hint::ctrl(KeyCode::Char('f'))],
            move_up: vec![key_hint::plain(KeyCode::Up), key_hint::ctrl(KeyCode::Char('p'))],
            move_down: vec![key_hint::plain(KeyCode::Down), key_hint::ctrl(KeyCode::Char('n'))],
            move_word_left: vec![
                key_hint::alt(KeyCode::Char('b')),
                KeyBinding::new(KeyCode::Left, KeyModifiers::ALT),
                KeyBinding::new(KeyCode::Left, KeyModifiers::CONTROL),
            ],
            move_word_right: vec![
                key_hint::alt(KeyCode::Char('f')),
                KeyBinding::new(KeyCode::Right, KeyModifiers::ALT),
                KeyBinding::new(KeyCode::Right, KeyModifiers::CONTROL),
            ],
            move_line_start: vec![
                key_hint::plain(KeyCode::Home),
                key_hint::ctrl(KeyCode::Char('a')),
            ],
            move_line_end: vec![key_hint::plain(KeyCode::End), key_hint::ctrl(KeyCode::Char('e'))],
            delete_backward: vec![
                key_hint::plain(KeyCode::Backspace),
                key_hint::shift(KeyCode::Backspace),
                key_hint::ctrl(KeyCode::Char('h')),
            ],
            delete_forward: vec![
                key_hint::plain(KeyCode::Delete),
                key_hint::shift(KeyCode::Delete),
                key_hint::ctrl(KeyCode::Char('d')),
            ],
            delete_backward_word: vec![
                key_hint::alt(KeyCode::Backspace),
                key_hint::ctrl(KeyCode::Backspace),
                KeyBinding::new(KeyCode::Backspace, KeyModifiers::ALT),
            ],
            delete_forward_word: vec![
                key_hint::alt(KeyCode::Delete),
                key_hint::ctrl(KeyCode::Delete),
                KeyBinding::new(KeyCode::Delete, KeyModifiers::ALT),
            ],
            kill_line_start: vec![key_hint::ctrl(KeyCode::Char('u'))],
            kill_whole_line: vec![key_hint::ctrl(KeyCode::Char('k'))],
            kill_line_end: vec![key_hint::ctrl(KeyCode::Char('k'))],
            yank: vec![key_hint::ctrl(KeyCode::Char('y'))],
        }
    }
}

impl Default for EditorKeymap {
    fn default() -> Self {
        Self::default_composer()
    }
}

/// Composer-level submit binding: plain Enter (Shift+Enter is newline via editor map).
pub fn composer_submit_keys() -> Vec<KeyBinding> {
    vec![key_hint::plain(KeyCode::Enter)]
}

#[derive(Clone, Debug)]
pub struct RuntimeKeymap {
    pub editor: EditorKeymap,
    pub vim_normal: VimNormalKeymap,
    pub vim_operator: VimOperatorKeymap,
    pub vim_text_object: VimTextObjectKeymap,
}

impl RuntimeKeymap {
    pub fn defaults() -> Self {
        Self::default_composer()
    }

    pub fn default_composer() -> Self {
        Self {
            editor: EditorKeymap::default_composer(),
            vim_normal: VimNormalKeymap::default(),
            vim_operator: VimOperatorKeymap::default(),
            vim_text_object: VimTextObjectKeymap::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VimNormalKeymap {
    pub enter_insert: Vec<KeyBinding>,
    pub append_after_cursor: Vec<KeyBinding>,
    pub append_line_end: Vec<KeyBinding>,
    pub insert_line_start: Vec<KeyBinding>,
    pub open_line_below: Vec<KeyBinding>,
    pub open_line_above: Vec<KeyBinding>,
    pub move_left: Vec<KeyBinding>,
    pub move_right: Vec<KeyBinding>,
    pub move_up: Vec<KeyBinding>,
    pub move_down: Vec<KeyBinding>,
    pub move_word_forward: Vec<KeyBinding>,
    pub move_word_backward: Vec<KeyBinding>,
    pub move_word_end: Vec<KeyBinding>,
    pub move_line_start: Vec<KeyBinding>,
    pub move_line_end: Vec<KeyBinding>,
    pub delete_char: Vec<KeyBinding>,
    pub substitute_char: Vec<KeyBinding>,
    pub delete_to_line_end: Vec<KeyBinding>,
    pub change_to_line_end: Vec<KeyBinding>,
    pub yank_line: Vec<KeyBinding>,
    pub paste_after: Vec<KeyBinding>,
    pub start_delete_operator: Vec<KeyBinding>,
    pub start_yank_operator: Vec<KeyBinding>,
    pub start_change_operator: Vec<KeyBinding>,
    pub cancel_operator: Vec<KeyBinding>,
}

#[derive(Clone, Debug, Default)]
pub struct VimOperatorKeymap {
    pub delete_line: Vec<KeyBinding>,
    pub yank_line: Vec<KeyBinding>,
    pub motion_left: Vec<KeyBinding>,
    pub motion_right: Vec<KeyBinding>,
    pub motion_up: Vec<KeyBinding>,
    pub motion_down: Vec<KeyBinding>,
    pub motion_word_forward: Vec<KeyBinding>,
    pub motion_word_backward: Vec<KeyBinding>,
    pub motion_word_end: Vec<KeyBinding>,
    pub motion_line_start: Vec<KeyBinding>,
    pub motion_line_end: Vec<KeyBinding>,
    pub select_inner_text_object: Vec<KeyBinding>,
    pub select_around_text_object: Vec<KeyBinding>,
    pub cancel: Vec<KeyBinding>,
}

#[derive(Clone, Debug, Default)]
pub struct VimTextObjectKeymap {
    pub word: Vec<KeyBinding>,
    pub big_word: Vec<KeyBinding>,
    pub parentheses: Vec<KeyBinding>,
    pub brackets: Vec<KeyBinding>,
    pub braces: Vec<KeyBinding>,
    pub double_quote: Vec<KeyBinding>,
    pub single_quote: Vec<KeyBinding>,
    pub backtick: Vec<KeyBinding>,
    pub cancel: Vec<KeyBinding>,
}
