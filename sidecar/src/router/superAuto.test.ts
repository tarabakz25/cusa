// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Super Auto Mode pipeline tests (issue #7):
//   rules + structural gate → local embedding (θ) → cloud LLM → fallback,
// plus latest-model resolution, the provider allowlist, and the new
// router.toml keys. Several tests assert behavior that FAILS on the
// pre-#7 pipeline (e.g. "LLM must NOT be consulted outside the θ band").

import { test } from "node:test";
import assert from "node:assert/strict";

import { Router } from "./index.ts";
import { builtInDefaultConfig, parseRouterTomlSafe } from "./config.ts";
import { structuralGate } from "./rules.ts";
import type { RouterConfig } from "./types.ts";
import type { RouterLlmClient } from "./llm.ts";

const CATALOG = [
  "composer-2",
  "composer-1.5",
  "claude-sonnet-4-6",
  "claude-sonnet-4",
  "claude-opus-4-7",
  "gpt-5.5",
  "gemini-3.1-pro",
  "grok-4-20",
  "kimi-k2.5",
  "default",
];

function superAutoConfig(overrides: Partial<RouterConfig> = {}): RouterConfig {
  return {
    ...builtInDefaultConfig(),
    rules: [],
    localClassifierEnabled: true,
    ...overrides,
  };
}

function trackingLlm(response: string): {
  client: RouterLlmClient;
  calls: () => number;
} {
  let n = 0;
  return {
    client: {
      classify: async () => {
        n++;
        return response;
      },
    },
    calls: () => n,
  };
}

// ---------------------------------------------------------------------
// Mode selection + rollback
// ---------------------------------------------------------------------

test("#7: local_classifier_enabled=false keeps the legacy auto pipeline (rollback path)", async () => {
  const { client, calls } = trackingLlm(
    JSON.stringify({ model: "big-model", rationale: "r" }),
  );
  const router = new Router({
    initialConfig: { ...builtInDefaultConfig(), rules: [] },
    llmClient: client,
  });
  // "fix this typo" would be a structural-gate/local hit in super-auto;
  // in legacy auto it must go straight to the LLM.
  const d = await router.route({ prompt: "fix this typo" });
  assert.equal(d.source, "llm");
  assert.equal(calls(), 1);
});

test("#7: ctx.routerMode overrides the config default per turn", async () => {
  const router = new Router({ initialConfig: superAutoConfig() });
  const auto = await router.route({
    prompt: "fix this typo",
    routerMode: "auto",
  });
  // Legacy auto with no rules and no LLM → fallback.
  assert.equal(auto.source, "fallback");
  const superAuto = await router.route({
    prompt: "fix this typo",
    routerMode: "super-auto",
  });
  assert.notEqual(superAuto.source, "fallback");
});

// ---------------------------------------------------------------------
// Structural gate (stage A)
// ---------------------------------------------------------------------

test("#7: structuralGate catches fences, traces, slash commands, short prompts", () => {
  assert.match(
    structuralGate("please review\n```ts\nconst x = 1;\n```")!.rationale,
    /code fence/,
  );
  assert.match(
    structuralGate("it crashed:\n    at Object.run (src/app.ts:42)")!.rationale,
    /stack trace/,
  );
  assert.match(
    structuralGate("Traceback (most recent call last): boom happened here")!
      .rationale,
    /stack trace/,
  );
  assert.match(structuralGate("/help me now")!.rationale, /slash-command/);
  assert.match(structuralGate("fix lint")!.rationale, /very short/);
  assert.equal(
    structuralGate(
      "design the architecture for our new billing service and explain it",
    ),
    null,
  );
});

test("#7: structural gate routes to the default model without any LLM call", async () => {
  const { client, calls } = trackingLlm("{}");
  const router = new Router({
    initialConfig: superAutoConfig(),
    llmClient: client,
  });
  const d = await router.route({
    prompt: "why does this throw?\n```\nboom()\n```",
  });
  assert.equal(d.source, "rule");
  assert.equal(d.model, "composer-2.5");
  assert.match(d.rationale, /structural: code fence/);
  assert.equal(calls(), 0);
});

// ---------------------------------------------------------------------
// Local classifier stage (θ gating)
// ---------------------------------------------------------------------

test("#7: cosine ≥ theta_high decides locally with source='local' (no cloud call)", async () => {
  const { client, calls } = trackingLlm("{}");
  const router = new Router({
    initialConfig: superAutoConfig(),
    llmClient: client,
  });
  const d = await router.route({
    prompt: "design the architecture for a new service, please",
  });
  assert.equal(d.source, "local");
  assert.equal(d.model, "claude-sonnet-4");
  assert.match(d.rationale, /nearest to:/);
  assert.match(d.rationale, /score=/);
  assert.equal(calls(), 0);
});

