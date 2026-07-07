// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Startup "resume chooser" overlay (SPEC-051, SPEC-053).
//
// The chooser is opt-in: it is shown before the boot handshake only when
// the CLI is launched with `--resume-picker` AND `SessionStore` has at
// least one prior session recorded for the current `cwd`. By default the
// CLI starts a new chat directly. The user picks one of three paths:
//
//   1. Start a fresh session (`session/create`).
//   2. Resume one of up to 8 recent sessions (`session/resume`).
//   3. Cancel (Esc / Ctrl-C) — treated the same as (1).
//
// The chooser is rendered *outside* the normal event loop: we enter raw
// mode + the alt screen, draw exactly once, block on `crossterm::event::read`
// until we get a decision, then restore the terminal. This keeps the rest
// of `run()` unchanged.

use crate::session_store::StoredSession;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// Maximum number of candidate rows we render. Newer entries win.
pub const MAX_CANDIDATES: usize = 8;

/// User's decision from the chooser.
#[derive(Debug, Clone)]
pub enum ChooserOutcome {
    /// Start a fresh session (no `--resume`).
    New,
    /// Resume the given session.
    Resume(StoredSession),
}

/// Overlay state driving the chooser. `selected == 0` is the "New session"
/// row; `selected >= 1` maps into `candidates[selected - 1]`.
#[derive(Debug, Clone)]
pub struct ResumeChooser {
    pub candidates: Vec<StoredSession>,
    pub selected: usize,
    pub cwd: String,
}

impl ResumeChooser {
    /// Construct with up to `MAX_CANDIDATES` entries, newest first. The
    /// caller is expected to have filtered by `cwd` already.
    pub fn new(mut candidates: Vec<StoredSession>, cwd: String) -> Self {
        candidates.truncate(MAX_CANDIDATES);
        Self {
            candidates,
            selected: 0,
            cwd,
        }
    }

    pub fn row_count(&self) -> usize {
        self.candidates.len() + 1
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.row_count() {
            self.selected += 1;
        }
    }

    /// Return the decision without consuming the chooser.
    pub fn commit(&self) -> ChooserOutcome {
        if self.selected == 0 {
            ChooserOutcome::New
        } else {
            match self.candidates.get(self.selected - 1) {
                Some(s) => ChooserOutcome::Resume(s.clone()),
                None => ChooserOutcome::New,
            }
        }
    }
}

/// Renderer. Rendered as a centered list overlay, styled identically to
/// the other pickers.
#[derive(Debug)]
pub struct ResumeChooserWidget<'a> {
    chooser: &'a ResumeChooser,
}

impl<'a> ResumeChooserWidget<'a> {
    pub fn new(chooser: &'a ResumeChooser) -> Self {
        Self { chooser }
    }

