// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// End-to-end integration between SessionManager and ContextManager for
// SPEC-090..093. Drives the fake SDK adapter across multiple turns and
// asserts that the second turn's `systemContext` carries the first
// turn's replay in XML form.

import { test } from "node:test";
import assert from "node:assert/strict";

import { FakeSdkAdapter } from "../agent/sdkAdapter.fake.ts";
import { SessionManager } from "../agent/session.ts";
import { ContextManager } from "./index.ts";
import type { RouterLlmClient } from "../router/llm.ts";

interface Emitted {
  method: string;
  params: unknown;
}

function harness(opts: { context: ContextManager }) {
  const emitted: Emitted[] = [];
  const adapter = new FakeSdkAdapter();
  const mgr = new SessionManager({
    adapter,
    notify: (method, params) => emitted.push({ method, params }),
    readApiKey: async () => ({ key: "sk_test", origin: "env" }),
    context: opts.context,
  });
  return { adapter, mgr, emitted };
}

async function until(pred: () => boolean, timeoutMs = 800): Promise<void> {
  const start = Date.now();
  while (!pred()) {
    if (Date.now() - start > timeoutMs) {
      throw new Error(`predicate timed out after ${timeoutMs}ms`);
    }
    await new Promise((r) => setTimeout(r, 5));
  }
}

async function turnsFinished(emitted: Emitted[], n: number): Promise<void> {
  await until(
    () => emitted.filter((e) => e.method === "run/finished").length >= n,
    2000,
  );
}

// ----------------------------------------------------------------------
// SPEC-090
// ----------------------------------------------------------------------

test("SPEC-090: second turn receives a <conversation> replay of the first turn's user + assistant text", async () => {
  const context = new ContextManager();
  const { adapter, mgr, emitted } = harness({ context });
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    model: "composer-2.5",
  });
  adapter.script(
    {
      events: [
        { kind: "text-delta", delta: "Foo returns bar.", textKind: "assistant" },
      ],
      result: { status: "finished", model: "composer-2.5" },
    },
    {
      events: [
        { kind: "text-delta", delta: "Bar is now baz.", textKind: "assistant" },
      ],
      result: { status: "finished", model: "composer-2.5" },
    },
  );
  await mgr.sendMessage({ sessionId: create.sessionId, text: "what does foo do?" });
  await turnsFinished(emitted, 1);
  await mgr.sendMessage({ sessionId: create.sessionId, text: "now change bar" });
  await turnsFinished(emitted, 2);

  const first = adapter.state.sendCalls[0]!;
  assert.equal(first.systemContext, undefined, "first turn has no prior history");
  const second = adapter.state.sendCalls[1]!;
  assert.ok(second.systemContext, "second turn must inject the history block");
  assert.match(second.systemContext!, /<conversation>/);
  assert.match(second.systemContext!, /<turn role="user">what does foo do\?<\/turn>/);
  assert.match(
    second.systemContext!,
    /<turn role="assistant" model="composer-2\.5">Foo returns bar\.<\/turn>/,
  );
  // Current turn's user text must NOT appear in the injected context.
  assert.equal(
    second.systemContext!.includes("now change bar"),
    false,
    "current-turn prompt must be sent as the fresh user text, not injected",
  );
});

// ----------------------------------------------------------------------
// SPEC-092: /context strategy=raw|summary
// ----------------------------------------------------------------------

