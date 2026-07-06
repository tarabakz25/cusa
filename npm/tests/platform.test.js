// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import test from "node:test";
import assert from "node:assert/strict";

import { detectPlatform, parseTarget } from "../lib/platform.js";

test("SPEC-081: detectPlatform returns darwin-arm64 on macOS ARM", () => {
  const result = detectPlatform({ platform: "darwin", arch: "arm64" });
  assert.equal(result.platform, "darwin");
  assert.equal(result.arch, "arm64");
  assert.equal(result.target, "darwin-arm64");
  assert.equal(result.exe, "cusa-tui");
  assert.equal(result.rustTriple, "aarch64-apple-darwin");
});

test("SPEC-081: detectPlatform returns win32-x64 with .exe suffix", () => {
  const result = detectPlatform({ platform: "win32", arch: "x64" });
  assert.equal(result.target, "win32-x64");
  assert.equal(result.exe, "cusa-tui.exe");
  assert.equal(result.rustTriple, "x86_64-pc-windows-msvc");
});

test("SPEC-081: detectPlatform returns linux-x64", () => {
  const result = detectPlatform({ platform: "linux", arch: "x64" });
  assert.equal(result.target, "linux-x64");
  assert.equal(result.exe, "cusa-tui");
  assert.equal(result.rustTriple, "x86_64-unknown-linux-gnu");
});

test("SPEC-081: detectPlatform throws for unsupported platform", () => {
  assert.throws(
    () => detectPlatform({ platform: "freebsd", arch: "x64" }),
    /unsupported platform 'freebsd'/,
  );
});

test("SPEC-081: detectPlatform throws for unsupported architecture", () => {
  assert.throws(
    () => detectPlatform({ platform: "linux", arch: "ia32" }),
    /unsupported architecture 'ia32'/,
  );
});

test("SPEC-081: parseTarget round-trips a valid slug", () => {
  const parsed = parseTarget("linux-arm64");
  assert.equal(parsed.platform, "linux");
  assert.equal(parsed.arch, "arm64");
  assert.equal(parsed.target, "linux-arm64");
});

test("SPEC-081: parseTarget rejects malformed slug", () => {
  assert.throws(() => parseTarget("darwinArm64"), /invalid --target/);
});

test("SPEC-081: parseTarget rejects unsupported combination", () => {
  assert.throws(
    () => parseTarget("solaris-x64"),
    /unsupported platform 'solaris'/,
  );
});
