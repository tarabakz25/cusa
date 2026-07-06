// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import { approvalPolicy, shouldEnableSandbox } from "./policy.ts";

test("SPEC-022: suggest mode prompts for shell/write tools", () => {
  assert.equal(
    approvalPolicy({ mode: "suggest", toolName: "shell", category: "shell" }),
    "prompt",
  );
  assert.equal(
    approvalPolicy({ mode: "suggest", toolName: "write", category: "write" }),
    "prompt",
  );
});

test("SPEC-023: auto-edit mode auto-approves read tools, prompts on write/shell", () => {
  assert.equal(
    approvalPolicy({ mode: "auto-edit", toolName: "read", category: "read" }),
    "auto-approve",
  );
  assert.equal(
    approvalPolicy({ mode: "auto-edit", toolName: "write", category: "write" }),
    "prompt",
  );
  assert.equal(
    approvalPolicy({ mode: "auto-edit", toolName: "shell", category: "shell" }),
    "prompt",
  );
});

test("SPEC-024: full-auto auto-approves everything and enables sandbox", () => {
  assert.equal(
    approvalPolicy({ mode: "full-auto", toolName: "shell", category: "shell" }),
    "auto-approve",
  );
  assert.equal(
    approvalPolicy({ mode: "full-auto", toolName: "write", category: "write" }),
    "auto-approve",
  );
  assert.equal(shouldEnableSandbox("full-auto"), true);
  assert.equal(shouldEnableSandbox("suggest"), false);
  assert.equal(shouldEnableSandbox("auto-edit"), false);
});
