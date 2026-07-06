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
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use std::io::{self, Stdout};

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

    /// Resize the custom terminal viewport to the current backend dimensions.
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

fn sync_viewport(terminal: &mut InteractiveTerminal) -> io::Result<()> {
    let size = terminal.size()?;
    terminal.set_viewport_area(Rect::new(0, 0, size.width, size.height));
    Ok(())
}
