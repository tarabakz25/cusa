// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-060 / SPEC-061: cumulative + per-turn token usage accounting.
//
// - `UsageAccumulator` tracks cumulative usage for a session across turns.
// - `TurnUsageTracker` collects per-turn deltas emitted via `stream/usage`
//   and computes the delta reported at `run/finished`.

import type { TokenUsage, TokenUsageDelta } from "../rpc/schema.js";

export function emptyTokenUsage(): TokenUsage {
  return {
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    reasoningTokens: 0,
    totalTokens: 0,
  };
}

export function addTokenUsage(a: TokenUsage, b: TokenUsage): TokenUsage {
  return {
    inputTokens: a.inputTokens + b.inputTokens,
    outputTokens: a.outputTokens + b.outputTokens,
    cacheReadTokens: (a.cacheReadTokens ?? 0) + (b.cacheReadTokens ?? 0),
    cacheCreationTokens:
      (a.cacheCreationTokens ?? 0) + (b.cacheCreationTokens ?? 0),
    reasoningTokens: (a.reasoningTokens ?? 0) + (b.reasoningTokens ?? 0),
    totalTokens: a.totalTokens + b.totalTokens,
    byModel: mergeByModel(a.byModel, b.byModel),
  };
}

function mergeByModel(
  a?: Record<string, TokenUsageDelta>,
  b?: Record<string, TokenUsageDelta>,
): Record<string, TokenUsageDelta> | undefined {
  if (!a && !b) return undefined;
  const out: Record<string, TokenUsageDelta> = {};
  for (const src of [a, b]) {
    if (!src) continue;
    for (const [id, d] of Object.entries(src)) {
      const cur = out[id] ?? {
        inputTokens: 0,
        outputTokens: 0,
        totalTokens: 0,
      };
      out[id] = {
        inputTokens: cur.inputTokens + d.inputTokens,
        outputTokens: cur.outputTokens + d.outputTokens,
        totalTokens: cur.totalTokens + d.totalTokens,
      };
    }
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/**
 * Per-session cumulative usage. Fed by every `stream/usage` and the
 * `run/finished` snapshot (whichever arrives most recently for that turn).
 */
export class UsageAccumulator {
  private cumulative: TokenUsage = emptyTokenUsage();

  add(delta: TokenUsage, model?: string): void {
    const withModel: TokenUsage = model
      ? {
          ...delta,
          byModel: {
            [model]: {
              inputTokens: delta.inputTokens,
              outputTokens: delta.outputTokens,
              totalTokens: delta.totalTokens,
            },
          },
        }
      : delta;
    this.cumulative = addTokenUsage(this.cumulative, withModel);
  }

  snapshot(): TokenUsage {
    return {
      ...this.cumulative,
      byModel: this.cumulative.byModel
        ? { ...this.cumulative.byModel }
        : undefined,
    };
  }
}

/**
 * Tracks a single turn's usage events. The SDK may emit multiple
 * `stream/usage` events; we treat the last one as authoritative.
 */
export class TurnUsageTracker {
  private latest: TokenUsage | undefined;

  observe(u: TokenUsage): void {
    this.latest = u;
  }

  turnDelta(): TokenUsage {
    return this.latest ?? emptyTokenUsage();
  }
}
