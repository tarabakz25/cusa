// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Codex-style bottom pane: tinted composer + status row (ChatComposer layout).

use crate::app::state::AppState;
use crate::codex_adapter::composer::ComposerWidget;
use crate::codex_adapter::welcome::composer_footer_line;
use crate::codex_ui::ui_consts::FOOTER_INDENT_COLS;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Paragraph, Widget};

const FOOTER_ROW_HEIGHT: u16 = 1;

/// Bottom pane matching Codex `ChatComposer` + footer status row.
#[derive(Debug, Clone)]
pub struct BottomPaneWidget {
    composer: ComposerWidget,
    footer: ratatui::text::Line<'static>,
}

impl BottomPaneWidget {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            composer: ComposerWidget::from_state(state),
            footer: composer_footer_line(&state.session),
        }
    }

    pub fn desired_height(state: &AppState, width: u16) -> u16 {
        ComposerWidget::desired_height_for_state(state, width) + FOOTER_ROW_HEIGHT
    }
}

impl Widget for BottomPaneWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let [composer_rect, footer_rect] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(FOOTER_ROW_HEIGHT.min(area.height)),
            ])
            .areas(area);

        self.composer.render_composer_surface(composer_rect, buf);

        if footer_rect.height > 0 && footer_rect.width > FOOTER_INDENT_COLS as u16 {
            let indented = Rect {
                x: footer_rect.x.saturating_add(FOOTER_INDENT_COLS as u16),
                y: footer_rect.y,
                width: footer_rect.width.saturating_sub(FOOTER_INDENT_COLS as u16),
                height: footer_rect.height,
            };
            Paragraph::new(self.footer).render(indented, buf);
        }
    }
}
