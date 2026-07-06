// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Integration-style tests for the top-level shim + postinstall behavior.
// We spawn a fresh Node process running each script so we exercise the same
// entry points npm would.

import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const npmRoot = path.resolve(here, "..");
const shim = path.join(npmRoot, "bin", "cusa.js");
const postinstall = path.join(npmRoot, "postinstall.js");

function runNode(script, args, env = {}) {
  return spawnSync(process.execPath, [script, ...args], {
    env: { ...process.env, ...env },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

test("SPEC-080: postinstall exits 0 when the release URL is unreachable", () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "cusa-pi-"));
  const result = runNode(postinstall, [], {
    CUSA_HOME: home,
    // Point at a port nothing is listening on.
    CUSA_RELEASE_BASE_URL: "http://127.0.0.1:1/never",
    // Ensure we don't accidentally hit the "skip in CI" branch.
    CI: "",
    CUSA_ALLOW_CI_DOWNLOAD: "",
    // Ensure we don't accidentally hit the "skip via env" branch.
    CUSA_SKIP_POSTINSTALL: "",
  });
  assert.equal(result.status, 0, `postinstall should exit 0; got ${result.status}. stderr=${result.stderr}`);
  assert.match(
    result.stderr + result.stdout,
    /cusa: could not fetch native binary|cusa download-binary|no native TUI binary/,
    "postinstall should emit a recovery hint",
  );
});

test("SPEC-080: postinstall respects CUSA_SKIP_POSTINSTALL=1", () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "cusa-pi-skip-"));
  const result = runNode(postinstall, [], {
    CUSA_HOME: home,
    CUSA_SKIP_POSTINSTALL: "1",
    CI: "",
  });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /CUSA_SKIP_POSTINSTALL=1/);
});

test("SPEC-080: postinstall skips in CI unless opted in", () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "cusa-pi-ci-"));
  const result = runNode(postinstall, [], {
    CUSA_HOME: home,
    CI: "1",
    CUSA_ALLOW_CI_DOWNLOAD: "",
    CUSA_SKIP_POSTINSTALL: "",
  });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /postinstall skipped in CI/);
});

test("SPEC-082: shim prints its version and does not crash without the binary", () => {
  const result = runNode(shim, ["--version"], {
    // Guarantee we do not accidentally find a bundled binary.
    CUSA_TUI: "",
    CUSA_HOME: mkdtempSync(path.join(os.tmpdir(), "cusa-shim-")),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^cusa \d/, `stdout was ${JSON.stringify(result.stdout)}`);
});

test("SPEC-101 shim: `cusa login --stdin` writes the piped key to config.toml", () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "cusa-shim-login-"));
  const child = spawnSync(process.execPath, [shim, "login", "--stdin"], {
    env: { ...process.env, CUSA_HOME: home },
    input: "cursor_test_key_xyz\n",
    encoding: "utf8",
  });
  assert.equal(child.status, 0, child.stderr);
  assert.match(child.stdout, /wrote API key/);
});
