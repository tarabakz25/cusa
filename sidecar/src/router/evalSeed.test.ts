// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Classifier regression harness over the synthetic eval seed (issue #7,
// Phase 1). Guards the pinned embedder's decision quality: if someone
// swaps or "improves" the embedding without bumping the pin, these
// assertions catch drift. θ defaults (0.55 / 0.35) were chosen with
// `scripts/theta-grid.ts` over this seed.
//
// Known limitation (documented in the issue): the seed is synthetic, so
// θ must be re-tuned on Phase-2 dev captures before tightening further.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

import {
  BUILTIN_EMBEDDER_ID,
  LocalEmbeddingClassifier,
  builtInDefaultExemplars,
  createEmbedder,
} from "./localClassifier.ts";
import { structuralGate } from "./rules.ts";
import { builtInDefaultConfig } from "./config.ts";

interface SeedRow {
  id: string;
  prompt: string;
  expected_model: string | null;
  source: string;
  tags: string[];
}

const here = path.dirname(fileURLToPath(import.meta.url));
const rows: SeedRow[] = readFileSync(
  path.join(here, "__fixtures__/eval-seed.jsonl"),
  "utf8",
)
  .trim()
  .split("\n")
  .map((l) => JSON.parse(l) as SeedRow);

const classifier = new LocalEmbeddingClassifier({
  exemplars: builtInDefaultExemplars,
  embedder: createEmbedder(BUILTIN_EMBEDDER_ID)!,
});

test("#7: eval seed is well-formed synthetic data", () => {
  assert.ok(rows.length >= 25, `only ${rows.length} rows`);
  for (const row of rows) {
    assert.equal(row.source, "synthetic");
    assert.ok(row.id.length > 0);
    assert.ok(row.prompt.length > 0);
  }
});

test("#7: structural-tagged rows are caught by the structural gate", () => {
  for (const row of rows.filter((r) => r.tags.includes("structural"))) {
    assert.ok(structuralGate(row.prompt), `gate missed ${row.id}`);
  }
});

test("#7: nearest-neighbour accuracy on labeled rows is 100% on this seed", () => {
  const labeled = rows.filter(
    (r) => r.expected_model !== null && !r.tags.includes("structural"),
  );
  assert.ok(labeled.length >= 20);
  for (const row of labeled) {
    const c = classifier.classify(row.prompt);
    assert.ok(c, `no classification for ${row.id}`);
    assert.equal(
      c!.model,
      row.expected_model,
      `${row.id}: predicted ${c!.model}, expected ${row.expected_model} (score=${c!.score.toFixed(3)})`,
    );
  }
});

test("#7: zero misroutes above the default theta_high (AC: high-confidence lane is safe)", () => {
  const { thetaHigh } = builtInDefaultConfig();
  for (const row of rows.filter((r) => !r.tags.includes("structural"))) {
    const c = classifier.classify(row.prompt)!;
    if (c.score >= thetaHigh && row.expected_model !== null) {
      assert.equal(c.model, row.expected_model, `misroute above θ_high: ${row.id}`);
    }
    if (row.expected_model === null) {
      assert.ok(
        c.score < thetaHigh,
        `ambiguous row ${row.id} must not decide locally (score=${c.score.toFixed(3)})`,
      );
    }
  }
});

test("#7: a useful share of the seed decides locally at default θ (cost win)", () => {
  const { thetaHigh } = builtInDefaultConfig();
  const semantic = rows.filter((r) => !r.tags.includes("structural"));
  const decided = semantic.filter(
    (r) => classifier.classify(r.prompt)!.score >= thetaHigh,
  );
  // theta-grid over this seed: 72% decided at 0.55. Guard a floor of 50%
  // so embedder regressions that hollow out the fast path are caught.
  assert.ok(
    decided.length / semantic.length >= 0.5,
    `only ${decided.length}/${semantic.length} decided locally`,
  );
});

test("#7: warm local classification stays far under the 50 ms AC budget", () => {
  // Warm up (exemplar embeddings are already precomputed in the ctor).
  classifier.classify("warm up the caches");
  const started = process.hrtime.bigint();
  const N = 50;
  for (let i = 0; i < N; i++) {
    classifier.classify(rows[i % rows.length]!.prompt);
  }
  const perCallMs =
    Number(process.hrtime.bigint() - started) / 1_000_000 / N;
  assert.ok(perCallMs < 50, `warm classify took ${perCallMs.toFixed(2)} ms`);
});
