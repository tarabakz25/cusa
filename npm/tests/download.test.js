// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-080: exercise the download pipeline against a local HTTP server that
// serves a real gzipped tarball. Verifies:
//   - the URL layout matches the release convention,
//   - the SHA-256 file is fetched and honored,
//   - a mismatched checksum is rejected,
//   - the binary is unpacked to $CUSA_HOME/bin/<target>/<exe> with mode 0755,
//   - the parent .cusa/ directory is created if absent.

import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import process from "node:process";

import {
  downloadBinary,
  extractSingleBinary,
  parseChecksumFile,
  releaseUrls,
  shouldBypassProxy,
} from "../lib/download.js";

function tmp(prefix) {
  return mkdtempSync(path.join(os.tmpdir(), prefix));
}

/**
 * Build a tar.gz containing a single file at the given basename with the
 * given payload. Uses the system `tar` binary — available on macOS and Linux
 * runners.
 */
function buildTarball({ exeName, payload }) {
  const stage = tmp("cusa-tar-");
  const filePath = path.join(stage, exeName);
  writeFileSync(filePath, payload);
  chmodSync(filePath, 0o755);
  const tarPath = path.join(stage, `${exeName}.tar.gz`);
  execFileSync("tar", ["-czf", tarPath, "-C", stage, exeName]);
  return { tarPath, buf: readFileSync(tarPath) };
}

function startServer(handler) {
  return new Promise((resolve) => {
    const server = http.createServer(handler);
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      const port = typeof addr === "object" && addr ? addr.port : 0;
      resolve({ server, baseUrl: `http://127.0.0.1:${port}` });
    });
  });
}

test("SPEC-080: download.js writes binary to CUSA_HOME with mode 0755", async () => {
  const version = "1.2.3";
  const target = "linux-x64";
  const exe = "cusa-tui";
  const payload = Buffer.from("#!/bin/sh\necho fake cusa-tui\n");
  const { buf: tarBuf } = buildTarball({ exeName: exe, payload });
  const sha = createHash("sha256").update(tarBuf).digest("hex");

  const { server, baseUrl } = await startServer((req, res) => {
    if (req.url === `/v${version}/cusa-tui-${target}.tar.gz`) {
      res.writeHead(200, { "content-type": "application/gzip" });
      res.end(tarBuf);
    } else if (req.url === `/v${version}/cusa-tui-${target}.tar.gz.sha256`) {
      res.writeHead(200, { "content-type": "text/plain" });
      res.end(`${sha}  cusa-tui-${target}.tar.gz\n`);
    } else {
      res.writeHead(404);
      res.end();
    }
  });

  const cusaHome = tmp("cusa-home-");
  try {
    const result = await downloadBinary({
      version,
      target,
      cusaHome,
      baseUrl,
    });
    assert.equal(result.target, target);
    assert.equal(result.cached, false);
    assert.equal(result.path, path.join(cusaHome, "bin", target, exe));

    const written = readFileSync(result.path);
    assert.deepEqual(Uint8Array.from(written), Uint8Array.from(payload));

    if (process.platform !== "win32") {
      const st = statSync(result.path);
      assert.equal(
        (st.mode & 0o777).toString(8),
        "755",
        "binary should be mode 0755 on POSIX",
      );
    }
  } finally {
    server.close();
  }
});

test("SPEC-080: download.js verifies sha256 and rejects mismatch", async () => {
  const version = "9.9.9";
  const target = "linux-arm64";
  const { buf: tarBuf } = buildTarball({
    exeName: "cusa-tui",
    payload: Buffer.from("real"),
  });
  const wrongSha =
    "0000000000000000000000000000000000000000000000000000000000000000";

  const { server, baseUrl } = await startServer((req, res) => {
    if (req.url?.endsWith(".tar.gz")) {
      res.writeHead(200);
      res.end(tarBuf);
    } else if (req.url?.endsWith(".sha256")) {
      res.writeHead(200);
      res.end(`${wrongSha}\n`);
    } else {
      res.writeHead(404);
      res.end();
    }
  });
  const cusaHome = tmp("cusa-home-mismatch-");

  try {
    await assert.rejects(
      downloadBinary({ version, target, cusaHome, baseUrl }),
      /SHA-256 mismatch/,
    );
    // And no partial file is left behind:
    assert.throws(
      () => statSync(path.join(cusaHome, "bin", target, "cusa-tui")),
      /ENOENT/,
    );
  } finally {
    server.close();
  }
});

