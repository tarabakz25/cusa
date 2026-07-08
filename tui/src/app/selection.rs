// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// tmux-style copy-on-select (PR #9).
//
// The TUI captures mouse events and implements its own drag selection over
// the rendered frame buffer: press-and-drag highlights a stream-style region
// (like a terminal's native selection), and releasing the button copies the
// selected text to the clipboard — no explicit Cmd+C / Ctrl+Shift+C needed —
// then shows a `copied …` toast.
//
// Clipboard strategy (both best-effort, in parallel):
//   1. OSC 52 escape written to the controlling terminal. Works in iTerm2,
//      kitty, WezTerm, Alacritty, Windows Terminal, and through tmux/SSH.
//   2. A platform clipboard command (`pbcopy`, `wl-copy`, `xclip`, `xsel`,
//      `clip.exe`) spawned on a detached thread. Covers terminals without
//      OSC 52 support (e.g. macOS Terminal.app) when running locally.
//
// Native selection remains available via the terminal's capture-bypass
// modifier (Shift+drag in most terminals, Option/Alt+drag in iTerm2).

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

/// An in-progress or just-finished mouse selection, in screen cells.
///
/// `anchor` is where the button went down; `head` is the latest drag
/// position. Both are inclusive. The selected region is the *stream* range
/// between the two positions in reading order (like native terminal
/// selection / tmux copy mode), not a rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (u16, u16),
    pub head: (u16, u16),
    /// True once at least one drag event extended the selection. A plain
    /// click (down + up without drag) never copies.
    pub dragged: bool,
}

impl Selection {
    pub fn new(x: u16, y: u16) -> Self {
        Self {
            anchor: (x, y),
            head: (x, y),
            dragged: false,
        }
    }

    /// Selection endpoints as (start, end) linear cell indices over `area`,
    /// in reading order. Inclusive on both ends.
    fn linear_range(&self, area: Rect) -> (usize, usize) {
        let a = linear_index(clamp_to(area, self.anchor), area);
        let b = linear_index(clamp_to(area, self.head), area);
        (a.min(b), a.max(b))
    }
}

fn clamp_to(area: Rect, (x, y): (u16, u16)) -> (u16, u16) {
    let max_x = area.right().saturating_sub(1).max(area.left());
    let max_y = area.bottom().saturating_sub(1).max(area.top());
    (x.clamp(area.left(), max_x), y.clamp(area.top(), max_y))
}

fn linear_index((x, y): (u16, u16), area: Rect) -> usize {
    (y - area.top()) as usize * area.width as usize + (x - area.left()) as usize
}

/// What the event loop should do in response to a mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    /// Nothing to do.
    None,
    /// Selection changed; redraw so the highlight tracks the drag.
    Redraw,
    /// Button released after a drag: extract + copy the selection.
    Copy(Selection),
    /// Wheel scrolled — forward as an arrow key, matching the terminal's
    /// "alternate scroll" behavior that applied before mouse capture.
    ScrollUp,
    ScrollDown,
}

/// Reduce a crossterm mouse event against the current selection state.
///
/// `area` is the viewport the selection is clamped to (the full screen for
/// this app). Pure state transition — the caller performs the actual copy.
pub fn on_mouse_event(
    selection: &mut Option<Selection>,
    area: Rect,
    ev: &MouseEvent,
) -> MouseAction {
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let (x, y) = clamp_to(area, (ev.column, ev.row));
            let had_highlight = selection.is_some();
            *selection = Some(Selection::new(x, y));
            if had_highlight {
                MouseAction::Redraw
            } else {
                MouseAction::None
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => match selection {
            Some(sel) => {
                let head = clamp_to(area, (ev.column, ev.row));
                if head != sel.head || !sel.dragged {
                    sel.head = head;
                    sel.dragged = true;
                    MouseAction::Redraw
                } else {
                    MouseAction::None
                }
            }
            // Drag without a preceding Down (e.g. capture enabled mid-drag):
            // treat as the start of a selection.
            None => {
                let (x, y) = clamp_to(area, (ev.column, ev.row));
                *selection = Some(Selection::new(x, y));
                MouseAction::Redraw
            }
        },
        MouseEventKind::Up(MouseButton::Left) => match selection.take() {
            Some(sel) if sel.dragged => MouseAction::Copy(sel),
            // Plain click: clear any leftover highlight, never copy.
            Some(_) => MouseAction::Redraw,
            None => MouseAction::None,
        },
        MouseEventKind::ScrollUp => MouseAction::ScrollUp,
        MouseEventKind::ScrollDown => MouseAction::ScrollDown,
        _ => MouseAction::None,
    }
}

