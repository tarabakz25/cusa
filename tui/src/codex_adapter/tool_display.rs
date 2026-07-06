// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Maps cusa tool transcript entries into vendored diff/exec render paths (SPEC-108).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::app::transcript::TranscriptEntry;
use crate::codex_adapter::types::{FileChangeView, ToolBlockView};
use crate::codex_ui::diff_model::FileChange;
use crate::codex_ui::diff_render::create_diff_summary;
use crate::codex_ui::exec_cell::{ExecCell, render_exec_cell};
use crate::codex_ui::exec_command::split_command_string;
use ratatui::text::Line;

/// Classify and render a tool-related transcript entry through vendored widgets.
pub fn render_tool_entry(entry: &TranscriptEntry, cwd: &Path, width: u16) -> Option<Vec<Line<'static>>> {
    let view = tool_block_view(entry)?;
    Some(render_tool_block(&view, cwd, width))
}

pub fn tool_block_view(entry: &TranscriptEntry) -> Option<ToolBlockView> {
    match entry {
        TranscriptEntry::ToolCall { name, args_preview } => Some(classify_tool_call(name, args_preview)),
        TranscriptEntry::ToolResult { name, ok, preview } => {
            Some(classify_tool_result(name, *ok, preview))
        }
        _ => None,
    }
}

pub fn render_tool_block(view: &ToolBlockView, cwd: &Path, width: u16) -> Vec<Line<'static>> {
    match view {
        ToolBlockView::Shell {
            command,
            output,
            exit_code,
            active,
        } => {
            let argv = split_command_string(command);
            let mut cell = ExecCell::new(argv, *active);
            if let Some(code) = exit_code {
                cell = cell.with_output(*code, output.clone().unwrap_or_default());
            }
            render_exec_cell(&cell, width)
        }
        ToolBlockView::Diff { changes } => {
            let mapped = changes
                .iter()
                .map(|(path, change)| (path.clone(), map_file_change(change)))
                .collect::<HashMap<_, _>>();
            create_diff_summary(&mapped, cwd, width as usize)
        }
        ToolBlockView::GenericCall { name, args_preview } => vec![Line::from(format!(
            "⚙ {name} {args_preview}"
        ))],
        ToolBlockView::GenericResult { name, ok, preview } => {
            let symbol = if *ok { "✓" } else { "✗" };
            vec![Line::from(format!("{symbol} {name} {preview}"))]
        }
    }
}

fn classify_tool_call(name: &str, args_preview: &str) -> ToolBlockView {
    let lower = name.to_ascii_lowercase();
    if is_shell_tool(&lower) {
        if let Some(command) = extract_command_arg(args_preview) {
            return ToolBlockView::Shell {
                command,
                output: None,
                exit_code: None,
                active: true,
            };
        }
    }
    if is_patch_tool(&lower) {
        if let Some(changes) = parse_file_changes(args_preview) {
            return ToolBlockView::Diff { changes };
        }
    }
    ToolBlockView::GenericCall {
        name: name.to_string(),
        args_preview: args_preview.to_string(),
    }
}

fn classify_tool_result(name: &str, ok: bool, preview: &str) -> ToolBlockView {
    let lower = name.to_ascii_lowercase();
    if is_shell_tool(&lower) || name.starts_with("tool#") {
        let exit_code = if ok { Some(0) } else { Some(1) };
        let command = extract_command_arg(preview).unwrap_or_else(|| preview.to_string());
        let output = if extract_command_arg(preview).is_some() {
            String::new()
        } else {
            preview.to_string()
        };
        return ToolBlockView::Shell {
            command,
            output: Some(output),
            exit_code,
            active: false,
        };
    }
    if is_patch_tool(&lower) {
        if let Some(changes) = parse_file_changes(preview) {
            return ToolBlockView::Diff { changes };
        }
    }
    ToolBlockView::GenericResult {
        name: name.to_string(),
        ok,
        preview: preview.to_string(),
    }
}

fn is_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "shell"
            | "shell_exec"
            | "bash"
            | "run_terminal_cmd"
            | "terminal"
            | "exec"
    )
}

fn is_patch_tool(name: &str) -> bool {
    matches!(
        name,
        "apply_patch"
            | "patch"
            | "edit_file"
            | "write"
            | "create_file"
            | "str_replace"
    )
}

fn extract_command_arg(args_preview: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args_preview) {
        for key in ["cmd", "command", "script"] {
            if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                return Some(s.to_string());
            }
        }
    }
    if !args_preview.is_empty() {
        return Some(args_preview.to_string());
    }
    None
}

fn parse_file_changes(text: &str) -> Option<HashMap<PathBuf, FileChangeView>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    let mut out = HashMap::new();
    for (path, change) in obj {
        let path = PathBuf::from(path);
        if let Some(content) = change.get("content").and_then(|c| c.as_str()) {
            out.insert(
                path,
                FileChangeView::Add {
                    content: content.to_string(),
                },
            );
            continue;
        }
        if let Some(diff) = change.get("unified_diff").and_then(|d| d.as_str()) {
            out.insert(
                path,
                FileChangeView::Update {
                    unified_diff: diff.to_string(),
                    move_path: change
                        .get("move_path")
                        .and_then(|m| m.as_str())
                        .map(PathBuf::from),
                },
            );
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn map_file_change(view: &FileChangeView) -> FileChange {
    match view {
        FileChangeView::Add { content } => FileChange::Add {
            content: content.clone(),
        },
        FileChangeView::Delete { content } => FileChange::Delete {
            content: content.clone(),
        },
        FileChangeView::Update {
            unified_diff,
            move_path,
        } => FileChange::Update {
            unified_diff: unified_diff.clone(),
            move_path: move_path.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::transcript::TranscriptEntry;

    fn render(entry: &TranscriptEntry) -> String {
        render_tool_entry(entry, Path::new("/repo"), 80)
            .unwrap_or_default()
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
    fn spec_108_shell_exec_block_renders_ran_line() {
        let entry = TranscriptEntry::ToolCall {
            name: "shell_exec".into(),
            args_preview: r#"{"cmd":"echo ok"}"#.into(),
        };
        let text = render(&entry);
        assert!(text.contains("Running") || text.contains("Ran"), "{text}");
        assert!(text.contains("echo ok"), "{text}");
    }

    #[test]
    fn spec_108_diff_block_renders_edited_header() {
        let entry = TranscriptEntry::ToolResult {
            name: "apply_patch".into(),
            ok: true,
            preview: r#"{"src/main.rs":{"content":"fn main() {}\n"}}"#.into(),
        };
        let text = render(&entry);
        assert!(text.contains("Added") || text.contains("Edited"), "{text}");
        assert!(text.contains("src/main.rs"), "{text}");
    }
}