test("SPEC-080: download.js follows HTTP redirects", async () => {
  const version = "1.0.0";
  const target = "linux-x64";
  const { buf: tarBuf } = buildTarball({
    exeName: "cusa-tui",
    payload: Buffer.from("hello"),
  });
  const sha = createHash("sha256").update(tarBuf).digest("hex");

  let redirected = false;
  const { server, baseUrl } = await startServer((req, res) => {
    if (req.url?.endsWith(".sha256")) {
      res.writeHead(200);
      res.end(`${sha}\n`);
      return;
    }
    if (req.url === `/v${version}/cusa-tui-${target}.tar.gz`) {
      redirected = true;
      res.writeHead(302, { location: "/final.tar.gz" });
      res.end();
      return;
    }
    if (req.url === "/final.tar.gz") {
      res.writeHead(200);
      res.end(tarBuf);
      return;
    }
    res.writeHead(404);
    res.end();
  });

  const cusaHome = tmp("cusa-home-redirect-");
  try {
    const r = await downloadBinary({ version, target, cusaHome, baseUrl });
    assert.equal(redirected, true);
    assert.equal(r.cached, false);
  } finally {
    server.close();
  }
});

test("SPEC-080: download.js returns cached=true when file already exists", async () => {
  const cusaHome = tmp("cusa-home-cached-");
  const target = "linux-x64";
  const binDir = path.join(cusaHome, "bin", target);
  mkdirSync(binDir, { recursive: true });
  const binPath = path.join(binDir, "cusa-tui");
  writeFileSync(binPath, "already here");
  chmodSync(binPath, 0o755);

  // Point at a base URL that will never be contacted; if downloadBinary
  // tries to reach it, this call fails.
  const r = await downloadBinary({
    version: "0.0.1",
    target,
    cusaHome,
    baseUrl: "http://127.0.0.1:1/never",
  });
  assert.equal(r.cached, true);
  assert.equal(r.path, binPath);
});

test("SPEC-080: releaseUrls builds the canonical GitHub Release URL", () => {
  const { tarball, checksum } = releaseUrls({
    version: "0.1.2",
    target: "darwin-arm64",
  });
  assert.equal(
    tarball,
    "https://github.com/cusa-cli/cusa/releases/download/v0.1.2/cusa-tui-darwin-arm64.tar.gz",
  );
  assert.equal(checksum, `${tarball}.sha256`);
});

test("SPEC-080: releaseUrls respects baseUrl override", () => {
  const { tarball } = releaseUrls({
    version: "0.0.1",
    target: "linux-x64",
    baseUrl: "https://example.test/releases",
  });
  assert.equal(
    tarball,
    "https://example.test/releases/v0.0.1/cusa-tui-linux-x64.tar.gz",
  );
});

test("SPEC-080: parseChecksumFile handles sha256sum and bare hex formats", () => {
  const hex = "a".repeat(64);
  assert.equal(parseChecksumFile(`${hex}  file.tar.gz\n`), hex);
  assert.equal(parseChecksumFile(`${hex}\n`), hex);
  assert.throws(
    () => parseChecksumFile("not-a-hash file\n"),
    /could not parse sha256/,
  );
  assert.throws(() => parseChecksumFile(""), /empty/);
});

test("SPEC-080: extractSingleBinary picks the file matching exe name", () => {
  const { buf } = buildTarball({
    exeName: "cusa-tui",
    payload: Buffer.from("payload"),
  });
  const out = extractSingleBinary(buf, "cusa-tui");
  assert.equal(out.toString("utf8"), "payload");
});

test("SPEC-080: shouldBypassProxy honors NO_PROXY entries", () => {
  assert.equal(shouldBypassProxy("github.com", "*"), true);
  assert.equal(shouldBypassProxy("github.com", "github.com"), true);
  assert.equal(shouldBypassProxy("api.github.com", ".github.com"), true);
  assert.equal(shouldBypassProxy("api.github.com", "github.com"), true);
  assert.equal(shouldBypassProxy("example.com", "github.com,gitlab.com"), false);
  assert.equal(shouldBypassProxy("example.com", ""), false);
  assert.equal(shouldBypassProxy("example.com", undefined), false);
});
