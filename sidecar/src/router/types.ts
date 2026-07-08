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
  /**
   * Per-session routing mode override (issue #7). Absent → the config's
   * `local_classifier_enabled` decides between "auto" and "super-auto".
   */
  routerMode?: RouterMode;
  /**
   * Snapshot of raw `models/list` ids (may be stale or absent). Used in
   * super-auto mode to resolve family aliases to the newest concrete id
   * and to enforce the provider allowlist. Absent → resolution is a
   * passthrough.
   */
  catalogModels?: string[];
}

/**
 * Routing mode (issue #7). "auto" is the legacy pipeline
 * (rules → cloud LLM → fallback); "super-auto" adds the structural
 * gates, the local semantic classifier, latest-model resolution, and the
 * provider allowlist. Manual pinning is expressed via
 * `sessionManualModel`, not a mode.
 */
export type RouterMode = "auto" | "super-auto";

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
 * One exemplar group for the local semantic classifier (issue #7).
 * `examples` are embedded once; the incoming prompt is matched against
 * them by cosine similarity.
 */
export interface ExemplarSpec {
  /** Model (or family alias) to route to when an example is nearest. */
  model: string;
  /** Rationale surfaced on the TUI router line. */
  rationale: string;
  /** Representative prompts for this route. */
  examples: string[];
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
  /**
   * Startup default for Super Auto Mode: true → "super-auto",
   * false → legacy "auto". Rollback path: flip this back to false and
   * the pipeline is byte-identical to the pre-#7 behavior.
   */
  localClassifierEnabled: boolean;
  /** Cosine ≥ θ_high → local decision is final (no cloud call). */
  thetaHigh: number;
  /** θ_low ≤ cosine < θ_high → ambiguous band, escalate to cloud LLM. */
  thetaLow: number;
  /**
   * Pinned embedder id (role C in issue #7). NEVER auto-updated —
   * a version bump invalidates exemplar embeddings and thresholds.
   */
  embeddingModel: string;
  /**
   * Brand allowlist for auto routing (role A/B). Empty → allow all.
   * Manual `/model <id>` overrides are exempt (warn only).
   */
  allowedProviders: string[];
  /** Exemplar groups for the local classifier. Empty → built-ins. */
  exemplars: ExemplarSpec[];
}
