// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Re-exports vendored Codex foundation modules (SPEC-105).

#[path = "../../vendor/codex-ui/mod.rs"]
mod inner;

pub use inner::*;

#[cfg(test)]
mod spec_105_tests {
    use super::{color, custom_terminal, style, terminal_palette, ui_consts, width};

    #[test]
    fn spec_105_foundation_modules_compile() {
        let _ = terminal_palette::stdout_color_level();
        let _ = style::user_message_style();
        let _ = width::usable_content_width(80, 2);
        assert_eq!(ui_consts::LIVE_PREFIX_COLS, 2);
        assert!(color::is_light((255, 255, 255)));
        // `custom_terminal` is linked when this test binary builds.
        let _ = std::any::type_name::<custom_terminal::Frame<'_>>();
    }
}
