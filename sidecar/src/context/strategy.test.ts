// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_BYTE_BUDGET,
  DEFAULT_RAW_TURNS,
  pickStrategy,
} from "./strategy.ts";
import type { ConversationTurn } from "./format.ts";

function turn(index: number, bodyBytes: number): ConversationTurn {
  const body = "x".repeat(bodyBytes);
  return {
    index,
    userPrompt: body,
    assistantText: body,
    toolCallsSummary: [],
  };
}

function history(count: number, bytesPerTurn: number): ConversationTurn[] {
  const out: ConversationTurn[] = [];
  for (let i = 0; i < count; i++) out.push(turn(i, bytesPerTurn));
  return out;
}

// ----------------------------------------------------------------------
// SPEC-091: automatic byte-budget switch
// ----------------------------------------------------------------------

test("SPEC-091: default byte budget is 32 KiB and default raw window is 6", () => {
  assert.equal(DEFAULT_BYTE_BUDGET, 32 * 1024);
  assert.equal(DEFAULT_RAW_TURNS, 6);
});

test("SPEC-091: auto picks raw when the last-N turns fit inside the budget", () => {
  const h = history(4, 200);
  const d = pickStrategy({ forced: "auto", history: h });
  assert.equal(d.strategy, "raw");
  assert.equal(d.autoSwitchedToSummary, false);
  assert.equal(d.forced, false);
});

test("SPEC-091: auto switches to summary when raw > byte budget", () => {
  const h = history(6, 10 * 1024); // ~60 KiB total
  const d = pickStrategy({ forced: "auto", history: h });
  assert.equal(d.strategy, "summary");
  assert.equal(d.autoSwitchedToSummary, true);
});

test("SPEC-091: auto with empty history stays on raw (nothing to switch away from)", () => {
  const d = pickStrategy({ forced: "auto", history: [] });
  assert.equal(d.strategy, "raw");
  assert.equal(d.autoSwitchedToSummary, false);
});

// ----------------------------------------------------------------------
// SPEC-092: manual forced strategy overrides the picker
// ----------------------------------------------------------------------

test("SPEC-092: forced='raw' returns raw even when budget is exceeded", () => {
  const h = history(10, 10 * 1024);
  const d = pickStrategy({ forced: "raw", history: h });
  assert.equal(d.strategy, "raw");
  assert.equal(d.forced, true);
});

test("SPEC-092: forced='summary' returns summary even for tiny histories", () => {
  const d = pickStrategy({ forced: "summary", history: history(1, 10) });
  assert.equal(d.strategy, "summary");
  assert.equal(d.forced, true);
});

test("SPEC-092: forced='auto' restores the byte-budget-driven picker", () => {
  const large = history(6, 10 * 1024);
  const small = history(2, 10);
  assert.equal(pickStrategy({ forced: "auto", history: large }).strategy, "summary");
  assert.equal(pickStrategy({ forced: "auto", history: small }).strategy, "raw");
});

test("SPEC-092: overriding the budget changes the auto boundary", () => {
  const h = history(3, 1024);
  const dNarrow = pickStrategy({ forced: "auto", history: h, byteBudget: 100 });
  assert.equal(dNarrow.strategy, "summary");
  const dWide = pickStrategy({
    forced: "auto",
    history: h,
    byteBudget: 1024 * 1024,
  });
  assert.equal(dWide.strategy, "raw");
});
