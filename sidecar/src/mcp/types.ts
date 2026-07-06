// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// MCP loader shared types.

/**
 * A single MCP server configuration entry. Structurally compatible with
 * the SDK's `McpServerConfig` type but kept loose so tests can inject
 * fakes without pulling in the SDK typings.
 */
export interface McpServerConfig {
  type?: "stdio" | "http" | "sse";
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  url?: string;
  headers?: Record<string, string>;
}

export type McpServerConfigMap = Record<string, McpServerConfig>;

/**
 * Snapshot of the composed set of MCP servers active for a session. The
 * `layer` field tracks whether the server came from the user file, the
 * project file, or the caller-supplied inline overrides — useful for
 * `/mcp` display and for debugging.
 */
export interface McpServerRuntime {
  id: string;
  config: McpServerConfig;
  /** Which source layer contributed the final config. */
  layer: "inline" | "project" | "user";
}