/// Overlay the selection highlight (REVERSED) onto a rendered buffer.
///
/// Stream selection covers up to three rectangles: the tail of the first
/// row, all full middle rows, and the head of the last row.
pub fn highlight(buf: &mut Buffer, sel: &Selection) {
    let area = buf.area;
    if area.width == 0 || area.height == 0 {
        return;
    }
    let style = Style::default().add_modifier(Modifier::REVERSED);
    let (start, end) = sel.linear_range(area);
    let w = area.width as usize;
    let (sx, sy) = (start % w, start / w);
    let (ex, ey) = (end % w, end / w);
    for y in sy..=ey {
        let row_start = if y == sy { sx } else { 0 };
        let row_end = if y == ey { ex } else { w - 1 };
        let rect = Rect::new(
            area.left() + row_start as u16,
            area.top() + y as u16,
            (row_end - row_start + 1) as u16,
            1,
        );
        buf.set_style(rect, style);
    }
}

/// Extract the text covered by `sel` from a rendered frame buffer.
///
/// Walks each row grapheme-wise so wide characters (CJK etc.) are emitted
/// once and their continuation cells skipped. Trailing whitespace is trimmed
/// per row; rows are joined with `\n` (tmux-like line-wise semantics).
pub fn extract_text(buf: &Buffer, sel: &Selection) -> String {
    let area = buf.area;
    if area.width == 0 || area.height == 0 {
        return String::new();
    }
    let (start, end) = sel.linear_range(area);
    let w = area.width as usize;
    let (sy, ey) = (start / w, end / w);
    let mut lines: Vec<String> = Vec::new();
    for y in sy..=ey {
        let row_sel_start = if y == sy { start % w } else { 0 };
        let row_sel_end = if y == ey { end % w } else { w - 1 };
        let mut line = String::new();
        let mut col = 0usize;
        while col < w {
            let x = area.left() + col as u16;
            let yy = area.top() + y as u16;
            let Some(cell) = buf.cell((x, yy)) else { break };
            let symbol = cell.symbol();
            let width = UnicodeWidthStr::width(symbol).max(1);
            if col >= row_sel_start && col <= row_sel_end {
                line.push_str(symbol);
            }
            col += width;
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

/// OSC 52 sequence that puts `text` on the system clipboard, for terminals
/// that support it (iTerm2, kitty, WezTerm, Alacritty, Windows Terminal;
/// forwarded by tmux with `set-clipboard on|external`, and over SSH).
pub fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()))
}

