// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Rule engine used by the Router (SPEC-010, SPEC-013). Given a
// `RouteContext` and a list of `RuleSpec`, return the first rule that
// matches, or `null`.

import type { RouteContext, RuleMatch, RuleSpec } from "./types.js";

/**
 * Return the first matching rule, or `null` if no rule matches.
 * Traversal order matches the config's declaration order.
 */
export function firstMatchingRule(
  ctx: RouteContext,
  rules: readonly RuleSpec[],
): RuleSpec | null {
  const prompt = ctx.prompt.trim();
  for (const rule of rules) {
    if (matches(rule.match, prompt)) return rule;
  }
  return null;
}

function matches(m: RuleMatch, prompt: string): boolean {
  const lower = prompt.toLowerCase();
  const len = prompt.length;

  if (m.minLength !== undefined && len < m.minLength) return false;
  if (m.maxLength !== undefined && len > m.maxLength) return false;

  if (m.anyOf !== undefined && m.anyOf.length > 0) {
    let any = false;
    for (const s of m.anyOf) {
      if (lower.includes(s.toLowerCase())) {
        any = true;
        break;
      }
    }
    if (!any) return false;
  }

  if (m.allOf !== undefined && m.allOf.length > 0) {
    for (const s of m.allOf) {
      if (!lower.includes(s.toLowerCase())) return false;
    }
  }

  if (m.keywords !== undefined && m.keywords.length > 0) {
    let any = false;
    for (const kw of m.keywords) {
      const escaped = kw.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const re = new RegExp(`\\b${escaped}\\b`, "i");
      if (re.test(prompt)) {
        any = true;
        break;
      }
    }
    if (!any) return false;
  }

  if (m.regex !== undefined && m.regex.length > 0) {
    let any = false;
    for (const src of m.regex) {
      try {
        if (new RegExp(src, "i").test(prompt)) {
          any = true;
          break;
        }
      } catch {
        // Malformed regex → treat as non-matching.
      }
    }
    if (!any) return false;
  }

  // A rule with no populated predicate matches nothing — guard against
  // accidental "always match" catch-alls.
  const hasAnyPredicate =
    (m.anyOf && m.anyOf.length > 0) ||
    (m.allOf && m.allOf.length > 0) ||
    (m.keywords && m.keywords.length > 0) ||
    (m.regex && m.regex.length > 0) ||
    m.minLength !== undefined ||
    m.maxLength !== undefined;
  return !!hasAnyPredicate;
}

/**
 * Structural gate (issue #7, super-auto mode only): ~0 ms deterministic
 * checks that confidently identify "fast default model" turns before any
 * semantic classification runs — code fences, stack traces, slash-command
 * lookalikes, and very short prompts. Returns a rationale for the router
 * line, or `null` when no gate fires. The Router maps a hit onto the
 * configured default model with source "rule".
 */
export function structuralGate(prompt: string): { rationale: string } | null {
  const trimmed = prompt.trim();
  if (trimmed.length === 0) return null;
  if (trimmed.includes("```")) {
    return { rationale: "structural: code fence" };
  }
  if (
    /(^|\n)\s*at .+:\d+/.test(trimmed) ||
    /Traceback \(most recent call last\)/.test(trimmed) ||
    /(^|\n)\s*panic:/.test(trimmed) ||
    /(^|\n)\s*File ".+", line \d+/.test(trimmed)
  ) {
    return { rationale: "structural: stack trace" };
  }
  if (trimmed.startsWith("/") && !trimmed.includes("\n")) {
    return { rationale: "structural: slash-command-like" };
  }
  if (trimmed.length <= 20) {
    return { rationale: "structural: very short prompt" };
  }
  return null;
}

/**
 * A conservative set of built-in rules used when the user has no
 * `~/.cusa/router.toml`. Keeps common short prompts on the fast model.
 */
export const builtInDefaultRules: readonly RuleSpec[] = [
  {
    name: "explain-code",
    model: "composer-2.5",
    rationale: "explain / describe request",
    match: {
      anyOf: ["explain", "what does", "how does", "walk through"],
    },
  },
  {
    name: "hard-reasoning",
    model: "claude-sonnet-4",
    rationale: "long-form reasoning",
    match: {
      keywords: ["prove", "derive", "why"],
      minLength: 200,
    },
  },
];
