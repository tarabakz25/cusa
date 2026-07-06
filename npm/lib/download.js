// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-080: fetch the platform-specific TUI binary from GitHub Releases,
// verify its SHA-256 checksum, and install it at
//   $CUSA_HOME/bin/<target>/<exe>
// with mode 0755. Kept dep-free — Node built-ins only (node:https, node:zlib,
// node:crypto) so the published package stays zero-runtime-deps.
//
// Also provides a minimal tar reader tuned for our release format: a single
// gzipped tarball whose payload is one regular file (the binary), optionally
// nested under a per-target directory. Anything more complex (long names,
// PAX, sparse) is intentionally rejected — release tooling produces the
// simple shape.

import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  existsSync,
  renameSync,
  writeFileSync,
  unlinkSync,
} from "node:fs";
import http from "node:http";
import https from "node:https";
import path from "node:path";
import process from "node:process";
import tls from "node:tls";
import { URL } from "node:url";
import zlib from "node:zlib";

import { detectPlatform, parseTarget } from "./platform.js";

const DEFAULT_RELEASE_BASE_URL =
  "https://github.com/cusa-cli/cusa/releases/download";

const MAX_REDIRECTS = 5;

/**
 * Compute the release URLs for the given version + target.
 *
 * @param {{ version: string, target: string, baseUrl?: string }} params
 */
export function releaseUrls({ version, target, baseUrl }) {
  const base =
    baseUrl ??
    process.env.CUSA_RELEASE_BASE_URL ??
    DEFAULT_RELEASE_BASE_URL;
  const stem = `cusa-tui-${target}`;
  const prefix = `${base.replace(/\/+$/, "")}/v${version}/${stem}`;
  return {
    tarball: `${prefix}.tar.gz`,
    checksum: `${prefix}.tar.gz.sha256`,
  };
}

/**
 * Resolve `$CUSA_HOME`. Defaults to `~/.cusa`.
 * @param {NodeJS.ProcessEnv} [env]
 */
export function cusaHomeFromEnv(env = process.env) {
  if (env.CUSA_HOME && env.CUSA_HOME.length > 0) return env.CUSA_HOME;
  const home = env.HOME || env.USERPROFILE || ".";
  return path.join(home, ".cusa");
}

/**
 * @param {{
 *   version: string,
 *   target?: string,                        // slug like darwin-arm64
 *   cusaHome?: string,
 *   baseUrl?: string,
 *   force?: boolean,
 *   logger?: (msg: string) => void,
 * }} params
 * @returns {Promise<{ target: string, path: string, cached: boolean }>}
 */
export async function downloadBinary(params) {
  const logger = params.logger ?? (() => {});
  const cusaHome = params.cusaHome ?? cusaHomeFromEnv();
  const targetInfo = params.target
    ? parseTarget(params.target)
    : detectPlatform();
  const targetDir = path.join(cusaHome, "bin", targetInfo.target);
  const binPath = path.join(targetDir, targetInfo.exe);

  if (!params.force && existsSync(binPath)) {
    logger(`cusa: cached binary found at ${binPath}`);
    return { target: targetInfo.target, path: binPath, cached: true };
  }

  const urls = releaseUrls({
    version: params.version,
    target: targetInfo.target,
    baseUrl: params.baseUrl,
  });

  logger(`cusa: fetching ${urls.checksum}`);
  const checksumBuf = await httpGetBuffer(urls.checksum);
  const expectedHex = parseChecksumFile(checksumBuf.toString("utf8"));

  logger(`cusa: fetching ${urls.tarball}`);
  const tarballBuf = await httpGetBuffer(urls.tarball);

  const actualHex = createHash("sha256").update(tarballBuf).digest("hex");
  if (actualHex.toLowerCase() !== expectedHex.toLowerCase()) {
    throw new Error(
      `cusa: SHA-256 mismatch for ${urls.tarball}\n` +
        `  expected ${expectedHex}\n  actual   ${actualHex}`,
    );
  }

  const exePayload = extractSingleBinary(tarballBuf, targetInfo.exe);

  mkdirSync(cusaHome, { recursive: true, mode: 0o755 });
  mkdirSync(targetDir, { recursive: true, mode: 0o755 });

  const tmp = `${binPath}.tmp-${process.pid}`;
  try {
    writeFileSync(tmp, exePayload);
    chmodSync(tmp, 0o755);
    renameSync(tmp, binPath);
  } catch (err) {
    try {
      if (existsSync(tmp)) unlinkSync(tmp);
    } catch {
      /* ignore */
    }
    throw err;
  }

  return { target: targetInfo.target, path: binPath, cached: false };
}

/**
 * Accept either `<hex>` on a line by itself or `<hex>  <filename>` as
 * produced by `sha256sum` / `shasum -a 256`.
 *
 * @param {string} text
 * @returns {string} lowercase 64-char hex.
 */
