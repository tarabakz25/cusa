// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Router config loader + hot-reload watcher (SPEC-014).
//
// Design goals:
// - No external TOML dependency; we own a small tolerant subset parser
//   that accepts the exact grammar documented in the spec.
// - Hot reload: `watchRouterConfig` returns an object that the Router can
//   swap its config into atomically. Parse failures leave the previous
//   config in place and emit a `warn` log through the provided sink.
// - Missing config file → built-in defaults + a one-time `log` line.

import { readFile } from "node:fs/promises";
import { existsSync, watch, type FSWatcher } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

import { builtInDefaultRules } from "./rules.js";
import { BUILTIN_EMBEDDER_ID } from "./localClassifier.js";
import { providerOf } from "./modelResolver.js";
import type { ExemplarSpec, RouterConfig, RuleMatch, RuleSpec } from "./types.js";

/** Reasonable defaults when the user has no `~/.cusa/router.toml`. */
export function builtInDefaultConfig(): RouterConfig {
  return {
    defaultModel: "composer-2.5",
    llmEnabled: true,
    // NFR-1: LLM classification path must settle in ≤ 1500 ms p95, so the
    // hard timeout defaults to exactly that budget (was 5000; issue #7).
    llmTimeoutMs: 1500,
    llmClassifierModel: "composer-2.5",
    rules: [...builtInDefaultRules],
    // Super Auto Mode (issue #7) — OFF by default so the legacy "auto"
    // pipeline is untouched until the user opts in.
    localClassifierEnabled: false,
    thetaHigh: 0.55,
    thetaLow: 0.35,
    embeddingModel: BUILTIN_EMBEDDER_ID,
    allowedProviders: ["composer", "claude", "gpt", "gemini", "grok"],
    exemplars: [],
  };
}

export interface LoadRouterConfigOptions {
  /** Full path to router.toml. Defaults to `~/.cusa/router.toml`. */
  configPath?: string;
  /** File reader (test hook). */
  readFileImpl?: (p: string) => Promise<string>;
  /** Existence check (test hook). */
  existsImpl?: (p: string) => boolean;
  /**
   * Sink for one-time log lines (e.g. "no router.toml found; using
   * defaults"). Called at most once per `loadRouterConfig()` invocation.
   */
  log?: (level: "info" | "warn" | "error", message: string) => void;
}

export interface LoadedRouterConfig {
  config: RouterConfig;
  /** Path that was inspected (whether or not it existed). */
  path: string;
  /** True when the config came from a file; false when defaults. */
  fromFile: boolean;
  /** Non-fatal parse warnings, if any. */
  warnings: string[];
}

/**
 * Load and parse `~/.cusa/router.toml`. When missing or unreadable, the
 * built-in defaults are returned with `fromFile = false`.
 */
export async function loadRouterConfig(
  opts: LoadRouterConfigOptions = {},
): Promise<LoadedRouterConfig> {
  const configPath =
    opts.configPath ?? path.join(homedir(), ".cusa", "router.toml");
  const exists = (opts.existsImpl ?? existsSync)(configPath);
  if (!exists) {
    opts.log?.(
      "info",
      `no router.toml at ${configPath}; using built-in defaults. Run \`cusa router init\` to customize.`,
    );
    return {
      config: builtInDefaultConfig(),
      path: configPath,
      fromFile: false,
      warnings: [],
    };
  }
  const reader = opts.readFileImpl ?? ((p) => readFile(p, "utf8"));
  let text: string;
  try {
    text = await reader(configPath);
  } catch (err) {
    opts.log?.(
      "warn",
      `failed to read router.toml at ${configPath}: ${
        (err as Error).message
      }. Using defaults.`,
    );
    return {
      config: builtInDefaultConfig(),
      path: configPath,
      fromFile: false,
      warnings: [`read error: ${(err as Error).message}`],
    };
  }
  const parsed = parseRouterTomlSafe(text);
  if (parsed.errors.length > 0) {
    opts.log?.(
      "warn",
      `router.toml parse errors: ${parsed.errors.join("; ")}. Using defaults.`,
    );
    return {
      config: builtInDefaultConfig(),
      path: configPath,
      fromFile: false,
      warnings: parsed.errors,
    };
  }
  return {
    config: parsed.config,
    path: configPath,
    fromFile: true,
    warnings: parsed.warnings,
  };
}

