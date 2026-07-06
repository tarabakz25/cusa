// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { loadLayeredMcpConfig } from "./loader.ts";

async function scaffold(): Promise<{
  cleanup: () => Promise<void>;
  userPath: string;
  projectPath: string;
  cwd: string;
}> {
  const base = await mkdtemp(path.join(tmpdir(), "cusa-mcp-"));
  const userDir = path.join(base, "user");
  const projectDir = path.join(base, "project");
  await mkdir(userDir, { recursive: true });
  await mkdir(projectDir, { recursive: true });
  return {
    userPath: path.join(userDir, "mcp.json"),
    projectPath: path.join(projectDir, "mcp.json"),
    cwd: projectDir,
    cleanup: () => rm(base, { recursive: true, force: true }),
  };
}

// ---- SPEC-040 -----------------------------------------------------------

test("SPEC-040: layered load merges user → project → inline (later wins)", async () => {
  const s = await scaffold();
  try {
    await writeFile(
      s.userPath,
      JSON.stringify({
        mcpServers: {
          shared: { command: "user-cmd" },
          userOnly: { command: "u" },
        },
      }),
      "utf8",
    );
    await writeFile(
      s.projectPath,
      JSON.stringify({
        mcpServers: {
          shared: { command: "project-cmd" },
          projectOnly: { command: "p" },
        },
      }),
      "utf8",
    );
    const loaded = await loadLayeredMcpConfig({
      cwd: s.cwd,
      inline: {
        shared: { command: "inline-cmd" },
        inlineOnly: { command: "i" },
      },
      userConfigPath: s.userPath,
      projectConfigPath: s.projectPath,
    });
    assert.equal(loaded.composed["shared"]!.command, "inline-cmd");
    assert.equal(loaded.composed["userOnly"]!.command, "u");
    assert.equal(loaded.composed["projectOnly"]!.command, "p");
    assert.equal(loaded.composed["inlineOnly"]!.command, "i");
    assert.equal(loaded.runtimes.get("shared")!.layer, "inline");
    assert.equal(loaded.runtimes.get("userOnly")!.layer, "user");
    assert.equal(loaded.runtimes.get("projectOnly")!.layer, "project");
  } finally {
    await s.cleanup();
  }
});

test("SPEC-040: missing files silently return empty layer", async () => {
  const loaded = await loadLayeredMcpConfig({
    cwd: "/definitely-not-here",
    userConfigPath: "/nope/user.json",
    projectConfigPath: "/nope/project.json",
  });
  assert.deepEqual(loaded.composed, {});
  assert.deepEqual(loaded.warnings, []);
});

test("SPEC-040: malformed JSON warns and treats the layer as empty", async () => {
  const s = await scaffold();
  try {
    await writeFile(s.projectPath, "{not-valid", "utf8");
    const warns: string[] = [];
    const loaded = await loadLayeredMcpConfig({
      cwd: s.cwd,
      userConfigPath: s.userPath,
      projectConfigPath: s.projectPath,
      onWarn: (m) => warns.push(m),
    });
    assert.deepEqual(loaded.composed, {});
    assert.ok(warns.some((w) => w.includes("parse error")));
  } finally {
    await s.cleanup();
  }
});

test("SPEC-040: accepts either { mcpServers: {...} } or a bare map", async () => {
  const s = await scaffold();
  try {
    await writeFile(
      s.projectPath,
      JSON.stringify({ bareServer: { command: "b" } }),
      "utf8",
    );
    const loaded = await loadLayeredMcpConfig({
      cwd: s.cwd,
      userConfigPath: s.userPath,
      projectConfigPath: s.projectPath,
    });
    assert.equal(loaded.composed["bareServer"]!.command, "b");
  } finally {
    await s.cleanup();
  }
});

// ---- SPEC-041 -----------------------------------------------------------

test("SPEC-041: session/create.mcpOverrides layers on top of project + user", async () => {
  const s = await scaffold();
  try {
    await writeFile(
      s.userPath,
      JSON.stringify({
        mcpServers: { user_srv: { command: "user" } },
      }),
      "utf8",
    );
    await writeFile(
      s.projectPath,
      JSON.stringify({
        mcpServers: {
          user_srv: { command: "project-override" },
          project_only: { command: "p" },
        },
      }),
      "utf8",
    );
    const loaded = await loadLayeredMcpConfig({
      cwd: s.cwd,
      userConfigPath: s.userPath,
      projectConfigPath: s.projectPath,
      inline: {
        mcpServers: {
          inline_only: { command: "i" },
          user_srv: { command: "inline-wins" },
        },
      },
    });
    assert.equal(loaded.composed["user_srv"]!.command, "inline-wins");
    assert.equal(loaded.composed["project_only"]!.command, "p");
    assert.equal(loaded.composed["inline_only"]!.command, "i");
    assert.equal(loaded.runtimes.get("user_srv")!.layer, "inline");
  } finally {
    await s.cleanup();
  }
});

test("SPEC-041: inline overrides accept a server-diff array with { id, enabled, config }", async () => {
  const s = await scaffold();
  try {
    await writeFile(
      s.projectPath,
      JSON.stringify({ existing: { command: "orig" } }),
      "utf8",
    );
    const loaded = await loadLayeredMcpConfig({
      cwd: s.cwd,
      userConfigPath: s.userPath,
      projectConfigPath: s.projectPath,
      inline: [
        { id: "added", enabled: true, config: { command: "new" } },
        { id: "existing", enabled: true, config: { command: "diff-replaces" } },
      ],
    });
    assert.equal(loaded.composed["added"]!.command, "new");
    assert.equal(loaded.composed["existing"]!.command, "diff-replaces");
  } finally {
    await s.cleanup();
  }
});

test("SPEC-041: inline diff-array entries with enabled=false are dropped from inline layer", async () => {
  const s = await scaffold();
  try {
    const loaded = await loadLayeredMcpConfig({
      cwd: s.cwd,
      userConfigPath: s.userPath,
      projectConfigPath: s.projectPath,
      inline: [
        { id: "gone", enabled: false, config: { command: "irrelevant" } },
        { id: "here", enabled: true, config: { command: "h" } },
      ],
    });
    assert.equal(loaded.composed["gone"], undefined);
    assert.equal(loaded.composed["here"]!.command, "h");
  } finally {
    await s.cleanup();
  }
});

test("SPEC-041: inline diff-array entries missing 'config' are skipped with a warning", async () => {
  const s = await scaffold();
  try {
    const warns: string[] = [];
    const loaded = await loadLayeredMcpConfig({
      cwd: s.cwd,
      userConfigPath: s.userPath,
      projectConfigPath: s.projectPath,
      inline: [{ id: "half", enabled: true } as unknown],
      onWarn: (m) => warns.push(m),
    });
    assert.deepEqual(loaded.composed, {});
    assert.ok(warns.some((w) => w.includes("half") && w.includes("no 'config'")));
  } finally {
    await s.cleanup();
  }
});
