// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Layered MCP config loader (SPEC-040).
//
// Read order and precedence: inline > project > user.
// - user   ~/.cursor/mcp.json
// - project <cwd>/.cursor/mcp.json
// - inline  Session's `mcpOverrides` param
//
// Values are merged by server id. Later layers replace earlier layers'
// entries wholesale (per SDK docs: "inline replaces" — safer than
// merging sub-configs that may have inconsistent transport types).

import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";

import type { McpServerConfig, McpServerConfigMap, McpServerRuntime } from "./types.js";

export interface LoadMcpOptions {
  cwd: string;
  inline?: unknown;
  userConfigPath?: string;
  projectConfigPath?: string;
  readFileImpl?: (p: string) => Promise<string>;
  onWarn?: (msg: string) => void;
}

export interface LoadedMcpConfig {
  composed: McpServerConfigMap;
  runtimes: Map<string, McpServerRuntime>;
  layers: {
    user: McpServerConfigMap;
    project: McpServerConfigMap;
    inline: McpServerConfigMap;
  };
  warnings: string[];
}

/**
 * Read + parse the user and project `mcp.json` files (missing files
 * silently skipped), merge with inline overrides, and return the
 * composed map ready to hand to the SDK.
 */
export async function loadLayeredMcpConfig(
  opts: LoadMcpOptions,
): Promise<LoadedMcpConfig> {
  const warnings: string[] = [];
  const emitWarn = (m: string): void => {
    warnings.push(m);
    opts.onWarn?.(m);
  };
  const reader = opts.readFileImpl ?? ((p) => readFile(p, "utf8"));
  const userPath =
    opts.userConfigPath ?? path.join(homedir(), ".cursor", "mcp.json");
  const projectPath =
    opts.projectConfigPath ?? path.join(opts.cwd, ".cursor", "mcp.json");

  const user = await readLayer(userPath, reader, "user", emitWarn);
  const project = await readLayer(projectPath, reader, "project", emitWarn);
  const inline = normaliseServers(opts.inline, "inline", emitWarn);

  const composed: McpServerConfigMap = {};
  const runtimes = new Map<string, McpServerRuntime>();
  for (const [id, cfg] of Object.entries(user)) {
    composed[id] = cfg;
    runtimes.set(id, { id, config: cfg, layer: "user" });
  }
  for (const [id, cfg] of Object.entries(project)) {
    composed[id] = cfg;
    runtimes.set(id, { id, config: cfg, layer: "project" });
  }
  for (const [id, cfg] of Object.entries(inline)) {
    composed[id] = cfg;
    runtimes.set(id, { id, config: cfg, layer: "inline" });
  }
  return {
    composed,
    runtimes,
    layers: { user, project, inline },
    warnings,
  };
}

async function readLayer(
  filePath: string,
  reader: (p: string) => Promise<string>,
  layer: "user" | "project",
  emitWarn: (msg: string) => void,
): Promise<McpServerConfigMap> {
  let text: string;
  try {
    text = await reader(filePath);
  } catch {
    return {}; // missing file — silently skip
  }
  try {
    const raw = JSON.parse(text) as unknown;
    return normaliseServers(raw, layer, emitWarn);
  } catch (err) {
    emitWarn(
      `mcp.json parse error at ${filePath}: ${(err as Error).message}; ignoring`,
    );
    return {};
  }
}

/**
 * Accept multiple shapes:
 *   - `{ mcpServers: { id: {...} } }`
 *   - a bare `{ id: {...} }` map
 *   - an array of "server-diff" objects `{ id, enabled?, config? }`
 *     (SPEC-041 — inline `--mcp` payloads may use the diff shape).
 * Anything else is skipped with a warning.
 *
 * Diff-array entries with `enabled: false` are dropped (they mean the
 * user explicitly disabled a server for this session). Entries with
 * `enabled: true` but no `config` are also dropped since we have nothing
 * to hand to the SDK — callers are expected to enable/disable known
 * servers, not to enable an undefined one.
 */
function normaliseServers(
  raw: unknown,
  layer: string,
  emitWarn: (msg: string) => void,
): McpServerConfigMap {
  if (raw === undefined || raw === null) return {};
  if (Array.isArray(raw)) {
    return normaliseDiffArray(raw, layer, emitWarn);
  }
  if (typeof raw !== "object") {
    emitWarn(`${layer} mcp config was not an object; ignoring`);
    return {};
  }
  const obj = raw as Record<string, unknown>;
  let source: Record<string, unknown> = obj;
  if (obj["mcpServers"] && typeof obj["mcpServers"] === "object") {
    source = obj["mcpServers"] as Record<string, unknown>;
  }
  const out: McpServerConfigMap = {};
  for (const [id, cfg] of Object.entries(source)) {
    if (!cfg || typeof cfg !== "object" || Array.isArray(cfg)) {
      emitWarn(`${layer} mcp server '${id}' is not an object; skipping`);
      continue;
    }
    out[id] = cfg as McpServerConfig;
  }
  return out;
}

function normaliseDiffArray(
  raw: unknown[],
  layer: string,
  emitWarn: (msg: string) => void,
): McpServerConfigMap {
  const out: McpServerConfigMap = {};
  for (const entry of raw) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      emitWarn(`${layer} mcp diff entry was not an object; skipping`);
      continue;
    }
    const rec = entry as Record<string, unknown>;
    const id = rec["id"];
    if (typeof id !== "string" || id.length === 0) {
      emitWarn(`${layer} mcp diff entry missing 'id'; skipping`);
      continue;
    }
    const enabled = rec["enabled"];
    if (enabled === false) {
      // Explicit disable: drop from the composed map so the SDK doesn't
      // receive the server. Later layers (project/user) may still add
      // it back — that matches the layered precedence contract.
      continue;
    }
    const cfg = rec["config"];
    if (!cfg || typeof cfg !== "object" || Array.isArray(cfg)) {
      emitWarn(
        `${layer} mcp diff entry '${id}' has no 'config'; skipping`,
      );
      continue;
    }
    out[id] = cfg as McpServerConfig;
  }
  return out;
}
