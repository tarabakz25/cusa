// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Slash command parser and umbrella router for SPEC-002 (basic: /help, /clear,
// /reset, /quit) and SPEC-003 (extended: /model, /mode, /approval, /skills,
// /mcp, /cost, /resume). Individual extended commands are tagged under their
// finer-grained SPEC IDs (e.g. SPEC-016, SPEC-021, SPEC-032).
//
// Grammar: a leading `/` followed by a command name, an optional single
// whitespace separator, and a free-form argument tail. Unknown commands
// produce `SlashCommand::Unknown` so the caller can surface a toast without
// crashing.

use std::fmt;

/// Every slash command recognized by the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    /// `/help` — show the overlay with the command list.
    Help,
    /// `/clear` — clear the transcript but keep the session alive.
    Clear,
    /// `/reset` — dispose the current session and start fresh.
    Reset,
    /// `/quit` — clean shutdown.
    Quit,

    /// `/model` (SPEC-016). `None` opens a picker overlay; `Some("auto")`
    /// clears the manual override; `Some("<id>")` sets the sticky override.
    Model(Option<String>),
    /// `/approval` (SPEC-021). `None` opens the mode picker overlay;
    /// `Some("suggest"|"auto-edit"|"full-auto")` sets the mode directly.
    Approval(Option<String>),
    /// `/mode` — alias for `/approval` (matches the spec's slash list).
    Mode(Option<String>),
    /// `/skills` (SPEC-032) — open the skills toggle overlay.
    Skills,
    /// `/mcp` (SPEC-042) — open the MCP servers overlay.
    Mcp,

    /// `/cost` (SPEC-062) — open the per-turn cost/usage pane.
    Cost,
    /// `/context` (SPEC-092). `None` opens the info overlay; `Some(<name>)`
    /// forces the given strategy. Accepted names are `auto`, `raw`,
    /// `summary` (case-insensitive).
    Context(Option<String>),

    /// `/resume` (SPEC-003 / SPEC-051..053). Empty args open the picker;
    /// a non-empty arg resumes by agent id, prefix, or 1-based index.
    Resume(String),

    /// Unknown command name; caller shows an error toast.
    Unknown(String),
}

impl SlashCommand {
    /// Human-readable name (without the leading slash).
    pub fn name(&self) -> &'static str {
        match self {
            SlashCommand::Help => "help",
            SlashCommand::Clear => "clear",
            SlashCommand::Reset => "reset",
            SlashCommand::Quit => "quit",
            SlashCommand::Model(_) => "model",
            SlashCommand::Mode(_) => "mode",
            SlashCommand::Approval(_) => "approval",
            SlashCommand::Skills => "skills",
            SlashCommand::Mcp => "mcp",
            SlashCommand::Cost => "cost",
            SlashCommand::Resume(_) => "resume",
            SlashCommand::Context(_) => "context",
            SlashCommand::Unknown(_) => "unknown",
        }
    }

    /// True if this variant is still stubbed and should surface a
    /// "not implemented yet" toast when invoked.
    pub fn is_stub(&self) -> bool {
        false
    }
}

impl fmt::Display for SlashCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}", self.name())
    }
}

/// Parse a single line into either a slash command or a regular prompt.
///
/// Returns `None` if the input does not start with `/` (the caller should
/// treat it as a plain prompt). Returns `Some(SlashCommand::Unknown)` for
/// syntactically-valid but unrecognized command names.
pub fn parse(line: &str) -> Option<SlashCommand> {
    let line = line.trim_start();
    let rest = line.strip_prefix('/')?;
    if rest.is_empty() {
        return Some(SlashCommand::Unknown(String::new()));
    }
    let (name, args) = split_head(rest);
    let args = args.trim().to_string();
    let opt_arg = if args.is_empty() { None } else { Some(args.clone()) };
    let cmd = match name.to_ascii_lowercase().as_str() {
        "help" | "?" => SlashCommand::Help,
        "clear" | "new" => SlashCommand::Clear,
        "reset" => SlashCommand::Reset,
        "quit" | "exit" => SlashCommand::Quit,
        "model" => SlashCommand::Model(opt_arg),
        "mode" => SlashCommand::Mode(opt_arg),
        "approval" => SlashCommand::Approval(opt_arg),
        "skills" => SlashCommand::Skills,
        "mcp" => SlashCommand::Mcp,
        "cost" => SlashCommand::Cost,
        "resume" => SlashCommand::Resume(args),
        "context" => SlashCommand::Context(parse_context_arg(&args)),
        other => SlashCommand::Unknown(other.to_string()),
    };
    Some(cmd)
}

