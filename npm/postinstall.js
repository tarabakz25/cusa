// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-080 / SPEC-081 / R-6.
//
// The published `cusa` package ships JS + a bundled sidecar. The native TUI
// binary is downloaded from GitHub Releases on first install. Design goals:
//
//   * Never break `npm install`. Any failure — unsupported platform, network
//     down, corporate proxy blocks GitHub — is reported to the user with a
//     recovery hint and the script exits 0.
//   * Skip when told to (`CUSA_SKIP_POSTINSTALL=1`, or CI without opt-in via
//     `CUSA_ALLOW_CI_DOWNLOAD=1`).
//   * Skip when a bundled binary is already present under `binaries/<t>/`
//     (developer bundle) or a previous install already staged the binary at
//     `$CUSA_HOME/bin/<t>/`.
//   * Support corporate networks via HTTP_PROXY / HTTPS_PROXY / NO_PROXY.
//
// Everything is done with Node built-ins so the published package stays
// zero-runtime-deps.

import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import {
  cusaHomeFromEnv,
  downloadBinary,
  releaseUrls,
} from "./lib/download.js";
import { detectPlatform } from "./lib/platform.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = here;

async function main() {
  if (shouldSkip()) return;

  let targetInfo;
  try {
    targetInfo = detectPlatform();
  } catch (err) {
    // Unsupported platform: warn and exit 0. User can still set CUSA_TUI.
    warn(err.message);
    return;
  }

  if (bundledBinaryExists(targetInfo)) {
    log(`cusa: bundled binary present for ${targetInfo.target}; skipping download.`);
    return;
  }
  if (cachedBinaryExists(targetInfo)) {
    log(`cusa: cached binary already installed for ${targetInfo.target}.`);
    return;
  }

  const version = readPkgVersion();
  const urls = releaseUrls({ version, target: targetInfo.target });
  log(`cusa: downloading ${targetInfo.target} binary v${version} from GitHub Releases…`);

  try {
    const result = await downloadBinary({
      version,
      target: targetInfo.target,
      logger: log,
    });
    log(`cusa: installed ${result.target} binary v${version}`);
  } catch (err) {
    warn(
      `cusa: could not fetch native binary (${err.message}).\n` +
        `      Tried: ${urls.tarball}\n` +
        `      Recovery options:\n` +
        `        - Retry manually:  cusa download-binary\n` +
        `        - Or point at a local build:  export CUSA_TUI=/path/to/cusa-tui\n` +
        `      Corporate networks: set HTTP_PROXY / HTTPS_PROXY / NO_PROXY.`,
    );
    // Exit 0 so the package install itself does not fail.
  }
}

function shouldSkip() {
  if (process.env.CUSA_SKIP_POSTINSTALL === "1") {
    log("cusa: postinstall skipped (CUSA_SKIP_POSTINSTALL=1).");
    return true;
  }
  if (process.env.CI === "1" && process.env.CUSA_ALLOW_CI_DOWNLOAD !== "1") {
    log(
      "cusa: postinstall skipped in CI (set CUSA_ALLOW_CI_DOWNLOAD=1 to opt in).",
    );
    return true;
  }
  return false;
}

function bundledBinaryExists(targetInfo) {
  const p = path.join(pkgRoot, "binaries", targetInfo.target, targetInfo.exe);
  return existsSync(p);
}

function cachedBinaryExists(targetInfo) {
  const p = path.join(
    cusaHomeFromEnv(),
    "bin",
    targetInfo.target,
    targetInfo.exe,
  );
  return existsSync(p);
}

function readPkgVersion() {
  try {
    const pkg = JSON.parse(
      readFileSync(path.join(pkgRoot, "package.json"), "utf8"),
    );
    return pkg.version || "0.0.0";
  } catch {
    return "0.0.0";
  }
}

function log(msg) {
  process.stdout.write(`${msg}\n`);
}

function warn(msg) {
  process.stderr.write(`${msg}\n`);
}

main().catch((err) => {
  // Absolutely must not break `npm install`.
  warn(`cusa postinstall: unexpected error (${err.message}); continuing.`);
});
