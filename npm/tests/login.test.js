// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

import { renderConfig, tomlString, writeApiKey } from "../lib/login.js";

function tmpHome() {
  return mkdtempSync(path.join(os.tmpdir(), "cusa-login-"));
}

test("SPEC-101: cusa login writes ~/.cusa/config.toml at mode 0600 with the given key", () => {
  const home = tmpHome();
  const result = writeApiKey({
    apiKey: "key_abcDEF1234",
    cusaHome: home,
    platform: "linux",
  });
  assert.equal(result.path, path.join(home, "config.toml"));

  const contents = readFileSync(result.path, "utf8");
  assert.match(contents, /^\[api\]$/m);
  assert.match(contents, /^api_key = "key_abcDEF1234"$/m);

  if (process.platform !== "win32") {
    const st = statSync(result.path);
    assert.equal(
      (st.mode & 0o777).toString(8),
      "600",
      "config.toml should be mode 0600 on POSIX",
    );
  }
});

test("SPEC-101: writeApiKey replaces existing api_key inside [api]", () => {
  const home = tmpHome();
  const cfgPath = path.join(home, "config.toml");
  writeFileSync(
    cfgPath,
    `[core]\nnice = true\n\n[api]\napi_key = "OLD"\nfoo = 1\n\n[other]\nx = 2\n`,
    { mode: 0o600 },
  );
  writeApiKey({ apiKey: "NEW", cusaHome: home, platform: "linux" });
  const after = readFileSync(cfgPath, "utf8");
  assert.match(after, /^api_key = "NEW"$/m);
  assert.doesNotMatch(after, /"OLD"/);
  assert.match(after, /^foo = 1$/m);
  assert.match(after, /^\[other\]$/m);
});

test("SPEC-101: writeApiKey appends [api] when missing", () => {
  const out = renderConfig(`[core]\nnice = true\n`, "K");
  assert.match(out, /^\[core\]$/m);
  assert.match(out, /^\[api\]$/m);
  assert.match(out, /^api_key = "K"$/m);
});

test("SPEC-101: writeApiKey adds api_key when [api] exists but empty", () => {
  const out = renderConfig(`[api]\n`, "K");
  assert.match(out, /^\[api\]$/m);
  assert.match(out, /^api_key = "K"$/m);
});

test("SPEC-101: writeApiKey rejects empty key", () => {
  const home = tmpHome();
  assert.throws(
    () => writeApiKey({ apiKey: "   ", cusaHome: home, platform: "linux" }),
    /empty API key/,
  );
});

test("SPEC-101: writeApiKey refuses Windows without --force-windows", () => {
  const home = tmpHome();
  assert.throws(
    () => writeApiKey({ apiKey: "K", cusaHome: home, platform: "win32" }),
    /Windows is not fully supported/,
  );
});

test("SPEC-101: writeApiKey allows Windows with --force-windows", () => {
  const home = tmpHome();
  const result = writeApiKey({
    apiKey: "K",
    cusaHome: home,
    platform: "win32",
    forceWindows: true,
  });
  const contents = readFileSync(result.path, "utf8");
  assert.match(contents, /^api_key = "K"$/m);
});

test("SPEC-101: tomlString escapes quotes and backslashes", () => {
  assert.equal(tomlString("a\\b"), '"a\\\\b"');
  assert.equal(tomlString('he said "hi"'), '"he said \\"hi\\""');
  assert.equal(tomlString("plain-key"), '"plain-key"');
});