export interface RouterConfigWatcher {
  /** Latest successfully parsed config. Mutates on subsequent reloads. */
  current(): RouterConfig;
  /** Stop watching; safe to call multiple times. */
  close(): void;
}

export interface WatchRouterConfigOptions extends LoadRouterConfigOptions {
  /** Called whenever the config is atomically swapped. */
  onReload?: (next: RouterConfig, path: string) => void;
  /** Debounce for `fs.watch` events (ms). Defaults to 200. */
  debounceMs?: number;
}

/**
 * Start watching `~/.cusa/router.toml` and hot-reload on change. If the
 * file is missing at start time, defaults are used and the watcher is
 * left inactive (creation is not detected — the user restarts cusa to
 * re-enable). If parsing fails on reload, the previous good config is
 * kept and a warning is logged.
 */
export async function watchRouterConfig(
  opts: WatchRouterConfigOptions = {},
): Promise<RouterConfigWatcher> {
  const debounce = opts.debounceMs ?? 200;
  let loaded = await loadRouterConfig(opts);
  const swap = (next: RouterConfig): void => {
    loaded = { ...loaded, config: next };
    opts.onReload?.(next, loaded.path);
  };

  let watcher: FSWatcher | null = null;
  if (loaded.fromFile) {
    let timer: ReturnType<typeof setTimeout> | null = null;
    try {
      watcher = watch(loaded.path, { persistent: false }, () => {
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => {
          timer = null;
          void loadRouterConfig(opts).then((next) => {
            if (next.fromFile) swap(next.config);
            // else: parse failed; log emitted inside loadRouterConfig.
          });
        }, debounce);
      });
    } catch (err) {
      opts.log?.(
        "warn",
        `fs.watch(${loaded.path}) failed: ${(err as Error).message}`,
      );
      watcher = null;
    }
  }
  return {
    current: () => loaded.config,
    close: () => {
      if (watcher) {
        try {
          watcher.close();
        } catch {
          /* ignore */
        }
        watcher = null;
      }
    },
  };
}

// ---------- TOML subset parser -------------------------------------------

interface ParseResult {
  config: RouterConfig;
  warnings: string[];
  errors: string[];
}

/**
 * Parse the strict router.toml subset the spec documents:
 *   default_model = "id"
 *   llm_enabled = true
 *   llm_timeout_ms = 1500
 *   llm_classifier_model = "id"
 *   local_classifier_enabled = true        # Super Auto Mode (issue #7)
 *   theta_high = 0.55
 *   theta_low = 0.35
 *   embedding_model = "builtin:hash-ngram-v1"   # pinned; never auto-updated
 *   allowed_providers = ["composer", "claude", "gpt", "gemini", "grok"]
 *
 *   [[rule]]
 *   name = "..."
 *   model = "..."
 *   rationale = "..."
 *   match = { any_of = [...], keywords = [...], min_length = 200 }
 *
 *   [[exemplars]]
 *   model = "claude-sonnet"                # family alias → resolved latest
 *   rationale = "long-form reasoning"
 *   examples = ["prove that ...", "design the architecture for ..."]
 *
 * We do not depend on a real TOML parser; the subset is small and
 * unambiguous. Unknown top-level keys are ignored with a warning.
 */
