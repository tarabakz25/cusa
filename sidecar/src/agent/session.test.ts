// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import { FakeSdkAdapter } from "./sdkAdapter.fake.ts";
import { SessionManager, SessionRpcError } from "./session.ts";
import { RpcErrorCode } from "../rpc/schema.ts";

interface Emitted {
  method: string;
  params: unknown;
}

function newHarness(opts?: { apiKey?: string | null; sendTimeoutMs?: number }) {
  const emitted: Emitted[] = [];
  const notify = (method: string, params: unknown) =>
    emitted.push({ method, params });
  const adapter = new FakeSdkAdapter();
  const mgr = new SessionManager({
    adapter,
    notify,
    readApiKey: async () =>
      opts?.apiKey === null
        ? null
        : { key: opts?.apiKey ?? "sk_test", origin: "env" },
    ...(opts?.sendTimeoutMs === undefined
      ? {}
      : { sendTimeoutMs: opts.sendTimeoutMs }),
  });
  return { adapter, mgr, emitted };
}

async function until(pred: () => boolean, timeoutMs = 500): Promise<void> {
  const start = Date.now();
  while (!pred()) {
    if (Date.now() - start > timeoutMs) {
      throw new Error(`timed out waiting for predicate after ${timeoutMs}ms`);
    }
    await new Promise((r) => setTimeout(r, 5));
  }
}

// ----------------------------------------------------------------------
// SPEC-100
// ----------------------------------------------------------------------

test("SPEC-100: session/create rejects with NO_API_KEY when CURSOR_API_KEY is absent", async () => {
  const { mgr } = newHarness({ apiKey: null });
  await assert.rejects(
    async () => {
      await mgr.createSession({ cwd: "/tmp/repo" });
    },
    (err: unknown) => {
      assert.ok(err instanceof SessionRpcError);
      assert.equal((err as SessionRpcError).code, RpcErrorCode.NoApiKey);
      return true;
    },
  );
});

test("SPEC-100: api key never appears in any emitted RPC frame", async () => {
  const { mgr, adapter, emitted } = newHarness({ apiKey: "sk_leaky_secret" });
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    model: "composer-2.5",
  });
  adapter.script({
    events: [
      { kind: "text-delta", delta: "hello", textKind: "assistant" },
    ],
    result: { status: "finished" },
  });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "hi" });
  await until(() =>
    emitted.some((e) => e.method === "run/finished"),
  );
  for (const frame of emitted) {
    const serialized = JSON.stringify(frame);
    assert.ok(
      !serialized.includes("sk_leaky_secret"),
      `secret leaked in ${frame.method}: ${serialized}`,
    );
  }
});

// ----------------------------------------------------------------------
// SPEC-016 — models/list
// ----------------------------------------------------------------------

test("SPEC-016: models/list forwards the resolved api key to the SDK adapter", async () => {
  // The Cursor SDK falls back to process.env.CURSOR_API_KEY when no key is
  // passed, which breaks keys resolved from ~/.cusa/config.toml. The manager
  // must hand the resolved key to the adapter explicitly.
  const { mgr, adapter } = newHarness({ apiKey: "sk_from_config_toml" });
  const result = await mgr.listModels();
  assert.equal(result.models.length, 2);
  assert.deepEqual(adapter.state.listModelsKeys, ["sk_from_config_toml"]);
});

test("SPEC-016: models/list rejects with NO_API_KEY and caches on success", async () => {
  const missing = newHarness({ apiKey: null });
  await assert.rejects(
    () => missing.mgr.listModels(),
    (err: unknown) => {
      assert.ok(err instanceof SessionRpcError);
      assert.equal((err as SessionRpcError).code, RpcErrorCode.NoApiKey);
      return true;
    },
  );

  const ok = newHarness();
  await ok.mgr.listModels();
  await ok.mgr.listModels();
  assert.equal(
    ok.adapter.state.listModelsKeys.length,
    1,
    "second call must be served from the cache",
  );
});

// ----------------------------------------------------------------------
// SPEC-001
// ----------------------------------------------------------------------

test("SPEC-001: streams assistant deltas as stream/message notifications", async () => {
  const { mgr, adapter, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    model: "composer-2.5",
  });
  adapter.script({
    events: [
      { kind: "text-delta", delta: "Hel", textKind: "assistant" },
      { kind: "text-delta", delta: "lo", textKind: "assistant" },
      { kind: "text-delta", delta: " world", textKind: "assistant" },
    ],
    result: { status: "finished" },
  });
  const send = await mgr.sendMessage({
    sessionId: create.sessionId,
    text: "hi",
  });
  await until(() =>
    emitted.some((e) => e.method === "run/finished"),
  );
  const deltas = emitted
    .filter((e) => e.method === "stream/message")
    .map((e) => (e.params as { delta: string; runId: string }).delta);
  assert.deepEqual(deltas, ["Hel", "lo", " world"]);
  const routerLine = emitted.find((e) => e.method === "router/decision");
  assert.ok(routerLine, "should emit router/decision before streaming");
  const params = routerLine!.params as { runId: string };
  assert.equal(params.runId, send.runId);
});