/// Minimal RFC 4648 standard-alphabet base64 with padding. Local to avoid a
/// dependency for a 20-line function.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let idx = [
            (n >> 18) & 0x3f,
            (n >> 12) & 0x3f,
            (n >> 6) & 0x3f,
            n & 0x3f,
        ];
        for (i, &v) in idx.iter().enumerate() {
            if i <= chunk.len() {
                out.push(ALPHABET[v as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Toast text shown after a successful copy.
pub fn copied_toast_message(text: &str) -> String {
    const PREVIEW_MAX: usize = 24;
    let chars = text.chars().count();
    let single_line = !text.contains('\n');
    if single_line && chars <= PREVIEW_MAX {
        format!("copied \"{text}\"")
    } else {
        format!("copied {chars} chars")
    }
}

/// Copy `text` to the OS clipboard via a platform helper command, on a
/// detached thread so a slow or hung helper can never stall the event loop.
/// Best-effort: failures are silently ignored (OSC 52 is the primary path).
pub fn spawn_system_clipboard_copy(text: String) {
    std::thread::spawn(move || {
        let _ = system_clipboard_copy(&text);
    });
}

fn system_clipboard_copy(text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    for (cmd, args) in clipboard_command_candidates() {
        let spawned = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = spawned else { continue };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no clipboard helper found",
    ))
}

fn clipboard_command_candidates() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut candidates: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
    if cfg!(target_os = "macos") {
        candidates.push(("pbcopy", vec![]));
        return candidates;
    }
    if cfg!(target_os = "windows") {
        candidates.push(("clip.exe", vec![]));
        return candidates;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        candidates.push(("wl-copy", vec![]));
    }
    if std::env::var_os("DISPLAY").is_some() {
        candidates.push(("xclip", vec!["-selection", "clipboard"]));
        candidates.push(("xsel", vec!["-ib"]));
    }
    // WSL: Linux target, but the Windows clipboard is reachable.
    if std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::env::var_os("WSL_INTEROP").is_some()
    {
        candidates.push(("clip.exe", vec![]));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    fn area_10x4() -> Rect {
        Rect::new(0, 0, 10, 4)
    }

    // ---- mouse reducer ----

    #[test]
    fn down_starts_selection_without_redraw() {
        let mut sel = None;
        let action = on_mouse_event(
            &mut sel,
            area_10x4(),
            &mouse(MouseEventKind::Down(MouseButton::Left), 3, 1),
        );
        assert_eq!(MouseAction::None, action);
        assert_eq!(Some(Selection::new(3, 1)), sel);
    }

    #[test]
    fn drag_extends_selection_and_requests_redraw() {
        let mut sel = Some(Selection::new(3, 1));
        let action = on_mouse_event(
            &mut sel,
            area_10x4(),
            &mouse(MouseEventKind::Drag(MouseButton::Left), 7, 2),
        );
        assert_eq!(MouseAction::Redraw, action);
        let sel = sel.expect("selection");
        assert_eq!((7, 2), sel.head);
        assert!(sel.dragged);
    }

    #[test]
    fn up_after_drag_requests_copy_and_clears_selection() {
        let mut sel = Some(Selection {
            anchor: (3, 1),
            head: (7, 2),
            dragged: true,
        });
        let action = on_mouse_event(
            &mut sel,
            area_10x4(),
            &mouse(MouseEventKind::Up(MouseButton::Left), 7, 2),
        );
        assert!(matches!(action, MouseAction::Copy(_)));
        assert_eq!(None, sel);
    }

    #[test]
    fn plain_click_never_copies() {
        let mut sel = None;
        on_mouse_event(
            &mut sel,
            area_10x4(),
            &mouse(MouseEventKind::Down(MouseButton::Left), 3, 1),
        );
        let action = on_mouse_event(
            &mut sel,
            area_10x4(),
            &mouse(MouseEventKind::Up(MouseButton::Left), 3, 1),
        );
        assert_eq!(MouseAction::Redraw, action);
        assert_eq!(None, sel);
    }

    #[test]
    fn drag_positions_are_clamped_to_area() {
        let mut sel = Some(Selection::new(3, 1));
        on_mouse_event(
            &mut sel,
            area_10x4(),
            &mouse(MouseEventKind::Drag(MouseButton::Left), 200, 100),
        );
        assert_eq!((9, 3), sel.expect("selection").head);
    }

    #[test]
    fn wheel_maps_to_scroll_actions() {
        let mut sel = None;
        assert_eq!(
            MouseAction::ScrollUp,
            on_mouse_event(&mut sel, area_10x4(), &mouse(MouseEventKind::ScrollUp, 0, 0))
        );
        assert_eq!(
            MouseAction::ScrollDown,
            on_mouse_event(&mut sel, area_10x4(), &mouse(MouseEventKind::ScrollDown, 0, 0))
        );
    }

    // ---- extraction ----

    fn buffer_with_lines(lines: &[&str]) -> Buffer {
        let width = 10u16;
        let mut buf = Buffer::empty(Rect::new(0, 0, width, lines.len() as u16));
        for (y, line) in lines.iter().enumerate() {
            buf.set_string(0, y as u16, line, Style::default());
        }
        buf
    }

    #[test]
    fn extracts_single_row_slice() {
        let buf = buffer_with_lines(&["hello you"]);
        let sel = Selection {
            anchor: (0, 0),
            head: (4, 0),
            dragged: true,
        };
        assert_eq!("hello", extract_text(&buf, &sel));
    }

    #[test]
    fn extraction_is_order_independent() {
        let buf = buffer_with_lines(&["hello you"]);
        let sel = Selection {
            anchor: (4, 0),
            head: (0, 0),
            dragged: true,
        };
        assert_eq!("hello", extract_text(&buf, &sel));
    }

    #[test]
    fn extracts_multi_row_stream_and_trims_trailing_blanks() {
        let buf = buffer_with_lines(&["first", "second", "third"]);
        // From column 2 of row 0 through column 2 of row 2: tail of row 0,
        // all of row 1, head of row 2.
        let sel = Selection {
            anchor: (2, 0),
            head: (2, 2),
            dragged: true,
        };
        assert_eq!("rst\nsecond\nthi", extract_text(&buf, &sel));
    }

    #[test]
    fn extracts_wide_chars_once() {
        let buf = buffer_with_lines(&["中文 ok"]);
        let sel = Selection {
            anchor: (0, 0),
            head: (9, 0),
            dragged: true,
        };
        assert_eq!("中文 ok", extract_text(&buf, &sel));
    }

    #[test]
    fn whole_blank_row_extracts_as_empty_line() {
        let buf = buffer_with_lines(&["top", "", "bottom"]);
        let sel = Selection {
            anchor: (0, 0),
            head: (5, 2),
            dragged: true,
        };
        assert_eq!("top\n\nbottom", extract_text(&buf, &sel));
    }

    // ---- highlight ----

    #[test]
    fn highlight_covers_stream_region_only() {
        let mut buf = buffer_with_lines(&["first", "second", "third"]);
        let sel = Selection {
            anchor: (8, 0),
            head: (1, 2),
            dragged: true,
        };
        highlight(&mut buf, &sel);
        let reversed = |x: u16, y: u16| {
            buf.cell((x, y))
                .expect("cell")
                .modifier
                .contains(Modifier::REVERSED)
        };
        assert!(!reversed(7, 0), "before anchor must not be highlighted");
        assert!(reversed(8, 0));
        assert!(reversed(9, 0));
        assert!(reversed(0, 1) && reversed(9, 1), "middle row fully highlighted");
        assert!(reversed(0, 2) && reversed(1, 2));
        assert!(!reversed(2, 2), "after head must not be highlighted");
    }

    // ---- osc52 / base64 ----

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(expected, base64_encode(input.as_bytes()), "input={input:?}");
        }
    }

    #[test]
    fn osc52_wraps_base64_payload() {
        assert_eq!("\x1b]52;c;aGVsbG8=\x07", osc52_sequence("hello"));
    }

    // ---- toast ----

    #[test]
    fn toast_shows_short_text_verbatim() {
        assert_eq!("copied \"hello\"", copied_toast_message("hello"));
    }

    #[test]
    fn toast_falls_back_to_char_count_for_long_or_multiline() {
        assert_eq!("copied 3 chars", copied_toast_message("a\nb"));
        let long = "x".repeat(40);
        assert_eq!("copied 40 chars", copied_toast_message(&long));
    }
}
