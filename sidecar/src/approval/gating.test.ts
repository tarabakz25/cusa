// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Integration tests exercising the SessionManager approval-gating wiring
// (SPEC-020, SPEC-022, SPEC-023, SPEC-024). The SDK exposes no
// synchronous beforeToolCall hook (documented in handoff), so gating is
// observational — the sidecar emits `tool/approvalRequest` for the TUI's
// overlay and `tool/approvalResult` immediately after with `observed: true`.

import { test } from "node:test";
import assert from "node:assert/strict";

import { FakeSdkAdapter } from "../agent/sdkAdapter.fake.ts";
import { SessionManager } from "../agent/session.ts";

interface Emitted {
  method: string;
  params: unknown;
}

function newHarness() {
  const emitted: Emitted[] = [];
  const notify = (method: string, params: unknown) =>
    emitted.push({ method, params });
  const adapter = new FakeSdkAdapter();
  const mgr = new SessionManager({
    adapter,
    notify,
    readApiKey: async () => ({ key: "sk_test", origin: "env" }),
  });
  return { adapter, mgr, emitted };
}

async function until(pred: () => boolean, timeoutMs = 1000): Promise<void> {
  const start = Date.now();
  while (!pred()) {
    if (Date.now() - start > timeoutMs) {
      throw new Error(`timed out waiting for predicate after ${timeoutMs}ms`);
    }
    await new Promise((r) => setTimeout(r, 5));
  }
}

// ----------------------------------------------------------------------
// SPEC-020
// ----------------------------------------------------------------------

test("SPEC-020: session/create with full-auto passes sandboxOptions to the SDK", async () => {
  const { adapter, mgr } = newHarness();
  await mgr.createSession({
    cwd: "/tmp/repo",
    approvalMode: "full-auto",
  });
  const call = adapter.state.createCalls[0]!;
  assert.equal(call.approvalMode, "full-auto");
});

test("SPEC-020: default approval mode is 'suggest'", async () => {
  const { adapter, mgr } = newHarness();
  await mgr.createSession({ cwd: "/tmp/repo" });
  const call = adapter.state.createCalls[0]!;
  assert.equal(call.approvalMode, "suggest");
});

// ----------------------------------------------------------------------
// SPEC-022
// ----------------------------------------------------------------------

test("SPEC-022: suggest mode issues tool/approvalRequest for a write tool call", async () => {
  const { adapter, mgr, emitted } = newHarness();
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
  await mgr.sendMessage({ sessionId: create.sessionId, text: "edit" });
  await until(() => emitted.some((e) => e.method === "run/finished"));
  assert.ok(emitted.some((e) => e.method === "tool/approvalRequest"));
});

test("SPEC-022: (observational) tool/approvalResponse resolves the pending id and emits tool/approvalResult", async () => {
  const { adapter, mgr, emitted } = newHarness();
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
  await mgr.sendMessage({ sessionId: create.sessionId, text: "edit" });
  await until(() =>
    emitted.some((e) => e.method === "tool/approvalRequest"),
  );

  const req = emitted.find((e) => e.method === "tool/approvalRequest")!;
  const requestId = (req.params as { requestId: string }).requestId;

  // The observational result frame must have been emitted alongside
  // the approvalRequest (spec §Slice 4 approval gating).
  const result = emitted.find((e) => e.method === "tool/approvalResult");
  assert.ok(result, "expected observational tool/approvalResult");
  const rp = result!.params as {
    requestId: string;
    decision: string;
    observed: boolean;
  };
  assert.equal(rp.requestId, requestId);
  assert.equal(rp.decision, "prompt");
  assert.equal(rp.observed, true);

  // Responding still resolves the pending record (idempotent, no throw).
  const out = mgr.handleApprovalResponse({ requestId, decision: "approve" });
  assert.deepEqual(out, { ok: true });
});

test("SPEC-022: observational-only warning is emitted at most once per session", async () => {
  const { adapter, mgr, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    approvalMode: "suggest",
  });
  adapter.script(
    {
      events: [
        {
          kind: "tool-call",
          callId: "c1",
          name: "write",
          category: "write",
          args: {},
        },
      ],
      result: { status: "finished" },
    },
    {
      events: [
        {
          kind: "tool-call",
          callId: "c2",
          name: "write",
          category: "write",
          args: {},
        },
      ],
      result: { status: "finished" },
    },
  );
  await mgr.sendMessage({ sessionId: create.sessionId, text: "t1" });
  await until(
    () => emitted.filter((e) => e.method === "run/finished").length === 1,
  );
  await mgr.sendMessage({ sessionId: create.sessionId, text: "t2" });
  await until(
    () => emitted.filter((e) => e.method === "run/finished").length === 2,
  );
  const observationalWarns = emitted.filter(
    (e) =>
      e.method === "log" &&
      (e.params as { message?: string }).message?.includes("observational"),
  );
  assert.equal(observationalWarns.length, 1);
});

