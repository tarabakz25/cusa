// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Terminal default-color probe for vendored `terminal_palette` (composer tint).

use std::io;
use std::time::Duration;

pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct DefaultColors {
    pub(crate) fg: (u8, u8, u8),
    pub(crate) bg: (u8, u8, u8),
}

/// Standard VGA palette indices used by `COLORFGBG`.
fn ansi16_rgb(index: u8) -> (u8, u8, u8) {
    match index % 8 {
        0 => (0, 0, 0),
        1 => (205, 0, 0),
        2 => (0, 205, 0),
        3 => (205, 205, 0),
        4 => (0, 0, 238),
        5 => (205, 0, 205),
        6 => (0, 205, 205),
        7 => (229, 229, 229),
        _ => (0, 0, 0),
    }
}

fn colors_from_colorfgbg() -> Option<DefaultColors> {
    let raw = std::env::var("COLORFGBG").ok()?;
    let mut parts = raw.split(';');
    let fg_idx = parts.next()?.parse::<u8>().ok()?;
    let bg_idx = parts.next()?.parse::<u8>().ok()?;
    let bright = |i: u8| if i >= 8 { 55 } else { 0 };
    let mut fg = ansi16_rgb(fg_idx);
    let mut bg = ansi16_rgb(bg_idx);
    fg.0 = fg.0.saturating_add(bright(fg_idx));
    fg.1 = fg.1.saturating_add(bright(fg_idx));
    fg.2 = fg.2.saturating_add(bright(fg_idx));
    bg.0 = bg.0.saturating_add(bright(bg_idx));
    bg.1 = bg.1.saturating_add(bright(bg_idx));
    bg.2 = bg.2.saturating_add(bright(bg_idx));
    Some(DefaultColors { fg, bg })
}

/// Dark-terminal fallback when OSC 10/11 probe is unavailable (matches Codex tint path).
fn dark_terminal_fallback() -> DefaultColors {
    DefaultColors {
        fg: (229, 229, 229),
        bg: (0, 0, 0),
    }
}

/// Resolve default fg/bg for composer `user_message_style` tinting.
pub(crate) fn default_colors(
    _timeout: Duration,
) -> io::Result<Option<DefaultColors>> {
    Ok(Some(colors_from_colorfgbg().unwrap_or_else(dark_terminal_fallback)))
}
