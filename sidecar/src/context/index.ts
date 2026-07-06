// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Context subsystem — SPEC-090..093.
//
// Public entry point used by SessionManager:
//
//   const ctx = new ContextManager({ ... });
//   ctx.setSummarizerClient(client);          // optional
//   ctx.setForcedStrategy(sessionId, forced); // /context strategy=...
//   const built = await ctx.buildContext(sessionId);
//   ctx.recordTurn(sessionId, { userPrompt, assistantText, toolCallsSummary });
//   ctx.disposeSession(sessionId);
//
// The returned string is safe to inject as-is into the SDK's
// `systemContext`; empty string ⇒ no injection.

import { ConversationHistory, type AppendTurnInput } from "./history.js";
import {
  DEFAULT_BYTE_BUDGET,
  DEFAULT_RAW_TURNS,
  SUMMARY_TAIL_TURNS,
  pickStrategy,
  type EffectiveStrategy,
} from "./strategy.js";
import { renderRaw, renderSummary, type ConversationTurn } from "./format.js";
import {
  DEFAULT_SUMMARIZER_MODEL,
  DEFAULT_SUMMARIZER_TIMEOUT_MS,
  Summarizer,
} from "./summarizer.js";
import type { RouterLlmClient } from "../router/llm.js";
import type { ContextStrategy } from "../rpc/schema.js";

export {
  ConversationHistory,
  type AppendTurnInput,
} from "./history.js";
export {
  DEFAULT_BYTE_BUDGET,
  DEFAULT_RAW_TURNS,
  SUMMARY_TAIL_TURNS,
  pickStrategy,
} from "./strategy.js";
export type { EffectiveStrategy, StrategyDecision } from "./strategy.js";
export {
  renderRaw,
  renderSummary,
  rawRenderByteSize,
  type ConversationTurn,
  xmlEscape,
} from "./format.js";
export {
  Summarizer,
  buildSummarizerPrompt,
  DEFAULT_SUMMARIZER_MODEL,
  DEFAULT_SUMMARIZER_TIMEOUT_MS,
} from "./summarizer.js";
export {
  detectNativeConversationRetention,
  shouldUseNativeRetention,
  type ConversationMode,
  type FeatureDetectResult,
} from "./feature_detect.js";

export interface ContextManagerOptions {
  /** Set to `true` to skip manual injection entirely (SPEC-093 native). */
  useNative?: boolean;
  /** Default forced strategy for new sessions. */
  defaultForced?: ContextStrategy;
  /** Byte budget past which auto picks summary. Default 32 KiB. */
  byteBudget?: number;
  /** Last-N-turns window for raw replay. Default 6. */
  rawTurns?: number;
  /** Summarizer model. Default `composer-2.5`. */
  summarizerModel?: string;
  /** Summarizer per-call timeout. Default 8 s. */
  summarizerTimeoutMs?: number;
  /** LLM client for the summarizer path. Absent → summarizer disabled. */
  summarizerClient?: RouterLlmClient;
  /** Log sink. */
  log?: (level: "info" | "warn" | "error", msg: string) => void;
}

interface SessionEntry {
  history: ConversationHistory;
  forced: ContextStrategy;
  summarizer: Summarizer | null;
}

export interface BuiltContext {
  text: string;
  strategy: EffectiveStrategy | "native" | "empty";
  fallbackFromSummary: boolean;
}

export class ContextManager {
  private readonly sessions = new Map<string, SessionEntry>();
  private readonly byteBudget: number;
  private readonly rawTurns: number;
  private readonly summarizerModel: string;
  private readonly summarizerTimeoutMs: number;
  private readonly log: (level: "info" | "warn" | "error", msg: string) => void;
  private summarizerClient: RouterLlmClient | undefined;
  private useNative: boolean;
  private defaultForced: ContextStrategy;

  constructor(opts: ContextManagerOptions = {}) {
    this.byteBudget = opts.byteBudget ?? DEFAULT_BYTE_BUDGET;
    this.rawTurns = opts.rawTurns ?? DEFAULT_RAW_TURNS;
    this.summarizerModel = opts.summarizerModel ?? DEFAULT_SUMMARIZER_MODEL;
    this.summarizerTimeoutMs =
      opts.summarizerTimeoutMs ?? DEFAULT_SUMMARIZER_TIMEOUT_MS;
    this.log = opts.log ?? (() => {});
    this.useNative = opts.useNative ?? false;
    this.defaultForced = opts.defaultForced ?? "auto";
    if (opts.summarizerClient) this.summarizerClient = opts.summarizerClient;
  }