test("#7: ambiguous θ band escalates to the cloud LLM only", async () => {
  const { client, calls } = trackingLlm(
    JSON.stringify({ model: "claude-sonnet-4", rationale: "hard" }),
  );
  // Force every score into the ambiguous band.
  const router = new Router({
    initialConfig: superAutoConfig({ thetaHigh: 1.01, thetaLow: -1 }),
    llmClient: client,
  });
  const d = await router.route({
    prompt: "summarize the compiler pipeline design at a very abstract level",
  });
  assert.equal(d.source, "llm");
  assert.equal(calls(), 1);
});

test("#7: below theta_low falls back WITHOUT consulting the LLM (θ-band-only AC)", async () => {
  const { client, calls } = trackingLlm("{}");
  // theta_low above 1 → every score is below the band.
  const router = new Router({
    initialConfig: superAutoConfig({ thetaHigh: 1.03, thetaLow: 1.02 }),
    llmClient: client,
  });
  const d = await router.route({
    prompt: "orchestrate the interdimensional spaghetti with mild curiosity",
  });
  assert.equal(d.source, "fallback");
  assert.match(d.rationale, /no semantic match/);
  assert.equal(calls(), 0);
});

test("#7: unknown embedding_model degrades gracefully to the LLM path", async () => {
  const warns: string[] = [];
  const { client, calls } = trackingLlm(
    JSON.stringify({ model: "composer-2", rationale: "r" }),
  );
  const router = new Router({
    initialConfig: superAutoConfig({ embeddingModel: "potion-base-8M" }),
    llmClient: client,
    log: (level, m) => {
      if (level === "warn") warns.push(m);
    },
  });
  const d = await router.route({
    prompt: "a prompt that reaches the classifier stage unimpeded today",
  });
  assert.equal(d.source, "llm");
  assert.equal(calls(), 1);
  assert.ok(warns.some((w) => /embedding_model/.test(w)));
});

test("#7: updateConfig invalidates the exemplar cache (hot reload)", async () => {
  const router = new Router({ initialConfig: superAutoConfig() });
  const before = await router.route({ prompt: "fix this typo please now ok" });
  assert.equal(before.model, "composer-2.5");
  router.updateConfig(
    superAutoConfig({
      exemplars: [
        {
          model: "grok-4-20",
          rationale: "everything goes to grok now",
          examples: ["fix this typo please now ok"],
        },
      ],
    }),
  );
  const after = await router.route({ prompt: "fix this typo please now ok" });
  assert.equal(after.model, "grok-4-20");
});

// ---------------------------------------------------------------------
// Latest-model resolution + allowlist (roles A/B)
// ---------------------------------------------------------------------

test("#7: stale local-decision id (absent from catalog) resolves to the newest sibling", async () => {
  const router = new Router({ initialConfig: superAutoConfig() });
  // Built-in exemplar targets claude-sonnet-4, which is NOT in this
  // catalog anymore — the resolver must map it forward within the family.
  const catalog = CATALOG.filter((id) => id !== "claude-sonnet-4");
  const d = await router.route({
    prompt: "design the architecture for a new service, please",
    catalogModels: catalog,
  });
  assert.equal(d.source, "local");
  assert.equal(d.model, "claude-sonnet-4-6");
});

test("#7: concrete id still present in the catalog stays pinned (passthrough)", async () => {
  const router = new Router({ initialConfig: superAutoConfig() });
  const d = await router.route({
    prompt: "design the architecture for a new service, please",
    catalogModels: CATALOG,
  });
  assert.equal(d.source, "local");
  assert.equal(d.model, "claude-sonnet-4");
});

test("#7: rule decisions resolve forward too (family alias in config)", async () => {
  const router = new Router({
    initialConfig: superAutoConfig({
      rules: [
        {
          name: "quick",
          model: "composer",
          rationale: "fast lane",
          match: { anyOf: ["quick"] },
        },
      ],
    }),
  });
  const d = await router.route({
    prompt: "quick question about the build",
    catalogModels: CATALOG,
  });
  assert.equal(d.source, "rule");
  assert.equal(d.model, "composer-2");
});

test("#7: LLM answer with a stale/disallowed id is mapped into the allowed catalog", async () => {
  const { client } = trackingLlm(
    JSON.stringify({ model: "kimi-k2.5", rationale: "nope" }),
  );
  const router = new Router({
    initialConfig: superAutoConfig({ thetaHigh: 1.01, thetaLow: -1 }),
    llmClient: client,
  });
  const d = await router.route({
    prompt: "some genuinely ambiguous request about the middle things",
    catalogModels: CATALOG,
  });
  assert.equal(d.source, "llm");
  // kimi is not in allowed_providers → falls back to resolved default.
  assert.equal(d.model, "composer-2");
});

