// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// ConversationHistory — per-session store of completed turns.
//
// SPEC IDs:
// - SPEC-090: `last(n)` returns the last N turns for raw replay.
// - SPEC-091: `snapshotForSummary` returns the older-than-tail window
//   used by the summarizer, plus the raw tail.
//
// The history is intentionally unbounded in-memory. A future slice can
// swap in a ring-buffer if session lifetime becomes an issue; today the
// picker's byte budget already prevents the raw-render from bloating,
// and disposals clear the store.

import type { ConversationTurn } from "./format.js";

export interface AppendTurnInput {
  userPrompt: string;
  assistantText: string;
  toolCallsSummary: readonly string[];
  model?: string | undefined;
}

export class ConversationHistory {
  private readonly turns: ConversationTurn[] = [];
  private nextIndex = 0;

  /** Push a completed turn onto the history. */
  append(input: AppendTurnInput): ConversationTurn {
    const turn: ConversationTurn = {
      index: this.nextIndex++,
      userPrompt: input.userPrompt,
      assistantText: input.assistantText,
      toolCallsSummary: [...input.toolCallsSummary],
      ...(input.model !== undefined ? { model: input.model } : {}),
    };
    this.turns.push(turn);
    return turn;
  }

  /** Total number of retained turns. */
  size(): number {
    return this.turns.length;
  }

  /** Snapshot every retained turn (copy — callers can freely iterate). */
  all(): ConversationTurn[] {
    return this.turns.slice();
  }

  /** Last `n` turns (or fewer if the history is shorter). */
  last(n: number): ConversationTurn[] {
    if (n <= 0) return [];
    if (n >= this.turns.length) return this.turns.slice();
    return this.turns.slice(this.turns.length - n);
  }

  /**
   * Snapshot the head (older) + tail (raw kept) partition used by the
   * summarizer path (SPEC-091). `tailSize` counts turns from the end of
   * the history that stay raw; the rest are the "head" that gets fed to
   * the summarizer.
   *
   * When the history is shorter than `tailSize`, `head` is empty.
   */
  snapshotForSummary(tailSize: number): {
    head: ConversationTurn[];
    tail: ConversationTurn[];
  } {
    if (tailSize <= 0) return { head: this.turns.slice(), tail: [] };
    if (this.turns.length <= tailSize) {
      return { head: [], tail: this.turns.slice() };
    }
    const cut = this.turns.length - tailSize;
    return {
      head: this.turns.slice(0, cut),
      tail: this.turns.slice(cut),
    };
  }

  /** Drop every retained turn. Called on `session/dispose`. */
  clear(): void {
    this.turns.length = 0;
    // Preserve nextIndex so caching keyed on turn.index stays stable.
  }
}
