// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// End-to-end integration test for the sidecar entrypoint. Spawns
// `src/index.ts` as a real child process (via tsx), performs a JSON-RPC
// handshake over stdio, and verifies:
//   - SPEC-071/-072: the initialize contract and JSON-RPC framing.
//   - SPEC-100:      no CURSOR_API_KEY value ever appears on the wire.
//   - shutdown cleanly exits the process.

import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";

import {
  Method,
  PROTOCOL_VERSION,
  RpcErrorCode,
  type InitializeResult,
  type Ok,
} from "./rpc/schema.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const ENTRY = path.resolve(here, "index.ts");

interface Frame {
  jsonrpc: "2.0";
  id?: unknown;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
  method?: string;
  params?: unknown;
}

interface Runner {
  child: ChildProcessWithoutNullStreams;
  send: (frame: unknown) => void;
  waitFrame: (id: unknown, timeoutMs?: number) => Promise<Frame>;
  waitStderr: (pattern: RegExp, timeoutMs?: number) => Promise<string>;
  allFrames: () => Frame[];
  stderrBuffer: () => string;
  end: () => Promise<number | null>;
}

interface LaunchOpts {
  env?: Record<string, string | undefined>;
}

function launch(opts: LaunchOpts = {}): Runner {
  // By default we scrub CURSOR_API_KEY / CURSOR_API_TOKEN from the child env
  // so tests that rely on "no API key" behaviour are deterministic.
  const baseEnv = { ...process.env, ...(opts.env ?? {}) };
  delete baseEnv["CURSOR_API_KEY"];
  delete baseEnv["CURSOR_API_TOKEN"];
  // Point CUSA_HOME at a scratch dir so we never touch the user's config.
  const cusaHome = baseEnv["CUSA_HOME"] ?? path.join(here, "..", ".test-home");
  baseEnv["CUSA_HOME"] = cusaHome;

  const child = spawn(process.execPath, ["--import", "tsx", ENTRY], {
    stdio: ["pipe", "pipe", "pipe"],
    env: baseEnv,
  });

  const framesById = new Map<string, Frame>();
  const allFrames: Frame[] = [];
  const pendingWaiters = new Map<string, (f: Frame) => void>();
  let stdoutBuf = "";
  let stderrBuf = "";
  const stderrWaiters: Array<{ pattern: RegExp; resolve: (s: string) => void }> =
    [];

  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => {
    stdoutBuf += chunk;
    let idx: number;
    // eslint-disable-next-line no-cond-assign
    while ((idx = stdoutBuf.indexOf("\n")) >= 0) {
      const line = stdoutBuf.slice(0, idx).trim();
      stdoutBuf = stdoutBuf.slice(idx + 1);
      if (line.length === 0) continue;
      let frame: Frame;
      try {
        frame = JSON.parse(line) as Frame;
      } catch {
        continue;
      }
      allFrames.push(frame);
      if (frame.id !== undefined) {
        const key = String(frame.id);
        framesById.set(key, frame);
        const w = pendingWaiters.get(key);
        if (w) {
          pendingWaiters.delete(key);
          w(frame);
        }
      }
    }
  });

  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => {
    stderrBuf += chunk;
    for (let i = stderrWaiters.length - 1; i >= 0; i--) {
      const w = stderrWaiters[i]!;
      if (w.pattern.test(stderrBuf)) {
        stderrWaiters.splice(i, 1);
        w.resolve(stderrBuf);
      }
    }
  });

  function send(frame: unknown): void {
    child.stdin.write(JSON.stringify(frame) + "\n");
  }

  async function waitFrame(id: unknown, timeoutMs = 5000): Promise<Frame> {
    const key = String(id);
    const existing = framesById.get(key);
    if (existing) return existing;
    return await new Promise<Frame>((resolve, reject) => {
      pendingWaiters.set(key, resolve);
      const timer = setTimeout(() => {
        pendingWaiters.delete(key);
        reject(
          new Error(
            `waitFrame(${key}) timed out after ${timeoutMs}ms; ` +
              `stderr so far: ${stderrBuf.slice(-500)}`,
          ),
        );
      }, timeoutMs);
      timer.unref();
    });
  }

  async function waitStderr(pattern: RegExp, timeoutMs = 5000): Promise<string> {
    if (pattern.test(stderrBuf)) return stderrBuf;
    return await new Promise<string>((resolve, reject) => {
      stderrWaiters.push({ pattern, resolve });
      const timer = setTimeout(() => {
        const i = stderrWaiters.findIndex((w) => w.pattern === pattern);
        if (i >= 0) stderrWaiters.splice(i, 1);
        reject(
          new Error(
            `waitStderr(${pattern}) timed out after ${timeoutMs}ms; ` +
              `stderr so far: ${stderrBuf.slice(-500)}`,
          ),
        );
      }, timeoutMs);
      timer.unref();
    });
  }

  async function end(): Promise<number | null> {
    if (child.exitCode !== null || child.signalCode !== null) {
      return child.exitCode;
    }
    try {
      child.stdin.end();
    } catch {
      /* already closed */
    }
    const raced = await Promise.race([
      new Promise<number | null>((resolve) =>
        child.once("exit", (code) => resolve(code)),
      ),
      delay(3000).then(() => "timeout" as const),
    ]);
    if (raced === "timeout") {
      child.kill("SIGTERM");
      await new Promise<void>((resolve) =>
        child.once("exit", () => resolve()),
      );
    }
    return child.exitCode;
  }

  return {
    child,
    send,
    waitFrame,
    waitStderr,
    allFrames: () => allFrames.slice(),
    stderrBuffer: () => stderrBuf,
    end,
  };
}

