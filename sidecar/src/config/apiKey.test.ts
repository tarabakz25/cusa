// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import { parseApiKeyFromToml, readApiKey, redact } from "./apiKey.ts";

test("SPEC-100: readApiKey pulls from CURSOR_API_KEY env when set", async () => {
  const src = await readApiKey({
    env: { CURSOR_API_KEY: "sk_test_env" },
    configPath: "/nonexistent/config.toml",
  });
  assert.deepEqual(src, { key: "sk_test_env", origin: "env" });
});

test("SPEC-100: readApiKey falls back to ~/.cusa/config.toml", async () => {
  const src = await readApiKey({
    env: {},
    configPath: "/virtual/config.toml",
    readFileImpl: async (p) => {
      assert.equal(p, "/virtual/config.toml");
      return '# leading comment\napi_key = "sk_test_config"\n';
    },
  });
  assert.deepEqual(src, { key: "sk_test_config", origin: "config" });
});

test("SPEC-100: readApiKey returns null when neither env nor config has a key", async () => {
  const src = await readApiKey({
    env: {},
    configPath: "/virtual/missing.toml",
    readFileImpl: async () => {
      throw new Error("ENOENT");
    },
  });
  assert.equal(src, null);
});

test("SPEC-100: parseApiKeyFromToml tolerates comments and section headers", () => {
  const text = `# top comment\n[cursor]\napi_key = "sk_abc" # inline comment\nother = 42\n`;
  assert.equal(parseApiKeyFromToml(text), "sk_abc");
});

test("SPEC-100: redact() masks secret substrings", () => {
  assert.equal(redact("token=sk_supersecret", "sk_supersecret"), "token=[redacted]");
  assert.equal(redact("plain text", null), "plain text");
});
