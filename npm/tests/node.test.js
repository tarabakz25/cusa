// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import test from "node:test";
import assert from "node:assert/strict";

import { checkNodeVersion, enforceNode } from "../lib/node.js";

test("SPEC-082: checkNodeVersion accepts >= 20", () => {
  const r = checkNodeVersion("20.0.0");
  assert.equal(r.ok, true);
  assert.equal(r.detectedMajor, 20);
});

test("SPEC-082: checkNodeVersion accepts far-future major", () => {
  const r = checkNodeVersion("26.4.0");
  assert.equal(r.ok, true);
  assert.equal(r.detectedMajor, 26);
});

test("SPEC-082: checkNodeVersion rejects Node 18 with actionable message", () => {
  const r = checkNodeVersion("18.19.1");
  assert.equal(r.ok, false);
  assert.equal(r.detectedMajor, 18);
  assert.match(r.message, /requires Node.js >= 20/);
  assert.match(r.message, /Detected 18\.19\.1/);
  assert.match(r.message, /https:\/\/nodejs\.org\//);
});

test("SPEC-082: checkNodeVersion rejects garbage input", () => {
  const r = checkNodeVersion("not-a-version");
  assert.equal(r.ok, false);
  assert.match(r.message, /cannot parse/);
});

test("SPEC-082: shim rejects Node < 20 with actionable message (enforceNode)", () => {
  let stderrBuf = "";
  const stderr = {
    write(s) {
      stderrBuf += s;
      return true;
    },
  };
  let exitCode = null;
  const exitFn = (code) => {
    exitCode = code;
    return /** @type {never} */ (undefined);
  };
  enforceNode({ nodeVersion: "16.20.0", stderr, exitFn });
  assert.equal(exitCode, 1);
  assert.match(stderrBuf, /requires Node.js >= 20/);
  assert.match(stderrBuf, /Detected 16\.20\.0/);
});

test("SPEC-082: enforceNode with satisfying version does not exit", () => {
  let exitCode = null;
  const stderr = { write: () => true };
  const exitFn = (code) => {
    exitCode = code;
    return /** @type {never} */ (undefined);
  };
  const r = enforceNode({ nodeVersion: "22.1.0", stderr, exitFn });
  assert.equal(exitCode, null);
  assert.equal(r.ok, true);
});
