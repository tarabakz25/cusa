// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Dev-only routing-decision capture for θ tuning (issue #7, Phase 2).
//
// SECURITY / PRIVACY CONTRACT (NFR-2):
// - Gated on CUSA_EVAL_CAPTURE=1. Default OFF. When the variable is not
//   exactly "1", this module NEVER touches the filesystem — guaranteed by
//   test.
// - Samples are appended locally to `$CUSA_HOME|~/.cusa/dev/
//   eval-samples.jsonl` with file mode 0600. Nothing is ever sent
//   anywhere.
// - The file may contain raw prompts (potentially secrets/PII) — that is
//   why the gate exists and why the path lives under `dev/`.
// - Size-capped: appends stop once the file exceeds MAX_CAPTURE_BYTES.
// - Capture failures are swallowed; a turn must never fail because of
//   telemetry-for-tuning.

import { appendFile, chmod, mkdir, stat } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";

export const EVAL_CAPTURE_ENV = "CUSA_EVAL_CAPTURE";
export const MAX_CAPTURE_BYTES = 5 * 1024 * 1024;

export interface EvalSample {
  ts: string;
  prompt: string;
  chosen: { id: string };
  source: string;
  rationale: string;
  /** Cosine score when the local classifier produced one. */
  cosine?: number;
  thetaSnapshot?: { high: number; low: number };
  llmConsulted: boolean;
  /**
   * Correction signal: when a manual `/model <id>` override immediately
   * follows an auto decision, the auto-chosen id is recorded here — the
   * strongest available label that the auto decision was wrong.
   */
  correctionOf?: string;
}

export interface EvalCaptureOptions {
  env?: NodeJS.ProcessEnv;
  homeDirImpl?: () => string;
  log?: (level: "warn", message: string) => void;
}

export function evalCaptureEnabled(env: NodeJS.ProcessEnv = process.env): boolean {
  return env[EVAL_CAPTURE_ENV] === "1";
}

export function evalCapturePath(opts: EvalCaptureOptions = {}): string {
  const env = opts.env ?? process.env;
  const home = opts.homeDirImpl ?? homedir;
  const base = env["CUSA_HOME"] && env["CUSA_HOME"]!.length > 0
    ? env["CUSA_HOME"]!
    : path.join(home(), ".cusa");
  return path.join(base, "dev", "eval-samples.jsonl");
}

/**
 * Append one routing decision to the dev capture file. No-op (with zero
 * filesystem access) unless CUSA_EVAL_CAPTURE=1. Never throws.
 */
export async function appendEvalSample(
  sample: EvalSample,
  opts: EvalCaptureOptions = {},
): Promise<void> {
  const env = opts.env ?? process.env;
  if (!evalCaptureEnabled(env)) return;
  try {
    const file = evalCapturePath(opts);
    const dir = path.dirname(file);
    await mkdir(dir, { recursive: true, mode: 0o700 });
    let existingSize = 0;
    let existed = false;
    try {
      const st = await stat(file);
      existingSize = st.size;
      existed = true;
    } catch {
      /* new file */
    }
    if (existingSize > MAX_CAPTURE_BYTES) {
      opts.log?.(
        "warn",
        `eval capture: ${file} exceeds ${MAX_CAPTURE_BYTES} bytes; skipping (rotate or scrub it)`,
      );
      return;
    }
    await appendFile(file, `${JSON.stringify(sample)}\n`, { mode: 0o600 });
    if (!existed) {
      // appendFile's mode only applies on creation on some platforms;
      // enforce 0600 explicitly for the raw-prompt file.
      await chmod(file, 0o600);
    }
  } catch (err) {
    opts.log?.("warn", `eval capture failed: ${(err as Error).message}`);
  }
}
