// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-100: `CURSOR_API_KEY` never appears in RPC responses.
// This module owns every read of the key. Callers pass the result to the
// SDK and MUST NOT return it to the TUI or log it.

import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";

const KEY_ENV_VAR = "CURSOR_API_KEY";

export interface ApiKeySource {
  key: string;
  origin: "env" | "config";
}

export interface ReadApiKeyOptions {
  env?: NodeJS.ProcessEnv;
  configPath?: string;
  readFileImpl?: (p: string) => Promise<string>;
}

/**
 * Resolve the Cursor API key from environment or `~/.cusa/config.toml`.
 * Returns `null` when neither source is available. Never throws.
 */
export async function readApiKey(
  opts: ReadApiKeyOptions = {},
): Promise<ApiKeySource | null> {
  const env = opts.env ?? process.env;
  const envKey = env[KEY_ENV_VAR];
  if (typeof envKey === "string" && envKey.trim().length > 0) {
    return { key: envKey.trim(), origin: "env" };
  }
  const configPath =
    opts.configPath ?? path.join(homedir(), ".cusa", "config.toml");
  const reader = opts.readFileImpl ?? ((p) => readFile(p, "utf8"));
  try {
    const text = await reader(configPath);
    const key = parseApiKeyFromToml(text);
    if (key) return { key, origin: "config" };
  } catch {
    /* missing or unreadable is fine */
  }
  return null;
}

/**
 * Minimal parser for a single top-level `api_key = "…"` line. We intentionally
 * do not pull a full TOML dep; the file only carries a handful of scalars in
 * slice 1.
 */
export function parseApiKeyFromToml(text: string): string | null {
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.length === 0 || line.startsWith("#") || line.startsWith("[")) {
      continue;
    }
    const m = /^api_key\s*=\s*(?:"([^"]*)"|'([^']*)')\s*(?:#.*)?$/.exec(line);
    if (m) {
      const key = (m[1] ?? m[2] ?? "").trim();
      return key.length > 0 ? key : null;
    }
  }
  return null;
}

/**
 * Redact any occurrence of the given secret from an arbitrary string. Used
 * defensively in log lines. Passes through unchanged when `secret` is falsy
 * or shorter than 4 chars.
 */
export function redact(text: string, secret: string | undefined | null): string {
  if (!secret || secret.length < 4) return text;
  return text.split(secret).join("[redacted]");
}
