// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import test from "node:test";
import assert from "node:assert/strict";

import { parseSubcommandArgs, rewriteFullAutoForWindows } from "../lib/args.js";

test("R-7: rewriteFullAutoForWindows rewrites --approval=full-auto on win32", () => {
  const { argv, rewritten, warning } = rewriteFullAutoForWindows({
    argv: ["--approval=full-auto", "--resume", "abc"],
    platform: "win32",
  });
  assert.equal(rewritten, true);
  assert.deepEqual(argv, ["--approval=auto-edit", "--resume", "abc"]);
  assert.match(warning ?? "", /not supported on Windows/);
});

test("R-7: rewriteFullAutoForWindows rewrites --approval full-auto (split form)", () => {
  const { argv, rewritten } = rewriteFullAutoForWindows({
    argv: ["--approval", "full-auto"],
    platform: "win32",
  });
  assert.equal(rewritten, true);
  assert.deepEqual(argv, ["--approval", "auto-edit"]);
});

test("R-7: rewriteFullAutoForWindows rewrites --full-auto shortcut on win32", () => {
  const { argv, rewritten } = rewriteFullAutoForWindows({
    argv: ["--full-auto", "extra"],
    platform: "win32",
  });
  assert.equal(rewritten, true);
  assert.deepEqual(argv, ["--approval=auto-edit", "extra"]);
});

test("R-7: rewriteFullAutoForWindows is a no-op on darwin", () => {
  const before = ["--approval=full-auto", "--full-auto"];
  const { argv, rewritten, warning } = rewriteFullAutoForWindows({
    argv: before,
    platform: "darwin",
  });
  assert.equal(rewritten, false);
  assert.equal(warning, null);
  assert.deepEqual(argv, before);
});

test("R-7: rewriteFullAutoForWindows leaves auto-edit alone on win32", () => {
  const { rewritten, argv } = rewriteFullAutoForWindows({
    argv: ["--approval=auto-edit"],
    platform: "win32",
  });
  assert.equal(rewritten, false);
  assert.deepEqual(argv, ["--approval=auto-edit"]);
});

test("SPEC-101: parseSubcommandArgs recognizes booleans and strings", () => {
  const { flags, rest } = parseSubcommandArgs(
    ["--stdin", "--key=abc", "leftover"],
    { boolean: ["stdin"], string: ["key"] },
  );
  assert.equal(flags.stdin, true);
  assert.equal(flags.key, "abc");
  assert.deepEqual(rest, ["leftover"]);
});

test("SPEC-101: parseSubcommandArgs supports --key value (space form)", () => {
  const { flags } = parseSubcommandArgs(["--key", "val"], {
    string: ["key"],
  });
  assert.equal(flags.key, "val");
});

test("SPEC-101: parseSubcommandArgs rejects unknown flags", () => {
  assert.throws(
    () => parseSubcommandArgs(["--nope"], { boolean: ["stdin"] }),
    /unknown flag --nope/,
  );
});
