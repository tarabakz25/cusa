// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  DEFAULT_BACKUP_COUNT,
  DEFAULT_ROTATE_BYTES,
  RotatingLogger,
  formatLine,
  formatStamp,
} from "./rotate.ts";

async function scratch(): Promise<{ dir: string; cleanup: () => Promise<void> }> {
  const dir = await mkdtemp(path.join(tmpdir(), "cusa-log-"));
  return { dir, cleanup: () => rm(dir, { recursive: true, force: true }) };
}

// ----------------------------------------------------------------------
// SPEC-102: rotation + retention
// ----------------------------------------------------------------------

test("SPEC-102: defaults are 10 MiB and 3 backups", () => {
  assert.equal(DEFAULT_ROTATE_BYTES, 10 * 1024 * 1024);
  assert.equal(DEFAULT_BACKUP_COUNT, 3);
});

test("SPEC-102: rotate creates new file after 10 MiB and prunes to 3 backups", async () => {
  const { dir, cleanup } = await scratch();
  try {
    const file = path.join(dir, "cusa-sidecar.log");
    // Tiny threshold so the test can drive rotations synthetically.
    let clockMs = Date.parse("2026-07-06T15:00:00Z");
    const rotator = new RotatingLogger({
      filePath: file,
      rotateBytes: 128,
      backupCount: 3,
      now: () => {
        // Advance so successive rotations get distinct stamps.
        clockMs += 1500;
        return new Date(clockMs);
      },
    });
    const big = "x".repeat(200);
    // Write 5 lines that each exceed the rotation threshold. That
    // should yield 4 rotations and 1 live file; the 4th rotation
    // triggers a prune down to 3 backups.
    for (let i = 0; i < 5; i++) {
      await rotator.write({ level: "info", message: `line-${i}-${big}` });
    }
    await rotator.drain();
    const names = (await readdir(dir)).sort();
    const backups = names.filter(
      (n) => n.startsWith("cusa-sidecar.") && n !== "cusa-sidecar.log",
    );
    assert.equal(backups.length, 3, `expected 3 backups, got: ${names.join(", ")}`);
    assert.ok(names.includes("cusa-sidecar.log"), "live file must exist");
  } finally {
    await cleanup();
  }
});

test("SPEC-102: write below rotate threshold keeps a single file", async () => {
  const { dir, cleanup } = await scratch();
  try {
    const file = path.join(dir, "cusa-sidecar.log");
    const rotator = new RotatingLogger({
      filePath: file,
      rotateBytes: 100_000,
      backupCount: 3,
    });
    for (let i = 0; i < 3; i++) {
      await rotator.write({ level: "info", message: `msg ${i}` });
    }
    await rotator.drain();
    const names = await readdir(dir);
    assert.deepEqual(names.sort(), ["cusa-sidecar.log"]);
    const body = await readFile(file, "utf8");
    assert.match(body, /msg 0/);
    assert.match(body, /msg 1/);
    assert.match(body, /msg 2/);
  } finally {
    await cleanup();
  }
});

test("SPEC-102: writes carry ISO timestamp, level, optional target, and message", async () => {
  const { dir, cleanup } = await scratch();
  try {
    const file = path.join(dir, "cusa-sidecar.log");
    const rotator = new RotatingLogger({ filePath: file });
    await rotator.write({
      level: "warn",
      message: "hello",
      target: "sidecar/context",
      ts: new Date("2026-07-06T00:00:00Z"),
    });
    await rotator.drain();
    const text = await readFile(file, "utf8");
    assert.match(text, /2026-07-06T00:00:00\.000Z WARN \[sidecar\/context\] hello/);
  } finally {
    await cleanup();
  }
});

test("SPEC-102: formatStamp yields a filesystem-safe stamp", () => {
  const s = formatStamp(new Date("2026-01-02T03:04:05"));
  assert.match(s, /^\d{8}-\d{6}$/);
});

test("SPEC-102: formatLine handles missing target", () => {
  const line = formatLine(
    { level: "info", message: "hi" },
    new Date("2026-07-06T00:00:00Z"),
  );
  assert.equal(line, "2026-07-06T00:00:00.000Z INFO hi\n");
});

test("SPEC-102: write after close is a silent no-op", async () => {
  const { dir, cleanup } = await scratch();
  try {
    const file = path.join(dir, "x.log");
    const rotator = new RotatingLogger({ filePath: file });
    rotator.close();
    await rotator.write({ level: "info", message: "post-close" });
    await rotator.drain();
    const names = await readdir(dir);
    // The file was never opened; readdir returns []
    assert.deepEqual(names, []);
  } finally {
    await cleanup();
  }
});
