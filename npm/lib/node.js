// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-082: Node.js >= 20 enforcement. Kept as a small, testable module so
// that both the shim entry (`bin/cusa.js`) and unit tests can exercise the
// enforcement function without spawning a child Node process.

export const NODE_MAJOR_MIN = 20;

/**
 * @typedef {Object} EnforceResult
 * @property {boolean} ok
 * @property {string}  [message] Human-readable error, present iff !ok.
 * @property {number}  [detectedMajor]
 */

/**
 * Evaluate whether the given Node version satisfies the minimum. Pure — does
 * not exit or write anywhere; the caller decides how to react.
 *
 * @param {string} nodeVersion e.g. "20.11.1" (as in `process.versions.node`).
 * @param {number} [min=NODE_MAJOR_MIN]
 * @returns {EnforceResult}
 */
export function checkNodeVersion(nodeVersion, min = NODE_MAJOR_MIN) {
  if (typeof nodeVersion !== "string" || nodeVersion.length === 0) {
    return {
      ok: false,
      message: `cusa: cannot determine Node.js version (got ${JSON.stringify(nodeVersion)}).`,
    };
  }
  const major = Number(nodeVersion.split(".")[0]);
  if (Number.isNaN(major)) {
    return {
      ok: false,
      message: `cusa: cannot parse Node.js version '${nodeVersion}'.`,
    };
  }
  if (major < min) {
    return {
      ok: false,
      detectedMajor: major,
      message:
        `cusa: requires Node.js >= ${min}. Detected ${nodeVersion}.\n` +
        `      See https://nodejs.org/ for install instructions.`,
    };
  }
  return { ok: true, detectedMajor: major };
}

/**
 * Enforce the Node.js minimum by writing to `stderr` and calling `exitFn(1)`
 * on failure. Used by the shim; not intended for tests.
 *
 * @param {{
 *   nodeVersion?: string,
 *   stderr?: NodeJS.WritableStream,
 *   exitFn?: (code: number) => never,
 *   min?: number,
 * }} [opts]
 */
export function enforceNode(opts = {}) {
  const nodeVersion = opts.nodeVersion ?? process.versions.node;
  const stderr = opts.stderr ?? process.stderr;
  const exitFn = opts.exitFn ?? ((code) => process.exit(code));
  const min = opts.min ?? NODE_MAJOR_MIN;

  const result = checkNodeVersion(nodeVersion, min);
  if (!result.ok) {
    stderr.write(`${result.message}\n`);
    return exitFn(1);
  }
  return result;
}
