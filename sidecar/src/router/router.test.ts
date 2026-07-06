// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Router unit tests. Names include the SPEC IDs so grep-audits stay easy.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";

import { Router } from "./index.ts";
import {
  builtInDefaultConfig,
  loadRouterConfig,
  parseRouterTomlSafe,
  watchRouterConfig,
} from "./config.ts";
import { firstMatchingRule, builtInDefaultRules } from "./rules.ts";
import { buildClassifierPrompt, parseClassifierOutput } from "./llm.ts";
import type { RouterLlmClient } from "./llm.ts";

// ----------------------------------------------------------------------
// SPEC-010: classification (rule-first)
// ----------------------------------------------------------------------

test("SPEC-010: firstMatchingRule returns the first rule that satisfies its predicates", () => {
  const rules = [
    {
      name: "greeting",
      model: "cheap-model",
      rationale: "greet",
      match: { anyOf: ["hello"] },
    },
    {
      name: "hard",
      model: "big-model",
      rationale: "reason",
      match: { keywords: ["prove"], minLength: 10 },
    },
  ];
  const r = firstMatchingRule(
    { prompt: "please prove the halting problem is undecidable" },
    rules,
  );
  assert.equal(r?.name, "hard");
});

test("SPEC-010: rule with no populated predicate never matches (guards catch-alls)", () => {
  const rules = [
    { name: "empty", model: "m", rationale: "", match: {} },
  ];
  assert.equal(firstMatchingRule({ prompt: "anything" }, rules), null);
});

// ----------------------------------------------------------------------
// SPEC-011: Router returns a Cursor SDK-compatible ModelSelection per turn
// ----------------------------------------------------------------------

test("SPEC-011: Router.route returns a decision with a non-empty model id and camelCase source", async () => {
  const router = new Router();
  const d = await router.route({ prompt: "please explain this function" });
  assert.equal(typeof d.model, "string");
  assert.ok(d.model.length > 0);
  assert.ok(["rule", "llm", "override", "fallback"].includes(d.source));
});

// ----------------------------------------------------------------------
// SPEC-013: hybrid pipeline (rules → llm → fallback)
// ----------------------------------------------------------------------

test("SPEC-013: manual override wins over rules and LLM", async () => {
  const router = new Router({
    initialConfig: {
      ...builtInDefaultConfig(),
      llmEnabled: true,
    },
    llmClient: makeExplodingLlmClient(),
  });
  const d = await router.route({
    prompt: "explain this",
    sessionManualModel: "claude-sonnet-4",
  });
  assert.equal(d.source, "override");
  assert.equal(d.model, "claude-sonnet-4");
  assert.match(d.rationale, /override/);
});

test("SPEC-013: first matching rule short-circuits before LLM", async () => {
  let llmCalled = false;
  const router = new Router({
    initialConfig: builtInDefaultConfig(),
    llmClient: {
      classify: async () => {
        llmCalled = true;
        return "{}";
      },
    },
  });
  const d = await router.route({ prompt: "explain this function" });
  assert.equal(d.source, "rule");
  assert.equal(d.model, "composer-2.5");
  assert.equal(llmCalled, false);
});

test("SPEC-013: LLM fallback used when no rule matches (source='llm')", async () => {
  const router = new Router({
    initialConfig: {
      ...builtInDefaultConfig(),
      // Strip built-in rules so we deterministically fall into LLM.
      rules: [],
    },
    llmClient: {
      classify: async () =>
        JSON.stringify({ model: "big-model", rationale: "hard task" }),
    },
  });
  const d = await router.route({ prompt: "solve this maths problem for me" });
  assert.equal(d.source, "llm");
  assert.equal(d.model, "big-model");
});

test("SPEC-013: LLM disabled → default-model fallback (source='fallback')", async () => {
  const router = new Router({
    initialConfig: {
      ...builtInDefaultConfig(),
      rules: [],
      llmEnabled: false,
    },
  });
  const d = await router.route({ prompt: "unclassifiable prompt" });
  assert.equal(d.source, "fallback");
  assert.equal(d.model, "composer-2.5");
});

// ----------------------------------------------------------------------
// SPEC-014: config file editable + hot-reloadable
// ----------------------------------------------------------------------

