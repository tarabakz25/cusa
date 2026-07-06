// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, statSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

import {
  argvIsVerbose,
  ensureLogDir,
  logDirLooksHealthy,
} from "../lib/logs.js";

function tmpHome() {
  return mkdtempSync(path.join(os.tmpdir(), "cusa-logs-"));
}

test("SPEC-102: shim ensures ~/.cusa/logs/ exists with mode 0700 when --verbose is passed", () => {
  const home = tmpHome();
  const { logsDir } = ensureLogDir({ cusaHome: home });
  assert.equal(logsDir, path.join(home, "logs"));
  const st = statSync(logsDir);
  assert.ok(st.isDirectory(), "logsDir should be a directory");
  if (process.platform !== "win32") {
    assert.equal(
      (st.mode & 0o777).toString(8),
      "700",
      "logsDir should be mode 0700 on POSIX",
    );
  }
  assert.equal(logDirLooksHealthy(logsDir), true);
});

test("SPEC-102: ensureLogDir is idempotent", () => {
  const home = tmpHome();
  ensureLogDir({ cusaHome: home });
  const { logsDir } = ensureLogDir({ cusaHome: home });
  const st = statSync(logsDir);
  assert.ok(st.isDirectory());
  if (process.platform !== "win32") {
    assert.equal((st.mode & 0o777).toString(8), "700");
  }
});

test("SPEC-102: argvIsVerbose detects --verbose and -v", () => {
  assert.equal(argvIsVerbose(["--verbose"]), true);
  assert.equal(argvIsVerbose(["-v"]), true);
  assert.equal(argvIsVerbose(["--verbose=1"]), true);
  assert.equal(argvIsVerbose(["--other", "--verbose", "trailing"]), true);
});

test("SPEC-102: argvIsVerbose returns false when absent", () => {
  assert.equal(argvIsVerbose([]), false);
  assert.equal(argvIsVerbose(["--resume", "agent-1"]), false);
});
