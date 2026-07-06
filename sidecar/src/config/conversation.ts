// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Parser for the `[conversation]` block in `~/.cusa/config.toml`
// (SPEC-093 config knobs).
//
// Expected shape:
//   [conversation]
//   mode = "auto"                # "auto" | "manual" | "native"
//   raw_turns = 6
//   byte_budget = 32768
//   summarizer_timeout_ms = 8000
//   summarizer_model = "composer-2.5"
//
// Missing file or missing section → built-in defaults. Malformed values
// warn once and fall back to the default. Parser is deliberately tiny
// so we don't pull in a TOML dependency for a handful of scalars.

import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";

import type { ConversationMode } from "../context/feature_detect.js";
import {
  DEFAULT_BYTE_BUDGET,
  DEFAULT_RAW_TURNS,
} from "../context/strategy.js";
import {
  DEFAULT_SUMMARIZER_MODEL,
  DEFAULT_SUMMARIZER_TIMEOUT_MS,
} from "../context/summarizer.js";

export interface ConversationConfig {
  mode: ConversationMode;
  rawTurns: number;
  byteBudget: number;
  summarizerTimeoutMs: number;
  summarizerModel: string;
}

export function defaultConversationConfig(): ConversationConfig {
  return {
    mode: "auto",
    rawTurns: DEFAULT_RAW_TURNS,
    byteBudget: DEFAULT_BYTE_BUDGET,
    summarizerTimeoutMs: DEFAULT_SUMMARIZER_TIMEOUT_MS,
    summarizerModel: DEFAULT_SUMMARIZER_MODEL,
  };
}

export interface LoadConversationConfigOptions {
  configPath?: string;
  readFileImpl?: (p: string) => Promise<string>;
  log?: (level: "info" | "warn" | "error", msg: string) => void;
}

export interface LoadedConversationConfig {
  config: ConversationConfig;
  path: string;
  fromFile: boolean;
  warnings: string[];
}

export async function loadConversationConfig(
  opts: LoadConversationConfigOptions = {},
): Promise<LoadedConversationConfig> {
  const configPath =
    opts.configPath ?? path.join(homedir(), ".cusa", "config.toml");
  const reader = opts.readFileImpl ?? ((p) => readFile(p, "utf8"));
  let text: string;
  try {
    text = await reader(configPath);
  } catch {
    return {
      config: defaultConversationConfig(),
      path: configPath,
      fromFile: false,
      warnings: [],
    };
  }
  const parsed = parseConversationSection(text);
  for (const w of parsed.warnings) {
    opts.log?.("warn", `config.toml [conversation]: ${w}`);
  }
  return {
    config: parsed.config,
    path: configPath,
    fromFile: parsed.found,
    warnings: parsed.warnings,
  };
}

interface ParseResult {
  config: ConversationConfig;
  warnings: string[];
  found: boolean;
}

/**
 * Extract just the `[conversation]` scalars — the file may also carry
 * `api_key`, other unrelated sections, etc.
 */
export function parseConversationSection(text: string): ParseResult {
  const cfg = defaultConversationConfig();
  const warnings: string[] = [];
  let found = false;
  let inSection = false;
  for (const raw of text.split(/\r?\n/)) {
    const line = stripComment(raw).trim();
    if (line.length === 0) continue;
    if (/^\[[^\]]+\]$/.test(line)) {
      inSection = line === "[conversation]";
      if (inSection) found = true;
      continue;
    }
    if (!inSection) continue;
    const m = /^([a-z_]+)\s*=\s*(.+)$/i.exec(line);
    if (!m) {
      warnings.push(`unrecognized line: ${line}`);
      continue;
    }
    const key = m[1]!;
    const rest = m[2]!.trim();
    const val = parseScalar(rest);
    if (val === undefined) {
      warnings.push(`unrecognized value for ${key}`);
      continue;
    }
    switch (key) {
      case "mode": {
        if (typeof val !== "string") {
          warnings.push("mode must be a string");
        } else if (val === "auto" || val === "manual" || val === "native") {
          cfg.mode = val;
        } else {
          warnings.push(`invalid mode '${val}' (expected auto|manual|native)`);
        }
        break;
      }
      case "raw_turns":
        if (typeof val !== "number" || val <= 0) {
          warnings.push("raw_turns must be a positive integer");
        } else {
          cfg.rawTurns = Math.floor(val);
        }
        break;
      case "byte_budget":
        if (typeof val !== "number" || val <= 0) {
          warnings.push("byte_budget must be a positive integer");
        } else {
          cfg.byteBudget = Math.floor(val);
        }
        break;
      case "summarizer_timeout_ms":
        if (typeof val !== "number" || val <= 0) {
          warnings.push("summarizer_timeout_ms must be a positive integer");
        } else {
          cfg.summarizerTimeoutMs = Math.floor(val);
        }
        break;
      case "summarizer_model":
        if (typeof val !== "string" || val.length === 0) {
          warnings.push("summarizer_model must be a non-empty string");
        } else {
          cfg.summarizerModel = val;
        }
        break;
      default:
        warnings.push(`unknown key '${key}' in [conversation]`);
    }
  }
  return { config: cfg, warnings, found };
}

function stripComment(line: string): string {
  let out = "";
  let inStr: '"' | "'" | null = null;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i]!;
    if (inStr) {
      out += ch;
      if (ch === inStr) inStr = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      inStr = ch;
      out += ch;
      continue;
    }
    if (ch === "#") break;
    out += ch;
  }
  return out;
}

function parseScalar(s: string): unknown {
  const t = s.trim();
  if (t.length === 0) return undefined;
  if (t.startsWith('"') || t.startsWith("'")) {
    const m = /^(["'])((?:\\.|[^\\])*)\1$/.exec(t);
    if (!m) return undefined;
    return m[2]!.replace(/\\(.)/g, "$1");
  }
  if (t === "true" || t === "false") return t === "true";
  if (/^-?\d+$/.test(t)) return Number(t);
  return undefined;
}
