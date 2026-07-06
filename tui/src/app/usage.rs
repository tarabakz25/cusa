// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Token-usage accumulator (SPEC-060, SPEC-061).
//
// The sidecar emits `stream/usage` and `run/finished` notifications carrying
// `TokenUsage` values. The TUI keeps two views of that data:
//   1. **Cumulative** — every token the session has consumed, rendered in the
//      status line (SPEC-060).
//   2. **Per-turn delta** — the most recent turn's usage, rendered as a
//      one-line summary after the turn completes (SPEC-061).
//
// The accumulator handles two payload shapes the sidecar may emit:
//   * *Absolute* — running total per turn (Cursor's `SDKUsageMessage` style).
//   * *Incremental* — deltas since the last emit.
// We treat any usage whose totals are less than or equal to the previous
// turn's tail as incremental and add it; otherwise we compute the delta.

use cusa_rpc::{TokenUsage, TokenUsageDelta};
use std::collections::BTreeMap;

/// One completed turn's usage row, kept for the `/cost` pane (SPEC-062).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnUsage {
    /// 1-based turn index within the current session.
    pub turn_index: u32,
    /// Model that ran the turn (empty when unknown).
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
}

/// Per-model aggregate row for the `/cost` pane's top pane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelAggregate {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub turns: u32,
}

/// Snapshot of cumulative + last-turn usage, suitable for direct rendering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSnapshot {
    /// Running total across the entire session.
    pub cumulative: TokenUsage,
    /// Delta for the last completed turn (or partial deltas mid-turn).
    pub last_turn: TokenUsageDelta,
    /// SPEC-062: per-turn history, oldest first.
    pub turns: Vec<TurnUsage>,
    /// SPEC-062: per-model aggregates, sorted by descending total tokens.
    pub by_model: Vec<ModelAggregate>,
}

impl UsageSnapshot {
    /// Human-readable "in/out/total" summary used in the status line.
    pub fn status_line(&self) -> String {
        format!(
            "tokens in {} out {} total {}",
            fmt_num(self.cumulative.input_tokens),
            fmt_num(self.cumulative.output_tokens),
            fmt_num(self.cumulative.total_tokens),
        )
    }

    /// One-line per-turn delta used after a turn completes (SPEC-061).
    pub fn turn_summary(&self) -> String {
        format!(
            "turn Δ in {} · out {} · total {}",
            fmt_num(self.last_turn.input_tokens),
            fmt_num(self.last_turn.output_tokens),
            fmt_num(self.last_turn.total_tokens),
        )
    }
}

