// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// θ grid search over the synthetic eval seed (issue #7, Phase 1).
//
// Usage:
//   cd sidecar && node --import tsx scripts/theta-grid.ts
//
// For each (θ_high, θ_low) pair it reports, on labeled seed rows:
//   - decided:   share of rows the local stage decides (score ≥ θ_high)
//   - accuracy:  correctness among locally-decided rows
//   - escalated: share falling into the ambiguous band (cloud LLM cost)
// Pick the smallest θ_high with accuracy 1.0 and a tolerable escalation
// rate; Phase 2 dev captures then re-tune on real prompts.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

import {
  LocalEmbeddingClassifier,
  builtInDefaultExemplars,
  createEmbedder,
  BUILTIN_EMBEDDER_ID,
} from "../src/router/localClassifier.ts";
import { structuralGate } from "../src/router/rules.ts";

interface SeedRow {
  id: string;
  prompt: string;
  expected_model: string | null;
  tags: string[];
}

const here = path.dirname(fileURLToPath(import.meta.url));
const seedPath = path.join(here, "../src/router/__fixtures__/eval-seed.jsonl");
const rows: SeedRow[] = readFileSync(seedPath, "utf8")
  .trim()
  .split("\n")
  .map((l) => JSON.parse(l) as SeedRow);

const classifier = new LocalEmbeddingClassifier({
  exemplars: builtInDefaultExemplars,
  embedder: createEmbedder(BUILTIN_EMBEDDER_ID)!,
});

interface Scored extends SeedRow {
  structural: boolean;
  predicted: string | null;
  score: number;
}

const scored: Scored[] = rows.map((row) => {
  const gate = structuralGate(row.prompt);
  const c = classifier.classify(row.prompt);
  return {
    ...row,
    structural: gate !== null,
    predicted: c?.model ?? null,
    score: c?.score ?? -1,
  };
});

const grid: number[] = [];
for (let t = 0.2; t <= 0.85; t += 0.05) grid.push(Number(t.toFixed(2)));

console.log("θ_high  decided  accuracy  escalated(θ_low=0.35→θ_high)");
for (const thetaHigh of grid) {
  const semantic = scored.filter((r) => !r.structural);
  const decided = semantic.filter((r) => r.score >= thetaHigh);
  const labeled = decided.filter((r) => r.expected_model !== null);
  const correct = labeled.filter((r) => r.predicted === r.expected_model);
  const escalated = semantic.filter(
    (r) => r.score >= 0.35 && r.score < thetaHigh,
  );
  const acc = labeled.length === 0 ? 1 : correct.length / labeled.length;
  console.log(
    `${thetaHigh.toFixed(2)}    ${(decided.length / semantic.length).toFixed(2)}     ${acc.toFixed(2)}      ${(escalated.length / semantic.length).toFixed(2)}`,
  );
}

console.log("\nper-row detail (non-structural):");
for (const r of scored.filter((r) => !r.structural)) {
  const okMark =
    r.expected_model === null
      ? "·"
      : r.predicted === r.expected_model
        ? "✓"
        : "✗";
  console.log(
    `${okMark} ${r.id}  score=${r.score.toFixed(3)}  predicted=${r.predicted}  expected=${r.expected_model}`,
  );
}
