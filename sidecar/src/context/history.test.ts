// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import { ConversationHistory } from "./history.ts";

function seed(h: ConversationHistory, n: number): void {
  for (let i = 0; i < n; i++) {
    h.append({
      userPrompt: `u${i}`,
      assistantText: `a${i}`,
      toolCallsSummary: [`write t${i}`],
      model: "composer-2.5",
    });
  }
}

// ----------------------------------------------------------------------
// SPEC-090
// ----------------------------------------------------------------------

test("SPEC-090: append() records turns with monotonically increasing indices", () => {
  const h = new ConversationHistory();
  const a = h.append({ userPrompt: "a", assistantText: "A", toolCallsSummary: [] });
  const b = h.append({ userPrompt: "b", assistantText: "B", toolCallsSummary: [] });
  assert.equal(a.index, 0);
  assert.equal(b.index, 1);
  assert.equal(h.size(), 2);
});

test("SPEC-090: last(n) returns the last n turns (or fewer)", () => {
  const h = new ConversationHistory();
  seed(h, 5);
  assert.deepEqual(
    h.last(3).map((t) => t.userPrompt),
    ["u2", "u3", "u4"],
  );
  assert.equal(h.last(0).length, 0);
  // requesting more than we have returns the full history
  assert.equal(h.last(999).length, 5);
});

test("SPEC-090: append() defensively copies the tool summary array", () => {
  const h = new ConversationHistory();
  const tools = ["write t0"];
  h.append({ userPrompt: "u", assistantText: "a", toolCallsSummary: tools });
  tools.push("shell later");
  assert.deepEqual(h.last(1)[0]!.toolCallsSummary, ["write t0"]);
});

test("SPEC-090: clear() empties the history but preserves the running index", () => {
  const h = new ConversationHistory();
  h.append({ userPrompt: "a", assistantText: "A", toolCallsSummary: [] });
  h.append({ userPrompt: "b", assistantText: "B", toolCallsSummary: [] });
  h.clear();
  assert.equal(h.size(), 0);
  // A subsequent append should not reuse index 0.
  const t = h.append({ userPrompt: "c", assistantText: "C", toolCallsSummary: [] });
  assert.equal(t.index, 2);
});

// ----------------------------------------------------------------------
// SPEC-091
// ----------------------------------------------------------------------

test("SPEC-091: snapshotForSummary partitions into head + last-N tail", () => {
  const h = new ConversationHistory();
  seed(h, 5);
  const snap = h.snapshotForSummary(2);
  assert.deepEqual(
    snap.head.map((t) => t.userPrompt),
    ["u0", "u1", "u2"],
  );
  assert.deepEqual(
    snap.tail.map((t) => t.userPrompt),
    ["u3", "u4"],
  );
});

test("SPEC-091: snapshotForSummary with history shorter than tailSize yields empty head", () => {
  const h = new ConversationHistory();
  seed(h, 2);
  const snap = h.snapshotForSummary(4);
  assert.deepEqual(snap.head, []);
  assert.equal(snap.tail.length, 2);
});

test("SPEC-091: snapshotForSummary with tailSize=0 sends everything to head", () => {
  const h = new ConversationHistory();
  seed(h, 3);
  const snap = h.snapshotForSummary(0);
  assert.equal(snap.head.length, 3);
  assert.equal(snap.tail.length, 0);
});
