// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//
// Render-only exec cell display (SPEC-108).

use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use super::super::exec_command::strip_bash_lc_and_escape;
use super::super::motion::{activity_indicator, MotionMode, ReducedMotionIndicator};
use super::super::render::line_utils::prefix_lines;
use super::model::{output_lines, CommandOutput, ExecCell, OutputLinesParams, TOOL_CALL_MAX_LINES};

pub fn render_exec_cell(cell: &ExecCell, width: u16) -> Vec<Line<'static>> {
    let call = &cell.call;
    let success = call.output.as_ref().map(|o| o.exit_code == 0);
    let bullet = match success {
        Some(true) => Span::from("•").green().bold(),
        Some(false) => Span::from("•").red().bold(),
        None => activity_indicator(
            None,
            MotionMode::Reduced,
            ReducedMotionIndicator::StaticBullet,
        )
        .unwrap_or_else(|| "•".dim()),
    };

    let title = if call.active {
        "Running"
    } else {
        "Ran"
    };

    let cmd_display = strip_bash_lc_and_escape(&call.command);
    let header = Line::from(vec![
        bullet,
        " ".into(),
        title.bold(),
        " ".into(),
        cmd_display.into(),
    ]);

    let mut lines = vec![truncate_line_to_width(header, width)];

    if let Some(output) = call.output.as_ref() {
        let raw = output_lines(
            Some(output),
            OutputLinesParams {
                line_limit: TOOL_CALL_MAX_LINES,
                only_err: false,
                include_angle_pipe: false,
                include_prefix: false,
            },
        );
        if raw.lines.is_empty() {
            lines.extend(prefix_lines(
                vec![Line::from("(no output)".dim())],
                "  └ ".dim(),
                "    ".into(),
            ));
        } else {
            lines.extend(prefix_lines(
                raw.lines,
                "  └ ".dim(),
                "    ".into(),
            ));
        }
    } else if !call.active {
        lines.extend(prefix_lines(
            vec![Line::from("(no output)".dim())],
            "  └ ".dim(),
            "    ".into(),
        ));
    }

    lines
}

fn truncate_line_to_width(mut line: Line<'static>, width: u16) -> Line<'static> {
    if width == 0 {
        return Line::from("");
    }
    let mut used = 0usize;
    let max = width as usize;
    let mut out_spans = Vec::new();
    for span in line.spans.drain(..) {
        let text = span.content.into_owned();
        let len = text.chars().count();
        if used + len <= max {
            used += len;
            out_spans.push(Span::styled(text, span.style));
        } else if used < max {
            let take = max.saturating_sub(used);
            let truncated: String = text.chars().take(take).collect();
            out_spans.push(Span::styled(truncated, span.style));
            break;
        } else {
            break;
        }
    }
    Line::from(out_spans).style(line.style)
}

#[cfg(all(test, feature = "vendor-tests"))]
mod tests {
    use super::*;
    use super::super::model::ExecCell;

    fn render_text(cell: &ExecCell, width: u16) -> String {
        render_exec_cell(cell, width)
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn render_exec_cell_compact_success() {
        let cell = ExecCell::new(vec!["echo".into(), "ok".into()], false)
            .with_output(0, String::new());
        let text = render_text(&cell, 80);
        assert!(text.contains("Ran"));
        assert!(text.contains("echo ok"));
        assert!(text.contains("(no output)"));
    }
}