fn split_head(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(idx) => (&s[..idx], &s[idx..]),
        None => (s, ""),
    }
}

/// Extract the `strategy=...` value from `/context` arguments. Accepts
/// both the `strategy=foo` (spec) and bare `foo` forms.
fn parse_context_arg(args: &str) -> Option<String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("strategy=") {
        let value = rest.trim().trim_matches(|c: char| c == '"' || c == '\'');
        if value.is_empty() {
            return None;
        }
        return Some(value.to_ascii_lowercase());
    }
    Some(trimmed.to_ascii_lowercase())
}

/// Ordered list of `(command, one-line description)` used by `/help`.
pub fn help_entries() -> &'static [(&'static str, &'static str)] {
    &[
        ("/help", "Show this list."),
        ("/clear (/new)", "Clear the transcript, keep the session."),
        ("/reset", "Dispose the current session and start fresh."),
        ("/quit", "Exit cusa."),
        ("/model <id|auto>", "Set the model for subsequent turns."),
        ("/mode", "Change approval mode (alias for /approval)."),
        ("/approval", "Cycle or pick the approval mode."),
        ("/skills", "Toggle skill injection."),
        ("/mcp", "Inspect / toggle MCP servers."),
        ("/cost", "Show per-turn cost + per-model aggregates."),
        ("/resume [id|prefix|index]", "Resume a prior session for this directory."),
        ("/context strategy=<auto|raw|summary>", "Force history injection strategy."),
    ]
}

/// One row offered by the composer's slash-command suggestion popup
/// (SPEC-002). `takes_args` controls whether Tab-completion appends a
/// trailing space so the user can keep typing an argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandHint {
    /// Canonical command name, without the leading slash.
    pub name: &'static str,
    /// Alternate names accepted by `parse` and matched by the popup's
    /// prefix filter (e.g. `/new` for `/clear`). Kept in sync with the
    /// alias arms in `parse` — see
    /// `spec_002_every_suggestion_parses_to_a_known_command`.
    pub aliases: &'static [&'static str],
    /// One-line description rendered next to the name.
    pub description: &'static str,
    /// True when the command accepts an argument tail.
    pub takes_args: bool,
}

impl CommandHint {
    /// Popup display label, without the leading slash: the canonical name
    /// plus any aliases — `clear (new)`. Tab/Enter always complete to the
    /// canonical `name`; the label is display-only.
    pub fn label(&self) -> String {
        if self.aliases.is_empty() {
            self.name.to_string()
        } else {
            format!("{} ({})", self.name, self.aliases.join(", "))
        }
    }

    /// True when `candidate` prefix-matches the canonical name or any alias.
    fn matches_prefix(&self, candidate: &str) -> bool {
        self.name.starts_with(candidate) || self.aliases.iter().any(|a| a.starts_with(candidate))
    }

    /// True when `candidate` equals the canonical name or any alias.
    fn matches_exact(&self, candidate: &str) -> bool {
        self.name == candidate || self.aliases.contains(&candidate)
    }
}

/// Commands offered by the suggestion popup, in display order. Kept in sync
/// with `parse` — see `spec_002_every_suggestion_parses_to_a_known_command`.
const SUGGESTABLE: &[CommandHint] = &[
    CommandHint { name: "help", aliases: &[], description: "Show the command list.", takes_args: false },
    CommandHint { name: "clear", aliases: &["new"], description: "Clear the transcript, keep the session.", takes_args: false },
    CommandHint { name: "reset", aliases: &[], description: "Dispose the current session and start fresh.", takes_args: false },
    CommandHint { name: "quit", aliases: &[], description: "Exit cusa.", takes_args: false },
    CommandHint { name: "model", aliases: &[], description: "Pick a model or set one (<id|auto>).", takes_args: true },
    CommandHint { name: "mode", aliases: &[], description: "Change approval mode (alias for /approval).", takes_args: true },
    CommandHint { name: "approval", aliases: &[], description: "Cycle or pick the approval mode.", takes_args: true },
    CommandHint { name: "skills", aliases: &[], description: "Toggle skill injection.", takes_args: false },
    CommandHint { name: "mcp", aliases: &[], description: "Inspect / toggle MCP servers.", takes_args: false },
    CommandHint { name: "cost", aliases: &[], description: "Show per-turn cost + per-model aggregates.", takes_args: false },
    CommandHint { name: "resume", aliases: &[], description: "Resume a prior session.", takes_args: true },
    CommandHint { name: "context", aliases: &[], description: "Force history strategy (strategy=<auto|raw|summary>).", takes_args: true },
];

