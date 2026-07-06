// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// First-run API key onboarding (SPEC-101).
//
// When neither `CURSOR_API_KEY` nor `~/.cusa/config.toml` is present, the
// TUI shows a blocking login overlay before the sidecar handshake. The
// entered key is saved to config.toml (mode 0600) and the boot continues.

use anyhow::{bail, Result};
use crossterm::event::{Event as CtEvent, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use ratatui::Terminal;
use std::io;

use crate::config::{self, config_path};

/// Outcome of the login overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    /// Key saved to `~/.cusa/config.toml`.
    Saved,
    /// User cancelled (Esc / Ctrl-C).
    Cancelled,
}

#[derive(Debug, Clone)]
struct LoginPrompt {
    buffer: String,
    error: Option<String>,
}

impl LoginPrompt {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            error: None,
        }
    }

    fn masked_display(&self) -> String {
        "*".repeat(self.buffer.chars().count())
    }

    fn push_char(&mut self, c: char) {
        self.error = None;
        self.buffer.push(c);
    }

    fn pop_char(&mut self) {
        self.error = None;
        self.buffer.pop();
    }

    fn save(&mut self) -> Result<()> {
        if self.buffer.trim().is_empty() {
            self.error = Some("API key cannot be empty".into());
            return Ok(());
        }
        config::write_api_key(&self.buffer)?;
        Ok(())
    }
}

struct LoginWidget<'a> {
    prompt: &'a LoginPrompt,
}

impl LoginWidget<'_> {
    fn lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(Span::styled(
                "Cursor API key required",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Get a key: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "cursor.com/dashboard/integrations",
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(Span::styled(
                "Paste it below (input is hidden). Saved to ~/.cusa/config.toml.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("API key: ", Style::default().fg(Color::Magenta)),
                Span::styled(
                    self.prompt.masked_display(),
                    Style::default().fg(Color::White),
                ),
                Span::styled("▌", Style::default().fg(Color::Magenta)),
            ]),
        ];

        if let Some(err) = &self.prompt.error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter save · Esc cancel",
            Style::default().fg(Color::Cyan),
        )));
        lines
    }
}

impl Widget for LoginWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rect = centered(76, 14, area);
        Clear.render(rect, buf);
        let block = Block::default()
            .title(" setup ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));
        let inner = block.inner(rect);
        block.render(rect, buf);
        Paragraph::new(self.lines())
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }
}

fn centered(width: u16, height: u16, area: Rect) -> Rect {
    use ratatui::layout::{Constraint, Direction, Layout};
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}

/// Blocking login overlay. Returns `Saved` when the key was written.
pub fn run_blocking() -> Result<LoginOutcome> {
    let mut prompt = LoginPrompt::new();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let outcome = loop {
        terminal.draw(|f| {
            f.render_widget(LoginWidget { prompt: &prompt }, f.area());
        })?;

        if let CtEvent::Key(k) = crossterm::event::read()? {
            match (k.code, k.modifiers) {
                (KeyCode::Enter, _) => {
                    if let Err(err) = prompt.save() {
                        prompt.error = Some(format!("save failed: {err:#}"));
                        continue;
                    }
                    if prompt.error.is_some() {
                        continue;
                    }
                    break LoginOutcome::Saved;
                }
                (KeyCode::Esc, _) => break LoginOutcome::Cancelled,
                (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                    break LoginOutcome::Cancelled;
                }
                (KeyCode::Backspace, _) => prompt.pop_char(),
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                    prompt.push_char(c);
                }
                _ => {}
            }
        }
    };

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    Ok(outcome)
}

/// Ensure an API key is available. Prompts interactively when missing.
pub fn ensure_api_key() -> Result<()> {
    if config::api_key_configured() {
        return Ok(());
    }

    eprintln!(
        "cusa: no Cursor API key found. Opening setup… \
         (get one at https://cursor.com/dashboard/integrations)"
    );

    match run_blocking()? {
        LoginOutcome::Saved => {
            eprintln!(
                "cusa: API key saved to {}",
                config_path().display()
            );
            Ok(())
        }
        LoginOutcome::Cancelled => {
            bail!(
                "setup cancelled. Set CURSOR_API_KEY or run again to save a key to {}",
                config_path().display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_prompt_masks_input() {
        let mut p = LoginPrompt::new();
        p.push_char('a');
        p.push_char('b');
        p.push_char('c');
        assert_eq!(p.masked_display(), "***");
        p.pop_char();
        assert_eq!(p.masked_display(), "**");
    }

    #[test]
    fn login_prompt_rejects_empty_save() {
        let mut p = LoginPrompt::new();
        p.save().unwrap();
        assert!(p.error.is_some());
    }
}
