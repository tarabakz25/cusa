// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Overlay (modal/toast) infrastructure (SPEC-074, SPEC-016, SPEC-021,
// SPEC-022, SPEC-032, SPEC-042).
//
// Overlays sit on top of the transcript. Slice 2 defined three variants:
//   * `Help` — a scrollable list of slash commands.
//   * `Toast` — a transient one-line notice (auto-dismisses on next key).
//   * `Fatal` — a red modal centered on screen; user must dismiss with
//     Enter/Esc. Carries the sidecar log path per SPEC-074.
//
// Phase D adds:
//   * `Approval { … }` — Y/N/A tool-call approval modal (SPEC-022/023).
//   * `ModelPicker` — arrow-keyed model chooser fed by `models/list`.
//   * `ApprovalPicker` — three-way approval-mode chooser (SPEC-021).
//   * `Skills` — togglable skills overlay (SPEC-032).
//   * `Mcp` — MCP server list + expandable rows (SPEC-042).

use crate::app::state::ContextStrategy;
use crate::app::usage::UsageSnapshot;
use cusa_rpc::{
    ApprovalMode, McpServerInfo, McpServerStatus, ModelInfo, ModelParameterDefinition,
    ModelParameterValue, ModelSelection, SkillInfo, ToolCategory,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use std::path::PathBuf;
use std::time::Instant;

/// Skills overlay state (SPEC-032). Owns the picker rows + toggle flags.
#[derive(Debug, Clone)]
pub struct SkillsOverlay {
    pub loading: bool,
    pub items: Vec<SkillRow>,
    pub cursor: usize,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

impl SkillsOverlay {
    pub fn loading() -> Self {
        Self {
            loading: true,
            items: Vec::new(),
            cursor: 0,
            error: None,
            warnings: Vec::new(),
        }
    }

    /// Populate from a `skills/list` payload, preserving the current
    /// enabled set.
    pub fn populate(
        &mut self,
        skills: Vec<SkillInfo>,
        warnings: Vec<String>,
        enabled: &[String],
    ) {
        self.loading = false;
        self.warnings = warnings;
        self.items = skills
            .into_iter()
            .map(|s| SkillRow {
                enabled: enabled.contains(&s.id),
                skill: s,
            })
            .collect();
        if self.cursor >= self.items.len() {
            self.cursor = self.items.len().saturating_sub(1);
        }
    }

    /// Collect the ids of currently-enabled skills, in original order.
    pub fn enabled_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.skill.id.clone())
            .collect()
    }

    pub fn toggle_cursor(&mut self) {
        if let Some(row) = self.items.get_mut(self.cursor) {
            row.enabled = !row.enabled;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.items.len() {
            self.cursor += 1;
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillRow {
    pub skill: SkillInfo,
    pub enabled: bool,
}

/// MCP overlay state (SPEC-042).
#[derive(Debug, Clone)]
pub struct McpOverlay {
    pub loading: bool,
    pub servers: Vec<McpServerInfo>,
    pub cursor: usize,
    pub expanded: Option<usize>,
    pub error: Option<String>,
}

impl McpOverlay {
    pub fn loading() -> Self {
        Self {
            loading: true,
            servers: Vec::new(),
            cursor: 0,
            expanded: None,
            error: None,
        }
    }

    pub fn populate(&mut self, servers: Vec<McpServerInfo>) {
        self.loading = false;
        self.servers = servers;
        if self.cursor >= self.servers.len() {
            self.cursor = self.servers.len().saturating_sub(1);
        }
    }

    pub fn toggle_cursor_expansion(&mut self) {
        if self.expanded == Some(self.cursor) {
            self.expanded = None;
        } else if !self.servers.is_empty() {
            self.expanded = Some(self.cursor);
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.servers.len() {
            self.cursor += 1;
        }
    }
}

/// Which pane has keyboard focus in the model picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelPickerFocus {
    #[default]
    Models,
    Parameters,
}

/// Model picker overlay state (SPEC-016).
#[derive(Debug, Clone)]
pub struct ModelPickerOverlay {
    pub loading: bool,
    pub models: Vec<ModelInfo>,
    pub cursor: usize,
    pub error: Option<String>,
    pub focus: ModelPickerFocus,
    pub param_cursor: usize,
    /// Selected value per parameter id for the highlighted model.
    pub param_values: Vec<(String, String)>,
}

impl ModelPickerOverlay {
    pub fn loading() -> Self {
        Self {
            loading: true,
            models: Vec::new(),
            cursor: 0,
            error: None,
            focus: ModelPickerFocus::Models,
            param_cursor: 0,
            param_values: Vec::new(),
        }
    }

    pub fn populated(models: Vec<ModelInfo>) -> Self {
        let mut overlay = Self {
            loading: false,
            models,
            cursor: 0,
            error: None,
            focus: ModelPickerFocus::Models,
            param_cursor: 0,
            param_values: Vec::new(),
        };
        overlay.sync_params_for_cursor();
        overlay
    }

    pub fn restore_selection(&mut self, selection: &ModelSelection) {
        if let Some(i) = self.models.iter().position(|m| m.id == selection.id) {
            self.cursor = i;
            self.sync_params_for_cursor();
            for param in &selection.params {
                if let Some((_, value)) = self
                    .param_values
                    .iter_mut()
                    .find(|(id, _)| id == &param.id)
                {
                    *value = param.value.clone();
                }
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            ModelPickerFocus::Models => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.sync_params_for_cursor();
                }
            }
            ModelPickerFocus::Parameters => {
                if self.param_cursor > 0 {
                    self.param_cursor -= 1;
                } else {
                    self.focus = ModelPickerFocus::Models;
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.focus {
            ModelPickerFocus::Models => {
                if self.cursor + 1 < self.models.len() {
                    self.cursor += 1;
                    self.sync_params_for_cursor();
                } else if self.selected_model().is_some_and(|m| !m.parameters.is_empty()) {
                    self.focus = ModelPickerFocus::Parameters;
                    self.param_cursor = 0;
                }
            }
            ModelPickerFocus::Parameters => {
                let count = self.param_row_count();
                if self.param_cursor + 1 < count {
                    self.param_cursor += 1;
                }
            }
        }
    }

    pub fn focus_parameters(&mut self) {
        if self.param_row_count() > 0 {
            self.focus = ModelPickerFocus::Parameters;
            self.param_cursor = 0;
        }
    }

    pub fn focus_models(&mut self) {
        self.focus = ModelPickerFocus::Models;
    }

    pub fn cycle_param_value(&mut self, delta: i32) {
        let Some(def) = self
            .selected_model()
            .and_then(|m| m.parameters.get(self.param_cursor))
            .cloned()
        else {
            return;
        };
        let Some((_, value)) = self
            .param_values
            .iter_mut()
            .find(|(id, _)| id == &def.id)
        else {
            return;
        };
        let idx = def
            .values
            .iter()
            .position(|v| v.value == *value)
            .unwrap_or(0);
        let len = def.values.len();
        if len == 0 {
            return;
        }
        let next = (idx as i32 + delta).rem_euclid(len as i32) as usize;
        value.clone_from(&def.values[next].value);
    }

    pub fn selected_model(&self) -> Option<&ModelInfo> {
        self.models.get(self.cursor)
    }

    pub fn selected(&self) -> Option<&ModelInfo> {
        self.selected_model()
    }

    pub fn has_parameters(&self) -> bool {
        self.param_row_count() > 0
    }

    pub fn param_row_count(&self) -> usize {
        self.selected_model()
            .map(|m| m.parameters.len())
            .unwrap_or(0)
    }

    pub fn build_selection(&self) -> Option<ModelSelection> {
        let model = self.selected_model()?;
        let params = self
            .param_values
            .iter()
            .map(|(id, value)| ModelParameterValue {
                id: id.clone(),
                value: value.clone(),
            })
            .collect();
        Some(ModelSelection {
            id: model.id.clone(),
            params,
        })
    }

    fn sync_params_for_cursor(&mut self) {
        self.param_values.clear();
        self.param_cursor = 0;
        self.focus = ModelPickerFocus::Models;
        let Some(model) = self.models.get(self.cursor) else {
            return;
        };
        for def in &model.parameters {
            let default = def
                .values
                .first()
                .map(|v| v.value.clone())
                .unwrap_or_default();
            self.param_values.push((def.id.clone(), default));
        }
    }
}

/// Approval-mode picker overlay (SPEC-021).
#[derive(Debug, Clone)]
pub struct ApprovalPickerOverlay {
    pub cursor: usize,
}

impl ApprovalPickerOverlay {
    pub fn new(current: ApprovalMode) -> Self {
        Self {
            cursor: mode_index(current),
        }
    }

    pub fn selected(&self) -> ApprovalMode {
        mode_from_index(self.cursor)
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < 3 {
            self.cursor += 1;
        }
    }
}

pub fn mode_index(mode: ApprovalMode) -> usize {
    match mode {
        ApprovalMode::Suggest => 0,
        ApprovalMode::AutoEdit => 1,
        ApprovalMode::FullAuto => 2,
    }
}

pub fn mode_from_index(i: usize) -> ApprovalMode {
    match i {
        1 => ApprovalMode::AutoEdit,
        2 => ApprovalMode::FullAuto,
        _ => ApprovalMode::Suggest,
    }
}

/// Cycle helper — used by both `/approval` and the Tab hotkey (SPEC-021).
pub fn cycle_approval_mode(mode: ApprovalMode) -> ApprovalMode {
    match mode {
        ApprovalMode::Suggest => ApprovalMode::AutoEdit,
        ApprovalMode::AutoEdit => ApprovalMode::FullAuto,
        ApprovalMode::FullAuto => ApprovalMode::Suggest,
    }
}

/// Approval-modal state (SPEC-022/023). Extends the Slice 2 stub with the
/// request id + tool category so the key handler can build a real
/// `tool/approvalResponse`.
#[derive(Debug, Clone)]
pub struct ApprovalOverlay {
    pub tool_name: String,
    pub args_preview: String,
    pub request_id: String,
    pub category: ToolCategory,
}

/// `/cost` overlay state (SPEC-062).
#[derive(Debug, Clone)]
pub struct CostOverlay {
    pub snapshot: UsageSnapshot,
    /// Row offset applied to the bottom pane (per-turn list). Higher =
    /// older turns visible. Capped by row count minus visible rows.
    pub scroll: usize,
}

impl CostOverlay {
    pub fn new(snapshot: UsageSnapshot) -> Self {
        Self { snapshot, scroll: 0 }
    }

    pub fn scroll_down(&mut self, step: usize) {
        let max = self.snapshot.turns.len().saturating_sub(1);
        self.scroll = (self.scroll + step).min(max);
    }

    pub fn scroll_up(&mut self, step: usize) {
        self.scroll = self.scroll.saturating_sub(step);
    }
}

/// `/context` overlay state (SPEC-092). Shows the current strategy and
/// the available choices; not interactive.
#[derive(Debug, Clone)]
pub struct ContextOverlay {
    pub current: ContextStrategy,
}

#[derive(Debug, Clone, Default)]
pub enum Overlay {
    #[default]
    None,
    Help,
    Toast {
        message: String,
        created: Instant,
    },
    Fatal {
        message: String,
        log_path: Option<PathBuf>,
    },
    /// Approval dialog for a tool call (SPEC-022/023).
    Approval(ApprovalOverlay),
    /// Model picker (SPEC-016).
    ModelPicker(ModelPickerOverlay),
    /// Approval-mode picker (SPEC-021).
    ApprovalPicker(ApprovalPickerOverlay),
    /// Skills toggle overlay (SPEC-032).
    Skills(SkillsOverlay),
    /// MCP server list overlay (SPEC-042).
    Mcp(McpOverlay),
    /// Cost / usage pane (SPEC-062).
    Cost(CostOverlay),
    /// Context strategy info pane (SPEC-092).
    Context(ContextOverlay),
}

impl Overlay {
    pub fn is_open(&self) -> bool {
        !matches!(self, Overlay::None)
    }

    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            Overlay::Fatal { .. }
                | Overlay::Approval(_)
                | Overlay::ModelPicker(_)
                | Overlay::ApprovalPicker(_)
                | Overlay::Skills(_)
                | Overlay::Mcp(_)
                | Overlay::Cost(_)
                | Overlay::Context(_)
        )
    }

    /// True if the overlay auto-dismisses on any key press (toast only).
    pub fn is_toast(&self) -> bool {
        matches!(self, Overlay::Toast { .. })
    }
}

/// Helper: center a fixed-size rect inside `area`.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}

/// Renderer for the overlay layer.
#[derive(Debug)]
pub struct OverlayWidget<'a> {
    overlay: &'a Overlay,
}

impl<'a> OverlayWidget<'a> {
    pub fn new(overlay: &'a Overlay) -> Self {
        Self { overlay }
    }
}

impl<'a> Widget for OverlayWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.overlay {
            Overlay::None => {}
            Overlay::Help => render_help(area, buf),
            Overlay::Toast { message, .. } => render_toast(message, area, buf),
            Overlay::Fatal { message, log_path } => {
                render_fatal(message, log_path.as_deref(), area, buf)
            }
            Overlay::Approval(a) => render_approval(a, area, buf),
            Overlay::ModelPicker(m) => render_model_picker(m, area, buf),
            Overlay::ApprovalPicker(p) => render_approval_picker(p, area, buf),
            Overlay::Skills(s) => render_skills(s, area, buf),
            Overlay::Mcp(m) => render_mcp(m, area, buf),
            Overlay::Cost(c) => render_cost(c, area, buf),
            Overlay::Context(c) => render_context(c, area, buf),
        }
    }
}