  /** Called after a session is created. Safe to call multiple times. */
  registerSession(sessionId: string): void {
    if (this.sessions.has(sessionId)) return;
    this.sessions.set(sessionId, {
      history: new ConversationHistory(),
      forced: this.defaultForced,
      summarizer: this.newSummarizer(),
    });
  }

  disposeSession(sessionId: string): void {
    const entry = this.sessions.get(sessionId);
    if (!entry) return;
    entry.history.clear();
    this.sessions.delete(sessionId);
  }

  /** SPEC-092: force / unforce a strategy for the session. */
  setForcedStrategy(sessionId: string, strategy: ContextStrategy): void {
    const entry = this.requireSession(sessionId);
    entry.forced = strategy;
  }

  /** Current forced-strategy value (for tests / debugging). */
  forcedStrategy(sessionId: string): ContextStrategy {
    return this.requireSession(sessionId).forced;
  }

  /** Enable/disable native retention (used by feature-detect wire-up). */
  setUseNative(useNative: boolean): void {
    this.useNative = useNative;
  }

  /** Whether manual injection is currently active. */
  isNative(): boolean {
    return this.useNative;
  }

  /** Wire the summarizer client after the SDK adapter is ready. */
  setSummarizerClient(client: RouterLlmClient | undefined): void {
    this.summarizerClient = client;
    // Re-arm any registered sessions so they pick up the new client.
    for (const entry of this.sessions.values()) {
      entry.summarizer = this.newSummarizer();
    }
  }

  /**
   * Append a completed turn onto the session's history. Silently no-ops
   * for unknown sessionIds (safe when called from the after-run hook
   * during a race).
   */
  recordTurn(sessionId: string, input: AppendTurnInput): void {
    const entry = this.sessions.get(sessionId);
    if (!entry) return;
    entry.history.append(input);
  }

  /**
   * Build the conversation-context string to prepend to system context.
   * Empty string → nothing to inject (either native retention is on, or
   * the session has no prior turns).
   */
  async buildContext(sessionId: string): Promise<BuiltContext> {
    if (this.useNative) {
      return { text: "", strategy: "native", fallbackFromSummary: false };
    }
    const entry = this.sessions.get(sessionId);
    if (!entry || entry.history.size() === 0) {
      return { text: "", strategy: "empty", fallbackFromSummary: false };
    }

    const decision = pickStrategy({
      forced: entry.forced,
      byteBudget: this.byteBudget,
      rawTurns: this.rawTurns,
      history: entry.history.all(),
    });

    if (decision.strategy === "raw") {
      const tail = entry.history.last(this.rawTurns);
      return {
        text: renderRaw(tail),
        strategy: "raw",
        fallbackFromSummary: false,
      };
    }

    // strategy = "summary" — either forced or auto-flip.
    if (!entry.summarizer) {
      this.log(
        "warn",
        `context: summary strategy requested but no summarizer client wired; falling back to raw for session ${sessionId}`,
      );
      const tail = entry.history.last(this.rawTurns);
      return {
        text: renderRaw(tail),
        strategy: "raw",
        fallbackFromSummary: true,
      };
    }

    const { head, tail } = entry.history.snapshotForSummary(
      SUMMARY_TAIL_TURNS,
    );
    if (head.length === 0) {
      return {
        text: renderRaw(tail),
        strategy: "raw",
        fallbackFromSummary: false,
      };
    }
    const summary = await entry.summarizer.summarize(head);
    if (!summary) {
      this.log(
        "warn",
        `context: summarizer timed out / failed for session ${sessionId}; falling back to raw replay`,
      );
      const raw = entry.history.last(this.rawTurns);
      return {
        text: renderRaw(raw),
        strategy: "raw",
        fallbackFromSummary: true,
      };
    }
    return {
      text: renderSummary(summary.summary, tail),
      strategy: "summary",
      fallbackFromSummary: false,
    };
  }

  private requireSession(sessionId: string): SessionEntry {
    const entry = this.sessions.get(sessionId);
    if (!entry) {
      throw new Error(`context: unknown sessionId '${sessionId}'`);
    }
    return entry;
  }

  private newSummarizer(): Summarizer | null {
    if (!this.summarizerClient) return null;
    return new Summarizer({
      client: this.summarizerClient,
      model: this.summarizerModel,
      timeoutMs: this.summarizerTimeoutMs,
    });
  }

  /** Test-only: fetch the raw history for inspection. */
  historyFor(sessionId: string): ConversationTurn[] | undefined {
    return this.sessions.get(sessionId)?.history.all();
  }
}