test("SPEC-092: setContextStrategy('summary') forces the summary path when a summarizer is wired", async () => {
  let summarizerCalls = 0;
  const summarizerClient: RouterLlmClient = {
    classify: async () => {
      summarizerCalls++;
      return "user asked about foo; assistant explained it.";
    },
  };
  const context = new ContextManager({ summarizerClient });
  const { adapter, mgr, emitted } = harness({ context });
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    model: "composer-2.5",
  });
  adapter.script(
    {
      events: [
        { kind: "text-delta", delta: "explanation", textKind: "assistant" },
      ],
      result: { status: "finished", model: "composer-2.5" },
    },
    {
      events: [
        { kind: "text-delta", delta: "explanation 2", textKind: "assistant" },
      ],
      result: { status: "finished", model: "composer-2.5" },
    },
    {
      events: [
        { kind: "text-delta", delta: "explanation 3", textKind: "assistant" },
      ],
      result: { status: "finished", model: "composer-2.5" },
    },
    {
      events: [
        { kind: "text-delta", delta: "explanation 4", textKind: "assistant" },
      ],
      result: { status: "finished", model: "composer-2.5" },
    },
  );
  await mgr.sendMessage({ sessionId: create.sessionId, text: "q1" });
  await turnsFinished(emitted, 1);
  await mgr.sendMessage({ sessionId: create.sessionId, text: "q2" });
  await turnsFinished(emitted, 2);
  await mgr.sendMessage({ sessionId: create.sessionId, text: "q3" });
  await turnsFinished(emitted, 3);

  // Force summary now.
  mgr.setContextStrategy({ sessionId: create.sessionId, strategy: "summary" });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "q4" });
  await turnsFinished(emitted, 4);

  const fourth = adapter.state.sendCalls[3]!;
  assert.ok(fourth.systemContext);
  assert.match(fourth.systemContext!, /<summary>/, "summary block must be present");
  assert.equal(summarizerCalls > 0, true, "summarizer must have been called");
});

test("SPEC-092: setContextStrategy('raw') sticks even when history is huge", async () => {
  // No summarizer client wired — if the picker forced summary and we
  // fell back, we'd see a warn log about fallback. This test locks the
  // strategy to raw explicitly.
  const context = new ContextManager({ byteBudget: 10 });
  const { adapter, mgr, emitted } = harness({ context });
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    model: "composer-2.5",
  });
  adapter.script(
    {
      events: [
        { kind: "text-delta", delta: "x".repeat(500), textKind: "assistant" },
      ],
      result: { status: "finished" },
    },
    {
      events: [
        { kind: "text-delta", delta: "y", textKind: "assistant" },
      ],
      result: { status: "finished" },
    },
  );
  await mgr.sendMessage({ sessionId: create.sessionId, text: "big turn" });
  await turnsFinished(emitted, 1);
  mgr.setContextStrategy({ sessionId: create.sessionId, strategy: "raw" });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "next" });
  await turnsFinished(emitted, 2);
  const second = adapter.state.sendCalls[1]!;
  assert.match(second.systemContext ?? "", /<conversation>/);
  assert.equal(
    (second.systemContext ?? "").includes("<summary>"),
    false,
    "raw strategy must not emit <summary>",
  );
});

// ----------------------------------------------------------------------
// SPEC-093
// ----------------------------------------------------------------------

test("SPEC-093: setUseNative(true) causes buildContext to skip manual injection", async () => {
  const context = new ContextManager();
  context.setUseNative(true);
  const { adapter, mgr, emitted } = harness({ context });
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  adapter.script(
    {
      events: [{ kind: "text-delta", delta: "hi", textKind: "assistant" }],
      result: { status: "finished" },
    },
    {
      events: [{ kind: "text-delta", delta: "again", textKind: "assistant" }],
      result: { status: "finished" },
    },
  );
  await mgr.sendMessage({ sessionId: create.sessionId, text: "hi" });
  await turnsFinished(emitted, 1);
  await mgr.sendMessage({ sessionId: create.sessionId, text: "again" });
  await turnsFinished(emitted, 2);
  assert.equal(
    adapter.state.sendCalls[1]!.systemContext,
    undefined,
    "native retention must skip manual injection",
  );
});

test("SPEC-090: session/dispose clears the ConversationHistory", async () => {
  const context = new ContextManager();
  const { adapter, mgr, emitted } = harness({ context });
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  adapter.script({
    events: [{ kind: "text-delta", delta: "hi", textKind: "assistant" }],
    result: { status: "finished" },
  });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "hi" });
  await turnsFinished(emitted, 1);
  assert.equal(context.historyFor(create.sessionId)?.length, 1);
  await mgr.disposeSession({ sessionId: create.sessionId });
  assert.equal(context.historyFor(create.sessionId), undefined);
});
