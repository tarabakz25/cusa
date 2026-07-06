// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//
// Render-only exec cell model (SPEC-108).

use ratatui::style::Stylize;
use ratatui::text::Line;

#[derive(Clone, Debug, Default)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub aggregated_output: String,
}

#[derive(Clone, Debug)]
pub struct ExecCall {
    pub command: Vec<String>,
    pub output: Option<CommandOutput>,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct ExecCell {
    pub call: ExecCall,
}

impl ExecCell {
    pub fn new(command: Vec<String>, active: bool) -> Self {
        Self {
            call: ExecCall {
                command,
                output: None,
                active,
            },
        }
    }

    pub fn with_output(mut self, exit_code: i32, aggregated_output: String) -> Self {
        self.call.output = Some(CommandOutput {
            exit_code,
            aggregated_output,
        });
        self.call.active = false;
        self
    }
}

pub const TOOL_CALL_MAX_LINES: usize = 5;

pub struct OutputLinesParams {
    pub line_limit: usize,
    pub only_err: bool,
    pub include_angle_pipe: bool,
    pub include_prefix: bool,
}

pub struct OutputLines {
    pub lines: Vec<ratatui::text::Line<'static>>,
    pub omitted: Option<usize>,
}

pub fn output_lines(
    output: Option<&CommandOutput>,
    params: OutputLinesParams,
) -> OutputLines {
    let OutputLinesParams {
        line_limit,
        only_err,
        include_angle_pipe,
        include_prefix,
    } = params;
    let Some(output) = output else {
        return OutputLines {
            lines: Vec::new(),
            omitted: None,
        };
    };
    if only_err && output.exit_code == 0 {
        return OutputLines {
            lines: Vec::new(),
            omitted: None,
        };
    }

    let src = &output.aggregated_output;
    let raw_lines: Vec<&str> = src.lines().collect();
    let total = raw_lines.len();
    let head_end = total.min(line_limit);
    let mut out = Vec::new();

    for (i, raw) in raw_lines[..head_end].iter().enumerate() {
        let prefix = if !include_prefix {
            ""
        } else if i == 0 && include_angle_pipe {
            "  └ "
        } else {
            "    "
        };
        out.push(ratatui::text::Line::from(vec![
            prefix.into(),
            ratatui::style::Stylize::dim(raw.to_string()),
        ]));
    }

    let show_ellipsis = total > 2 * line_limit;
    let omitted = if show_ellipsis {
        Some(total - 2 * line_limit)
    } else {
        None
    };
    if show_ellipsis {
        let omitted_count = total - 2 * line_limit;
        out.push(ratatui::text::Line::from(format!(
            "… +{omitted_count} lines"
        ).dim()));
    }

    let tail_start = if show_ellipsis {
        total - line_limit
    } else {
        head_end
    };
    for raw in raw_lines[tail_start..].iter() {
        let prefix = if include_prefix { "    " } else { "" };
        out.push(ratatui::text::Line::from(vec![
            prefix.into(),
            ratatui::style::Stylize::dim(raw.to_string()),
        ]));
    }

    OutputLines { lines: out, omitted }
}
