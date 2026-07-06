// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Public MCP API used by SessionManager (SPEC-040, SPEC-042, SPEC-043).
//
// Composition rules:
//   - `compose()` merges the layered files (inline > project > user).
//   - `composeForTurn()` additionally filters by the session's enabled
//     server ids, honouring the "disabled = excluded from mcpServers"
//     rule from the spec (SDK docs: inline mcpServers replace creation-
//     time entries wholesale).
//   - `list()` enumerates every composed server with a best-effort tool
//     list. Servers whose readiness handshake exceeds 10 s are marked
//     `failed` (SPEC-042 edge case).

import { loadLayeredMcpConfig } from "./loader.js";
import type {
  McpServerConfig,
  McpServerConfigMap,
  McpServerRuntime,
} from "./types.js";
import type {
  McpServerInfo,
  McpServerStatus,
  McpToolInfo,
} from "../rpc/schema.js";

export {
  loadLayeredMcpConfig,
  type LoadMcpOptions,
  type LoadedMcpConfig,
} from "./loader.js";
export type { McpServerConfig, McpServerConfigMap, McpServerRuntime } from "./types.js";

/**
 * Client that can enumerate tools per configured server. In production
 * we'll layer this over the SDK's MCP handshake once it exposes a
 * tool-listing API; today it's stubbed and tests inject fakes.
 */
export interface McpProbeClient {
  listTools(
    serverId: string,
    config: McpServerConfig,
  ): Promise<McpToolInfo[]>;
}

/** Timeout for status = "failed" per SPEC-042 edge case. */
export const MCP_READY_TIMEOUT_MS = 10_000;

export interface McpManagerOptions {
  log?: (level: "info" | "warn" | "error", msg: string) => void;
  probe?: McpProbeClient;
  /** Override the 10 s ready-timeout in tests. */
  readyTimeoutMs?: number;
}

export class McpManager {
  private readonly log: (level: "info" | "warn" | "error", msg: string) => void;
  private readonly probe?: McpProbeClient;
  private readonly readyTimeoutMs: number;
  private lastComposedMap: McpServerConfigMap | null = null;
  private lastRuntimes: Map<string, McpServerRuntime> | null = null;

  constructor(opts: McpManagerOptions = {}) {
    this.log = opts.log ?? (() => {});
    if (opts.probe !== undefined) this.probe = opts.probe;
    this.readyTimeoutMs = opts.readyTimeoutMs ?? MCP_READY_TIMEOUT_MS;
  }

  /** Compose the layered map without any per-session enable/disable filter. */
  async compose(args: {
    cwd: string;
    inline?: unknown;
  }): Promise<McpServerConfigMap | undefined> {
    const loaded = await loadLayeredMcpConfig({
      cwd: args.cwd,
      inline: args.inline,
      onWarn: (m) => this.log("warn", `mcp: ${m}`),
    });
    this.lastComposedMap = loaded.composed;
    this.lastRuntimes = loaded.runtimes;
    if (Object.keys(loaded.composed).length === 0) return undefined;
    return loaded.composed;
  }

  /** Compose + filter for a per-turn `agent.send({ mcpServers })` call. */
  async composeForTurn(args: {
    cwd: string;
    inline?: unknown;
    enabledIds: Set<string> | null;
  }): Promise<McpServerConfigMap | undefined> {
    const composed = await this.compose({
      cwd: args.cwd,
      inline: args.inline,
    });
    if (!composed) return undefined;
    if (args.enabledIds === null) return composed; // all enabled
    const filtered: McpServerConfigMap = {};
    for (const [id, cfg] of Object.entries(composed)) {
      if (args.enabledIds.has(id)) filtered[id] = cfg;
    }
    return Object.keys(filtered).length > 0 ? filtered : undefined;
  }

  lastComposed(): McpServerConfigMap | null {
    return this.lastComposedMap;
  }

  /**
   * Enumerate servers for `mcp/list`. The `enabledIds` set follows the
   * same "null = all enabled" convention as `composeForTurn`.
   */
  async list(args: {
    composed: McpServerConfigMap | undefined;
    enabledIds: Set<string> | null;
  }): Promise<McpServerInfo[]> {
    const composed = args.composed ?? {};
    const ids = Object.keys(composed).sort();
    const out: McpServerInfo[] = [];
    for (const id of ids) {
      const cfg = composed[id]!;
      const enabled =
        args.enabledIds === null ? true : args.enabledIds.has(id);
      const transport = detectTransport(cfg);
      let status: McpServerStatus = enabled ? "ready" : "disabled";
      let tools: McpToolInfo[] = [];
      let error: string | undefined;
      if (enabled) {
        if (this.probe) {
          const probed = await this.probeWithTimeout(id, cfg);
          if (probed.ok) {
            tools = probed.tools;
          } else {
            status = "failed";
            error = probed.error;
          }
        } else {
          // No probe wired — fall back to "ready with empty tools" as
          // documented in the spec.
          tools = [];
        }
      }
      const entry: McpServerInfo = {
        id,
        transport,
        status,
        enabled,
      };
      if (tools.length > 0) entry.tools = tools;
      if (error) entry.error = error;
      out.push(entry);
    }
    return out;
  }

  private async probeWithTimeout(
    id: string,
    config: McpServerConfig,
  ): Promise<{ ok: true; tools: McpToolInfo[] } | { ok: false; error: string }> {
    const start = this.probe!.listTools(id, config);
    const timer = new Promise<{ ok: false; error: string }>((resolve) => {
      const t = setTimeout(() => {
        resolve({
          ok: false,
          error: `mcp server '${id}' did not respond within ${this.readyTimeoutMs}ms`,
        });
      }, this.readyTimeoutMs);
      // Unref so the timer never keeps Node alive.
      (t as { unref?: () => void }).unref?.();
    });
    try {
      const tools = await Promise.race([
        start.then((t) => ({ ok: true as const, tools: t })),
        timer,
      ]);
      return tools;
    } catch (err) {
      return { ok: false, error: (err as Error).message ?? "probe failed" };
    }
  }
}

function detectTransport(cfg: McpServerConfig): string {
  if (cfg.type) return cfg.type;
  if (cfg.url) return "http";
  if (cfg.command) return "stdio";
  return "unknown";
}
