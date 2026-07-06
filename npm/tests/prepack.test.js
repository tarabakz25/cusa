// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-083: THIRD_PARTY_NOTICES.md is present in the publish tarball, and
// the prepack script copies it into `npm/` before `npm pack` runs. We can't
// easily invoke `npm pack` inside a unit test without polluting the repo, so
// we exercise the copy step directly against a temp `npm/` mirror.

import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const prepackScript = path.join(repoRoot, "scripts", "prepack.js");

function scaffold(kind) {
  const root = mkdtempSync(path.join(os.tmpdir(), `cusa-prepack-${kind}-`));
  mkdirSync(path.join(root, "sidecar", "dist"), { recursive: true });
  writeFileSync(
    path.join(root, "sidecar", "dist", "index.js"),
    "// fake sidecar\n",
  );
  writeFileSync(
    path.join(root, "sidecar", "package.json"),
    JSON.stringify({ name: "cusa-sidecar", version: "0.0.0", type: "module" }),
  );
  writeFileSync(path.join(root, "LICENSE"), "Apache-2.0 fake\n");
  writeFileSync(
    path.join(root, "THIRD_PARTY_NOTICES.md"),
    "# Third party notices\n",
  );
  // Mirror scripts/prepack.js into the fake root so it resolves relative
  // paths correctly.
  mkdirSync(path.join(root, "scripts"));
  cpSync(prepackScript, path.join(root, "scripts", "prepack.js"));
  mkdirSync(path.join(root, "npm"));
  writeFileSync(
    path.join(root, "npm", "package.json"),
    JSON.stringify({ name: "cusa", version: "0.0.0" }),
  );
  writeFileSync(path.join(root, "npm", "README.md"), "# cusa\n");
  return root;
}

function runPrepack(root, env = {}) {
  return execFileSync(process.execPath, [path.join(root, "scripts", "prepack.js")], {
    env: { ...process.env, ...env },
    encoding: "utf8",
  });
}

test("SPEC-083: prepack copies THIRD_PARTY_NOTICES.md and LICENSE into npm/", () => {
  const root = scaffold("ok");
  // Provide at least one file under npm/binaries/ so prepack does not fail.
  mkdirSync(path.join(root, "npm", "binaries", "darwin-arm64"), { recursive: true });
  writeFileSync(
    path.join(root, "npm", "binaries", "darwin-arm64", "cusa-tui"),
    "fake",
  );
  const out = runPrepack(root);
  assert.match(out, /prepack: OK/);
  assert.equal(
    readFileSync(path.join(root, "npm", "THIRD_PARTY_NOTICES.md"), "utf8"),
    "# Third party notices\n",
  );
  assert.equal(
    readFileSync(path.join(root, "npm", "LICENSE"), "utf8"),
    "Apache-2.0 fake\n",
  );
  assert.ok(existsSync(path.join(root, "npm", "sidecar", "dist", "index.js")));
  assert.ok(existsSync(path.join(root, "npm", "sidecar", "package.json")));
});

test("SPEC-083: prepack fails loudly without sidecar/dist/index.js", () => {
  const root = scaffold("nosidecar");
  execFileSync("rm", [path.join(root, "sidecar", "dist", "index.js")]);
  let threw = false;
  try {
    runPrepack(root, { CUSA_ALLOW_EMPTY_BINARIES: "1" });
  } catch (err) {
    threw = true;
    assert.match(err.stderr ?? "", /required file missing.*index\.js/);
  }
  assert.equal(threw, true, "prepack should have exited non-zero");
});

test("SPEC-083: prepack refuses empty npm/binaries/ by default", () => {
  const root = scaffold("empty");
  try {
    runPrepack(root);
    assert.fail("prepack should have exited non-zero due to empty binaries/");
  } catch (err) {
    assert.match(err.stderr ?? "", /binaries.*empty|CUSA_ALLOW_EMPTY_BINARIES/);
  }
});

test("SPEC-083: prepack allows empty npm/binaries/ with opt-in", () => {
  const root = scaffold("optin");
  const out = runPrepack(root, { CUSA_ALLOW_EMPTY_BINARIES: "1" });
  assert.match(out, /prepack: OK/);
});
