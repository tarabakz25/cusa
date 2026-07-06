// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Router types shared by rule engine, config loader, LLM classifier, and
// the top-level Router class.
//
// SPEC IDs relevant here:
// - SPEC-010: RouteContext captures the shape the router consumes.
// - SPEC-011: RouterDecision is a Cursor SDK `ModelSelection` (`{ id }`)
//   projected onto our RPC schema's `model: string` field.

import type { RouterSource } from "../rpc/schema.js";

/**
 * Inputs collected from the current session/turn used by the router to
 * decide which model to route this turn to. Sidecar callers build one of
 * these from the SessionState + the incoming `session/send` params.
 */
export interface RouteContext {
  /** Raw user prompt text. */
  prompt: string;
  /** The current agent's model (before this turn). */
  currentModel?: string;
  /** Session's default model (as configured at `session/create`). */
  defaultModel?: string;
  /** Sticky per-session manual override (e.g. from `/model <id>`). */
  sessionManualModel?: string;
  /** Names of enabled skills — potentially informs rule matching. */
  enabledSkills?: string[];
}

/**
 * The router's per-turn decision. `model` is the id passed to the SDK's
 * `send({ model: { id } })`; `source` explains provenance for the TUI's
 * router line; `rationale` is the human-readable one-liner.
 */
export interface RouterDecision {
  model: string;
  rationale: string;
  source: RouterSource;
}

/**
 * A single classification rule. Rules fire in file order; first match wins.
 * Match predicates are additive — all populated fields must be satisfied.
 */
export interface RuleSpec {
  /** Stable id for logging (unique within a config). */
  name: string;
  /** Model to route to when the rule matches. */
  model: string;
  /** Rationale text passed to the TUI's router line. */
  rationale: string;
  /** Case-insensitive substring or /regex/-style patterns. */
  match: RuleMatch;
}

export interface RuleMatch {
  /** Any-of substrings (case-insensitive). */
  anyOf?: string[];
  /** All-of substrings (case-insensitive). */
  allOf?: string[];
  /** Explicit keyword tokens (word-boundary, case-insensitive). */
  keywords?: string[];
  /** Regex patterns (JS syntax). Matches if any regex matches. */
  regex?: string[];
  /** Minimum prompt byte length (post-trim). */
  minLength?: number;
  /** Maximum prompt byte length (post-trim). */
  maxLength?: number;
}

/**
 * Parsed router configuration. When no config file exists, an equivalent
 * default value is used (see `builtInDefaultConfig`).
 */
export interface RouterConfig {
  defaultModel: string;
  llmEnabled: boolean;
  llmTimeoutMs: number;
  llmClassifierModel: string;
  rules: RuleSpec[];
}
