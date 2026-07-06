// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Stand-ins for upstream `codex_*` helpers used by vendored Codex UI modules.
// No `codex-*` crate dependencies (SPEC-105, SPEC-107).

/// Subset of upstream terminal name categories used for color-level heuristics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalName {
    WindowsTerminal,
    Unknown,
}

/// Minimal terminal metadata for palette fallbacks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalInfo {
    pub name: TerminalName,
}

/// Conservative default: unknown terminal, no special-case color upgrades.
pub fn terminal_info() -> TerminalInfo {
    TerminalInfo {
        name: TerminalName::Unknown,
    }
}

/// User-input protocol stand-ins for vendored composer `TextArea` (SPEC-106).
pub mod user_input {
    use std::ops::Range;

    pub const MAX_USER_INPUT_TEXT_CHARS: usize = 100_000;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ByteRange {
        pub start: usize,
        pub end: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct TextElement {
        pub byte_range: ByteRange,
        pub placeholder: Option<String>,
    }

    impl TextElement {
        pub fn new(byte_range: ByteRange, placeholder: Option<String>) -> Self {
            Self {
                byte_range,
                placeholder,
            }
        }

        pub fn placeholder<'a>(&'a self, text: &'a str) -> Option<&'a str> {
            self.placeholder
                .as_deref()
                .or_else(|| text.get(self.byte_range.start..self.byte_range.end))
        }
    }

    impl From<Range<usize>> for ByteRange {
        fn from(value: Range<usize>) -> Self {
            Self {
                start: value.start,
                end: value.end,
            }
        }
    }
}

/// Normalize markdown link hash location suffixes for display.
pub fn normalize_markdown_hash_location_suffix(suffix: &str) -> Option<String> {
    let trimmed = suffix.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
