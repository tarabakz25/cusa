// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// RpcServer unit tests. The server is driven with a pair of in-memory
// PassThrough streams (no real stdio), and every assertion targets the
// wire format sent back through the output stream.

import { test } from "node:test";
import assert from "node:assert/strict";
import { PassThrough } from "node:stream";

import {
  Method,
  PROTOCOL_VERSION,
  RpcErrorCode,
  type InitializeResult,
} from "./schema.js";
import { RpcMethodError, RpcServer } from "./server.js";

// ---------- Test harness --------------------------------------------------

interface WireFrame {
  jsonrpc: "2.0";
  id?: unknown;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

interface Harness {
  server: RpcServer;
  input: PassThrough;
  output: PassThrough;
  logs: string[];
  frames: () => WireFrame[];
  waitForFrames: (n: number, timeoutMs?: number) => Promise<WireFrame[]>;
  send: (frame: unknown) => void;
  runPromise: Promise<void>;
  shutdown: () => Promise<void>;
}

const tick = (ms = 0): Promise<void> =>
  new Promise((r) => {
    setTimeout(r, ms);
  });

function makeHarness(): Harness {
  const input = new PassThrough();
  const output = new PassThrough();
  const logs: string[] = [];
  const server = new RpcServer({
    input,
    output,
    log: (line) => logs.push(line),
  });

  const collected: WireFrame[] = [];
  let buffer = "";
  output.setEncoding("utf8");
  output.on("data", (chunk: string) => {
    buffer += chunk;
    let idx: number;
    // eslint-disable-next-line no-cond-assign
    while ((idx = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, idx).trim();
      buffer = buffer.slice(idx + 1);
      if (line.length === 0) continue;
      collected.push(JSON.parse(line) as WireFrame);
    }
  });

  const runPromise = server.run();

  function send(frame: unknown): void {
    input.write(JSON.stringify(frame) + "\n");
  }

  function frames(): WireFrame[] {
    return collected.slice();
  }

  async function waitForFrames(n: number, timeoutMs = 500): Promise<WireFrame[]> {
    const deadline = Date.now() + timeoutMs;
    while (collected.length < n && Date.now() < deadline) {
      await tick(5);
    }
    if (collected.length < n) {
      throw new Error(
        `waitForFrames: expected ${n} frames within ${timeoutMs}ms; got ${collected.length}`,
      );
    }
    return collected.slice();
  }

  async function shutdown(): Promise<void> {
    input.end();
    await runPromise;
  }

  return {
    server,
    input,
    output,
    logs,
    frames,
    waitForFrames,
    send,
    runPromise,
    shutdown,
  };
}

// ---------- Happy-path dispatch ------------------------------------------

test("responds to initialize with the handler result", async () => {
  const h = makeHarness();
  h.server.on(Method.Initialize, async () => {
    const result: InitializeResult = {
      protocolVersion: PROTOCOL_VERSION,
      sidecarVersion: "0.0.1",
      sdkVersion: "1.0.23",
      nodeVersion: process.versions.node,
      capabilities: {
        streaming: true,
        cancel: true,
        resume: true,
        sandbox: false,
        mcp: true,
        skills: true,
        routerLlm: false,
      },
    };
    return result;
  });

  h.send({
    jsonrpc: "2.0",
    id: 1,
    method: Method.Initialize,
    params: {
      protocolVersion: PROTOCOL_VERSION,
      clientInfo: { name: "cusa-tui", version: "0.0.1" },
    },
  });

  const [frame] = await h.waitForFrames(1);
  assert.equal(frame?.jsonrpc, "2.0");
  assert.equal(frame?.id, 1);
  const r = frame?.result as InitializeResult;
  assert.equal(r.protocolVersion, PROTOCOL_VERSION);
  assert.equal(r.sidecarVersion, "0.0.1");
  assert.equal(r.capabilities.streaming, true);

  await h.shutdown();
});

test("logs parse errors and does not emit an unattached response frame", async () => {
  // The JSON-RPC spec requires an `id` to attach a response to; a parse
  // error on a totally malformed input has no id, so the server logs it
  // and stays silent on the wire.
  const h = makeHarness();
  h.input.write("this is not json\n");
  await tick(20);

  assert.equal(h.frames().length, 0, "no response frame for id-less parse error");
  assert.ok(
    h.logs.some((l) => /parse error/i.test(l)),
    "parse error should be logged",
  );
  assert.ok(
    h.logs.some((l) => /unattached error/i.test(l)),
    "unattached error should be logged",
  );

  await h.shutdown();
});

test("returns MethodNotFound for unknown methods", async () => {
  const h = makeHarness();
  h.send({ jsonrpc: "2.0", id: "req-42", method: "no/such/method" });
  const [frame] = await h.waitForFrames(1);
  assert.equal(frame?.id, "req-42");
  assert.equal(frame?.error?.code, RpcErrorCode.MethodNotFound);
  assert.match(frame?.error?.message ?? "", /no\/such\/method/);
  await h.shutdown();
});

test("ignores non-request frames without responding", async () => {
  const h = makeHarness();
  // A stray response frame (no `method`) must be silently ignored.
  h.send({ jsonrpc: "2.0", id: 7, result: { ok: true } });
  // A completely bogus but valid JSON should not produce a response either.
  h.send({ hello: "world" });

  await tick(20);
  assert.equal(h.frames().length, 0);
  assert.ok(h.logs.some((l) => /ignoring non-request frame/i.test(l)));

  await h.shutdown();
});

test("wraps handler exceptions as internal errors", async () => {
  const h = makeHarness();
  h.server.on("boom", () => {
    throw new Error("kaboom");
  });
  h.send({ jsonrpc: "2.0", id: 1, method: "boom" });
  const [frame] = await h.waitForFrames(1);
  assert.equal(frame?.id, 1);
  assert.equal(frame?.error?.code, RpcErrorCode.InternalError);
  assert.match(frame?.error?.message ?? "", /kaboom/);
  await h.shutdown();
});

test("propagates RpcMethodError code + data to the client", async () => {
  const h = makeHarness();
  h.server.on("weighted", () => {
    throw new RpcMethodError(RpcErrorCode.NoApiKey, "set CURSOR_API_KEY", {
      envKey: "CURSOR_API_KEY",
    });
  });

  h.send({ jsonrpc: "2.0", id: "x", method: "weighted" });
  const [frame] = await h.waitForFrames(1);
  assert.equal(frame?.error?.code, RpcErrorCode.NoApiKey);
  assert.match(frame?.error?.message ?? "", /CURSOR_API_KEY/);
  assert.deepEqual(frame?.error?.data, { envKey: "CURSOR_API_KEY" });
  await h.shutdown();
});

test("handlers can push notifications back via context", async () => {
  const h = makeHarness();
  h.server.on("emit", (_p, ctx) => {
    ctx.sendNotification(Method.Log, {
      level: "info",
      message: "hi from handler",
    });
    return { ok: true };
  });

  h.send({ jsonrpc: "2.0", id: 1, method: "emit" });

  const frames = await h.waitForFrames(2);
  const note = frames.find((f) => f.method === Method.Log);
  const resp = frames.find((f) => f.id === 1);

  assert.ok(note, "notification frame must be emitted");
  assert.equal(note?.jsonrpc, "2.0");
  assert.equal(
    (note?.params as { message?: string } | undefined)?.message,
    "hi from handler",
  );
  assert.ok(resp, "response must still be emitted");
  assert.deepEqual(resp?.result, { ok: true });

  await h.shutdown();
});

test("notify() writes a well-formed notification frame", async () => {
  const h = makeHarness();
  h.server.notify(Method.StreamMessage, {
    runId: "r1",
    delta: "chunk",
    kind: "assistant",
  });

  const [frame] = await h.waitForFrames(1);
  assert.equal(frame?.jsonrpc, "2.0");
  assert.equal(frame?.method, Method.StreamMessage);
  assert.deepEqual(frame?.params, {
    runId: "r1",
    delta: "chunk",
    kind: "assistant",
  });
  assert.equal(frame?.id, undefined);

  await h.shutdown();
});

test("shutdown method closes the run loop after responding", async () => {
  const h = makeHarness();
  h.server.on(Method.Shutdown, () => ({ ok: true }));

  h.send({ jsonrpc: "2.0", id: 99, method: Method.Shutdown });

  // run() should resolve on its own once the shutdown handler runs.
  await h.runPromise;

  const [frame] = h.frames();
  assert.equal(frame?.id, 99);
  assert.deepEqual(frame?.result, { ok: true });
});

test("multiple concurrent requests all get responses", async () => {
  const h = makeHarness();
  h.server.on("fast", async () => ({ n: 1 }));
  h.server.on("slow", async () => {
    await tick(25);
    return { n: 2 };
  });

  h.send({ jsonrpc: "2.0", id: 1, method: "slow" });
  h.send({ jsonrpc: "2.0", id: 2, method: "fast" });

  const frames = await h.waitForFrames(2, 500);
  const ids = frames.map((f) => f.id).sort();
  assert.deepEqual(ids, [1, 2]);

  await h.shutdown();
});

test("input close resolves the run promise", async () => {
  const h = makeHarness();
  h.input.end();
  await h.runPromise;
  assert.ok(true, "run() resolved on close");
});

test("handles frames split across chunks", async () => {
  const h = makeHarness();
  h.server.on("echo", (p) => p);

  const frame = { jsonrpc: "2.0", id: 5, method: "echo", params: { a: 1 } };
  const s = JSON.stringify(frame) + "\n";
  // Write byte-by-byte to exercise the buffering path.
  for (const ch of s) h.input.write(ch);

  const [reply] = await h.waitForFrames(1);
  assert.equal(reply?.id, 5);
  assert.deepEqual(reply?.result, { a: 1 });

  await h.shutdown();
});

test("ignores blank lines silently", async () => {
  const h = makeHarness();
  h.input.write("\n\n\n");
  await tick(15);
  assert.equal(h.frames().length, 0);
  await h.shutdown();
});

test("RpcMethodError shape", () => {
  const err = new RpcMethodError(-32004, "unsupported", { sdk: "0.9" });
  assert.equal(err.name, "RpcMethodError");
  assert.equal(err.code, -32004);
  assert.equal(err.message, "unsupported");
  assert.deepEqual(err.data, { sdk: "0.9" });
  assert.ok(err instanceof Error);
});