fn render_help(area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(60, 16, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" /help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(rect);
    block.render(rect, buf);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (cmd, desc) in crate::app::slash::help_entries() {
        lines.push(Line::from(vec![
            Span::styled(
                (*cmd).to_string(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled((*desc).to_string(), Style::default().fg(Color::White)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc / Enter to close",
        Style::default().fg(Color::DarkGray),
    )));
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn render_toast(message: &str, area: Rect, buf: &mut Buffer) {
    let width = ((message.chars().count() + 4).min(60) as u16).max(20);
    let rect = centered_rect(width, 3, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(rect);
    block.render(rect, buf);
    let line = Line::from(Span::styled(
        message.to_string(),
        Style::default().fg(Color::Yellow),
    ));
    Paragraph::new(line).render(inner, buf);
}

fn render_fatal(message: &str, log_path: Option<&std::path::Path>, area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(70, 10, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" fatal ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
    let inner = block.inner(rect);
    block.render(rect, buf);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        message.to_string(),
        Style::default().fg(Color::Red),
    )));
    lines.push(Line::from(""));
    if let Some(p) = log_path {
        lines.push(Line::from(vec![
            Span::styled("log: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                p.display().to_string(),
                Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press Enter to quit · Esc to dismiss",
        Style::default().fg(Color::DarkGray),
    )));
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn render_approval(a: &ApprovalOverlay, area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(64, 9, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" approve tool call? ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(rect);
    block.render(rect, buf);
    let category_label = match a.category {
        ToolCategory::Read => "read",
        ToolCategory::Write => "write",
        ToolCategory::Shell => "shell",
        ToolCategory::Mcp => "mcp",
        ToolCategory::Other => "other",
        ToolCategory::Unknown => "unknown",
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("tool: ", Style::default().fg(Color::DarkGray)),
            Span::styled(a.tool_name.clone(), Style::default().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled(
                format!("[{category_label}]"),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("args: ", Style::default().fg(Color::DarkGray)),
            Span::raw(a.args_preview.clone()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "y approve · n deny · a always · Esc dismiss",
            Style::default().fg(Color::Cyan),
        )),
    ];
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn render_model_picker(m: &ModelPickerOverlay, area: Rect, buf: &mut Buffer) {
    let height = if m.has_parameters() { 22 } else { 16 };
    let rect = centered_rect(64, height, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" /model ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(rect);
    block.render(rect, buf);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spacer = Line::from("");
    if m.loading {
        lines.push(Line::from(Span::styled(
            "loading models…",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(err) = &m.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        )));
    } else if m.models.is_empty() {
        lines.push(Line::from(Span::styled(
            "no models returned by sidecar",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let param_rows = if m.has_parameters() {
            m.param_row_count() + 2
        } else {
            0
        };
        let hint_rows = 2usize;
        let model_visible = (inner.height as usize)
            .saturating_sub(hint_rows + param_rows)
            .max(1);
        let cursor = m.cursor.min(m.models.len().saturating_sub(1));
        let start = (cursor + 1).saturating_sub(model_visible);
        let more_above = start > 0;
        let more_below = start + model_visible < m.models.len();
        for (i, model) in m
            .models
            .iter()
            .enumerate()
            .skip(start)
            .take(model_visible)
        {
            let selected = i == m.cursor;
            let focused = m.focus == ModelPickerFocus::Models && selected;
            let marker = if focused {
                "› "
            } else if selected {
                "• "
            } else {
                "  "
            };
            let label = model
                .display_name
                .as_deref()
                .unwrap_or(model.id.as_str());
            let name_style = if focused {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else if selected {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    marker.to_string(),
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                ),
                Span::styled(label.to_string(), name_style),
            ]));
        }
        if more_above || more_below {
            let mut hint = format!("{}/{}", cursor + 1, m.models.len());
            if more_above {
                hint.push_str(" · ↑ more");
            }
            if more_below {
                hint.push_str(" · ↓ more");
            }
            spacer = Line::from(Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            ));
        }

        if m.has_parameters() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "options",
                Style::default().fg(Color::DarkGray),
            )));
            if let Some(model) = m.selected_model() {
                for (idx, def) in model.parameters.iter().enumerate() {
                    let focused =
                        m.focus == ModelPickerFocus::Parameters && idx == m.param_cursor;
                    let marker = if focused { "› " } else { "  " };
                    let current = m
                        .param_values
                        .iter()
                        .find(|(id, _)| id == &def.id)
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    let value_label = def
                        .values
                        .iter()
                        .find(|v| v.value == current)
                        .and_then(|v| v.display_name.as_deref())
                        .unwrap_or(current);
                    let name = def.display_name.as_deref().unwrap_or(def.id.as_str());
                    let label_style = if focused {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            marker.to_string(),
                            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!("{name}: "), label_style),
                        Span::styled(
                            format!("‹ {value_label} ›"),
                            Style::default().fg(Color::Cyan),
                        ),
                    ]));
                }
            }
        }
    }
    lines.push(spacer);
    let hint = if m.has_parameters() {
        "↑/↓ navigate · ←/→ adjust · Tab options · Enter apply · Esc cancel"
    } else {
        "↑/↓ select · Enter apply · Esc cancel"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::Cyan),
    )));
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn render_approval_picker(p: &ApprovalPickerOverlay, area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(56, 10, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" /approval ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(rect);
    block.render(rect, buf);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let entries = [
        ("1", "suggest", "prompt for every tool call"),
        ("2", "auto-edit", "auto-approve reads; prompt writes/shell"),
        ("3", "full-auto", "auto-approve all (uses sandbox)"),
    ];
    for (idx, (n, name, desc)) in entries.iter().enumerate() {
        let selected = idx == p.cursor;
        let marker = if selected { "› " } else { "  " };
        let key = Span::styled(
            format!("{n} "),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        );
        let name_span = Span::styled(
            (*name).to_string(),
            if selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            },
        );
        let desc_span = Span::styled(
            format!("  {desc}"),
            Style::default().fg(Color::DarkGray),
        );
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(Color::Magenta)),
            key,
            name_span,
            desc_span,
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "1/2/3 · ↑/↓ + Enter · Esc cancel",
        Style::default().fg(Color::Cyan),
    )));
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn render_skills(s: &SkillsOverlay, area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(72, 18, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" /skills ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(rect);
    block.render(rect, buf);

    let mut lines: Vec<Line<'static>> = Vec::new();
    if s.loading {
        lines.push(Line::from(Span::styled(
            "loading skills…",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(err) = &s.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        )));
    } else if s.items.is_empty() {
        lines.push(Line::from(Span::styled(
            "no skills discovered under ~/.cursor/skills or repo",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, row) in s.items.iter().enumerate() {
            let selected = i == s.cursor;
            let marker = if selected { "› " } else { "  " };
            let box_ = if row.enabled { "[x] " } else { "[ ] " };
            let mut spans = vec![
                Span::styled(marker.to_string(), Style::default().fg(Color::Magenta)),
                Span::styled(
                    box_.to_string(),
                    Style::default().fg(if row.enabled { Color::Green } else { Color::DarkGray }),
                ),
                Span::styled(
                    row.skill.id.clone(),
                    if selected {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::raw("  —  "),
                Span::styled(
                    row.skill.name.clone(),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if selected && !row.skill.description.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    truncate(&row.skill.description, 40),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    if !s.warnings.is_empty() {
        lines.push(Line::from(""));
        for w in &s.warnings {
            lines.push(Line::from(Span::styled(
                format!("! {w}"),
                Style::default().fg(Color::Yellow),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Space toggle · Enter commit · Esc cancel",
        Style::default().fg(Color::Cyan),
    )));
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn render_mcp(m: &McpOverlay, area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(74, 20, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" /mcp ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(rect);
    block.render(rect, buf);

    let mut lines: Vec<Line<'static>> = Vec::new();
    if m.loading {
        lines.push(Line::from(Span::styled(
            "loading mcp servers…",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(err) = &m.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        )));
    } else if m.servers.is_empty() {
        lines.push(Line::from(Span::styled(
            "no mcp servers configured",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, srv) in m.servers.iter().enumerate() {
            let selected = i == m.cursor;
            let marker = if selected { "› " } else { "  " };
            let (status_color, status_label) = mcp_status_style(srv.status);
            let mut spans = vec![
                Span::styled(marker.to_string(), Style::default().fg(Color::Magenta)),
                Span::styled(
                    srv.id.clone(),
                    if selected {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::raw("  · "),
                Span::styled(
                    srv.transport.clone(),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  · "),
                Span::styled(status_label.to_string(), Style::default().fg(status_color)),
            ];
            if srv.enabled {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    "[enabled]",
                    Style::default().fg(Color::Green),
                ));
            } else {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    "[disabled]",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(spans));
            if m.expanded == Some(i) {
                if srv.tools.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (no tools reported)",
                        Style::default().fg(Color::DarkGray),
                    )));
                } else {
                    for t in &srv.tools {
                        let desc = if t.description.is_empty() {
                            String::new()
                        } else {
                            format!("  — {}", t.description)
                        };
                        lines.push(Line::from(vec![
                            Span::styled(
                                "    · ".to_string(),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(t.name.clone(), Style::default().fg(Color::Cyan)),
                            Span::styled(desc, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                }
                if let Some(err) = &srv.error {
                    lines.push(Line::from(Span::styled(
                        format!("    ! {err}"),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ select · →/Enter expand · Space toggle · Esc close",
        Style::default().fg(Color::Cyan),
    )));
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn render_cost(c: &CostOverlay, area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(78, 22, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" /cost ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(rect);
    block.render(rect, buf);

    // Vertical split: top pane = per-model aggregates, bottom = per-turn.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                  // header
            Constraint::Length(models_height(&c.snapshot)),
            Constraint::Length(1),                  // separator
            Constraint::Min(3),                     // per-turn list
            Constraint::Length(1),                  // hint
        ])
        .split(inner);

    // Header
    let header_line = Line::from(vec![
        Span::styled(
            "Cost / usage",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "   total in {}  ·  out {}  ·  total {}",
                fmt_num(c.snapshot.cumulative.input_tokens),
                fmt_num(c.snapshot.cumulative.output_tokens),
                fmt_num(c.snapshot.cumulative.total_tokens),
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    Paragraph::new(header_line).render(chunks[0], buf);

    // Top pane: per-model aggregates.
    let mut model_lines: Vec<Line<'static>> = Vec::new();
    if c.snapshot.by_model.is_empty() {
        model_lines.push(Line::from(Span::styled(
            "no completed turns yet",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for row in &c.snapshot.by_model {
            let model_label = if row.model.is_empty() {
                "(unknown)".to_string()
            } else {
                row.model.clone()
            };
            model_lines.push(Line::from(vec![
                Span::styled(
                    format!("[{model_label}]"),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(
                        "  in={}  out={}  total={}  ({} turns)",
                        fmt_num(row.input_tokens),
                        fmt_num(row.output_tokens),
                        fmt_num(row.total_tokens),
                        row.turns,
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }
    Paragraph::new(model_lines)
        .wrap(Wrap { trim: false })
        .render(chunks[1], buf);

    // Separator
    Paragraph::new(Line::from(Span::styled(
        "── per-turn ──────────────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    )))
    .render(chunks[2], buf);

    // Bottom pane: per-turn list, newest first, with scroll.
    let mut turn_lines: Vec<Line<'static>> = Vec::new();
    if c.snapshot.turns.is_empty() {
        turn_lines.push(Line::from(Span::styled(
            "no completed turns yet",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let mut turns: Vec<&crate::app::usage::TurnUsage> = c.snapshot.turns.iter().collect();
        turns.reverse();
        for t in turns.iter().skip(c.scroll) {
            let model_label = if t.model.is_empty() {
                "?".to_string()
            } else {
                t.model.clone()
            };
            turn_lines.push(Line::from(vec![
                Span::styled(
                    format!("#{:>3}  ", t.turn_index),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{model_label:<20}"),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(
                        "in={:>7}  out={:>7}  total={:>7}  cache={:>7}",
                        fmt_num(t.input_tokens),
                        fmt_num(t.output_tokens),
                        fmt_num(t.total_tokens),
                        fmt_num(t.cache_read_tokens),
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }
    Paragraph::new(turn_lines)
        .wrap(Wrap { trim: false })
        .render(chunks[3], buf);

    // Hint
    Paragraph::new(Line::from(Span::styled(
        "PgUp/PgDn scroll · Esc close",
        Style::default().fg(Color::Cyan),
    )))
    .render(chunks[4], buf);
}

fn models_height(snapshot: &UsageSnapshot) -> u16 {
    let n = snapshot.by_model.len().max(1) as u16;
    n.min(6)
}

fn fmt_num(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

fn render_context(c: &ContextOverlay, area: Rect, buf: &mut Buffer) {
    let rect = centered_rect(64, 12, area);
    Clear.render(rect, buf);
    let block = Block::default()
        .title(" /context ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(rect);
    block.render(rect, buf);

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled(
                "current: ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                c.current.label().to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];
    lines.push(Line::from(Span::styled(
        "Force a history strategy for subsequent turns:",
        Style::default().fg(Color::White),
    )));
    lines.push(Line::from(""));
    for (name, desc) in [
        (
            "auto",
            "sidecar decides based on byte budget (default)",
        ),
        (
            "raw",
            "always inject the last N turns verbatim",
        ),
        (
            "summary",
            "always inject an LLM-summarized rolling context",
        ),
    ] {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  /context strategy={name}"),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("  — {desc}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc close",
        Style::default().fg(Color::Cyan),
    )));
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(inner, buf);
}

fn mcp_status_style(status: McpServerStatus) -> (Color, &'static str) {
    match status {
        McpServerStatus::Ready => (Color::Green, "ready"),
        McpServerStatus::Starting => (Color::Yellow, "starting"),
        McpServerStatus::Failed => (Color::Red, "failed"),
        McpServerStatus::Disabled => (Color::DarkGray, "disabled"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cusa_rpc::{McpToolInfo, SkillSource};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(overlay: &Overlay, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(OverlayWidget::new(overlay), f.area());
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
    fn spec_002_help_overlay_lists_commands() {
        let out = render(&Overlay::Help, 80, 24);
        assert!(out.contains("/help"));
        assert!(out.contains("/clear"));
        assert!(out.contains("/reset"));
        assert!(out.contains("/quit"));
    }

    #[test]
    fn spec_002_toast_renders_message() {
        let overlay = Overlay::Toast {
            message: "not yet implemented".into(),
            created: Instant::now(),
        };
        let out = render(&overlay, 80, 5);
        assert!(out.contains("not yet implemented"), "got {out}");
    }

    #[test]
    fn spec_074_fatal_overlay_shows_log_path() {
        let overlay = Overlay::Fatal {
            message: "sidecar exited".into(),
            log_path: Some(PathBuf::from("/tmp/cusa.log")),
        };
        let out = render(&overlay, 80, 20);
        assert!(out.contains("fatal"), "no title: {out}");
        assert!(out.contains("sidecar exited"));
        assert!(out.contains("/tmp/cusa.log"));
    }

    #[test]
    fn spec_016_model_picker_snapshot_lists_models() {
        let overlay = Overlay::ModelPicker(ModelPickerOverlay::populated(vec![
            ModelInfo {
                id: "composer-2.5".into(),
                display_name: Some("Composer 2.5".into()),
                provider: None,
                supports_thinking: false,
                parameters: vec![],
            },
            ModelInfo {
                id: "claude-sonnet-4".into(),
                display_name: None,
                provider: None,
                supports_thinking: true,
                parameters: vec![],
            },
        ]));
        let out = render(&overlay, 80, 20);
        assert!(out.contains("Composer 2.5"));
        assert!(!out.contains("composer-2.5"));
        assert!(out.contains("claude-sonnet-4"));
        assert!(out.contains("↑/↓ select"));
    }

    #[test]
    fn spec_016_model_picker_scrolls_cursor_into_view_on_long_lists() {
        // 24 models: the 16-row overlay (14 inner − 2 hint rows = 12
        // visible) cannot fit them all. Before the scroll-window fix the
        // list rendered from the top and the cursor clipped off-screen.
        let models: Vec<ModelInfo> = (0..24)
            .map(|i| ModelInfo {
                id: format!("model-{i:02}"),
                display_name: None,
                provider: None,
                supports_thinking: false,
                parameters: vec![],
            })
            .collect();
        let mut picker = ModelPickerOverlay::populated(models);
        for _ in 0..23 {
            picker.move_down();
        }
        assert_eq!(picker.cursor, 23);
        let out = render(&Overlay::ModelPicker(picker), 80, 24);
        assert!(
            out.contains("› model-23"),
            "cursor row must be visible after scrolling to the bottom: {out}"
        );
        assert!(
            !out.contains("model-00"),
            "top of the list must scroll out of view: {out}"
        );
        assert!(out.contains("24/24"), "scroll indicator missing: {out}");
        assert!(out.contains("↑ more"), "up-scroll hint missing: {out}");
    }

    #[test]
    fn spec_016_model_picker_short_list_needs_no_scroll_indicator() {
        let models: Vec<ModelInfo> = (0..3)
            .map(|i| ModelInfo {
                id: format!("model-{i}"),
                display_name: None,
                provider: None,
                supports_thinking: false,
                parameters: vec![],
            })
            .collect();
        let out = render(&Overlay::ModelPicker(ModelPickerOverlay::populated(models)), 80, 24);
        assert!(out.contains("model-0"));
        assert!(out.contains("model-2"));
        assert!(!out.contains("more"), "no indicator for short lists: {out}");
    }

    #[test]
    fn spec_016_model_picker_renders_parameter_controls() {
        use cusa_rpc::{ModelParameterDefinition, ModelParameterValueOption};
        let overlay = Overlay::ModelPicker(ModelPickerOverlay::populated(vec![ModelInfo {
            id: "composer-2.5".into(),
            display_name: Some("Composer 2.5".into()),
            provider: None,
            supports_thinking: false,
            parameters: vec![
                ModelParameterDefinition {
                    id: "effort".into(),
                    display_name: Some("Effort".into()),
                    values: vec![
                        ModelParameterValueOption {
                            value: "low".into(),
                            display_name: Some("Low".into()),
                        },
                        ModelParameterValueOption {
                            value: "high".into(),
                            display_name: Some("High".into()),
                        },
                    ],
                },
                ModelParameterDefinition {
                    id: "fast".into(),
                    display_name: Some("Fast".into()),
                    values: vec![
                        ModelParameterValueOption {
                            value: "false".into(),
                            display_name: Some("Off".into()),
                        },
                        ModelParameterValueOption {
                            value: "true".into(),
                            display_name: Some("On".into()),
                        },
                    ],
                },
            ],
        }]));
        let out = render(&overlay, 80, 24);
        assert!(out.contains("options"));
        assert!(out.contains("Effort"));
        assert!(out.contains("Fast"));
        assert!(out.contains("←/→ adjust"));
    }

    #[test]
    fn spec_016_model_picker_builds_selection_with_params() {
        use cusa_rpc::{ModelParameterDefinition, ModelParameterValueOption};
        let mut picker = ModelPickerOverlay::populated(vec![ModelInfo {
            id: "composer-2.5".into(),
            display_name: Some("Composer 2.5".into()),
            provider: None,
            supports_thinking: false,
            parameters: vec![ModelParameterDefinition {
                id: "effort".into(),
                display_name: Some("Effort".into()),
                values: vec![
                    ModelParameterValueOption {
                        value: "low".into(),
                        display_name: None,
                    },
                    ModelParameterValueOption {
                        value: "high".into(),
                        display_name: None,
                    },
                ],
            }],
        }]);
        picker.focus_parameters();
        picker.cycle_param_value(1);
        let sel = picker.build_selection().expect("selection");
        assert_eq!(sel.id, "composer-2.5");
        assert_eq!(sel.params.len(), 1);
        assert_eq!(sel.params[0].id, "effort");
        assert_eq!(sel.params[0].value, "high");
    }

    #[test]
    fn spec_021_approval_picker_snapshot_lists_three_modes() {
        let overlay = Overlay::ApprovalPicker(ApprovalPickerOverlay::new(ApprovalMode::Suggest));
        let out = render(&overlay, 80, 12);
        assert!(out.contains("suggest"));
        assert!(out.contains("auto-edit"));
        assert!(out.contains("full-auto"));
    }

    #[test]
    fn spec_022_approval_overlay_renders_category_and_hints() {
        let overlay = Overlay::Approval(ApprovalOverlay {
            tool_name: "shell_exec".into(),
            args_preview: "{\"cmd\":\"ls\"}".into(),
            request_id: "req-1".into(),
            category: ToolCategory::Shell,
        });
        let out = render(&overlay, 80, 12);
        assert!(out.contains("shell_exec"), "tool name missing: {out}");
        assert!(out.contains("[shell]"), "category missing: {out}");
        assert!(out.contains("y approve"), "hint missing: {out}");
    }

    #[test]
    fn spec_032_skills_overlay_lists_toggles() {
        let mut skills = SkillsOverlay::loading();
        skills.populate(
            vec![
                SkillInfo {
                    id: "foo".into(),
                    name: "Foo".into(),
                    description: "does foo".into(),
                    path: "/tmp/foo/SKILL.md".into(),
                    size_bytes: 0,
                    source: SkillSource::User,
                },
                SkillInfo {
                    id: "bar".into(),
                    name: "Bar".into(),
                    description: "does bar".into(),
                    path: "/tmp/bar/SKILL.md".into(),
                    size_bytes: 0,
                    source: SkillSource::Project,
                },
            ],
            vec![],
            &["foo".to_string()],
        );
        let overlay = Overlay::Skills(skills);
        let out = render(&overlay, 80, 20);
        assert!(out.contains("[x]"), "enabled marker missing: {out}");
        assert!(out.contains("[ ]"), "disabled marker missing: {out}");
        assert!(out.contains("foo"));
        assert!(out.contains("bar"));
        assert!(out.contains("Space toggle"));
    }

    #[test]
    fn spec_042_mcp_overlay_renders_servers_with_transport_and_status() {
        let mut mcp = McpOverlay::loading();
        mcp.populate(vec![
            McpServerInfo {
                id: "fs".into(),
                transport: "stdio".into(),
                status: McpServerStatus::Ready,
                tools: vec![McpToolInfo {
                    name: "fs_read".into(),
                    description: "read files".into(),
                }],
                enabled: true,
                error: None,
            },
            McpServerInfo {
                id: "web".into(),
                transport: "http".into(),
                status: McpServerStatus::Failed,
                tools: vec![],
                enabled: false,
                error: Some("bad token".into()),
            },
        ]);
        // expand the first server
        mcp.expanded = Some(0);
        let overlay = Overlay::Mcp(mcp);
        let out = render(&overlay, 80, 20);
        assert!(out.contains("fs"));
        assert!(out.contains("stdio"));
        assert!(out.contains("ready"));
        assert!(out.contains("web"));
        assert!(out.contains("failed"));
        assert!(out.contains("fs_read"), "expanded tool missing: {out}");
    }

    #[test]
    fn spec_042_mcp_expanded_row_shows_tool_list() {
        let mut mcp = McpOverlay::loading();
        mcp.populate(vec![McpServerInfo {
            id: "fs".into(),
            transport: "stdio".into(),
            status: McpServerStatus::Ready,
            tools: vec![
                McpToolInfo {
                    name: "read".into(),
                    description: "read file".into(),
                },
                McpToolInfo {
                    name: "write".into(),
                    description: "write file".into(),
                },
            ],
            enabled: true,
            error: None,
        }]);
        assert_eq!(mcp.expanded, None);
        mcp.toggle_cursor_expansion();
        assert_eq!(mcp.expanded, Some(0));
        let overlay = Overlay::Mcp(mcp);
        let out = render(&overlay, 80, 20);
        assert!(out.contains("read"));
        assert!(out.contains("write"));
    }
}
