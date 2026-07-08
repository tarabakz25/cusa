// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Model-id resolution for Super Auto Mode (issue #7).
//
// Roles:
// - `providerOf(id)`: brand of a model id (`claude-sonnet-4-6` → `claude`).
//   Used to enforce the `allowed_providers` allowlist.
// - `familyOf(id)`: family alias with version tokens removed
//   (`claude-sonnet-4-6` → `claude-sonnet`, `gpt-5.3-codex` → `gpt-codex`).
// - `resolveLatestModel(alias, catalog)`: map a family alias (or a stale
//   concrete id) onto the newest concrete id in the live `models/list`
//   catalog. Exact catalog members pass through unchanged so users can
//   still pin a frozen id.
//
// Version grammar is deliberately tolerant: brands mix dot and dash
// separators (`gpt-5.3-codex`, `claude-sonnet-4-6`, `grok-4-20`), so a
// version is the concatenation of every numeric token in declaration
// order, compared element-wise.

/** Ids that are never routable candidates (opaque server-side autos). */
const EXCLUDED_IDS = new Set(["default", "auto"]);

/** Brand → family used when an alias is brand-only (e.g. `claude`). */
const DEFAULT_FAMILY_BY_BRAND: Record<string, string> = {
  claude: "claude-sonnet",
  gemini: "gemini-pro",
  gpt: "gpt",
  composer: "composer",
  grok: "grok",
};

/**
 * Brand of a model id: the token before the first `-`. Returns `null`
 * for excluded/opaque ids (`default`, `auto`) and empty strings.
 */
export function providerOf(id: string): string | null {
  const trimmed = id.trim().toLowerCase();
  if (trimmed.length === 0 || EXCLUDED_IDS.has(trimmed)) return null;
  const head = trimmed.split("-", 1)[0]!;
  if (head.length === 0 || EXCLUDED_IDS.has(head)) return null;
  return head;
}

function isVersionToken(token: string): boolean {
  return /^\d+(\.\d+)*$/.test(token);
}

/**
 * Family alias of a model id: every non-numeric token joined in order.
 * `claude-sonnet-4-6` → `claude-sonnet`; `gpt-5.1-codex-max` →
 * `gpt-codex-max`; `gpt-5.5` → `gpt`; `composer` → `composer`.
 */
export function familyOf(id: string): string {
  const tokens = id.trim().toLowerCase().split("-");
  const kept = tokens.filter((t) => t.length > 0 && !isVersionToken(t));
  return kept.join("-");
}

/**
 * Version vector of a model id: every numeric token, split on dots,
 * concatenated in order. `gpt-5.3-codex` → [5,3]; `claude-sonnet-4-6` →
 * [4,6]; `composer-2` → [2]. Ids with no numeric token → [].
 */
export function versionOf(id: string): number[] {
  const out: number[] = [];
  for (const token of id.trim().toLowerCase().split("-")) {
    if (!isVersionToken(token)) continue;
    for (const part of token.split(".")) out.push(Number(part));
  }
  return out;
}

/** Element-wise version compare; missing elements count as 0. */
export function compareVersions(a: number[], b: number[]): number {
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const av = a[i] ?? 0;
    const bv = b[i] ?? 0;
    if (av !== bv) return av - bv;
  }
  return 0;
}

/**
 * Filter a raw `models/list` catalog down to the allowed brands.
 * Opaque ids (`default`, `auto`) are always excluded. An empty
 * `allowedProviders` list means "allow every brand".
 */
export function filterCatalog(
  catalog: readonly string[],
  allowedProviders: readonly string[],
): string[] {
  const allowed = new Set(allowedProviders.map((p) => p.toLowerCase()));
  return catalog.filter((id) => {
    const brand = providerOf(id);
    if (brand === null) return false;
    return allowed.size === 0 || allowed.has(brand);
  });
}

export interface ResolveResult {
  /** Concrete id to use. Falls back to the input alias when unresolvable. */
  id: string;
  /** True when the id was mapped through the catalog (alias ≠ id or newer sibling). */
  resolved: boolean;
  /** Human-readable warning when resolution was ambiguous or failed. */
  warning?: string;
}

/**
 * Resolve a family alias (or concrete id) to the newest concrete id in
 * `catalog`. Behavior, in order:
 *
 * 1. Exact catalog member → no-op passthrough (pinning keeps working).
 * 2. Group the catalog by `familyOf` and pick the newest member of the
 *    alias's family (numeric version compare; lexicographic-descending
 *    tie-break with a warning).
 * 3. Brand-only alias (`claude`) → resolve via the default tier for that
 *    brand (claude→sonnet, gemini→pro, gpt→non-codex) with a warning.
 * 4. No family match → return the alias unchanged with a warning; the
 *    caller decides whether to fall back to the default model.
 */
export function resolveLatestModel(
  alias: string,
  catalog: readonly string[],
): ResolveResult {
  const wanted = alias.trim();
  if (wanted.length === 0) return { id: alias, resolved: false };
  if (catalog.includes(wanted)) return { id: wanted, resolved: false };
  if (catalog.length === 0) {
    return {
      id: wanted,
      resolved: false,
      warning: `no model catalog available; using '${wanted}' as-is`,
    };
  }

  const byFamily = new Map<string, string[]>();
  for (const id of catalog) {
    const fam = familyOf(id);
    const list = byFamily.get(fam);
    if (list) list.push(id);
    else byFamily.set(fam, [id]);
  }

  let family = familyOf(wanted);
  let warning: string | undefined;
  if (!byFamily.has(family)) {
    // Brand-only alias? Try the default tier for the brand.
    const brand = providerOf(wanted);
    const defaultFamily = brand ? DEFAULT_FAMILY_BY_BRAND[brand] : undefined;
    if (
      family === brand &&
      defaultFamily !== undefined &&
      defaultFamily !== family &&
      byFamily.has(defaultFamily)
    ) {
      warning = `alias '${wanted}' is brand-only; resolving via default tier '${defaultFamily}'`;
      family = defaultFamily;
    } else {
      return {
        id: wanted,
        resolved: false,
        warning: `no catalog model matches family '${family}' for alias '${wanted}'`,
      };
    }
  }

  const members = byFamily.get(family)!;
  let best = members[0]!;
  let tied = false;
  for (const candidate of members.slice(1)) {
    const cmp = compareVersions(versionOf(candidate), versionOf(best));
    if (cmp > 0) {
      best = candidate;
      tied = false;
    } else if (cmp === 0) {
      // Deterministic tie-break: lexicographically greatest full id.
      tied = true;
      if (candidate > best) best = candidate;
    }
  }
  if (tied) {
    warning = warning
      ? `${warning}; version tie in family '${family}' broken lexicographically`
      : `version tie in family '${family}' broken lexicographically → '${best}'`;
  }
  const result: ResolveResult = { id: best, resolved: best !== wanted };
  if (warning !== undefined) result.warning = warning;
  return result;
}
