// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//
// Render-only diff summary blocks (SPEC-108). Syntax highlighting omitted;
// structure and colors match upstream `diff_render` for transcript tool cells.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diffy::Patch;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use super::diff_model::FileChange;
use super::exec_command::relativize_to_home;
use super::render::line_utils::prefix_lines;

#[derive(Clone, Copy)]
enum DiffLineType {
    Insert,
    Delete,
    Context,
}

struct Row {
    path: PathBuf,
    move_path: Option<PathBuf>,
    added: usize,
    removed: usize,
    change: FileChange,
}

/// Render a file-change map into Codex-style diff summary lines.
pub fn create_diff_summary(
    changes: &HashMap<PathBuf, FileChange>,
    cwd: &Path,
    wrap_cols: usize,
) -> Vec<Line<'static>> {
    let rows = collect_rows(changes);
    render_changes_block(rows, wrap_cols, cwd)
}

pub fn display_path_for(path: &Path, cwd: &Path) -> String {
    if path.is_relative() {
        return path.display().to_string();
    }
    if let Ok(stripped) = path.strip_prefix(cwd) {
        return stripped.display().to_string();
    }
    if let Some(rel) = relativize_to_home(path) {
        if rel.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{rel}", rel = rel.display());
    }
    path.display().to_string()
}

pub fn calculate_add_remove_from_diff(diff: &str) -> (usize, usize) {
    if let Ok(patch) = Patch::from_str(diff) {
        let mut added = 0;
        let mut removed = 0;
        for hunk in patch.hunks() {
            for line in hunk.lines() {
                match line {
                    diffy::Line::Insert(_) => added += 1,
                    diffy::Line::Delete(_) => removed += 1,
                    diffy::Line::Context(_) => {}
                }
            }
        }
        (added, removed)
    } else {
        (0, 0)
    }
}

fn collect_rows(changes: &HashMap<PathBuf, FileChange>) -> Vec<Row> {
    let mut rows: Vec<Row> = changes
        .iter()
        .map(|(path, change)| {
            let (added, removed) = match change {
                FileChange::Add { content } => (content.lines().count(), 0),
                FileChange::Delete { content } => (0, content.lines().count()),
                FileChange::Update { unified_diff, .. } => {
                    calculate_add_remove_from_diff(unified_diff)
                }
            };
            let move_path = match change {
                FileChange::Update {
                    move_path: Some(new),
                    ..
                } => Some(new.clone()),
                _ => None,
            };
            Row {
                path: path.clone(),
                move_path,
                added,
                removed,
                change: change.clone(),
            }
        })
        .collect();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

fn render_line_count_summary(added: usize, removed: usize) -> Vec<Span<'static>> {
    vec![
        "(".into(),
        format!("+{added}").green(),
        " ".into(),
        format!("-{removed}").red(),
        ")".into(),
    ]
}

fn render_changes_block(rows: Vec<Row>, wrap_cols: usize, cwd: &Path) -> Vec<Line<'static>> {
    if rows.is_empty() {
        return Vec::new();
    }

    let render_path = |row: &Row| -> Vec<Span<'static>> {
        let mut spans = vec![display_path_for(&row.path, cwd).into()];
        if let Some(move_path) = &row.move_path {
            spans.push(format!(" → {}", display_path_for(move_path, cwd)).into());
        }
        spans
    };

    let total_added: usize = rows.iter().map(|r| r.added).sum();
    let total_removed: usize = rows.iter().map(|r| r.removed).sum();
    let file_count = rows.len();
    let noun = if file_count == 1 { "file" } else { "files" };

    let mut header_spans: Vec<Span<'static>> = vec!["• ".dim()];
    if let [row] = &rows[..] {
        let verb = match &row.change {
            FileChange::Add { .. } => "Added",
            FileChange::Delete { .. } => "Deleted",
            _ => "Edited",
        };
        header_spans.push(verb.bold());
        header_spans.push(" ".into());
        header_spans.extend(render_path(row));
        header_spans.push(" ".into());
        header_spans.extend(render_line_count_summary(row.added, row.removed));
    } else {
        header_spans.push("Edited".bold());
        header_spans.push(format!(" {file_count} {noun} ").into());
        header_spans.extend(render_line_count_summary(total_added, total_removed));
    }

    let mut out = vec![Line::from(header_spans)];

    for (idx, row) in rows.into_iter().enumerate() {
        if idx > 0 {
            out.push(Line::from(""));
        }
        if file_count > 1 {
            let mut header: Vec<Span<'static>> = vec!["  └ ".dim()];
            header.extend(render_path(&row));
            header.push(" ".into());
            header.extend(render_line_count_summary(row.added, row.removed));
            out.push(Line::from(header));
        }

        let mut body = Vec::new();
        render_change(&row.change, &mut body, wrap_cols.saturating_sub(4));
        out.extend(prefix_lines(body, "    ".into(), "    ".into()));
    }

    out
}

