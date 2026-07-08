// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Interactive terminal setup via vendored Codex `custom_terminal` (SPEC-105).
// Tests continue to use `ratatui::Terminal` + `TestBackend` for snapshots.

use crate::codex_ui::custom_terminal;
use crate::codex_ui::terminal_palette::set_default_colors_from_startup_probe;
use crate::codex_adapter::terminal_probe;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::Rect;
use std::io::{self, Stdout, Write};

pub type InteractiveBackend = CrosstermBackend<Stdout>;
pub type InteractiveTerminal = custom_terminal::Terminal<InteractiveBackend>;

/// Owns raw-mode / alternate-screen setup; restores on drop.
pub struct TerminalSession {
    pub terminal: InteractiveTerminal,
}

impl TerminalSession {
    /// Enter raw mode + alternate screen and construct a full-screen viewport.
    pub fn open() -> io::Result<Self> {
        enable_raw_mode()?;
        let colors = terminal_probe::default_colors(terminal_probe::DEFAULT_TIMEOUT).ok().flatten();
        set_default_colors_from_startup_probe(colors);
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = custom_terminal::Terminal::with_options(backend)?;
        sync_viewport(&mut terminal)?;
        Ok(Self { terminal })
    }

    /// Resize the custom terminal viewport to the current backend dimensions
    /// and force a full repaint on the next draw.
    pub fn sync_viewport(terminal: &mut InteractiveTerminal) -> io::Result<()> {
        sync_viewport(terminal)
    }

    /// Tear down raw mode and restore the shell cursor.
    pub fn teardown(mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

fn sync_viewport<B>(terminal: &mut custom_terminal::Terminal<B>) -> io::Result<()>
where
    B: Backend + Write,
{
    let size = terminal.size()?;
    terminal.resize(size)?;
    terminal.set_viewport_area(Rect::new(0, 0, size.width, size.height));
    // On resize the emulator reflows (or keeps) the old screen content, so the
    // diff buffers no longer match what is physically on screen and a
    // diff-only flush leaves stale artifacts behind. Clear the visible screen
    // and reset the back buffer so the next draw repaints everything.
    terminal.clear_visible_screen()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::{ClearType, WindowSize};
    use ratatui::buffer::Cell;
    use ratatui::layout::{Position, Size};
    use ratatui::style::Stylize;
    use ratatui::text::Span;
    use ratatui::widgets::Widget;

    struct RecordingBackend {
        size: Size,
        cursor: Position,
        output: Vec<u8>,
        cleared_regions: Vec<ClearType>,
    }

    impl RecordingBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                size: Size { width, height },
                cursor: Position { x: 0, y: 0 },
                output: Vec::new(),
                cleared_regions: Vec::new(),
            }
        }

        fn output(&self) -> String {
            String::from_utf8_lossy(&self.output).into_owned()
        }
    }

    impl Write for RecordingBackend {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Backend for RecordingBackend {
        fn draw<'a, I>(&mut self, _content: I) -> io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            Ok(())
        }

        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn get_cursor_position(&mut self) -> io::Result<Position> {
            Ok(self.cursor)
        }

        fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
            self.cursor = position.into();
            Ok(())
        }

        fn clear(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
            self.cleared_regions.push(clear_type);
            Ok(())
        }

        fn append_lines(&mut self, _line_count: u16) -> io::Result<()> {
            Ok(())
        }

        fn scroll_region_up(
            &mut self,
            _region: std::ops::Range<u16>,
            _scroll_by: u16,
        ) -> io::Result<()> {
            Ok(())
        }

        fn scroll_region_down(
            &mut self,
            _region: std::ops::Range<u16>,
            _scroll_by: u16,
        ) -> io::Result<()> {
            Ok(())
        }

        fn size(&self) -> io::Result<Size> {
            Ok(self.size)
        }

        fn window_size(&mut self) -> io::Result<WindowSize> {
            Ok(WindowSize {
                columns_rows: self.size,
                pixels: self.size,
            })
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn draw_marker(terminal: &mut custom_terminal::Terminal<RecordingBackend>) {
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 1, 1);
                Span::raw("X").not_dim().render(area, frame.buffer);
            })
            .expect("draw");
    }

    #[test]
    fn sync_viewport_forces_full_repaint_after_resize() {
        let backend = RecordingBackend::new(80, 24);
        let mut terminal = custom_terminal::Terminal::with_options(backend).expect("terminal");
        sync_viewport(&mut terminal).expect("initial sync");

        draw_marker(&mut terminal);
        assert!(
            terminal.backend().output().contains('X'),
            "first draw should emit the marker cell"
        );

        // An unchanged frame diffs to nothing — sanity check for the diff path.
        terminal.backend_mut().output.clear();
        draw_marker(&mut terminal);
        assert!(
            !terminal.backend().output().contains('X'),
            "unchanged frame should not re-emit the marker cell"
        );

        // Simulate a terminal resize, then sync.
        terminal.backend_mut().size = Size {
            width: 60,
            height: 20,
        };
        terminal.backend_mut().output.clear();
        terminal.backend_mut().cleared_regions.clear();
        sync_viewport(&mut terminal).expect("resize sync");

        assert_eq!(terminal.viewport_area, Rect::new(0, 0, 60, 20));
        assert_eq!(
            terminal.last_known_screen_size,
            Size {
                width: 60,
                height: 20
            }
        );
        assert!(
            terminal.backend().cleared_regions.contains(&ClearType::All),
            "resize must clear the physical screen to drop reflowed artifacts"
        );

        // The next draw must repaint even cells whose content did not change.
        draw_marker(&mut terminal);
        assert!(
            terminal.backend().output().contains('X'),
            "draw after resize should repaint the full viewport"
        );
    }
}
