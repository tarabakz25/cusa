// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//
// Render-only shell command helpers (SPEC-108). Decoupled from `codex_shell_command`.

use std::path::Path;
use std::path::PathBuf;

/// Join argv with POSIX shell quoting.
pub fn escape_command(command: &[String]) -> String {
    shlex::try_join(command.iter().map(String::as_str)).unwrap_or_else(|_| command.join(" "))
}

/// Strip common `bash -lc` / `zsh -lc` wrappers and return the inner script.
pub fn strip_bash_lc_and_escape(command: &[String]) -> String {
    if let Some(script) = extract_shell_script(command) {
        return script;
    }
    escape_command(command)
}

fn extract_shell_script(command: &[String]) -> Option<String> {
    if command.len() < 3 {
        return None;
    }
    let shell = command[0].as_str();
    let flag = command[1].as_str();
    if flag != "-lc" && flag != "-c" {
        return None;
    }
    let is_shell = shell.ends_with("bash")
        || shell.ends_with("zsh")
        || shell.ends_with("sh")
        || shell == "bash"
        || shell == "zsh"
        || shell == "sh";
    if is_shell {
        Some(command[2..].join(" "))
    } else {
        None
    }
}

/// Split a command string into argv when round-trippable through `shlex`.
pub fn split_command_string(command: &str) -> Vec<String> {
    let Some(parts) = shlex::split(command) else {
        return vec![command.to_string()];
    };
    match shlex::try_join(parts.iter().map(String::as_str)) {
        Ok(round_trip)
            if round_trip == command
                || (!command.contains(":\\")
                    && shlex::split(&round_trip).as_ref() == Some(&parts)) =>
        {
            parts
        }
        _ => vec![command.to_string()],
    }
}

/// If `path` is absolute and inside `$HOME`, return the part after home.
pub fn relativize_to_home<P>(path: P) -> Option<PathBuf>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    if !path.is_absolute() {
        return None;
    }
    let home_dir = directories::UserDirs::new()?.home_dir().to_path_buf();
    let rel = path.strip_prefix(&home_dir).ok()?;
    Some(rel.to_path_buf())
}

#[cfg(all(test, feature = "vendor-tests"))]
mod tests {
    use super::*;

    #[test]
    fn strip_bash_lc_extracts_script() {
        let args = vec!["bash".into(), "-lc".into(), "echo hello".into()];
        assert_eq!(strip_bash_lc_and_escape(&args), "echo hello");
    }

    #[test]
    fn escape_command_quotes_spaces() {
        let args = vec!["foo".into(), "bar baz".into()];
        assert_eq!(escape_command(&args), "foo 'bar baz'");
    }
}