test("SPEC-014: loadRouterConfig parses default_model + rules + llm knobs from TOML", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "cusa-router-"));
  try {
    const file = path.join(dir, "router.toml");
    await writeFile(
      file,
      `
default_model = "special-default"
llm_enabled = false
llm_timeout_ms = 1234
llm_classifier_model = "classifier-x"

[[rule]]
name = "shell"
model = "shell-model"
rationale = "shells"
match = { any_of = ["shell", "bash"] }
`,
      "utf8",
    );
    const loaded = await loadRouterConfig({ configPath: file });
    assert.equal(loaded.fromFile, true);
    assert.equal(loaded.config.defaultModel, "special-default");
    assert.equal(loaded.config.llmEnabled, false);
    assert.equal(loaded.config.llmTimeoutMs, 1234);
    assert.equal(loaded.config.llmClassifierModel, "classifier-x");
    assert.equal(loaded.config.rules.length, 1);
    assert.equal(loaded.config.rules[0]!.model, "shell-model");
    assert.deepEqual(loaded.config.rules[0]!.match.anyOf, ["shell", "bash"]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("SPEC-014: missing config file → built-in defaults and a one-time info log", async () => {
  const logs: string[] = [];
  const loaded = await loadRouterConfig({
    configPath: "/definitely/does/not/exist.toml",
    log: (_l, m) => logs.push(m),
  });
  assert.equal(loaded.fromFile, false);
  assert.equal(loaded.config.defaultModel, "composer-2.5");
  assert.deepEqual(loaded.config.rules, [...builtInDefaultRules]);
  assert.equal(logs.length, 1);
  assert.match(logs[0]!, /router\.toml/);
});

test("SPEC-014: watchRouterConfig hot-reloads on file change", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "cusa-router-w-"));
  try {
    const file = path.join(dir, "router.toml");
    await writeFile(file, `default_model = "one"\n`, "utf8");
    let latest = "one";
    const watcher = await watchRouterConfig({
      configPath: file,
      debounceMs: 25,
      onReload: (next) => {
        latest = next.defaultModel;
      },
    });
    try {
      assert.equal(watcher.current().defaultModel, "one");
      // Small settle so the watch handle is definitely armed.
      await delay(50);
      await writeFile(file, `default_model = "two"\n`, "utf8");
      // Poll for the reload with a bounded budget so the test doesn't
      // wedge on slow fs.watch (macOS FSEvents can be laggy).
      const start = Date.now();
      while (latest !== "two" && Date.now() - start < 3000) {
        await delay(50);
      }
      assert.equal(latest, "two");
      assert.equal(watcher.current().defaultModel, "two");
    } finally {
      watcher.close();
    }
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("SPEC-014: parse error keeps previous good config in place", async () => {
  const logs: string[] = [];
  const parsed = parseRouterTomlSafe(`default_model = broken\n`);
  assert.ok(parsed.errors.length > 0);
  // The higher-level loader must not crash and must emit a warn.
  const loaded = await loadRouterConfig({
    configPath: "/tmp/nope",
    existsImpl: () => true,
    readFileImpl: async () => `default_model = broken\n`,
    log: (l, m) => logs.push(`${l}:${m}`),
  });
  assert.equal(loaded.fromFile, false);
  assert.equal(loaded.config.defaultModel, "composer-2.5");
  assert.ok(logs.some((l) => l.startsWith("warn:")));
});

// ----------------------------------------------------------------------
// SPEC-015: LLM 5 s hard timeout → session-default fallback
// ----------------------------------------------------------------------

test("SPEC-015: router llm timeout falls back to default model with source='fallback'", async () => {
  const router = new Router({
    initialConfig: {
      ...builtInDefaultConfig(),
      rules: [],
      llmEnabled: true,
      llmTimeoutMs: 30,
    },
    llmClient: {
      classify: (input) =>
        new Promise<string>((_resolve, reject) => {
          input.signal.addEventListener("abort", () => reject(new Error("aborted")));
        }),
    },
  });
  const d = await router.route({
    prompt: "won't match any built-in rule but the LLM hangs",
    currentModel: "session-default-model",
  });
  assert.equal(d.source, "fallback");
  assert.equal(d.model, "session-default-model");
  assert.match(d.rationale, /timeout/i);
});

test("SPEC-015: malformed classifier output falls back to default model", async () => {
  const router = new Router({
    initialConfig: {
      ...builtInDefaultConfig(),
      rules: [],
      llmEnabled: true,
    },
    llmClient: {
      classify: async () => "not-json at all",
    },
  });
  const d = await router.route({ prompt: "any" });
  assert.equal(d.source, "fallback");
});

// ----------------------------------------------------------------------
// Classifier prompt + parser
// ----------------------------------------------------------------------

test("SPEC-013: buildClassifierPrompt lists candidate models and asks for strict JSON", () => {
  const p = buildClassifierPrompt("do something hard", ["a", "b"]);
  assert.match(p, /Candidates: a, b/);
  assert.match(p, /\{"model": "<id>", "rationale": "<short reason>"\}/);
});

test("SPEC-013: parseClassifierOutput tolerates surrounding prose", () => {
  const raw = 'Sure! {"model":"claude-sonnet-4","rationale":"reasoning"} done.';
  const r = parseClassifierOutput(raw);
  assert.deepEqual(r, {
    model: "claude-sonnet-4",
    rationale: "reasoning",
  });
});

test("SPEC-013: parseClassifierOutput rejects missing model", () => {
  assert.equal(parseClassifierOutput("{}"), null);
  assert.equal(parseClassifierOutput("no braces at all"), null);
});

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

function makeExplodingLlmClient(): RouterLlmClient {
  return {
    classify: () => {
      throw new Error(
        "LLM classify should never be called when a shortcut path is taken",
      );
    },
  };
}
