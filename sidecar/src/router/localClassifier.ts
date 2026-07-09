// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Local semantic classifier for Super Auto Mode (issue #7).
//
// Sits between the deterministic rule stage and the cloud-LLM classifier:
// exemplar prompts from `router.toml [[exemplars]]` are embedded once, the
// incoming prompt is embedded per turn, and cosine similarity against the
// nearest exemplar acts as a confidence score gated by θ_high / θ_low.
//
// The embedder is PINNED and versioned (`builtin:hash-ngram-v1`): unlike
// the routing targets (always resolved to the latest catalog id), the
// embedding space must never drift underneath the exemplar cache and the
// decision thresholds. Upgrading to a stronger model (model2vec / MiniLM)
// is a drop-in `Embedder` swap behind a *new* pinned id.
//
// The built-in embedder is a deterministic hashed-feature embedding
// (word unigrams + word bigrams + char trigrams, signed FNV-1a hashing,
// L2-normalized). Zero dependencies, zero cold-start, sub-millisecond on
// prompts capped at EMBEDDING_INPUT_MAX_CHARS — which keeps NFR-1's
// ≤800 ms cold start and the 6-platform npm matrix intact ("never break
// install").

import type { ExemplarSpec } from "./types.js";

/** Giant pastes are truncated before embedding (latency guard). */
export const EMBEDDING_INPUT_MAX_CHARS = 1000;

/** Pinned id of the built-in embedder. Bump ONLY with a new suffix. */
export const BUILTIN_EMBEDDER_ID = "builtin:hash-ngram-v1";

export interface Embedder {
  /** Pinned, versioned identifier — part of the decision-boundary contract. */
  readonly id: string;
  /** Embed `text` into an L2-normalized vector. Must be deterministic. */
  embed(text: string): Float32Array;
}

const DIMS = 512;
const FNV_OFFSET = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

function fnv1a(str: string, seed: number): number {
  let h = (FNV_OFFSET ^ seed) >>> 0;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, FNV_PRIME) >>> 0;
  }
  return h >>> 0;
}

class HashNGramEmbedder implements Embedder {
  readonly id = BUILTIN_EMBEDDER_ID;

  embed(text: string): Float32Array {
    const vec = new Float32Array(DIMS);
    const normalized = text
      .slice(0, EMBEDDING_INPUT_MAX_CHARS)
      .toLowerCase()
      .replace(/\s+/g, " ")
      .trim();
    if (normalized.length === 0) return vec;

    const add = (feature: string, weight: number): void => {
      const h = fnv1a(feature, 0);
      const sign = (fnv1a(feature, 0x9e3779b9) & 1) === 0 ? 1 : -1;
      vec[h % DIMS]! += sign * weight;
    };

    const words = normalized.split(/[^a-z0-9]+/).filter((w) => w.length > 0);
    for (const w of words) add(`w:${w}`, 2);
    for (let i = 0; i + 1 < words.length; i++) {
      add(`b:${words[i]}_${words[i + 1]}`, 1.5);
    }
    const padded = `\u0002${normalized}\u0003`;
    for (let i = 0; i + 3 <= padded.length; i++) {
      add(`c:${padded.slice(i, i + 3)}`, 1);
    }

    let norm = 0;
    for (let i = 0; i < DIMS; i++) norm += vec[i]! * vec[i]!;
    norm = Math.sqrt(norm);
    if (norm > 0) {
      for (let i = 0; i < DIMS; i++) vec[i]! /= norm;
    }
    return vec;
  }
}

/**
 * Instantiate the embedder pinned by `embedding_model`. Unknown ids
 * return `null` so the router can degrade gracefully (rules + LLM only)
 * instead of crashing — e.g. a config written for a future embedder.
 */
export function createEmbedder(modelId: string): Embedder | null {
  if (modelId === BUILTIN_EMBEDDER_ID) return new HashNGramEmbedder();
  return null;
}

export interface LocalClassification {
  /** Model (or family alias) attached to the nearest exemplar group. */
  model: string;
  /** Rationale carrying the nearest exemplar for SPEC-012 transparency. */
  rationale: string;
  /** Cosine similarity against the nearest exemplar, in [-1, 1]. */
  score: number;
  /** The exemplar sentence that won. */
  nearestExample: string;
}

interface EmbeddedExemplar {
  model: string;
  rationale: string;
  example: string;
  vector: Float32Array;
}

/**
 * Exemplar-based nearest-neighbour classifier. Exemplar embeddings are
 * precomputed at construction; `classify` embeds the prompt and returns
 * the nearest exemplar with its cosine score. Threshold gating lives in
 * the Router — this class only measures.
 */
export class LocalEmbeddingClassifier {
  private readonly embedder: Embedder;
  private readonly exemplars: EmbeddedExemplar[];

  constructor(opts: { exemplars: readonly ExemplarSpec[]; embedder: Embedder }) {
    this.embedder = opts.embedder;
    this.exemplars = [];
    for (const spec of opts.exemplars) {
      for (const example of spec.examples) {
        this.exemplars.push({
          model: spec.model,
          rationale: spec.rationale,
          example,
          vector: this.embedder.embed(example),
        });
      }
    }
  }

  get embedderId(): string {
    return this.embedder.id;
  }

  get exemplarCount(): number {
    return this.exemplars.length;
  }

  classify(prompt: string): LocalClassification | null {
    if (this.exemplars.length === 0) return null;
    const v = this.embedder.embed(prompt);
    let best: EmbeddedExemplar | null = null;
    let bestScore = -Infinity;
    for (const ex of this.exemplars) {
      let dot = 0;
      for (let i = 0; i < DIMS; i++) dot += v[i]! * ex.vector[i]!;
      if (dot > bestScore) {
        bestScore = dot;
        best = ex;
      }
    }
    if (!best) return null;
    return {
      model: best.model,
      rationale: `nearest to: "${best.example}" (${best.rationale})`,
      score: bestScore,
      nearestExample: best.example,
    };
  }
}

/**
 * Conservative built-in exemplars used when super-auto is enabled but the
 * user has not configured `[[exemplars]]`. Model ids match the built-in
 * rules' targets; when a live catalog is available they are resolved to
 * the newest family sibling by the Router.
 */
export const builtInDefaultExemplars: readonly ExemplarSpec[] = [
  {
    model: "composer-2.5",
    rationale: "quick edit / small change",
    examples: [
      "fix this typo",
      "rename this variable to something clearer",
      "add a null check here",
      "update the import path",
      "write a unit test for this function",
      "format this file",
      "small refactor of this helper function",
    ],
  },
  {
    model: "claude-sonnet-4",
    rationale: "long-form reasoning / design",
    examples: [
      "design the architecture for a new service",
      "why does this deadlock happen under load",
      "prove that this algorithm terminates",
      "compare these two approaches and recommend one",
      "plan a migration strategy for the database schema",
      "analyze the tradeoffs of this design decision",
    ],
  },
];
