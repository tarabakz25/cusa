// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  defaultConversationConfig,
  loadConversationConfig,
  parseConversationSection,
} from "./conversation.ts";

// ----------------------------------------------------------------------
// SPEC-093 config knobs
// ----------------------------------------------------------------------

test("SPEC-093: defaults are 'auto' mode, 6 turns, 32 KiB, 8000 ms, composer-2.5", () => {
  const d = defaultConversationConfig();
  assert.equal(d.mode, "auto");
  assert.equal(d.rawTurns, 6);
  assert.equal(d.byteBudget, 32768);
  assert.equal(d.summarizerTimeoutMs, 8000);
  assert.equal(d.summarizerModel, "composer-2.5");
});

test("SPEC-093: parses a fully populated [conversation] section", () => {
  const src = `
api_key = "sk_x"

[conversation]
mode = "native"
raw_turns = 10
byte_budget = 65536
summarizer_timeout_ms = 12000
summarizer_model = "custom-model"
`;
  const { config, warnings, found } = parseConversationSection(src);
  assert.equal(found, true);
  assert.deepEqual(warnings, []);
  assert.equal(config.mode, "native");
  assert.equal(config.rawTurns, 10);
  assert.equal(config.byteBudget, 65536);
  assert.equal(config.summarizerTimeoutMs, 12000);
  assert.equal(config.summarizerModel, "custom-model");
});

test("SPEC-093: unknown mode warns and keeps the default", () => {
  const { config, warnings } = parseConversationSection(
    `[conversation]\nmode = "nonsense"\n`,
  );
  assert.equal(config.mode, "auto");
  assert.ok(warnings.some((w) => /invalid mode/.test(w)));
});

test("SPEC-093: bad numeric values warn and keep the default", () => {
  const { config, warnings } = parseConversationSection(
    `[conversation]\nraw_turns = -1\nbyte_budget = 0\n`,
  );
  assert.equal(config.rawTurns, 6);
  assert.equal(config.byteBudget, 32768);
  assert.equal(warnings.filter((w) => /must be a positive integer/.test(w)).length, 2);
});

test("SPEC-093: absent [conversation] section returns defaults", () => {
  const { config, found } = parseConversationSection(`api_key = "sk"\n`);
  assert.equal(found, false);
  assert.deepEqual(config, defaultConversationConfig());
});

test("SPEC-093: loadConversationConfig returns defaults when file is missing", async () => {
  const loaded = await loadConversationConfig({
    configPath: "/nope/does-not-exist.toml",
  });
  assert.equal(loaded.fromFile, false);
  assert.deepEqual(loaded.config, defaultConversationConfig());
});

test("SPEC-093: loadConversationConfig reads via injected file impl", async () => {
  const loaded = await loadConversationConfig({
    configPath: "/virtual/config.toml",
    readFileImpl: async () =>
      `[conversation]\nmode = "manual"\nraw_turns = 8\n`,
  });
  assert.equal(loaded.fromFile, true);
  assert.equal(loaded.config.mode, "manual");
  assert.equal(loaded.config.rawTurns, 8);
});
