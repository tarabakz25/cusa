// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import { Summarizer, buildSummarizerPrompt } from "./summarizer.ts";
import type { ConversationTurn } from "./format.ts";
import type { RouterLlmClient } from "../router/llm.ts";

function turn(index: number, body: string): ConversationTurn {
  return {
    index,
    userPrompt: `u${index}: ${body}`,
    assistantText: `a${index}: ${body}`,
    toolCallsSummary: [],
  };
}

function bigTurn(index: number, bytes: number): ConversationTurn {
  const body = "x".repeat(bytes);
  return {
    index,
    userPrompt: body,
    assistantText: body,
    toolCallsSummary: [],
  };
}

// ----------------------------------------------------------------------
// SPEC-091: LLM-summarized rolling context
// ----------------------------------------------------------------------

test("SPEC-091: summarize() calls the LLM classifier with the summarizer model", async () => {
  const seen: Array<{ model: string; prompt: string }> = [];
  const client: RouterLlmClient = {
    classify: async (input) => {
      seen.push({ model: input.model, prompt: input.prompt });
      return "  compact summary here  ";
    },
  };
  const s = new Summarizer({ client, model: "summarizer-model" });
  const res = await s.summarize([turn(0, "foo"), turn(1, "bar")]);
  assert.ok(res);
  assert.equal(res!.summary, "compact summary here");
  assert.equal(res!.fromCache, false);
  assert.equal(seen.length, 1);
  assert.equal(seen[0]!.model, "summarizer-model");
  assert.match(seen[0]!.prompt, /Conversation:/);
});

test("SPEC-091: empty head → empty summary, no LLM call", async () => {
  let called = 0;
  const client: RouterLlmClient = {
    classify: async () => {
      called++;
      return "should not happen";
    },
  };
  const s = new Summarizer({ client });
  const res = await s.summarize([]);
  assert.ok(res);
  assert.equal(res!.summary, "");
  assert.equal(called, 0);
});

test("SPEC-091: repeat call with the same head returns the cached summary", async () => {
  let calls = 0;
  const client: RouterLlmClient = {
    classify: async () => {
      calls++;
      return "sum";
    },
  };
  const s = new Summarizer({ client });
  const head = [turn(0, "a"), turn(1, "b")];
  const first = await s.summarize(head);
  const second = await s.summarize(head);
  assert.equal(calls, 1, "second call must hit the cache");
  assert.equal(second!.fromCache, true);
  assert.equal(first!.summary, second!.summary);
});

test("SPEC-091: small growth (< 25%) reuses the cached summary without a re-call", async () => {
  let calls = 0;
  const client: RouterLlmClient = {
    classify: async () => {
      calls++;
      return "sum-" + calls;
    },
  };
  const s = new Summarizer({ client });
  const base = [bigTurn(0, 1000), bigTurn(1, 1000)];
  await s.summarize(base);
  // Growth well below 25%: append one tiny turn.
  const grown = [...base, turn(2, "tiny")];
  const res = await s.summarize(grown);
  assert.equal(calls, 1, "growth < 25% must not re-summarize");
  assert.equal(res!.fromCache, true);
});

test("SPEC-091: growth beyond 25% triggers a re-summarize", async () => {
  let calls = 0;
  const client: RouterLlmClient = {
    classify: async () => {
      calls++;
      return `sum-${calls}`;
    },
  };
  const s = new Summarizer({ client });
  const base = [bigTurn(0, 500), bigTurn(1, 500)];
  await s.summarize(base);
  // Grow by ≥ 25% — append a chunk larger than 25% of the original.
  const grown = [...base, bigTurn(2, 5000)];
  const res = await s.summarize(grown);
  assert.equal(calls, 2, "≥ 25% growth must re-summarize");
  assert.equal(res!.fromCache, false);
});

test("SPEC-091: LLM timeout returns null so the caller can fall back to raw", async () => {
  const client: RouterLlmClient = {
    classify: (input) =>
      new Promise<string>((_resolve, reject) => {
        input.signal.addEventListener("abort", () => reject(new Error("aborted")));
      }),
  };
  const s = new Summarizer({ client, timeoutMs: 20 });
  const res = await s.summarize([turn(0, "x"), turn(1, "y")]);
  assert.equal(res, null);
});

test("SPEC-091: LLM error returns null (fallback to raw)", async () => {
  const client: RouterLlmClient = {
    classify: async () => {
      throw new Error("boom");
    },
  };
  const s = new Summarizer({ client });
  const res = await s.summarize([turn(0, "x")]);
  assert.equal(res, null);
});

test("SPEC-091: empty LLM output falls back to null", async () => {
  const client: RouterLlmClient = {
    classify: async () => "   \n  ",
  };
  const s = new Summarizer({ client });
  const res = await s.summarize([turn(0, "x")]);
  assert.equal(res, null);
});

test("SPEC-091: buildSummarizerPrompt embeds the raw conversation block", () => {
  const prompt = buildSummarizerPrompt([turn(0, "hi")]);
  assert.match(prompt, /Conversation:\n<conversation>/);
  assert.match(prompt, /~200 words max/);
});