test("#7: manual override is allowlist-EXEMPT but warns (SPEC-016 precedence)", async () => {
  const warns: string[] = [];
  const router = new Router({
    initialConfig: superAutoConfig(),
    log: (level, m) => {
      if (level === "warn") warns.push(m);
    },
  });
  const d = await router.route({
    prompt: "anything",
    sessionManualModel: "kimi-k2.5",
    catalogModels: CATALOG,
  });
  assert.equal(d.source, "override");
  assert.equal(d.model, "kimi-k2.5");
  assert.ok(warns.some((w) => /allowed_providers/.test(w)));
});

test("#7: no catalog snapshot → decisions pass through unresolved", async () => {
  const router = new Router({ initialConfig: superAutoConfig() });
  const d = await router.route({
    prompt: "design the architecture for a new service, please",
  });
  assert.equal(d.model, "claude-sonnet-4");
});

// ---------------------------------------------------------------------
// Config: new keys, defaults, validation
// ---------------------------------------------------------------------

test("#7: builtInDefaultConfig ships super-auto OFF, θ defaults, 1500ms LLM budget", () => {
  const cfg = builtInDefaultConfig();
  assert.equal(cfg.localClassifierEnabled, false);
  assert.equal(cfg.thetaHigh, 0.55);
  assert.equal(cfg.thetaLow, 0.35);
  assert.equal(cfg.embeddingModel, "builtin:hash-ngram-v1");
  assert.deepEqual(cfg.allowedProviders, [
    "composer",
    "claude",
    "gpt",
    "gemini",
    "grok",
  ]);
  // NFR-1: LLM classification ≤ 1500 ms p95 → default hard budget 1500.
  assert.equal(cfg.llmTimeoutMs, 1500);
});

test("#7: parseRouterTomlSafe reads θ floats, allowlist, and [[exemplars]]", () => {
  const parsed = parseRouterTomlSafe(`
allowed_providers = ["composer", "claude"]
default_model = "composer"
local_classifier_enabled = true
theta_high = 0.6
theta_low = 0.3
embedding_model = "builtin:hash-ngram-v1"

[[exemplars]]
model = "claude-sonnet"
rationale = "long-form reasoning"
examples = ["prove that this halts", "design the system"]

[[exemplars]]
model = "composer"
rationale = "quick edit"
examples = ["fix this"]
`);
  assert.deepEqual(parsed.errors, []);
  const cfg = parsed.config;
  assert.equal(cfg.localClassifierEnabled, true);
  assert.equal(cfg.thetaHigh, 0.6);
  assert.equal(cfg.thetaLow, 0.3);
  assert.deepEqual(cfg.allowedProviders, ["composer", "claude"]);
  assert.equal(cfg.exemplars.length, 2);
  assert.deepEqual(cfg.exemplars[1], {
    model: "composer",
    rationale: "quick edit",
    examples: ["fix this"],
  });
});

test("#7: default_model outside an explicit allowlist is a loud config error", () => {
  const parsed = parseRouterTomlSafe(`
allowed_providers = ["claude"]
default_model = "kimi-k2.5"
`);
  assert.ok(
    parsed.errors.some((e) => /default_model .* allowed_providers/.test(e)),
  );
});

test("#7: rules/exemplars outside an explicit allowlist are dropped with warnings", () => {
  const parsed = parseRouterTomlSafe(`
allowed_providers = ["composer"]
default_model = "composer"

[[rule]]
name = "bad"
model = "kimi-k2.5"
rationale = "nope"
match = { any_of = ["x"] }

[[exemplars]]
model = "grok-4-20"
rationale = "nope"
examples = ["y"]
`);
  assert.deepEqual(parsed.errors, []);
  assert.equal(parsed.config.rules.length, 0);
  assert.equal(parsed.config.exemplars.length, 0);
  assert.ok(parsed.warnings.some((w) => /dropping rule 'bad'/.test(w)));
  assert.ok(parsed.warnings.some((w) => /dropping exemplar/.test(w)));
});

test("#7: NO explicit allowlist keeps legacy configs with arbitrary ids valid", () => {
  const parsed = parseRouterTomlSafe(`
default_model = "special-default"

[[rule]]
name = "shell"
model = "shell-model"
rationale = "shells"
match = { any_of = ["shell"] }
`);
  assert.deepEqual(parsed.errors, []);
  assert.equal(parsed.config.defaultModel, "special-default");
  assert.equal(parsed.config.rules.length, 1);
});

test("#7: inverted θ band is a config error", () => {
  const parsed = parseRouterTomlSafe(`
theta_high = 0.2
theta_low = 0.8
`);
  assert.ok(parsed.errors.some((e) => /theta_low/.test(e)));
});
