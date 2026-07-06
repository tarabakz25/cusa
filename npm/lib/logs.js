// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-102: ensure `~/.cusa/logs/` exists with mode 0700 when `--verbose` is
// enabled. The actual log writer lives in the sidecar / TUI; the shim just
// prepares the directory so that when the child process starts writing it
// does not race the filesystem or leak permissions.

import { chmodSync, existsSync, mkdirSync, statSync } from "node:fs";
import path from "node:path";
import process from "node:process";

import { cusaHomeFromEnv } from "./download.js";

/**
 * @param {{ cusaHome?: string }} [opts]
 * @returns {{ home: string, logsDir: string }}
 */
export function ensureLogDir(opts = {}) {
  const home = opts.cusaHome ?? cusaHomeFromEnv();
  const logsDir = path.join(home, "logs");

  mkdirSync(home, { recursive: true, mode: 0o755 });
  mkdirSync(logsDir, { recursive: true, mode: 0o700 });

  // `mkdirSync` respects the process umask; on many systems this trims 0700
  // to 0700 & ~umask (e.g. 0700 & ~022 = 0700, but 0700 & ~077 = 0700 too).
  // Force the mode explicitly so tests can reliably assert it. Windows has
  // no POSIX modes; skip there.
  if (process.platform !== "win32") {
    try {
      chmodSync(logsDir, 0o700);
    } catch {
      /* best-effort */
    }
  }

  return { home, logsDir };
}

/**
 * Return true if the given argv contains `--verbose` (in any of the accepted
 * forms). Used by the shim to decide whether to call `ensureLogDir()`.
 *
 * @param {string[]} argv
 */
export function argvIsVerbose(argv) {
  for (const a of argv) {
    if (a === "--verbose" || a === "-v" || a.startsWith("--verbose=")) {
      return true;
    }
  }
  return false;
}

/**
 * Return true if the logs directory exists and (on POSIX) is mode 0700.
 * Purely diagnostic; unused by the runtime but handy for tests.
 *
 * @param {string} logsDir
 */
export function logDirLooksHealthy(logsDir) {
  if (!existsSync(logsDir)) return false;
  const st = statSync(logsDir);
  if (!st.isDirectory()) return false;
  if (process.platform === "win32") return true;
  return (st.mode & 0o777) === 0o700;
}
