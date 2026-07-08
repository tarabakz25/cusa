// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Model resolver tests (issue #7 — "always latest" for roles A/B, plus
// the provider allowlist). The catalog snapshot mirrors the issue's
// verified `Cursor.models.list()` shape.

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  compareVersions,
  familyOf,
  filterCatalog,
  providerOf,
  resolveLatestModel,
  versionOf,
} from "./modelResolver.ts";

const CATALOG = [
  "composer-2",
  "composer-1.5",
  "claude-opus-4-7",
  "claude-opus-4-6",
  "claude-opus-4-5",
  "claude-sonnet-4-6",
  "claude-sonnet-4-5",
  "claude-sonnet-4",
  "claude-haiku-4-5",
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.4-mini",
  "gpt-5.4-nano",
  "gpt-5.3-codex",
  "gpt-5.3-codex-spark",
  "gpt-5.2",
  "gpt-5.2-codex",
  "gpt-5.1",
  "gpt-5.1-codex-max",
  "gpt-5.1-codex-mini",
  "gpt-5-mini",
  "gemini-3.1-pro",
  "gemini-3-flash",
  "gemini-2.5-flash",
  "grok-4-20",
  "kimi-k2.5",
  "default",
];

const ALLOWED = ["composer", "claude", "gpt", "gemini", "grok"];

test("#7: providerOf extracts the brand before the first dash", () => {
  assert.equal(providerOf("claude-sonnet-4-6"), "claude");
  assert.equal(providerOf("gpt-5.3-codex"), "gpt");
  assert.equal(providerOf("grok-4-20"), "grok");
  assert.equal(providerOf("composer"), "composer");
  assert.equal(providerOf("default"), null);
  assert.equal(providerOf("auto"), null);
  assert.equal(providerOf(""), null);
});

test("#7: familyOf strips version tokens across dot/dash grammars", () => {
  assert.equal(familyOf("claude-sonnet-4-6"), "claude-sonnet");
  assert.equal(familyOf("gpt-5.3-codex"), "gpt-codex");
  assert.equal(familyOf("gpt-5.1-codex-max"), "gpt-codex-max");
  assert.equal(familyOf("gpt-5.5"), "gpt");
  assert.equal(familyOf("gemini-3.1-pro"), "gemini-pro");
  assert.equal(familyOf("composer-2"), "composer");
  assert.equal(familyOf("grok-4-20"), "grok");
});

test("#7: versionOf concatenates numeric tokens element-wise", () => {
  assert.deepEqual(versionOf("gpt-5.3-codex"), [5, 3]);
  assert.deepEqual(versionOf("claude-sonnet-4-6"), [4, 6]);
  assert.deepEqual(versionOf("grok-4-20"), [4, 20]);
  assert.deepEqual(versionOf("composer"), []);
  assert.equal(compareVersions([4, 6], [4]) > 0, true);
  assert.equal(compareVersions([2], [1, 5]) > 0, true);
  assert.equal(compareVersions([4], [4, 0]), 0);
});

test("#7: filterCatalog keeps only allowed brands and drops opaque autos", () => {
  const filtered = filterCatalog(CATALOG, ALLOWED);
  assert.ok(!filtered.includes("kimi-k2.5"));
  assert.ok(!filtered.includes("default"));
  assert.ok(filtered.includes("claude-sonnet-4-6"));
  assert.equal(filtered.length, CATALOG.length - 2);
  // Empty allowlist → allow every real brand, still no opaque ids.
  const open = filterCatalog(CATALOG, []);
  assert.ok(open.includes("kimi-k2.5"));
  assert.ok(!open.includes("default"));
});

test("#7: family alias resolves to the newest concrete id", () => {
  assert.equal(resolveLatestModel("composer", CATALOG).id, "composer-2");
  assert.equal(
    resolveLatestModel("claude-sonnet", CATALOG).id,
    "claude-sonnet-4-6",
  );
  assert.equal(resolveLatestModel("gpt-codex", CATALOG).id, "gpt-5.3-codex");
  assert.equal(resolveLatestModel("gemini-pro", CATALOG).id, "gemini-3.1-pro");
  assert.equal(
    resolveLatestModel("gemini-flash", CATALOG).id,
    "gemini-3-flash",
  );
});

test("#7: stale sibling id resolves forward to the newest family member", () => {
  const r = resolveLatestModel("claude-sonnet-4-2", CATALOG);
  assert.equal(r.id, "claude-sonnet-4-6");
  assert.equal(r.resolved, true);
});

test("#7: exact catalog member is a no-op passthrough (pinning works)", () => {
  const r = resolveLatestModel("claude-sonnet-4-5", CATALOG);
  assert.equal(r.id, "claude-sonnet-4-5");
  assert.equal(r.resolved, false);
  assert.equal(r.warning, undefined);
});

test("#7: brand-only alias resolves via default tier with a warning", () => {
  const claude = resolveLatestModel("claude", CATALOG);
  assert.equal(claude.id, "claude-sonnet-4-6");
  assert.match(claude.warning ?? "", /brand-only/);
  const gemini = resolveLatestModel("gemini", CATALOG);
  assert.equal(gemini.id, "gemini-3.1-pro");
  // "gpt" default tier is the non-codex line.
  const gpt = resolveLatestModel("gpt", CATALOG);
  assert.equal(gpt.id, "gpt-5.5");
});

test("#7: unresolvable alias returns unchanged with a warning", () => {
  const r = resolveLatestModel("o3-pro", CATALOG);
  assert.equal(r.id, "o3-pro");
  assert.equal(r.resolved, false);
  assert.match(r.warning ?? "", /no catalog model matches/);
});

test("#7: empty catalog passthrough with warning (offline tolerance)", () => {
  const r = resolveLatestModel("composer", []);
  assert.equal(r.id, "composer");
  assert.match(r.warning ?? "", /no model catalog/);
});

test("#7: version tie broken lexicographically + warned (deterministic)", () => {
  const r = resolveLatestModel("acme-fast", ["acme-2-fast", "acme-fast-2"]);
  // Both are family "acme-fast" version [2]; lexicographic max wins.
  assert.equal(r.id, "acme-fast-2");
  assert.match(r.warning ?? "", /tie/);
});
