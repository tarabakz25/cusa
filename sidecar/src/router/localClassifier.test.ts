// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Local semantic classifier tests (issue #7). The embedder is pinned —
// several of these tests exist precisely to fail if someone changes the
// embedding space without bumping the version id.

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  BUILTIN_EMBEDDER_ID,
  EMBEDDING_INPUT_MAX_CHARS,
  LocalEmbeddingClassifier,
  builtInDefaultExemplars,
  createEmbedder,
} from "./localClassifier.ts";

function makeClassifier(): LocalEmbeddingClassifier {
  const embedder = createEmbedder(BUILTIN_EMBEDDER_ID);
  assert.ok(embedder);
  return new LocalEmbeddingClassifier({
    exemplars: builtInDefaultExemplars,
    embedder: embedder!,
  });
}

test("#7: embedder id is pinned and createEmbedder rejects unknown pins", () => {
  assert.equal(BUILTIN_EMBEDDER_ID, "builtin:hash-ngram-v1");
  assert.ok(createEmbedder(BUILTIN_EMBEDDER_ID));
  assert.equal(createEmbedder("potion-base-8M"), null);
  assert.equal(createEmbedder(""), null);
});

test("#7: embedding is deterministic and L2-normalized", () => {
  const e = createEmbedder(BUILTIN_EMBEDDER_ID)!;
  const a = e.embed("refactor this module");
  const b = e.embed("refactor this module");
  assert.deepEqual(Array.from(a), Array.from(b));
  let norm = 0;
  for (const v of a) norm += v * v;
  assert.ok(Math.abs(norm - 1) < 1e-5, `norm^2 was ${norm}`);
});

test("#7: an exact exemplar sentence scores ~1.0 against itself", () => {
  const c = makeClassifier();
  const r = c.classify("fix this typo");
  assert.ok(r);
  assert.ok(r!.score > 0.99, `score was ${r!.score}`);
  assert.equal(r!.model, "composer-2.5");
  assert.match(r!.rationale, /nearest to: "fix this typo"/);
});

test("#7: paraphrases route to the semantically-nearest exemplar group", () => {
  const c = makeClassifier();
  const quick = c.classify("please fix the typo in this comment");
  assert.ok(quick);
  assert.equal(quick!.model, "composer-2.5");

  const deep = c.classify(
    "design the architecture for our new billing service and explain the tradeoffs",
  );
  assert.ok(deep);
  assert.equal(deep!.model, "claude-sonnet-4");
});

test("#7: unrelated gibberish scores below the default theta_low", () => {
  const c = makeClassifier();
  const r = c.classify("zqx vbnm kjhg wretplo asdf");
  assert.ok(r);
  assert.ok(r!.score < 0.35, `score was ${r!.score}`);
});

test("#7: similar prompt scores strictly higher than an unrelated one", () => {
  const c = makeClassifier();
  const near = c.classify("rename this variable")!;
  const far = c.classify("what is the capital of France")!;
  assert.ok(
    near.score > far.score,
    `near=${near.score} should beat far=${far.score}`,
  );
});

test("#7: giant pastes are truncated before embedding (latency guard)", () => {
  const e = createEmbedder(BUILTIN_EMBEDDER_ID)!;
  const base = "fix this ".repeat(EMBEDDING_INPUT_MAX_CHARS);
  const a = e.embed(base);
  const b = e.embed(base + "completely different suffix ".repeat(100));
  // Identical after the truncation point → identical vectors.
  assert.deepEqual(Array.from(a), Array.from(b));
});

test("#7: classifier with zero exemplars returns null", () => {
  const embedder = createEmbedder(BUILTIN_EMBEDDER_ID)!;
  const c = new LocalEmbeddingClassifier({ exemplars: [], embedder });
  assert.equal(c.classify("anything"), null);
});