// ----------------------------------------------------------------------
// SPEC-004
// ----------------------------------------------------------------------

test("SPEC-004: session/cancel drives run.cancel() and settles with cancelled status", async () => {
  const { mgr, adapter, emitted } = newHarness();
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  adapter.script({
    events: [{ kind: "text-delta", delta: "partial", textKind: "assistant" }],
    supportsCancel: true,
    hangUntilCancel: true,
  });
  const send = await mgr.sendMessage({
    sessionId: create.sessionId,
    text: "long task",
  });
  await until(() =>
    emitted.some((e) => e.method === "stream/message"),
  );
  await mgr.cancelRun({ sessionId: create.sessionId, runId: send.runId });
  await until(() =>
    emitted.some(
      (e) =>
        e.method === "run/finished" &&
        (e.params as { status: string }).status === "cancelled",
    ),
    2000,
  );
});

test("SPEC-004: session/cancel returns ok even when there is no active run", async () => {
  const { mgr } = newHarness();
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  const res = await mgr.cancelRun({
    sessionId: create.sessionId,
    runId: "never-existed",
  });
  assert.deepEqual(res, { ok: true });
});

// ----------------------------------------------------------------------
// SPEC-060 / SPEC-061 (integration)
// ----------------------------------------------------------------------

test("SPEC-060: cumulative usage across two turns accumulates stream/usage + run/finished", async () => {
  const { mgr, adapter, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    model: "composer-2.5",
  });
  adapter.script(
    {
      events: [
        { kind: "text-delta", delta: "a", textKind: "assistant" },
        {
          kind: "usage",
          usage: {
            inputTokens: 10,
            outputTokens: 5,
            cacheReadTokens: 0,
            cacheCreationTokens: 0,
            reasoningTokens: 0,
            totalTokens: 15,
          },
        },
      ],
      result: {
        status: "finished",
        usage: {
          inputTokens: 10,
          outputTokens: 5,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          reasoningTokens: 0,
          totalTokens: 15,
        },
      },
    },
    {
      events: [
        { kind: "text-delta", delta: "b", textKind: "assistant" },
        {
          kind: "usage",
          usage: {
            inputTokens: 20,
            outputTokens: 10,
            cacheReadTokens: 0,
            cacheCreationTokens: 0,
            reasoningTokens: 0,
            totalTokens: 30,
          },
        },
      ],
      result: {
        status: "finished",
        usage: {
          inputTokens: 20,
          outputTokens: 10,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          reasoningTokens: 0,
          totalTokens: 30,
        },
      },
    },
  );

  await mgr.sendMessage({ sessionId: create.sessionId, text: "first" });
  await until(
    () => emitted.filter((e) => e.method === "run/finished").length === 1,
  );
  await mgr.sendMessage({ sessionId: create.sessionId, text: "second" });
  await until(
    () => emitted.filter((e) => e.method === "run/finished").length === 2,
  );

  const finishes = emitted.filter((e) => e.method === "run/finished");
  const usages = emitted.filter((e) => e.method === "stream/usage");
  // per-turn deltas
  assert.equal(
    (finishes[0]!.params as { usage: { totalTokens: number } }).usage
      .totalTokens,
    15,
  );
  assert.equal(
    (finishes[1]!.params as { usage: { totalTokens: number } }).usage
      .totalTokens,
    30,
  );
  // stream/usage emitted twice mid-run
  assert.equal(usages.length, 2);
});

test("SPEC-061: run/finished carries the per-turn delta usage", async () => {
  const { mgr, adapter, emitted } = newHarness();
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  adapter.script({
    events: [
      {
        kind: "usage",
        usage: {
          inputTokens: 42,
          outputTokens: 8,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          reasoningTokens: 0,
          totalTokens: 50,
        },
      },
    ],
    result: {
      status: "finished",
      usage: {
        inputTokens: 42,
        outputTokens: 8,
        cacheReadTokens: 0,
        cacheCreationTokens: 0,
        reasoningTokens: 0,
        totalTokens: 50,
      },
    },
  });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "hi" });
  await until(() =>
    emitted.some((e) => e.method === "run/finished"),
  );
  const finished = emitted.find((e) => e.method === "run/finished");
  const params = finished!.params as {
    usage: { totalTokens: number; inputTokens: number };
  };
  assert.equal(params.usage.totalTokens, 50);
  assert.equal(params.usage.inputTokens, 42);
});

// ----------------------------------------------------------------------
// SPEC-071
// ----------------------------------------------------------------------

