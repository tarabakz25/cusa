// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Cursor API key discovery and persistence (SPEC-100, SPEC-101).
//
// Mirrors the sidecar's `readApiKey` / npm `login` conventions so the TUI
// can prompt for a key at first launch and write `~/.cusa/config.toml`
// before the sidecar handshake.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use super::config_path;

const KEY_ENV_VAR: &str = "CURSOR_API_KEY";

/// True when `CURSOR_API_KEY` is set or `config.toml` contains `api_key`.
pub fn api_key_configured() -> bool {
    resolve_api_key().is_some()
}

/// Read the key from env (preferred) or config file.
pub fn resolve_api_key() -> Option<String> {
    if let Ok(v) = std::env::var(KEY_ENV_VAR) {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    read_api_key_from_file(&config_path()).ok().flatten()
}

/// Parse `api_key = "…"` from TOML text. Matches the sidecar parser: any
/// non-comment, non-section-header line with `api_key`.
pub fn parse_api_key_from_toml(text: &str) -> Option<String> {
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some(key) = parse_api_key_line(line) {
            return Some(key);
        }
    }
    None
}

fn parse_api_key_line(line: &str) -> Option<String> {
    let line = line.split('#').next()?.trim();
    let rest = line.strip_prefix("api_key")?.trim_start();
    if !rest.starts_with('=') {
        return None;
    }
    let value = rest[1..].trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))?;
    let key = unquoted.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn read_api_key_from_file(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(parse_api_key_from_toml(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

/// Write or update `api_key` inside the `[api]` section (SPEC-101).
pub fn write_api_key(api_key: &str) -> Result<()> {
    let key = api_key.trim();
    if key.is_empty() {
        anyhow::bail!("refusing to write an empty API key");
    }

    let home = super::cusa_home();
    create_dir_all(&home).with_context(|| format!("create {}", home.display()))?;

    let path = config_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let contents = render_config(&existing, key);

    write_config_atomically(&path, &contents)?;
    Ok(())
}

fn render_config(current: &str, api_key: &str) -> String {
    let value = toml_string(api_key);
    if current.trim().is_empty() {
        return format!("[api]\napi_key = {value}\n");
    }

    let mut lines: Vec<String> = current.lines().map(String::from).collect();
    let section_start = find_section_start(&lines, "api");

    if section_start == -1 {
        let mut out = current.to_string();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
        out.push_str("[api]\n");
        out.push_str(&format!("api_key = {value}\n"));
        return out;
    }

    let section_end = find_section_end(&lines, section_start as usize);
    let mut replaced = false;
    for i in (section_start as usize + 1)..section_end {
        let trimmed = lines[i].trim_start();
        if trimmed.starts_with("api_key") {
            let leading = lines[i].len() - trimmed.len();
            lines[i] = format!("{}api_key = {value}", &lines[i][..leading]);
            replaced = true;
            break;
        }
    }
    if !replaced {
        lines.insert(section_end, format!("api_key = {value}"));
    }

    let mut result = lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn find_section_start(lines: &[String], name: &str) -> i64 {
    let target = format!("[{name}]");
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == target {
            return i as i64;
        }
    }
    -1
}

fn find_section_end(lines: &[String], start: usize) -> usize {
    for i in (start + 1)..lines.len() {
        let t = lines[i].trim();
        if t.starts_with('[') && t.ends_with(']') {
            return i;
        }
    }
    lines.len()
}

fn toml_string(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

#[cfg(unix)]
fn write_config_atomically(path: &Path, contents: &str) -> Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_config_atomically(path: &Path, contents: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_100_parse_api_key_from_toml_tolerates_sections_and_comments() {
        let text = "# top comment\n[api]\napi_key = \"sk_abc\" # inline\nother = 42\n";
        assert_eq!(parse_api_key_from_toml(text).as_deref(), Some("sk_abc"));
    }

    #[test]
    fn spec_101_render_config_creates_api_section() {
        assert_eq!(
            render_config("", "cursor_test"),
            "[api]\napi_key = \"cursor_test\"\n"
        );
    }

    #[test]
    fn spec_101_render_config_updates_existing_api_key() {
        let current = "[api]\napi_key = \"old\"\n";
        let next = render_config(current, "new_key");
        assert!(next.contains("api_key = \"new_key\""));
        assert!(!next.contains("old"));
    }

    #[test]
    fn spec_101_render_config_appends_api_section_when_missing() {
        let current = "[router]\ndefault = \"auto\"\n";
        let next = render_config(current, "k");
        assert!(next.contains("[router]"));
        assert!(next.contains("[api]"));
        assert!(next.contains("api_key = \"k\""));
    }

    #[test]
    fn spec_101_write_api_key_persists_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("cusa-api-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        write_config_atomically(&path, &render_config("", "cursor_persist")).unwrap();
        let read = read_api_key_from_file(&path).unwrap();
        assert_eq!(read.as_deref(), Some("cursor_persist"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
