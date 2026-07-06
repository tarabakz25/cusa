// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  detectNativeConversationRetention,
  shouldUseNativeRetention,
  type FeatureDetectResult,
} from "./feature_detect.ts";

// ----------------------------------------------------------------------
// SPEC-093
// ----------------------------------------------------------------------

test("SPEC-093: detection returns nativeRetention=false when SDK cannot be resolved", async () => {
  const res = await detectNativeConversationRetention({
    resolveEntry: () => null,
  });
  assert.equal(res.nativeRetention, false);
  assert.match(res.reason, /not resolvable/);
});

test("SPEC-093: detection returns false when SDK has no retention signals", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "cusa-detect-"));
  try {
    const sdkDir = path.join(dir, "node_modules", "@cursor", "sdk");
    const distEsm = path.join(sdkDir, "dist", "esm", "agent");
    await mkdir(distEsm, { recursive: true });
    await writeFile(
      path.join(sdkDir, "dist", "esm", "options.d.ts"),
      "export interface AgentOptions { model?: string; }",
      "utf8",
    );
    const entry = path.join(sdkDir, "dist", "esm", "index.d.ts");
    await writeFile(entry, "export * from './options.js';", "utf8");
    const res = await detectNativeConversationRetention({
      resolveEntry: () => entry,
    });
    assert.equal(res.nativeRetention, false);
    assert.match(res.reason, /no native retention signals/);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("SPEC-093: detection returns true when SDK d.ts exposes retainConversation", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "cusa-detect-"));
  try {
    const sdkDir = path.join(dir, "node_modules", "@cursor", "sdk");
    const distEsm = path.join(sdkDir, "dist", "esm", "agent");
    await mkdir(distEsm, { recursive: true });
    await writeFile(
      path.join(distEsm, "options.d.ts"),
      "export interface AgentOptions { retainConversation?: boolean; }",
      "utf8",
    );
    const entry = path.join(sdkDir, "dist", "esm", "index.d.ts");
    await writeFile(entry, "export * from './agent/options.js';", "utf8");
    const res = await detectNativeConversationRetention({
      resolveEntry: () => entry,
    });
    assert.equal(res.nativeRetention, true);
    assert.match(res.reason, /retainConversation/);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("SPEC-093: detection tolerates unreadable files (best-effort)", async () => {
  const res = await detectNativeConversationRetention({
    resolveEntry: () => "/tmp/definitely-not-here/dist/esm/index.d.ts",
    readFileImpl: async () => {
      throw new Error("ENOENT");
    },
  });
  assert.equal(res.nativeRetention, false);
});

// ----------------------------------------------------------------------
// SPEC-093: config knob resolves the detection result
// ----------------------------------------------------------------------

test("SPEC-093: shouldUseNativeRetention respects manual override", () => {
  const detection: FeatureDetectResult = {
    nativeRetention: true,
    reason: "detected retainConversation",
  };
  const r = shouldUseNativeRetention("manual", detection);
  assert.equal(r.useNative, false);
  assert.match(r.reason, /manual/);
});

test("SPEC-093: shouldUseNativeRetention respects native override even without detection", () => {
  const detection: FeatureDetectResult = {
    nativeRetention: false,
    reason: "nothing found",
  };
  const r = shouldUseNativeRetention("native", detection);
  assert.equal(r.useNative, true);
  assert.match(r.reason, /trusting SDK/);
});

test("SPEC-093: shouldUseNativeRetention 'auto' returns the detection result", () => {
  const yes = shouldUseNativeRetention("auto", {
    nativeRetention: true,
    reason: "found",
  });
  assert.equal(yes.useNative, true);
  const no = shouldUseNativeRetention("auto", {
    nativeRetention: false,
    reason: "none",
  });
  assert.equal(no.useNative, false);
});
