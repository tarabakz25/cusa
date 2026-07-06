// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//
// Minimal shimmer spans for status chrome (SPEC-109). Full animation is optional.

use ratatui::style::Stylize;
use ratatui::text::Span;

/// Static shimmer substitute: returns a single dim span (reduced-motion safe).
pub fn shimmer_spans(text: &str) -> Vec<Span<'static>> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![text.to_string().dim()]
    }
}
