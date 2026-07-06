// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// LLM-based history summarizer for the rolling-context strategy
// (SPEC-091).
//
// The summarizer reuses the same `RouterLlmClient` interface the Router
// uses so we don't spawn a second SDK path. Callers wire the same client
// (typically a small `composer-2.5` prompt through Agent.prompt) into
// both.
//
// Caching contract:
//   - We compute a stable key over the concatenated user-prompts +
//     assistant-texts of the "head" window.
//   - Re-summarize only when the underlying window grows by more than
//     25% (measured in bytes) since the last cache hit.
//   - Timeout: 8 s by default (configurable). On failure or timeout the
//     caller falls back to raw replay.

import { renderRaw, type ConversationTurn } from "./format.js";
import type { RouterLlmClient } from "../router/llm.js";

export const DEFAULT_SUMMARIZER_TIMEOUT_MS = 8000;
export const DEFAULT_SUMMARIZER_MODEL = "composer-2.5";
/** Re-summarize when the window grows by more than 25% since last hit. */
export const REGROW_THRESHOLD = 0.25;

export interface SummarizerOptions {
  client: RouterLlmClient;
  model?: string;
  timeoutMs?: number;
  /** Optional cache. Passed in so callers can reset per-session. */
  cache?: SummaryCache;
}

export interface SummaryCache {
  /** Stable id (byte length) of the last summarized window. */
  windowBytes: number;
  /** Last successful summary text. */
  summary: string;
  /** Index of the last summarized turn (inclusive). */
  lastIndex: number;
}

export interface SummarizeResult {
  summary: string;
  fromCache: boolean;
}

/**
 * Build the summarization prompt. Kept short so the model can turn
 * around under 8 s.
 */
export function buildSummarizerPrompt(
  turns: readonly ConversationTurn[],
): string {
  const raw = renderRaw(turns);
  return [
    "You compress a coding-agent conversation into a compact context so",
    "the next turn retains what happened. Preserve concrete details:",
    "  - file paths edited, functions/classes/routines changed",
    "  - open TODOs, blockers, or user-stated goals",
    "  - decisions and their rationale",
    "Do NOT roleplay, do NOT continue the conversation, do NOT respond",
    "to the user. Emit prose only, ~200 words max, no bullet lists.",
    "",
    "Conversation:",
    raw,
  ].join("\n");
}

export class Summarizer {
  private readonly client: RouterLlmClient;
  private readonly model: string;
  private readonly timeoutMs: number;
  private cache: SummaryCache | undefined;

  constructor(opts: SummarizerOptions) {
    this.client = opts.client;
    this.model = opts.model ?? DEFAULT_SUMMARIZER_MODEL;
    this.timeoutMs = opts.timeoutMs ?? DEFAULT_SUMMARIZER_TIMEOUT_MS;
    if (opts.cache) this.cache = opts.cache;
  }

  /** Expose the current cache (for tests / persistence). */
  currentCache(): SummaryCache | undefined {
    return this.cache;
  }

  /**
   * Summarize (or reuse a cached summary for) the given head window.
   * Returns `null` on timeout / failure — callers must fall back to
   * raw replay.
   */
  async summarize(
    head: readonly ConversationTurn[],
  ): Promise<SummarizeResult | null> {
    if (head.length === 0) return { summary: "", fromCache: false };
    const windowBytes = new TextEncoder().encode(renderRaw(head)).length;
    const lastIndex = head[head.length - 1]!.index;
    if (this.cache && this.cache.lastIndex === lastIndex) {
      return { summary: this.cache.summary, fromCache: true };
    }
    if (
      this.cache &&
      this.cache.lastIndex < lastIndex &&
      windowBytes <= this.cache.windowBytes * (1 + REGROW_THRESHOLD)
    ) {
      // Growth below the 25% threshold — reuse the stale summary.
      return { summary: this.cache.summary, fromCache: true };
    }

    const prompt = buildSummarizerPrompt(head);
    const summary = await runWithTimeout(
      this.client,
      { prompt, model: this.model },
      this.timeoutMs,
    );
    if (summary === null) return null;
    const trimmed = summary.trim();
    if (trimmed.length === 0) return null;
    this.cache = { windowBytes, summary: trimmed, lastIndex };
    return { summary: trimmed, fromCache: false };
  }
}

async function runWithTimeout(
  client: RouterLlmClient,
  input: { prompt: string; model: string },
  timeoutMs: number,
): Promise<string | null> {
  const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | null = setTimeout(() => {
    controller.abort();
  }, timeoutMs);
  (timer as { unref?: () => void }).unref?.();
  try {
    const raw = await client.classify({
      prompt: input.prompt,
      model: input.model,
      signal: controller.signal,
    });
    return raw;
  } catch {
    return null;
  } finally {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }
}