/// Extract the popup prefix from the composer buffer: the command token
/// currently being typed. Returns `None` when the popup should stay
/// hidden — the buffer does not start with `/`, or the user already typed
/// whitespace after the name (arguments underway / multi-line input).
pub fn popup_prefix(input: &str) -> Option<&str> {
    let trimmed = input.trim_start();
    let rest = trimmed.strip_prefix('/')?;
    if rest.contains(char::is_whitespace) {
        return None;
    }
    Some(rest)
}

/// Case-insensitive prefix filter over the suggestable commands, matching
/// canonical names and aliases alike (typing `/new` surfaces `/clear`).
/// An exact match (name or alias) sorts first so Enter never fires a
/// longer sibling (e.g. typing `/mode` must not run `/model`).
pub fn suggestions(prefix: &str) -> Vec<CommandHint> {
    let needle = prefix.to_ascii_lowercase();
    let mut out: Vec<CommandHint> = SUGGESTABLE
        .iter()
        .copied()
        .filter(|c| c.matches_prefix(needle.as_str()))
        .collect();
    // `false < true`, and the sort is stable: the exact match (if any)
    // moves to the front, everything else keeps display order.
    out.sort_by_key(|c| !c.matches_exact(needle.as_str()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_002_parse_returns_none_for_non_slash() {
        assert_eq!(parse("hello world"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("   "), None);
    }

    #[test]
    fn spec_002_parse_help_variants() {
        assert_eq!(parse("/help"), Some(SlashCommand::Help));
        assert_eq!(parse("  /help"), Some(SlashCommand::Help));
        assert_eq!(parse("/HELP"), Some(SlashCommand::Help));
        assert_eq!(parse("/?"), Some(SlashCommand::Help));
    }

    #[test]
    fn spec_002_parse_core_commands() {
        assert_eq!(parse("/clear"), Some(SlashCommand::Clear));
        assert_eq!(parse("/new"), Some(SlashCommand::Clear));
        assert_eq!(parse("/NEW"), Some(SlashCommand::Clear));
        assert_eq!(parse("/reset"), Some(SlashCommand::Reset));
        assert_eq!(parse("/quit"), Some(SlashCommand::Quit));
        assert_eq!(parse("/exit"), Some(SlashCommand::Quit));
    }

    #[test]
    fn spec_016_parse_model_variants() {
        assert_eq!(
            parse("/model claude-sonnet-4"),
            Some(SlashCommand::Model(Some("claude-sonnet-4".into())))
        );
        assert_eq!(parse("/model auto"), Some(SlashCommand::Model(Some("auto".into()))));
        assert_eq!(parse("/model"), Some(SlashCommand::Model(None)));
    }

    #[test]
    fn spec_021_parse_approval_and_mode_variants() {
        assert_eq!(parse("/approval"), Some(SlashCommand::Approval(None)));
        assert_eq!(
            parse("/approval suggest"),
            Some(SlashCommand::Approval(Some("suggest".into())))
        );
        assert_eq!(parse("/mode"), Some(SlashCommand::Mode(None)));
        assert_eq!(
            parse("/mode auto-edit"),
            Some(SlashCommand::Mode(Some("auto-edit".into())))
        );
    }

    #[test]
    fn spec_032_042_parse_skills_and_mcp() {
        assert_eq!(parse("/skills"), Some(SlashCommand::Skills));
        // Skills currently ignores trailing args.
        assert_eq!(parse("/skills anything"), Some(SlashCommand::Skills));
        assert_eq!(parse("/mcp"), Some(SlashCommand::Mcp));
    }

    #[test]
    fn spec_092_parse_context_variants() {
        assert_eq!(parse("/context"), Some(SlashCommand::Context(None)));
        assert_eq!(
            parse("/context strategy=raw"),
            Some(SlashCommand::Context(Some("raw".into())))
        );
        assert_eq!(
            parse("/context strategy=SUMMARY"),
            Some(SlashCommand::Context(Some("summary".into())))
        );
        assert_eq!(
            parse("/context strategy=auto"),
            Some(SlashCommand::Context(Some("auto".into())))
        );
        // Bare argument is accepted too, lowercased.
        assert_eq!(
            parse("/context Raw"),
            Some(SlashCommand::Context(Some("raw".into())))
        );
    }

    #[test]
    fn spec_003_resume_parses_args_and_is_not_stub() {
        assert_eq!(parse("/resume"), Some(SlashCommand::Resume(String::new())));
        assert_eq!(
            parse("/resume abc123"),
            Some(SlashCommand::Resume("abc123".into()))
        );
        assert!(!parse("/resume").unwrap().is_stub());
        assert!(!parse("/cost").unwrap().is_stub());
        assert!(!parse("/context").unwrap().is_stub());
    }

    #[test]
    fn spec_002_parse_unknown_carries_name() {
        assert_eq!(parse("/foo bar"), Some(SlashCommand::Unknown("foo".into())));
        assert_eq!(parse("/"), Some(SlashCommand::Unknown(String::new())));
    }

    #[test]
    fn spec_002_display_prefixes_slash() {
        assert_eq!(SlashCommand::Help.to_string(), "/help");
        assert_eq!(SlashCommand::Model(Some("x".into())).to_string(), "/model");
        assert_eq!(SlashCommand::Skills.to_string(), "/skills");
    }

    #[test]
    fn spec_002_help_entries_cover_documented_commands() {
        let names: Vec<&&str> = help_entries().iter().map(|(n, _)| n).collect();
        for expected in [
            "/help",
            "/clear (/new)",
            "/reset",
            "/quit",
            "/model <id|auto>",
            "/approval",
            "/skills",
            "/mcp",
        ] {
            assert!(names.iter().any(|n| **n == expected), "missing {expected}");
        }
    }

    #[test]
    fn spec_002_popup_prefix_extraction() {
        assert_eq!(popup_prefix("/"), Some(""));
        assert_eq!(popup_prefix("/mo"), Some("mo"));
        assert_eq!(popup_prefix("  /mo"), Some("mo"));
        assert_eq!(popup_prefix("hello"), None);
        assert_eq!(popup_prefix(""), None);
        // Arguments underway or multi-line input hide the popup.
        assert_eq!(popup_prefix("/model auto"), None);
        assert_eq!(popup_prefix("/mo\nde"), None);
    }

    #[test]
    fn spec_002_suggestions_filter_by_prefix() {
        assert_eq!(suggestions("").len(), 12);
        let mo: Vec<&str> = suggestions("mo").iter().map(|c| c.name).collect();
        assert_eq!(mo, vec!["model", "mode"]);
        assert!(suggestions("zzz").is_empty());
        assert_eq!(suggestions("HEL")[0].name, "help", "prefix match is case-insensitive");
    }

    #[test]
    fn spec_002_suggestions_exact_match_sorts_first() {
        // "/mode" matches both "mode" and "model"; the exact one must come
        // first so Enter runs what the user typed.
        let mode: Vec<&str> = suggestions("mode").iter().map(|c| c.name).collect();
        assert_eq!(mode, vec!["mode", "model"]);
    }

    #[test]
    fn spec_002_every_suggestion_parses_to_a_known_command() {
        for hint in suggestions("") {
            let parsed = parse(&format!("/{}", hint.name)).expect("parses");
            assert!(
                !matches!(parsed, SlashCommand::Unknown(_)),
                "/{} must be a known command",
                hint.name
            );
            // Every advertised alias must parse to the same command as the
            // canonical name (keeps SUGGESTABLE in sync with `parse`).
            for alias in hint.aliases {
                let alias_parsed = parse(&format!("/{alias}")).expect("alias parses");
                assert_eq!(
                    alias_parsed.name(),
                    parsed.name(),
                    "/{alias} must run /{}",
                    hint.name
                );
            }
        }
    }

    #[test]
    fn spec_002_suggestions_match_aliases() {
        // Typing `/n`, `/ne`, `/new` must surface /clear via its alias.
        for p in ["n", "ne", "new", "NEW"] {
            let names: Vec<&str> = suggestions(p).iter().map(|c| c.name).collect();
            assert_eq!(names, vec!["clear"], "prefix {p:?} must suggest clear");
        }
        // Overshooting the alias hides it again.
        assert!(suggestions("news").is_empty());
    }

    #[test]
    fn spec_002_clear_hint_label_advertises_alias_but_stays_canonical() {
        let hint = suggestions("new")[0];
        assert_eq!(hint.label(), "clear (new)", "popup label shows the alias");
        assert_eq!(hint.name, "clear", "Tab/Enter complete the canonical name");
        // Hints without aliases keep their plain label.
        assert_eq!(suggestions("help")[0].label(), "help");
    }
}