    pub fn lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(self.chooser.row_count() + 4);
        lines.push(Line::from(vec![
            Span::styled(
                "cwd: ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                self.chooser.cwd.clone(),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(""));
        for row in 0..self.chooser.row_count() {
            let selected = row == self.chooser.selected;
            let marker = if selected { "› " } else { "  " };
            let mut spans = vec![Span::styled(
                marker.to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )];
            if row == 0 {
                spans.push(Span::styled(
                    "New session (fresh agent)".to_string(),
                    if selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ));
            } else if let Some(s) = self.chooser.candidates.get(row - 1) {
                spans.push(Span::styled(
                    "Resume ".to_string(),
                    Style::default().fg(Color::Cyan),
                ));
                spans.push(Span::styled(
                    s.short_agent_id(),
                    if selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ));
                spans.push(Span::styled(
                    format!(
                        "  ·  {}  ·  {} turns  ·  {}",
                        s.model,
                        s.turns,
                        relative_time(s.last_used_at),
                    ),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑/↓ (j/k) select · Enter apply · Esc new session",
            Style::default().fg(Color::Cyan),
        )));
        lines
    }
}

impl<'a> Widget for ResumeChooserWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rect = centered(72, 18, area);
        Clear.render(rect, buf);
        let block = Block::default()
            .title(" resume ")
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

/// Apply a chooser outcome to the boot-time `AppState` so `handshake()`
/// can pass the correct arguments to `session/create` or `session/resume`.
///
/// * `New`: leave `AppState` untouched (fresh defaults).
/// * `Resume`: copy `approval_mode`, `enabled_skill_ids`, and (if not
///   already set from `--mcp`) `mcp_overrides` onto the state so the
///   handshake carries them into `session/resume`.
///
/// Returns `Some(agent_id)` for the resumed case so the caller can set
/// `cli.resume`.
pub fn apply_choice_to_state(
    state: &mut crate::app::state::AppState,
    outcome: &ChooserOutcome,
) -> Option<String> {
    match outcome {
        ChooserOutcome::New => None,
        ChooserOutcome::Resume(stored) => {
            state.session.approval_mode = stored.approval_mode;
            state.session.enabled_skill_ids = stored.enabled_skill_ids.clone();
            state.session.skills_count = stored.enabled_skill_ids.len();
            if state.mcp_overrides.is_none() {
                state.mcp_overrides = stored.mcp_overrides.clone();
            }
            Some(stored.agent_id.clone())
        }
    }
}

/// Human-readable "n minutes ago"-style relative timestamp. Assumes
/// `ts_secs` is a unix epoch second.
pub fn relative_time(ts_secs: i64) -> String {
    if ts_secs <= 0 {
        return "just now".to_string();
    }
    let now = crate::session_store::now_unix();
    let delta = (now - ts_secs).max(0);
    if delta < 60 {
        return "just now".to_string();
    }
    let m = delta / 60;
    if m < 60 {
        return format!("{m}m ago");
    }
    let h = m / 60;
    if h < 24 {
        return format!("{h}h ago");
    }
    let d = h / 24;
    format!("{d}d ago")
}

/// Run the chooser in blocking mode. Enters raw mode + alt screen exactly
/// like `run_event_loop`, draws once per key, and returns the outcome.
///
/// Should be called *before* the sidecar handshake so the terminal state
/// mirrors what the event loop expects.
pub fn run_blocking(candidates: Vec<StoredSession>, cwd: &str) -> anyhow::Result<ChooserOutcome> {
    use crossterm::event::{Event as CtEvent, KeyCode, KeyModifiers};
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::backend::CrosstermBackend;
    use ratatui::Terminal;
    use std::io;

    let mut chooser = ResumeChooser::new(candidates, cwd.to_string());
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let outcome = loop {
        terminal.draw(|f| {
            f.render_widget(ResumeChooserWidget::new(&chooser), f.area());
        })?;
        if let CtEvent::Key(k) = crossterm::event::read()? {
            match (k.code, k.modifiers) {
                (KeyCode::Up | KeyCode::Char('k'), _) => chooser.move_up(),
                (KeyCode::Down | KeyCode::Char('j'), _) => chooser.move_down(),
                (KeyCode::Enter, _) => break chooser.commit(),
                (KeyCode::Esc, _) => break ChooserOutcome::New,
                (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                    break ChooserOutcome::New;
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

#[cfg(test)]
mod tests {
    use super::*;
    use cusa_rpc::ApprovalMode;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make(id: &str, cwd: &str, last: i64) -> StoredSession {
        StoredSession {
            agent_id: id.into(),
            cwd: cwd.into(),
            model: "composer-2.5".into(),
            approval_mode: ApprovalMode::AutoEdit,
            enabled_skill_ids: vec!["skill".into()],
            mcp_overrides: None,
            created_at: last,
            last_used_at: last,
            turns: 3,
        }
    }

    fn render(chooser: &ResumeChooser) -> String {
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(ResumeChooserWidget::new(chooser), f.area());
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    #[test]
    fn spec_051_startup_chooser_lists_candidates_for_cwd_only() {
        // The chooser itself only sees candidates already filtered by the
        // caller; make sure list shape matches the spec.
        let store = crate::session_store::SessionStore::new({
            let mut p = std::env::temp_dir();
            p.push(format!("cusa-chooser-cwd-{}.json", std::process::id()));
            let _ = std::fs::remove_file(&p);
            p
        });
        store.record_new(make("a", "/repo", 100)).unwrap();
        store.record_new(make("b", "/other", 100)).unwrap();
        store.record_new(make("c", "/repo", 200)).unwrap();

        let for_repo = store.list_for_cwd("/repo");
        assert_eq!(for_repo.len(), 2);
        // Chooser must include both.
        let chooser = ResumeChooser::new(for_repo, "/repo".into());
        assert_eq!(chooser.candidates.len(), 2);
        // Newest first — "c" before "a" (last_used_at 200 vs 100).
        assert_eq!(chooser.candidates[0].agent_id, "c");
        assert_eq!(chooser.candidates[1].agent_id, "a");
        let _ = std::fs::remove_file(store.path());
    }

    #[test]
    fn spec_051_chooser_starts_on_new_session_row() {
        let chooser =
            ResumeChooser::new(vec![make("a", "/repo", 100)], "/repo".into());
        assert_eq!(chooser.selected, 0);
        match chooser.commit() {
            ChooserOutcome::New => {}
            _ => panic!("selected=0 must commit as New"),
        }
    }

    #[test]
    fn spec_051_chooser_arrow_moves_selection_and_commits_resume() {
        let mut chooser = ResumeChooser::new(
            vec![make("a", "/repo", 100), make("b", "/repo", 200)],
            "/repo".into(),
        );
        chooser.move_down();
        chooser.move_down();
        chooser.move_down();
        assert_eq!(chooser.selected, 2);
        chooser.move_up();
        assert_eq!(chooser.selected, 1);
        match chooser.commit() {
            ChooserOutcome::Resume(s) => assert_eq!(s.agent_id, "a"),
            _ => panic!("expected Resume outcome"),
        }
    }

    #[test]
    fn spec_051_chooser_caps_at_max_candidates() {
        let candidates: Vec<StoredSession> =
            (0..20).map(|i| make(&format!("a-{i}"), "/repo", i)).collect();
        let chooser = ResumeChooser::new(candidates, "/repo".into());
        assert_eq!(chooser.candidates.len(), MAX_CANDIDATES);
    }

    #[test]
    fn spec_051_chooser_renders_rows_and_hint() {
        let chooser = ResumeChooser::new(
            vec![make("agent-abcdef", "/repo", 100)],
            "/repo".into(),
        );
        let s = render(&chooser);
        assert!(s.contains("New session"), "new-row missing: {s}");
        assert!(s.contains("agent-ab"), "short id missing: {s}");
        assert!(s.contains("composer-2.5"), "model missing: {s}");
        assert!(s.contains("Esc new session"), "hint missing: {s}");
    }

    #[test]
    fn spec_053_resume_carries_stored_approval_mode_into_handshake() {
        // End-to-end: chooser Resume → apply_choice_to_state → AppState
        // has approval_mode, enabled_skill_ids and mcp_overrides set.
        let mut stored = make("agent-xyz", "/repo", 100);
        stored.mcp_overrides = Some(serde_json::json!({"servers":{"a":{}}}));
        let mut chooser = ResumeChooser::new(vec![stored.clone()], "/repo".into());
        chooser.move_down();
        let outcome = chooser.commit();
        let mut state = crate::app::state::AppState::new("/repo".into());
        let resume_id = apply_choice_to_state(&mut state, &outcome);
        assert_eq!(resume_id.as_deref(), Some("agent-xyz"));
        assert_eq!(state.session.approval_mode, ApprovalMode::AutoEdit);
        assert_eq!(state.session.enabled_skill_ids, vec!["skill".to_string()]);
        assert_eq!(state.session.skills_count, 1);
        assert_eq!(
            state.mcp_overrides,
            Some(serde_json::json!({"servers":{"a":{}}}))
        );
    }

    #[test]
    fn spec_053_new_choice_leaves_state_defaults() {
        let mut state = crate::app::state::AppState::new("/repo".into());
        state.session.approval_mode = ApprovalMode::Suggest;
        let id = apply_choice_to_state(&mut state, &ChooserOutcome::New);
        assert!(id.is_none());
        assert_eq!(state.session.approval_mode, ApprovalMode::Suggest);
        assert!(state.session.enabled_skill_ids.is_empty());
    }

    #[test]
    fn spec_053_cli_mcp_wins_over_stored_overrides() {
        let mut stored = make("a", "/repo", 100);
        stored.mcp_overrides = Some(serde_json::json!({"servers":{"stored":{}}}));
        let mut state = crate::app::state::AppState::new("/repo".into());
        state.mcp_overrides = Some(serde_json::json!({"servers":{"cli":{}}}));
        apply_choice_to_state(&mut state, &ChooserOutcome::Resume(stored));
        // CLI flag pre-populates state.mcp_overrides; it must not be
        // clobbered by the stored session.
        assert_eq!(
            state.mcp_overrides,
            Some(serde_json::json!({"servers":{"cli":{}}}))
        );
    }
}
