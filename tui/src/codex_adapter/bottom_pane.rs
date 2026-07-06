// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Codex-style bottom pane: multi-line composer + inline status row beneath.

use crate::app::state::AppState;
use crate::codex_adapter::composer::ComposerWidget;
use crate::codex_adapter::welcome::composer_footer_line;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Paragraph, Widget};

const COMPOSER_STATUS_HEIGHT: u16 = 1;

/// Bottom pane matching Codex layout: composer on top, model/cwd status below.
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
        ComposerWidget::desired_height_for_state(state, width) + COMPOSER_STATUS_HEIGHT
    }
}

impl Widget for BottomPaneWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(COMPOSER_STATUS_HEIGHT.min(area.height)),
            ])
            .split(area);
        self.composer.render(chunks[0], buf);
        if chunks[1].height > 0 {
            Paragraph::new(self.footer).render(chunks[1], buf);
        }
    }
}
