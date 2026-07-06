// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { McpManager, MCP_READY_TIMEOUT_MS } from "./index.ts";
import type { McpProbeClient } from "./index.ts";

async function scaffold(): Promise<{
  cleanup: () => Promise<void>;
  cwd: string;
  writeProjectConfig: (obj: unknown) => Promise<void>;
}> {
  const base = await mkdtemp(path.join(tmpdir(), "cusa-mcp-sess-"));
  const project = base;
  const dir = path.join(project, ".cursor");
  await mkdir(dir, { recursive: true });
  const write = async (obj: unknown) => {
    await writeFile(path.join(dir, "mcp.json"), JSON.stringify(obj), "utf8");
  };
  return {
    cwd: project,
    writeProjectConfig: write,
    cleanup: () => rm(base, { recursive: true, force: true }),
  };
}

// ---- SPEC-042 -----------------------------------------------------------

test("SPEC-042: mcp/list returns each server with detected transport and enumerated tools", async () => {
  const s = await scaffold();
  try {
    await s.writeProjectConfig({
      fs: { command: "npx", args: ["mcp-fs"] },
      web: { type: "http", url: "https://example.com/mcp" },
    });
    const probe: McpProbeClient = {
      listTools: async (id) => [{ name: `tool-${id}` }],
    };
    const mgr = new McpManager({ probe });
    const composed = await mgr.compose({ cwd: s.cwd });
    const list = await mgr.list({ composed, enabledIds: null });
    const byId = new Map(list.map((x) => [x.id, x]));
    assert.equal(byId.get("fs")!.transport, "stdio");
    assert.equal(byId.get("web")!.transport, "http");
    assert.equal(byId.get("fs")!.status, "ready");
    assert.deepEqual(byId.get("fs")!.tools, [{ name: "tool-fs" }]);
  } finally {
    await s.cleanup();
  }
});

test("SPEC-042: probe timeout marks server status = 'failed'", async () => {
  const s = await scaffold();
  try {
    await s.writeProjectConfig({ slow: { command: "sleep" } });
    const probe: McpProbeClient = {
      listTools: () => new Promise(() => {}), // never resolves
    };
    const mgr = new McpManager({
      probe,
      readyTimeoutMs: 30,
    });
    const composed = await mgr.compose({ cwd: s.cwd });
    const list = await mgr.list({ composed, enabledIds: null });
    assert.equal(list[0]!.status, "failed");
    assert.match(list[0]!.error ?? "", /did not respond/);
  } finally {
    await s.cleanup();
  }
});

test("SPEC-042: no probe wired → status 'ready' with empty tools", async () => {
  const s = await scaffold();
  try {
    await s.writeProjectConfig({ foo: { command: "foo" } });
    const mgr = new McpManager();
    const composed = await mgr.compose({ cwd: s.cwd });
    const list = await mgr.list({ composed, enabledIds: null });
    assert.equal(list[0]!.status, "ready");
    assert.equal(list[0]!.tools, undefined);
  } finally {
    await s.cleanup();
  }
});

test("SPEC-042: default ready timeout is 10 seconds", () => {
  assert.equal(MCP_READY_TIMEOUT_MS, 10_000);
});

// ---- SPEC-043 -----------------------------------------------------------

test("SPEC-043: disabled servers show status 'disabled' in mcp/list", async () => {
  const s = await scaffold();
  try {
    await s.writeProjectConfig({
      a: { command: "a" },
      b: { command: "b" },
    });
    const mgr = new McpManager();
    const composed = await mgr.compose({ cwd: s.cwd });
    const list = await mgr.list({
      composed,
      enabledIds: new Set(["a"]),
    });
    const byId = new Map(list.map((x) => [x.id, x]));
    assert.equal(byId.get("a")!.status, "ready");
    assert.equal(byId.get("b")!.status, "disabled");
    assert.equal(byId.get("b")!.enabled, false);
  } finally {
    await s.cleanup();
  }
});

test("SPEC-043: composeForTurn filters mcpServers by enabled ids", async () => {
  const s = await scaffold();
  try {
    await s.writeProjectConfig({
      a: { command: "a" },
      b: { command: "b" },
    });
    const mgr = new McpManager();
    const filtered = await mgr.composeForTurn({
      cwd: s.cwd,
      enabledIds: new Set(["b"]),
    });
    assert.deepEqual(Object.keys(filtered!), ["b"]);
  } finally {
    await s.cleanup();
  }
});

test("SPEC-043: composeForTurn with null enabledIds returns every server (all enabled)", async () => {
  const s = await scaffold();
  try {
    await s.writeProjectConfig({
      a: { command: "a" },
      b: { command: "b" },
    });
    const mgr = new McpManager();
    const all = await mgr.composeForTurn({
      cwd: s.cwd,
      enabledIds: null,
    });
    assert.deepEqual(Object.keys(all!).sort(), ["a", "b"]);
  } finally {
    await s.cleanup();
  }
});
