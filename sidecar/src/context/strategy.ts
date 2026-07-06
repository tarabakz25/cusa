// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Context-strategy picker (SPEC-091, SPEC-092).
//
// The picker decides between raw replay (SPEC-090) and rolling-summary
// (SPEC-091) based on the raw-render byte budget. Users can pin either
// via `/context strategy=raw|summary` (SPEC-092); "auto" restores the
// automatic byte-driven decision.

import type { ContextStrategy } from "../rpc/schema.js";
import { rawRenderByteSize, type ConversationTurn } from "./format.js";

export const DEFAULT_BYTE_BUDGET = 32 * 1024;
export const DEFAULT_RAW_TURNS = 6;
/** Number of raw turns kept at the end when the summary strategy fires. */
export const SUMMARY_TAIL_TURNS = 2;

export type EffectiveStrategy = "raw" | "summary";

export interface StrategyDecision {
  /** Which rendering strategy the caller must use. */
  strategy: EffectiveStrategy;
  /** True when auto-mode was clamped to "summary" because raw > budget. */
  autoSwitchedToSummary: boolean;
  /** True when the user's manual selection forced this choice. */
  forced: boolean;
}

export interface PickStrategyOptions {
  /** User-forced strategy (`context/setStrategy`); default = "auto". */
  forced: ContextStrategy;
  /** Byte budget past which auto flips to summary. Defaults to 32 KiB. */
  byteBudget?: number;
  /** How many turns feed the raw path. Defaults to 6. */
  rawTurns?: number;
  /** Complete session history in append order. */
  history: readonly ConversationTurn[];
}

/**
 * Decide which rendering strategy to use for the next turn. Callers
 * still have to fetch the corresponding raw turns / summary — this
 * function only returns the choice.
 */
export function pickStrategy(opts: PickStrategyOptions): StrategyDecision {
  const rawTurns = opts.rawTurns ?? DEFAULT_RAW_TURNS;
  const budget = opts.byteBudget ?? DEFAULT_BYTE_BUDGET;
  if (opts.forced === "raw") {
    return { strategy: "raw", autoSwitchedToSummary: false, forced: true };
  }
  if (opts.forced === "summary") {
    return {
      strategy: "summary",
      autoSwitchedToSummary: false,
      forced: true,
    };
  }
  // Auto: raw when the tail fits the byte budget, else summary.
  const tail =
    opts.history.length <= rawTurns
      ? opts.history.slice()
      : opts.history.slice(opts.history.length - rawTurns);
  const bytes = rawRenderByteSize(tail);
  if (bytes <= budget) {
    return { strategy: "raw", autoSwitchedToSummary: false, forced: false };
  }
  return { strategy: "summary", autoSwitchedToSummary: true, forced: false };
}