export function parseRouterTomlSafe(text: string): ParseResult {
  const warnings: string[] = [];
  const errors: string[] = [];
  const cfg = builtInDefaultConfig();
  cfg.rules = [];

  let inRule = false;
  let explicitAllowlist = false;
  let currentRule: Partial<{
    name: string;
    model: string;
    rationale: string;
    match: RuleMatch;
  }> | null = null;
  let inExemplar = false;
  let currentExemplar: Partial<ExemplarSpec> | null = null;

  const flush = () => {
    if (currentRule) {
      if (
        typeof currentRule.name !== "string" ||
        typeof currentRule.model !== "string" ||
        typeof currentRule.rationale !== "string" ||
        !currentRule.match
      ) {
        errors.push(
          `rule missing required fields: ${JSON.stringify(currentRule)}`,
        );
      } else {
        cfg.rules.push({
          name: currentRule.name,
          model: currentRule.model,
          rationale: currentRule.rationale,
          match: currentRule.match,
        });
      }
      currentRule = null;
    }
    if (currentExemplar) {
      if (
        typeof currentExemplar.model !== "string" ||
        typeof currentExemplar.rationale !== "string" ||
        !Array.isArray(currentExemplar.examples) ||
        currentExemplar.examples.length === 0
      ) {
        errors.push(
          `exemplar missing required fields: ${JSON.stringify(currentExemplar)}`,
        );
      } else {
        cfg.exemplars.push({
          model: currentExemplar.model,
          rationale: currentExemplar.rationale,
          examples: currentExemplar.examples,
        });
      }
      currentExemplar = null;
    }
  };

  const lines = text.split(/\r?\n/);
  for (let lineNo = 0; lineNo < lines.length; lineNo++) {
    const raw = lines[lineNo] ?? "";
    const line = stripComment(raw).trim();
    if (line.length === 0) continue;
    if (line === "[[rule]]") {
      flush();
      inRule = true;
      inExemplar = false;
      currentRule = { match: {} };
      continue;
    }
    if (line === "[[exemplars]]") {
      flush();
      inRule = false;
      inExemplar = true;
      currentExemplar = {};
      continue;
    }
    if (/^\[.+\]$/.test(line)) {
      flush();
      inRule = false;
      inExemplar = false;
      currentRule = null;
      currentExemplar = null;
      warnings.push(`ignoring unknown section ${line}`);
      continue;
    }
    const kv = /^([a-z_]+)\s*=\s*(.+)$/i.exec(line);
    if (!kv) {
      errors.push(`line ${lineNo + 1}: unrecognized syntax: ${line}`);
      continue;
    }
    const key = kv[1]!;
    const rest = kv[2]!.trim();
    if (inRule) {
      const value = parseScalarOrTable(rest, errors, lineNo + 1);
      if (value === undefined) continue;
      switch (key) {
        case "name":
        case "model":
        case "rationale":
          if (typeof value !== "string") {
            errors.push(`line ${lineNo + 1}: ${key} must be a string`);
          } else {
            (currentRule as Record<string, unknown>)[key] = value;
          }
          break;
        case "match":
          if (typeof value !== "object" || value === null || Array.isArray(value)) {
            errors.push(`line ${lineNo + 1}: match must be an inline table`);
          } else {
            currentRule!.match = normalizeMatchObject(
              value as Record<string, unknown>,
              errors,
              lineNo + 1,
            );
          }
          break;
        default:
          warnings.push(`unknown rule field '${key}' at line ${lineNo + 1}`);
      }
      continue;
    }
    if (inExemplar) {
      const value = parseScalarOrTable(rest, errors, lineNo + 1);
      if (value === undefined) continue;
      switch (key) {
        case "model":
        case "rationale":
          if (typeof value !== "string") {
            errors.push(`line ${lineNo + 1}: ${key} must be a string`);
          } else {
            (currentExemplar as Record<string, unknown>)[key] = value;
          }
          break;
        case "examples":
          if (
            !Array.isArray(value) ||
            value.some((x) => typeof x !== "string")
          ) {
            errors.push(
              `line ${lineNo + 1}: examples must be an array of strings`,
            );
          } else {
            currentExemplar!.examples = value as string[];
          }
          break;
        default:
          warnings.push(
            `unknown exemplar field '${key}' at line ${lineNo + 1}`,
          );
      }
      continue;
    }
    // Top-level scalar keys.
    const value = parseScalarOrTable(rest, errors, lineNo + 1);
    if (value === undefined) continue;
    switch (key) {
      case "default_model":
        if (typeof value !== "string") {
          errors.push(`line ${lineNo + 1}: default_model must be a string`);
        } else {
          cfg.defaultModel = value;
        }
        break;
      case "llm_enabled":
        if (typeof value !== "boolean") {
          errors.push(`line ${lineNo + 1}: llm_enabled must be a boolean`);
        } else {
          cfg.llmEnabled = value;
        }
        break;
      case "llm_timeout_ms":
        if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
          errors.push(
            `line ${lineNo + 1}: llm_timeout_ms must be a positive integer`,
          );
        } else {
          cfg.llmTimeoutMs = Math.floor(value);
        }
        break;
      case "llm_classifier_model":
        if (typeof value !== "string") {
          errors.push(
            `line ${lineNo + 1}: llm_classifier_model must be a string`,
          );
        } else {
          cfg.llmClassifierModel = value;
        }
        break;
      case "local_classifier_enabled":
        if (typeof value !== "boolean") {
          errors.push(
            `line ${lineNo + 1}: local_classifier_enabled must be a boolean`,
          );
        } else {
          cfg.localClassifierEnabled = value;
        }
        break;
      case "theta_high":
      case "theta_low":
        if (typeof value !== "number" || value < -1 || value > 1) {
          errors.push(
            `line ${lineNo + 1}: ${key} must be a number in [-1, 1]`,
          );
        } else if (key === "theta_high") {
          cfg.thetaHigh = value;
        } else {
          cfg.thetaLow = value;
        }
        break;
      case "embedding_model":
        if (typeof value !== "string") {
          errors.push(`line ${lineNo + 1}: embedding_model must be a string`);
        } else {
          cfg.embeddingModel = value;
        }
        break;
      case "allowed_providers": {
        if (!Array.isArray(value) || value.some((x) => typeof x !== "string")) {
          errors.push(
            `line ${lineNo + 1}: allowed_providers must be an array of strings`,
          );
        } else {
          cfg.allowedProviders = (value as string[]).map((p) =>
            p.toLowerCase(),
          );
          explicitAllowlist = true;
        }
        break;
      }
      default:
        warnings.push(`unknown top-level key '${key}' at line ${lineNo + 1}`);
    }
  }
  flush();
  validateConfig(cfg, warnings, errors, explicitAllowlist);
  return { config: cfg, warnings, errors };
}

