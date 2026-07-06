// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Maps `AppState` into vendored status chrome (SPEC-109).

use crate::app::state::AppState;
use crate::codex_adapter::types::ApprovalModeView;
use crate::codex_adapter::CusaViewModel;
use crate::codex_ui::status_chrome::{render_header_line, render_status_line, StatusSegment, StatusSegmentKind};
use ratatui::text::Line;

const HEADER_CWD_MAX: usize = 48;

/// Header row (row 0) with magenta `cusa` branding.
pub fn header_line(state: &AppState) -> Line<'static> {
    render_header_line(
        &state.session.short_id(),
        &state.session.cwd,
        HEADER_CWD_MAX,
    )
}

/// Status row (row 1) with Codex-style accent segments.
pub fn status_line(state: &AppState) -> Line<'static> {
    let mut segments = vec![StatusSegment {
        accent: StatusSegmentKind::Model,
        text: state.session.model.clone(),
    }];

    if state.session.manual_model_override.is_some() {
        segments.push(StatusSegment {
            accent: StatusSegmentKind::Mode,
            text: "[override]".into(),
        });
    }

    let mode = CusaViewModel::map_approval_mode(state.session.approval_mode);
    segments.push(StatusSegment {
        accent: StatusSegmentKind::Mode,
        text: mode.label().to_string(),
    });
    segments.push(StatusSegment {
        accent: StatusSegmentKind::Count,
        text: format!("skills({})", state.session.skills_count),
    });
    segments.push(StatusSegment {
        accent: StatusSegmentKind::Count,
        text: format!("mcp({})", state.session.mcp_count),
    });
    segments.push(StatusSegment {
        accent: StatusSegmentKind::Usage,
        text: state.usage.snapshot().status_line(),
    });
    segments.push(StatusSegment {
        accent: StatusSegmentKind::Meta,
        text: format!("sidecar:{}", state.session.sidecar_status.label()),
    });

    render_status_line(&segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn spec_109_header_shows_cusa_not_codex() {
        let state = AppState::new("/tmp/project".into());
        let text = line_text(&header_line(&state));
        assert!(text.contains("cusa"), "{text}");
        assert!(!text.contains("Codex"), "{text}");
    }

    #[test]
    fn spec_109_status_line_includes_model_and_counts() {
        let state = AppState::new("/tmp".into());
        let text = line_text(&status_line(&state));
        assert!(text.contains("auto"), "{text}");
        assert!(text.contains("skills(0)"), "{text}");
        assert!(text.contains("mcp(0)"), "{text}");
        assert!(text.contains("tokens"), "{text}");
    }
}
