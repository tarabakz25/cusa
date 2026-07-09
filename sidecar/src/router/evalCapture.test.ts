// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Dev-only eval capture tests (issue #7 Phase 2). The critical
// guarantees are NFR-2-shaped: nothing is written unless
// CUSA_EVAL_CAPTURE=1, and the raw-prompt file is chmod 0600.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat, writeFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  MAX_CAPTURE_BYTES,
  appendEvalSample,
  evalCaptureEnabled,
  evalCapturePath,
  type EvalSample,
} from "./evalCapture.ts";

function sample(overrides: Partial<EvalSample> = {}): EvalSample {
  return {
    ts: "2026-07-08T00:00:00.000Z",
    prompt: "fix this typo",
    chosen: { id: "composer-2" },
    source: "local",
    rationale: 'nearest to: "fix this typo"',
    cosine: 0.91,
    thetaSnapshot: { high: 0.55, low: 0.35 },
    llmConsulted: false,
    ...overrides,
  };
}

test("#7: capture is OFF by default and writes NOTHING when unset/other values", async () => {
  for (const env of [{}, { CUSA_EVAL_CAPTURE: "0" }, { CUSA_EVAL_CAPTURE: "true" }]) {
    const dir = await mkdtemp(path.join(tmpdir(), "cusa-cap-off-"));
    try {
      const e = { ...env, CUSA_HOME: dir } as NodeJS.ProcessEnv;
      assert.equal(evalCaptureEnabled(e), false);
      await appendEvalSample(sample(), { env: e });
      assert.equal(existsSync(path.join(dir, "dev")), false);
      assert.equal(existsSync(evalCapturePath({ env: e })), false);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  }
});

test("#7: CUSA_EVAL_CAPTURE=1 appends JSONL under CUSA_HOME/dev with mode 0600", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "cusa-cap-on-"));
  try {
    const env = { CUSA_EVAL_CAPTURE: "1", CUSA_HOME: dir } as NodeJS.ProcessEnv;
    await appendEvalSample(sample(), { env });
    await appendEvalSample(
      sample({ source: "override", correctionOf: "composer-2" }),
      { env },
    );
    const file = evalCapturePath({ env });
    const lines = (await readFile(file, "utf8")).trim().split("\n");
    assert.equal(lines.length, 2);
    const first = JSON.parse(lines[0]!) as EvalSample;
    assert.equal(first.prompt, "fix this typo");
    assert.equal(first.cosine, 0.91);
    const second = JSON.parse(lines[1]!) as EvalSample;
    assert.equal(second.correctionOf, "composer-2");
    const st = await stat(file);
    assert.equal(st.mode & 0o777, 0o600, "capture file must be 0600");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("#7: capture stops appending past the size cap (no unbounded growth)", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "cusa-cap-cap-"));
  try {
    const env = { CUSA_EVAL_CAPTURE: "1", CUSA_HOME: dir } as NodeJS.ProcessEnv;
    const file = evalCapturePath({ env });
    await mkdir(path.dirname(file), { recursive: true });
    await writeFile(file, "x".repeat(MAX_CAPTURE_BYTES + 1), { mode: 0o600 });
    const warns: string[] = [];
    await appendEvalSample(sample(), {
      env,
      log: (_l, m) => warns.push(m),
    });
    const st = await stat(file);
    assert.equal(st.size, MAX_CAPTURE_BYTES + 1, "no bytes appended past cap");
    assert.ok(warns.some((w) => /exceeds/.test(w)));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("#7: capture failures never throw into the turn", async () => {
  const env = {
    CUSA_EVAL_CAPTURE: "1",
    // A path that cannot be a directory root on POSIX.
    CUSA_HOME: "/dev/null/notadir",
  } as NodeJS.ProcessEnv;
  await assert.doesNotReject(appendEvalSample(sample(), { env }));
});