export function parseChecksumFile(text) {
  const line = text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .find((l) => l.length > 0);
  if (!line) throw new Error("cusa: checksum file is empty");
  const first = line.split(/\s+/)[0];
  if (!/^[0-9a-fA-F]{64}$/.test(first)) {
    throw new Error(
      `cusa: could not parse sha256 checksum (first token: '${first}')`,
    );
  }
  return first.toLowerCase();
}

/**
 * Fetch a URL and return the response body as a Buffer. Follows redirects
 * (up to MAX_REDIRECTS) and honors HTTP(S)_PROXY / NO_PROXY env vars.
 *
 * @param {string} url
 * @param {{ headers?: Record<string,string>, maxRedirects?: number }} [opts]
 * @returns {Promise<Buffer>}
 */
export function httpGetBuffer(url, opts = {}) {
  const maxRedirects = opts.maxRedirects ?? MAX_REDIRECTS;
  return new Promise((resolve, reject) => {
    const visit = (currentUrl, remaining) => {
      let parsed;
      try {
        parsed = new URL(currentUrl);
      } catch (err) {
        return reject(new Error(`cusa: invalid URL '${currentUrl}': ${err.message}`));
      }
      const isHttps = parsed.protocol === "https:";
      if (!isHttps && parsed.protocol !== "http:") {
        return reject(
          new Error(`cusa: unsupported URL protocol '${parsed.protocol}'`),
        );
      }
      const proxy = getProxyForUrl(parsed);
      const requestOptions = buildRequestOptions(parsed, opts.headers);

      const onResponse = (res) => {
        const status = res.statusCode ?? 0;
        if (status >= 300 && status < 400 && res.headers.location) {
          if (remaining <= 0) {
            res.resume();
            return reject(new Error("cusa: too many redirects"));
          }
          const next = new URL(res.headers.location, currentUrl).href;
          res.resume();
          return visit(next, remaining - 1);
        }
        if (status !== 200) {
          res.resume();
          return reject(
            new Error(`cusa: HTTP ${status} for ${currentUrl}`),
          );
        }
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.once("end", () => resolve(Buffer.concat(chunks)));
        res.once("error", reject);
      };

      if (proxy && isHttps) {
        openHttpsThroughProxy(parsed, proxy)
          .then((socket) => {
            const req = https.request(
              { ...requestOptions, createConnection: () => socket },
              onResponse,
            );
            req.on("error", reject);
            req.end();
          })
          .catch(reject);
        return;
      }

      if (proxy && !isHttps) {
        // Plain HTTP through proxy: send absolute URL.
        const p = new URL(proxy);
        const req = http.request(
          {
            host: p.hostname,
            port: p.port || 80,
            method: "GET",
            path: currentUrl,
            headers: {
              ...requestOptions.headers,
              Host: parsed.host,
            },
          },
          onResponse,
        );
        req.on("error", reject);
        req.end();
        return;
      }

      const client = isHttps ? https : http;
      const req = client.request(requestOptions, onResponse);
      req.on("error", reject);
      req.end();
    };
    visit(url, maxRedirects);
  });
}

function buildRequestOptions(parsed, extraHeaders) {
  return {
    hostname: parsed.hostname,
    port: parsed.port || (parsed.protocol === "https:" ? 443 : 80),
    method: "GET",
    path: `${parsed.pathname || "/"}${parsed.search || ""}`,
    headers: {
      "User-Agent": "cusa-postinstall (+https://github.com/cusa-cli/cusa)",
      Accept: "*/*",
      ...(extraHeaders || {}),
    },
  };
}

function getProxyForUrl(parsedUrl) {
  const isHttps = parsedUrl.protocol === "https:";
  const proxy = isHttps
    ? process.env.HTTPS_PROXY || process.env.https_proxy
    : process.env.HTTP_PROXY || process.env.http_proxy;
  if (!proxy) return null;
  const noProxy = process.env.NO_PROXY || process.env.no_proxy;
  if (shouldBypassProxy(parsedUrl.hostname, noProxy)) return null;
  return proxy;
}

/**
 * Match the semantics documented by curl and most proxy libraries.
 * @param {string} hostname
 * @param {string|undefined} noProxy
 */
export function shouldBypassProxy(hostname, noProxy) {
  if (!noProxy) return false;
  const h = hostname.toLowerCase();
  const parts = noProxy
    .split(",")
    .map((s) => s.trim().toLowerCase())
    .filter((s) => s.length > 0);
  for (const p of parts) {
    if (p === "*") return true;
    if (p === h) return true;
    const bare = p.startsWith(".") ? p.slice(1) : p;
    if (h === bare) return true;
    if (h.endsWith(`.${bare}`)) return true;
  }
  return false;
}