fn render_change(change: &FileChange, out: &mut Vec<Line<'static>>, width: usize) {
    match change {
        FileChange::Add { content } => {
            for (i, raw) in content.lines().enumerate() {
                out.extend(diff_line(i + 1, DiffLineType::Insert, raw, width));
            }
        }
        FileChange::Delete { content } => {
            for (i, raw) in content.lines().enumerate() {
                out.extend(diff_line(i + 1, DiffLineType::Delete, raw, width));
            }
        }
        FileChange::Update { unified_diff, .. } => {
            if let Ok(patch) = Patch::from_str(unified_diff) {
                let mut old_ln = 0usize;
                let mut new_ln = 0usize;
                for hunk in patch.hunks() {
                    old_ln = hunk.old_range().start();
                    new_ln = hunk.new_range().start();
                    for line in hunk.lines() {
                        match line {
                            diffy::Line::Insert(text) => {
                                out.extend(diff_line(new_ln, DiffLineType::Insert, text, width));
                                new_ln += 1;
                            }
                            diffy::Line::Delete(text) => {
                                out.extend(diff_line(old_ln, DiffLineType::Delete, text, width));
                                old_ln += 1;
                            }
                            diffy::Line::Context(text) => {
                                out.extend(diff_line(new_ln, DiffLineType::Context, text, width));
                                old_ln += 1;
                                new_ln += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn diff_line(
    line_no: usize,
    kind: DiffLineType,
    text: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let gutter = format!("{line_no:>4} ");
    let sign = match kind {
        DiffLineType::Insert => "+",
        DiffLineType::Delete => "-",
        DiffLineType::Context => " ",
    };
    let content_style = match kind {
        DiffLineType::Insert => Span::raw(text.to_string()).green(),
        DiffLineType::Delete => Span::raw(text.to_string()).red().dim(),
        DiffLineType::Context => Span::raw(text.to_string()),
    };
    let prefix = format!("{gutter}{sign}");
    let prefix_width = prefix.chars().count();
    let max_content = width.saturating_sub(prefix_width).max(1);

    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut offset = 0usize;
    let mut first = true;
    while offset < chars.len() {
        let end = (offset + max_content).min(chars.len());
        let chunk: String = chars[offset..end].iter().collect();
        offset = end;

        let line_prefix = if first {
            prefix.clone()
        } else {
            "     ".to_string()
        };
        let chunk_style = match kind {
            DiffLineType::Insert => Span::raw(chunk).green(),
            DiffLineType::Delete => Span::raw(chunk).red().dim(),
            DiffLineType::Context => Span::raw(chunk),
        };
        lines.push(Line::from(vec![line_prefix.dim(), chunk_style]));
        first = false;
    }
    if lines.is_empty() {
        lines.push(Line::from(vec![prefix.dim(), content_style]));
    }
    lines
}

#[cfg(all(test, feature = "vendor-tests"))]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn line_text(lines: &[Line<'static>]) -> String {
        lines
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
    fn create_diff_summary_single_file_add() {
        let mut changes = HashMap::new();
        changes.insert(
            PathBuf::from("src/main.rs"),
            FileChange::Add {
                content: "fn main() {}\n".into(),
            },
        );
        let lines = create_diff_summary(&changes, Path::new("/repo"), 80);
        let text = line_text(&lines);
        assert!(text.contains("Added"));
        assert!(text.contains("src/main.rs"));
        assert!(text.contains("+1"));
    }
}