/**
 * Post-parse validation (issue #7):
 * - θ_low ≤ θ_high — otherwise the ambiguous band is inverted (error).
 * - When (and only when) the file explicitly sets `allowed_providers`:
 *   `default_model` must belong to an allowed brand — loud config error
 *   (the loader then falls back to built-in defaults); rules / exemplars
 *   routing to a disallowed brand are dropped with a warning;
 *   `llm_classifier_model` outside the allowlist warns only. Configs
 *   without an explicit allowlist keep full backward compatibility with
 *   arbitrary model ids (the allowlist still filters the *catalog* in
 *   super-auto mode).
 */
function validateConfig(
  cfg: RouterConfig,
  warnings: string[],
  errors: string[],
  explicitAllowlist: boolean,
): void {
  if (cfg.thetaLow > cfg.thetaHigh) {
    errors.push(
      `theta_low (${cfg.thetaLow}) must be <= theta_high (${cfg.thetaHigh})`,
    );
  }
  if (!explicitAllowlist) return;
  const allowed = new Set(cfg.allowedProviders);
  const isAllowed = (id: string): boolean => {
    if (allowed.size === 0) return true;
    const brand = providerOf(id);
    return brand !== null && allowed.has(brand);
  };
  if (!isAllowed(cfg.defaultModel)) {
    errors.push(
      `default_model '${cfg.defaultModel}' is not in allowed_providers [${cfg.allowedProviders.join(", ")}]`,
    );
  }
  if (!isAllowed(cfg.llmClassifierModel)) {
    warnings.push(
      `llm_classifier_model '${cfg.llmClassifierModel}' is not in allowed_providers`,
    );
  }
  cfg.rules = cfg.rules.filter((r) => {
    if (isAllowed(r.model)) return true;
    warnings.push(
      `dropping rule '${r.name}': model '${r.model}' is not in allowed_providers`,
    );
    return false;
  });
  cfg.exemplars = cfg.exemplars.filter((e) => {
    if (isAllowed(e.model)) return true;
    warnings.push(
      `dropping exemplar for '${e.model}': not in allowed_providers`,
    );
    return false;
  });
}