test("SPEC-071: SessionManager wires session/create → adapter.createAgent with model + settingSources", async () => {
  const { mgr, adapter } = newHarness();
  await mgr.createSession({
    cwd: "/tmp/repo",
    model: "composer-2.5",
    approvalMode: "full-auto",
    settingSources: ["user", "project"],
  });
  assert.equal(adapter.state.createCalls.length, 1);
  const call = adapter.state.createCalls[0]!;
  assert.equal(call.model, "composer-2.5");
  assert.equal(call.approvalMode, "full-auto");
  assert.deepEqual(call.settingSources, ["user", "project"]);
  assert.equal(call.apiKey, "sk_test");
});

test("SPEC-071: session/resume calls adapter.resumeAgent with the given agentId", async () => {
  const { mgr, adapter } = newHarness();
  const result = await mgr.resumeSession({
    agentId: "prev-agent-123",
    cwd: "/tmp/repo",
    approvalMode: "suggest",
    mcpOverrides: { foo: { command: "echo" } },
  });
  assert.equal(adapter.state.resumeCalls.length, 1);
  assert.equal(adapter.state.resumeCalls[0]!.agentId, "prev-agent-123");
  assert.equal(adapter.state.resumeCalls[0]!.opts.apiKey, "sk_test");
  assert.ok(result.sessionId.startsWith("sess_"));
});

test("SPEC-071: session/dispose forwards to adapter.dispose", async () => {
  const { mgr, adapter } = newHarness();
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  await mgr.disposeSession({ sessionId: create.sessionId });
  assert.equal(adapter.state.disposedAgentIds.length, 1);
});

// ----------------------------------------------------------------------
// Tool approval bridge (SPEC-020..023 groundwork)
// ----------------------------------------------------------------------

test("SPEC-022: write tool call in suggest mode emits tool/approvalRequest", async () => {
  const { mgr, adapter, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    approvalMode: "suggest",
  });
  adapter.script({
    events: [
      {
        kind: "tool-call",
        callId: "c1",
        name: "write",
        category: "write",
        args: { path: "foo.txt" },
      },
    ],
    result: { status: "finished" },
  });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "edit foo" });
  await until(() =>
    emitted.some((e) => e.method === "run/finished"),
  );
  assert.ok(emitted.some((e) => e.method === "tool/approvalRequest"));
});

test("SPEC-024: full-auto skips tool/approvalRequest for write tools", async () => {
  const { mgr, adapter, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    approvalMode: "full-auto",
  });
  adapter.script({
    events: [
      {
        kind: "tool-call",
        callId: "c1",
        name: "shell",
        category: "shell",
        args: { command: "ls" },
      },
    ],
    result: { status: "finished" },
  });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "run ls" });
  await until(() =>
    emitted.some((e) => e.method === "run/finished"),
  );
  assert.equal(
    emitted.filter((e) => e.method === "tool/approvalRequest").length,
    0,
  );
});

// ----------------------------------------------------------------------
// issue #5: agent.send() timeout
// ----------------------------------------------------------------------

test("issue #5: session/send rejects with AGENT_ERROR when agent.send() never resolves", async () => {
  const { mgr, adapter } = newHarness({ sendTimeoutMs: 25 });
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  adapter.hangNextSend = true;
  await assert.rejects(
    async () => {
      await mgr.sendMessage({ sessionId: create.sessionId, text: "hello" });
    },
    (err: unknown) => {
      assert.ok(err instanceof SessionRpcError);
      assert.equal((err as SessionRpcError).code, RpcErrorCode.AgentError);
      assert.match((err as SessionRpcError).message, /timed out after 25 ms/);
      return true;
    },
  );
});

test("issue #5: a timed-out send does not wedge the session — the next send succeeds", async () => {
  const { mgr, adapter, emitted } = newHarness({ sendTimeoutMs: 25 });
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  adapter.hangNextSend = true;
  await assert.rejects(async () => {
    await mgr.sendMessage({ sessionId: create.sessionId, text: "first" });
  });
  adapter.script({
    events: [{ kind: "text-delta", delta: "ok", textKind: "assistant" }],
    result: { status: "finished" },
  });
  const send = await mgr.sendMessage({
    sessionId: create.sessionId,
    text: "second",
  });
  assert.ok(send.runId);
  await until(() => emitted.some((e) => e.method === "run/finished"));
});

test("issue #5: a send that resolves within the budget is unaffected by the timer", async () => {
  const { mgr, adapter, emitted } = newHarness({ sendTimeoutMs: 5_000 });
  const create = await mgr.createSession({ cwd: "/tmp/repo" });
  adapter.script({
    events: [{ kind: "text-delta", delta: "hi", textKind: "assistant" }],
    result: { status: "finished" },
  });
  const send = await mgr.sendMessage({
    sessionId: create.sessionId,
    text: "quick",
  });
  assert.ok(send.runId);
  await until(() => emitted.some((e) => e.method === "run/finished"));
});