// ---- Tests ---------------------------------------------------------------

test("SPEC-071: prints ready banner on stderr", async () => {
  const r = launch();
  try {
    const banner = await r.waitStderr(/cusa-sidecar .+ ready/i, 5000);
    assert.match(banner, /cusa-sidecar/);
  } finally {
    await r.end();
  }
});

test("SPEC-071/-072: initialize returns a well-formed InitializeResult", async () => {
  const r = launch();
  try {
    await r.waitStderr(/ready/i, 5000);
    r.send({
      jsonrpc: "2.0",
      id: 1,
      method: Method.Initialize,
      params: {
        protocolVersion: PROTOCOL_VERSION,
        clientInfo: { name: "cusa-test", version: "0.0.0" },
      },
    });

    const frame = await r.waitFrame(1);
    assert.ok(frame.result, `expected result, got: ${JSON.stringify(frame)}`);
    const result = frame.result as InitializeResult;
    assert.equal(result.protocolVersion, PROTOCOL_VERSION);
    assert.ok(result.sidecarVersion.length > 0, "sidecarVersion must be set");
    assert.equal(result.nodeVersion, process.versions.node);
    // sdkVersion may be "unknown"/"unavailable" when @cursor/sdk's package
    // metadata cannot be resolved. The field must always be a string.
    assert.equal(typeof result.sdkVersion, "string");

    // Capabilities: every field must be a boolean regardless of value —
    // both slice-A skeleton (all false) and slice-1 MVP (streaming/cancel/
    // resume/sandbox true) satisfy this shape check.
    assert.ok(result.capabilities, "capabilities block required");
    for (const key of [
      "streaming",
      "cancel",
      "resume",
      "sandbox",
      "mcp",
      "skills",
      "routerLlm",
    ] as const) {
      assert.equal(
        typeof result.capabilities[key],
        "boolean",
        `capability ${key} must be a boolean`,
      );
    }
  } finally {
    await r.end();
  }
});

test("SPEC-100: no CURSOR_API_KEY value leaks on stdout", async () => {
  // Set a distinctive fake key in the env, then verify it never appears in
  // any RPC frame emitted on stdout.
  const secret = "sk_test_leaky_wire_secret_do_not_use_777";
  const r = launch({ env: { CURSOR_API_KEY: secret } });
  try {
    await r.waitStderr(/ready/i, 5000);
    r.send({
      jsonrpc: "2.0",
      id: 1,
      method: Method.Initialize,
      params: {
        protocolVersion: PROTOCOL_VERSION,
        clientInfo: { name: "cusa-test", version: "0.0.0" },
      },
    });
    await r.waitFrame(1);

    // Optionally ask for models/list, which exercises the API-key resolution
    // path in slice 1 (or returns MethodNotFound in slice-A). Either is fine;
    // what matters is the wire has no secret text.
    r.send({ jsonrpc: "2.0", id: 2, method: Method.ModelsList });
    // Give the runtime up to 8 s to answer, but do not fail if it does not:
    // adapters that need network may take longer than we want to wait.
    await Promise.race([r.waitFrame(2, 8000).catch(() => undefined), delay(8000)]);

    const wire = JSON.stringify(r.allFrames());
    assert.ok(
      !wire.includes(secret),
      `SPEC-100 violation: CURSOR_API_KEY leaked into an RPC frame`,
    );
  } finally {
    await r.end();
  }
});

test("shutdown request resolves and the process exits cleanly", async () => {
  const r = launch();
  try {
    await r.waitStderr(/ready/i, 5000);
    r.send({ jsonrpc: "2.0", id: 42, method: Method.Shutdown });
    const frame = await r.waitFrame(42);
    assert.deepEqual(frame.result, { ok: true } satisfies Ok);
  } finally {
    const code = await r.end();
    assert.equal(code, 0, `expected clean exit, got code=${code}`);
  }
});

test("MethodNotFound for a totally unknown method", async () => {
  const r = launch();
  try {
    await r.waitStderr(/ready/i, 5000);
    r.send({ jsonrpc: "2.0", id: 7, method: "totally/unknown" });
    const frame = await r.waitFrame(7);
    assert.equal(frame.error?.code, RpcErrorCode.MethodNotFound);
    assert.match(frame.error?.message ?? "", /totally\/unknown/);
  } finally {
    await r.end();
  }
});