/**
 * Open an HTTPS connection through an HTTP CONNECT proxy.
 */
function openHttpsThroughProxy(targetUrl, proxyUrl) {
  return new Promise((resolve, reject) => {
    let proxy;
    try {
      proxy = new URL(proxyUrl);
    } catch (err) {
      return reject(new Error(`cusa: invalid proxy URL '${proxyUrl}': ${err.message}`));
    }
    const port = Number(targetUrl.port) || 443;
    const host = targetUrl.hostname;
    const req = http.request({
      host: proxy.hostname,
      port: Number(proxy.port) || 80,
      method: "CONNECT",
      path: `${host}:${port}`,
      headers: {
        Host: `${host}:${port}`,
        "Proxy-Connection": "keep-alive",
        ...(proxy.username || proxy.password
          ? {
              "Proxy-Authorization": `Basic ${Buffer.from(
                `${decodeURIComponent(proxy.username)}:${decodeURIComponent(proxy.password)}`,
              ).toString("base64")}`,
            }
          : {}),
      },
    });
    req.once("connect", (res, socket) => {
      if (res.statusCode !== 200) {
        socket.destroy();
        return reject(
          new Error(`cusa: proxy CONNECT failed (${res.statusCode})`),
        );
      }
      const tlsSocket = tls.connect({
        socket,
        servername: host,
        host,
        port,
      });
      tlsSocket.once("secureConnect", () => resolve(tlsSocket));
      tlsSocket.once("error", reject);
    });
    req.once("error", reject);
    req.end();
  });
}

// ---------------------------------------------------------------------------
// Minimal tar.gz reader.
// ---------------------------------------------------------------------------

/**
 * Given the raw tarball bytes, gunzip and return the first regular file whose
 * basename matches `exeName`. If no exact match is found and there is exactly
 * one regular file, return that file (release layouts may nest the binary).
 *
 * @param {Buffer} tarGzBuf
 * @param {string} exeName
 * @returns {Buffer}
 */
export function extractSingleBinary(tarGzBuf, exeName) {
  const tar = zlib.gunzipSync(tarGzBuf);
  const entries = readTar(tar);
  const files = entries.filter((e) => e.type === "file");
  if (files.length === 0) {
    throw new Error("cusa: tar.gz contains no regular files");
  }
  const exact = files.find(
    (e) => e.name === exeName || path.basename(e.name) === exeName,
  );
  if (exact) return exact.data;
  if (files.length === 1) return files[0].data;
  throw new Error(
    `cusa: could not locate '${exeName}' in tar.gz (found: ${files
      .map((f) => f.name)
      .join(", ")})`,
  );
}

/**
 * Parse a POSIX ustar-format tar buffer. Rejects PAX/GNU long-name entries;
 * our release tooling produces plain ustar.
 *
 * @param {Buffer} buf
 */
function readTar(buf) {
  const entries = [];
  let offset = 0;
  while (offset + 512 <= buf.length) {
    const header = buf.subarray(offset, offset + 512);
    if (header.every((b) => b === 0)) break;

    const name = readCString(header, 0, 100);
    const prefix = readCString(header, 345, 155);
    const fullName = prefix ? `${prefix}/${name}` : name;
    const sizeStr = readCString(header, 124, 12).replace(/\0+$/, "").trim();
    const size = parseInt(sizeStr, 8);
    if (!Number.isFinite(size) || size < 0) {
      throw new Error(`cusa: tar header has invalid size '${sizeStr}'`);
    }
    const typeflagByte = header[156];
    let type;
    if (typeflagByte === 0 || typeflagByte === 0x30 /* '0' */) type = "file";
    else if (typeflagByte === 0x35 /* '5' */) type = "dir";
    else if (typeflagByte === 0x4c || typeflagByte === 0x4b /* GNU long */) {
      throw new Error(
        `cusa: tar contains unsupported GNU long-name entry (typeflag ${String.fromCharCode(typeflagByte)})`,
      );
    } else type = "other";

    offset += 512;
    const dataStart = offset;
    const dataEnd = dataStart + size;
    const padded = dataStart + Math.ceil(size / 512) * 512;

    if (type === "file") {
      if (dataEnd > buf.length) {
        throw new Error("cusa: tar file entry exceeds buffer");
      }
      entries.push({
        type,
        name: fullName,
        data: buf.subarray(dataStart, dataEnd),
      });
    }

    offset = padded;
  }
  return entries;
}

function readCString(buf, start, length) {
  const slice = buf.subarray(start, start + length);
  const nul = slice.indexOf(0);
  const end = nul === -1 ? slice.length : nul;
  return slice.subarray(0, end).toString("utf8");
}