// ----------------------------------------------------------------------
// SPEC-023
// ----------------------------------------------------------------------

test("SPEC-023: auto-edit mode auto-approves read tools and prompts on shell", async () => {
  const { adapter, mgr, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    approvalMode: "auto-edit",
  });
  adapter.script({
    events: [
      {
        kind: "tool-call",
        callId: "c1",
        name: "read",
        category: "read",
        args: { path: "a.txt" },
      },
      {
        kind: "tool-call",
        callId: "c2",
        name: "shell",
        category: "shell",
        args: { command: "ls" },
      },
    ],
    result: { status: "finished" },
  });
  await mgr.sendMessage({ sessionId: create.sessionId, text: "run" });
  await until(() => emitted.some((e) => e.method === "run/finished"));
  // Exactly one approval request — for the shell call, not the read.
  const requests = emitted.filter((e) => e.method === "tool/approvalRequest");
  assert.equal(requests.length, 1);
  assert.equal((requests[0]!.params as { name: string }).name, "shell");
});

// ----------------------------------------------------------------------
// SPEC-024
// ----------------------------------------------------------------------

test("SPEC-024: full-auto suppresses tool/approvalRequest and passes sandbox to SDK", async () => {
  const { adapter, mgr, emitted } = newHarness();
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
  await until(() => emitted.some((e) => e.method === "run/finished"));
  assert.equal(
    emitted.filter((e) => e.method === "tool/approvalRequest").length,
    0,
  );
  // The adapter must have received approvalMode: "full-auto" (the
  // real SDK adapter maps that to `local.sandboxOptions.enabled=true`).
  const call = adapter.state.createCalls[0]!;
  assert.equal(call.approvalMode, "full-auto");
});

// ----------------------------------------------------------------------
// "always" cache
// ----------------------------------------------------------------------

test("SPEC-022: 'always' decision is cached and future prompts auto-approve", async () => {
  const { adapter, mgr, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    approvalMode: "suggest",
  });
  adapter.script(
    {
      events: [
        {
          kind: "tool-call",
          callId: "c1",
          name: "write",
          category: "write",
          args: {},
        },
      ],
      result: { status: "finished" },
    },
    {
      events: [
        {
          kind: "tool-call",
          callId: "c2",
          name: "write",
          category: "write",
          args: {},
        },
      ],
      result: { status: "finished" },
    },
  );

  await mgr.sendMessage({ sessionId: create.sessionId, text: "first" });
  await until(() =>
    emitted.some((e) => e.method === "tool/approvalRequest"),
  );
  const req1 = emitted.find((e) => e.method === "tool/approvalRequest")!;
  const rid1 = (req1.params as { requestId: string }).requestId;
  mgr.handleApprovalResponse({ requestId: rid1, decision: "always" });
  await until(
    () => emitted.filter((e) => e.method === "run/finished").length === 1,
  );

  const requestsBefore = emitted.filter(
    (e) => e.method === "tool/approvalRequest",
  ).length;
  await mgr.sendMessage({ sessionId: create.sessionId, text: "second" });
  await until(
    () => emitted.filter((e) => e.method === "run/finished").length === 2,
  );
  const requestsAfter = emitted.filter(
    (e) => e.method === "tool/approvalRequest",
  ).length;
  assert.equal(
    requestsAfter,
    requestsBefore,
    "no new approvalRequest should fire after 'always' cache seeded",
  );
});

// ----------------------------------------------------------------------
// session/setApprovalMode
// ----------------------------------------------------------------------

test("SPEC-020: setApprovalMode mutates the session mode and logs SDK live-toggle limitation", async () => {
  const { mgr, emitted } = newHarness();
  const create = await mgr.createSession({
    cwd: "/tmp/repo",
    approvalMode: "suggest",
  });
  const res = mgr.setApprovalMode({
    sessionId: create.sessionId,
    mode: "full-auto",
  });
  assert.equal(res.ok, true);
  assert.equal(res.liveSdkUpdate, false);
  const warnLog = emitted.find(
    (e) =>
      e.method === "log" &&
      (e.params as { message?: string }).message?.includes(
        "cannot live-toggle sandbox",
      ),
  );
  assert.ok(warnLog, "expected a log warn about live-toggle limitation");
});