fn fmt_num(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

/// Accumulator state. Public for the app to hold in `AppState`.
#[derive(Debug, Clone, Default)]
pub struct UsageAccumulator {
    snapshot: UsageSnapshot,
    /// Rolling total for the *current* turn (before it finishes). Cleared on
    /// `finish_turn`.
    current_turn: TokenUsageDelta,
    /// Rolling cache-read counter for the current turn (SPEC-062: shown
    /// per-turn in the `/cost` pane).
    current_turn_cache: u64,
    /// Total number of turns finished so far. Used as the turn index.
    turn_count: u32,
}

impl UsageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> &UsageSnapshot {
        &self.snapshot
    }

    /// Ingest a `stream/usage` payload. The sidecar can emit multiple usage
    /// events per turn; each is treated as a **delta** and added to both the
    /// current-turn accumulator and the cumulative totals.
    pub fn ingest_stream(&mut self, usage: &TokenUsage) {
        let delta = TokenUsageDelta {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens.max(usage.input_tokens + usage.output_tokens),
        };
        add_delta(&mut self.current_turn, &delta);
        self.current_turn_cache = self
            .current_turn_cache
            .saturating_add(usage.cache_read_tokens);
        add_usage(&mut self.snapshot.cumulative, usage);
    }

    /// Called by `run/finished`. The `usage` payload here is the run's
    /// **final** total; we compute the turn delta against the current-turn
    /// accumulator (if any partial usage arrived via streaming) and reconcile
    /// the cumulative counter.
    pub fn finish_turn(&mut self, final_usage: &TokenUsage) {
        self.finish_turn_with_model(final_usage, None);
    }

    /// Same as [`finish_turn`] but tags the just-completed turn with a
    /// model id so the per-model aggregate view (SPEC-062) can update.
    pub fn finish_turn_with_model(&mut self, final_usage: &TokenUsage, model: Option<&str>) {
        // If the sidecar sent the final usage without any streaming deltas,
        // the current-turn accumulator is zero and we treat final_usage as
        // the turn delta.
        let (turn_delta, cache_read) = if self.current_turn.total_tokens == 0
            && self.current_turn.input_tokens == 0
            && self.current_turn.output_tokens == 0
        {
            let delta = TokenUsageDelta {
                input_tokens: final_usage.input_tokens,
                output_tokens: final_usage.output_tokens,
                total_tokens: final_usage
                    .total_tokens
                    .max(final_usage.input_tokens + final_usage.output_tokens),
            };
            add_usage(&mut self.snapshot.cumulative, final_usage);
            (delta, final_usage.cache_read_tokens)
        } else {
            let cache = std::mem::take(&mut self.current_turn_cache);
            (std::mem::take(&mut self.current_turn), cache)
        };

        self.snapshot.last_turn = turn_delta.clone();
        self.current_turn = TokenUsageDelta::default();
        self.current_turn_cache = 0;

        self.turn_count = self.turn_count.saturating_add(1);
        let model_id = model.unwrap_or_default().to_string();
        self.snapshot.turns.push(TurnUsage {
            turn_index: self.turn_count,
            model: model_id.clone(),
            input_tokens: turn_delta.input_tokens,
            output_tokens: turn_delta.output_tokens,
            total_tokens: turn_delta.total_tokens,
            cache_read_tokens: cache_read,
        });
        rebuild_model_aggregates(&mut self.snapshot);
    }

    /// Reset everything (used by `/clear` and `/reset`).
    pub fn reset(&mut self) {
        self.snapshot = UsageSnapshot::default();
        self.current_turn = TokenUsageDelta::default();
        self.current_turn_cache = 0;
        self.turn_count = 0;
    }
}

fn rebuild_model_aggregates(snapshot: &mut UsageSnapshot) {
    let mut map: BTreeMap<String, ModelAggregate> = BTreeMap::new();
    for t in &snapshot.turns {
        let entry = map.entry(t.model.clone()).or_insert_with(|| ModelAggregate {
            model: t.model.clone(),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            turns: 0,
        });
        entry.input_tokens = entry.input_tokens.saturating_add(t.input_tokens);
        entry.output_tokens = entry.output_tokens.saturating_add(t.output_tokens);
        entry.total_tokens = entry.total_tokens.saturating_add(t.total_tokens);
        entry.turns = entry.turns.saturating_add(1);
    }
    let mut rows: Vec<ModelAggregate> = map.into_values().collect();
    rows.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens).then_with(|| a.model.cmp(&b.model)));
    snapshot.by_model = rows;
}

fn add_delta(dst: &mut TokenUsageDelta, src: &TokenUsageDelta) {
    dst.input_tokens = dst.input_tokens.saturating_add(src.input_tokens);
    dst.output_tokens = dst.output_tokens.saturating_add(src.output_tokens);
    dst.total_tokens = dst.total_tokens.saturating_add(src.total_tokens);
}

