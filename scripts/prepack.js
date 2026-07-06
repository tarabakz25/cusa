// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// `npm pack` / `npm publish` hook for the `cusa` package. Runs from the
// repo root (invoked by `npm/package.json`'s "prepack" script). Assembles
// everything the publish tarball needs, but does not modify `npm/bin/`,
// `npm/lib/`, or `npm/postinstall.js` — those are the actual code.
//
// Steps:
//   1. Verify the sidecar has been built (`sidecar/dist/index.js` exists).
//   2. Mirror `sidecar/dist/` → `npm/sidecar/dist/`, and copy the sidecar's
//      `package.json` so the sidecar keeps its own pinned deps at runtime.
//   3. Copy `THIRD_PARTY_NOTICES.md` + `LICENSE` from the repo root into
//      `npm/`. (SPEC-083.)
//   4. Sanity-check `npm/binaries/` — refuse to publish an empty binary tree
//      unless `CUSA_ALLOW_EMPTY_BINARIES=1`.
//
// This script is idempotent and safe to run on a dirty tree.

import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  statSync,
} from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const npmRoot = path.join(repoRoot, "npm");
const sidecarSrcDir = path.join(repoRoot, "sidecar");
const sidecarSrcDist = path.join(sidecarSrcDir, "dist");
const sidecarDest = path.join(npmRoot, "sidecar");

function main() {
  requireFile(path.join(sidecarSrcDist, "index.js"));
  requireFile(path.join(sidecarSrcDir, "package.json"));

  mkdirSync(sidecarDest, { recursive: true });
  cpSync(sidecarSrcDist, path.join(sidecarDest, "dist"), {
    recursive: true,
    force: true,
  });
  copyFileSync(
    path.join(sidecarSrcDir, "package.json"),
    path.join(sidecarDest, "package.json"),
  );

  copyIfExists(path.join(repoRoot, "THIRD_PARTY_NOTICES.md"), npmRoot);
  copyIfExists(path.join(repoRoot, "LICENSE"), npmRoot);

  const binariesDir = path.join(npmRoot, "binaries");
  if (!hasAnyFile(binariesDir) && process.env.CUSA_ALLOW_EMPTY_BINARIES !== "1") {
    fail(
      `prepack: ${binariesDir} is empty. Run scripts/build-release.sh first,\n` +
        "         or set CUSA_ALLOW_EMPTY_BINARIES=1 for a JS-only test pack.",
    );
  }

  log("prepack: OK");
}

function requireFile(p) {
  if (!existsSync(p)) {
    fail(`prepack: required file missing: ${p}`);
  }
}

function copyIfExists(src, destDir) {
  if (!existsSync(src)) {
    log(`prepack: skipping (not found) ${src}`);
    return;
  }
  mkdirSync(destDir, { recursive: true });
  copyFileSync(src, path.join(destDir, path.basename(src)));
  log(`prepack: copied ${path.basename(src)} → ${destDir}/`);
}

function hasAnyFile(dir) {
  if (!existsSync(dir)) return false;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isFile()) return true;
    if (entry.isDirectory()) {
      if (hasAnyFile(path.join(dir, entry.name))) return true;
    }
  }
  return false;
}

function log(msg) {
  process.stdout.write(`${msg}\n`);
}

function fail(msg) {
  process.stderr.write(`${msg}\n`);
  process.exit(1);
}

main();
