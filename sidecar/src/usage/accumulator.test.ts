// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  addTokenUsage,
  emptyTokenUsage,
  TurnUsageTracker,
  UsageAccumulator,
} from "./accumulator.ts";

test("SPEC-060: UsageAccumulator sums cumulative usage across turns", () => {
  const acc = new UsageAccumulator();
  acc.add(
    {
      inputTokens: 100,
      outputTokens: 50,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      reasoningTokens: 0,
      totalTokens: 150,
    },
    "composer-2.5",
  );
  acc.add(
    {
      inputTokens: 40,
      outputTokens: 60,
      cacheReadTokens: 10,
      cacheCreationTokens: 5,
      reasoningTokens: 2,
      totalTokens: 100,
    },
    "claude-sonnet-4",
  );
  const snap = acc.snapshot();
  assert.equal(snap.totalTokens, 250);
  assert.equal(snap.inputTokens, 140);
  assert.equal(snap.outputTokens, 110);
  assert.equal(snap.cacheReadTokens, 10);
  assert.equal(snap.cacheCreationTokens, 5);
  assert.equal(snap.reasoningTokens, 2);
  assert.ok(snap.byModel);
  assert.equal(snap.byModel!["composer-2.5"]?.totalTokens, 150);
  assert.equal(snap.byModel!["claude-sonnet-4"]?.totalTokens, 100);
});

test("SPEC-061: TurnUsageTracker treats the latest stream/usage event as authoritative", () => {
  const t = new TurnUsageTracker();
  assert.equal(t.turnDelta().totalTokens, 0);
  t.observe({
    inputTokens: 1,
    outputTokens: 2,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    reasoningTokens: 0,
    totalTokens: 3,
  });
  t.observe({
    inputTokens: 5,
    outputTokens: 5,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    reasoningTokens: 0,
    totalTokens: 10,
  });
  assert.equal(t.turnDelta().totalTokens, 10);
});

test("SPEC-060: addTokenUsage adds two TokenUsage snapshots correctly", () => {
  const a = emptyTokenUsage();
  const b = {
    inputTokens: 7,
    outputTokens: 8,
    cacheReadTokens: 1,
    cacheCreationTokens: 2,
    reasoningTokens: 3,
    totalTokens: 15,
  };
  const sum = addTokenUsage(a, b);
  assert.deepEqual(sum, { ...b, byModel: undefined });
});