function stripComment(line: string): string {
  // Only strip # outside of quoted regions. Cheap state machine.
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

/**
 * Parse a right-hand-side value: string, integer, boolean, array, or an
 * inline table.
 */
function parseScalarOrTable(
  s: string,
  errors: string[],
  lineNo: number,
): unknown {
  const t = s.trim();
  if (t.length === 0) {
    errors.push(`line ${lineNo}: empty value`);
    return undefined;
  }
  if (t.startsWith('"') || t.startsWith("'")) {
    const m = /^(["'])((?:\\.|[^\\])*)\1$/.exec(t);
    if (!m) {
      errors.push(`line ${lineNo}: malformed string ${t}`);
      return undefined;
    }
    return m[2]!.replace(/\\(.)/g, "$1");
  }
  if (t === "true" || t === "false") return t === "true";
  if (/^-?\d+(\.\d+)?$/.test(t)) return Number(t);
  if (t.startsWith("[")) return parseArray(t, errors, lineNo);
  if (t.startsWith("{")) return parseInlineTable(t, errors, lineNo);
  errors.push(`line ${lineNo}: unrecognized value ${t}`);
  return undefined;
}

function parseArray(s: string, errors: string[], lineNo: number): unknown[] {
  const trimmed = s.trim();
  if (!trimmed.startsWith("[") || !trimmed.endsWith("]")) {
    errors.push(`line ${lineNo}: malformed array ${s}`);
    return [];
  }
  const body = trimmed.slice(1, -1);
  const parts = splitTopLevel(body, ",");
  const arr: unknown[] = [];
  for (const p of parts) {
    const item = p.trim();
    if (item.length === 0) continue;
    const value = parseScalarOrTable(item, errors, lineNo);
    if (value !== undefined) arr.push(value);
  }
  return arr;
}

function parseInlineTable(
  s: string,
  errors: string[],
  lineNo: number,
): Record<string, unknown> {
  const trimmed = s.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) {
    errors.push(`line ${lineNo}: malformed inline table ${s}`);
    return {};
  }
  const body = trimmed.slice(1, -1);
  const parts = splitTopLevel(body, ",");
  const obj: Record<string, unknown> = {};
  for (const p of parts) {
    const item = p.trim();
    if (item.length === 0) continue;
    const kv = /^([a-z_]+)\s*=\s*(.+)$/i.exec(item);
    if (!kv) {
      errors.push(`line ${lineNo}: malformed inline entry '${item}'`);
      continue;
    }
    const key = kv[1]!;
    const val = parseScalarOrTable(kv[2]!, errors, lineNo);
    if (val !== undefined) obj[key] = val;
  }
  return obj;
}

/**
 * Split by delimiter at top level (respecting nested brackets / braces
 * / quotes).
 */
function splitTopLevel(s: string, delim: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let inStr: '"' | "'" | null = null;
  let acc = "";
  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;
    if (inStr) {
      acc += ch;
      if (ch === inStr) inStr = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      inStr = ch;
      acc += ch;
      continue;
    }
    if (ch === "[" || ch === "{") depth++;
    else if (ch === "]" || ch === "}") depth--;
    if (depth === 0 && ch === delim) {
      parts.push(acc);
      acc = "";
      continue;
    }
    acc += ch;
  }
  if (acc.length > 0) parts.push(acc);
  return parts;
}

function normalizeMatchObject(
  raw: Record<string, unknown>,
  errors: string[],
  lineNo: number,
): RuleMatch {
  const out: RuleMatch = {};
  for (const [k, v] of Object.entries(raw)) {
    switch (k) {
      case "any_of":
        out.anyOf = asStringArray(v, errors, lineNo, k);
        break;
      case "all_of":
        out.allOf = asStringArray(v, errors, lineNo, k);
        break;
      case "keywords":
        out.keywords = asStringArray(v, errors, lineNo, k);
        break;
      case "regex":
        out.regex = asStringArray(v, errors, lineNo, k);
        break;
      case "min_length":
        if (typeof v !== "number") {
          errors.push(`line ${lineNo}: match.min_length must be an integer`);
        } else {
          out.minLength = Math.floor(v);
        }
        break;
      case "max_length":
        if (typeof v !== "number") {
          errors.push(`line ${lineNo}: match.max_length must be an integer`);
        } else {
          out.maxLength = Math.floor(v);
        }
        break;
      default:
        errors.push(`line ${lineNo}: unknown match key '${k}'`);
    }
  }
  return out;
}

function asStringArray(
  v: unknown,
  errors: string[],
  lineNo: number,
  key: string,
): string[] {
  if (!Array.isArray(v)) {
    errors.push(`line ${lineNo}: match.${key} must be an array`);
    return [];
  }
  const out: string[] = [];
  for (const item of v) {
    if (typeof item !== "string") {
      errors.push(`line ${lineNo}: match.${key} entries must be strings`);
      continue;
    }
    out.push(item);
  }
  return out;
}

export type { RuleSpec };