fn add_usage(dst: &mut TokenUsage, src: &TokenUsage) {
    dst.input_tokens = dst.input_tokens.saturating_add(src.input_tokens);
    dst.output_tokens = dst.output_tokens.saturating_add(src.output_tokens);
    dst.cache_read_tokens = dst.cache_read_tokens.saturating_add(src.cache_read_tokens);
    dst.cache_creation_tokens = dst
        .cache_creation_tokens
        .saturating_add(src.cache_creation_tokens);
    dst.reasoning_tokens = dst.reasoning_tokens.saturating_add(src.reasoning_tokens);
    let derived_total = src.total_tokens.max(src.input_tokens + src.output_tokens);
    dst.total_tokens = dst.total_tokens.saturating_add(derived_total);
    for (model, delta) in &src.by_model {
        let entry = dst.by_model.entry(model.clone()).or_default();
        add_delta(entry, delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64) -> TokenUsage {
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            ..Default::default()
        }
    }

    #[test]
    fn spec_060_cumulative_grows_across_turns() {
        let mut a = UsageAccumulator::new();
        a.finish_turn(&usage(10, 20));
        a.finish_turn(&usage(5, 40));
        assert_eq!(a.snapshot().cumulative.input_tokens, 15);
        assert_eq!(a.snapshot().cumulative.output_tokens, 60);
        assert_eq!(a.snapshot().cumulative.total_tokens, 75);
    }

    #[test]
    fn spec_060_status_line_format() {
        let mut a = UsageAccumulator::new();
        a.finish_turn(&usage(1234, 5678));
        let line = a.snapshot().status_line();
        assert!(line.contains("in 1.2K"), "got {line}");
        assert!(line.contains("out 5.7K"));
        assert!(line.contains("total"));
    }

    #[test]
    fn spec_061_turn_delta_equals_final_when_no_stream() {
        let mut a = UsageAccumulator::new();
        a.finish_turn(&usage(100, 250));
        assert_eq!(a.snapshot().last_turn.input_tokens, 100);
        assert_eq!(a.snapshot().last_turn.output_tokens, 250);
        assert_eq!(a.snapshot().last_turn.total_tokens, 350);
    }

    #[test]
    fn spec_061_turn_delta_uses_streamed_when_present() {
        let mut a = UsageAccumulator::new();
        a.ingest_stream(&usage(30, 40));
        a.ingest_stream(&usage(20, 60));
        // final also arrives, but current_turn is non-zero so we use it.
        a.finish_turn(&usage(0, 0));
        assert_eq!(a.snapshot().last_turn.input_tokens, 50);
        assert_eq!(a.snapshot().last_turn.output_tokens, 100);
        assert_eq!(a.snapshot().last_turn.total_tokens, 150);
    }

    #[test]
    fn spec_061_two_turns_reset_last_turn_delta() {
        let mut a = UsageAccumulator::new();
        a.finish_turn(&usage(10, 20));
        assert_eq!(a.snapshot().last_turn.total_tokens, 30);
        a.finish_turn(&usage(2, 3));
        assert_eq!(a.snapshot().last_turn.total_tokens, 5);
    }

    #[test]
    fn spec_060_reset_zeroes_snapshot() {
        let mut a = UsageAccumulator::new();
        a.finish_turn(&usage(10, 20));
        a.reset();
        assert_eq!(a.snapshot().cumulative, TokenUsage::default());
        assert_eq!(a.snapshot().last_turn, TokenUsageDelta::default());
    }

    #[test]
    fn spec_060_status_line_units_scale() {
        let a = UsageAccumulator::default();
        assert!(a.snapshot().status_line().contains("tokens in 0"));
        let mut a2 = UsageAccumulator::new();
        a2.finish_turn(&usage(1_500_000, 500_000));
        assert!(a2.snapshot().status_line().contains("in 1.5M"));
    }

    #[test]
    fn spec_061_turn_summary_format() {
        let mut a = UsageAccumulator::new();
        a.finish_turn(&usage(7, 8));
        let s = a.snapshot().turn_summary();
        assert!(s.contains("in 7"));
        assert!(s.contains("out 8"));
        assert!(s.contains("total 15"));
    }
}
