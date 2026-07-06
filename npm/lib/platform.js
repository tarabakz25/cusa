// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Platform detection for the cusa native TUI binary (SPEC-081).
// Returns a { target, exe, ... } tuple suitable for locating the prebuilt
// binary under `binaries/<target>/<exe>` or the cached location under
// `$CUSA_HOME/bin/<target>/<exe>`.

import process from "node:process";

const SUPPORTED_PLATFORMS = new Set(["darwin", "linux", "win32"]);
const SUPPORTED_ARCHES = new Set(["x64", "arm64"]);

/**
 * @typedef {Object} PlatformTarget
 * @property {"darwin"|"linux"|"win32"} platform
 * @property {"arm64"|"x64"} arch
 * @property {string} target       Slug used in filenames (e.g. `darwin-arm64`).
 * @property {string} exe          Executable filename (with `.exe` on Windows).
 * @property {string} rustTriple   Rust target triple; useful for CI matrices.
 */

/**
 * Detect the current platform, or a caller-provided one (useful for tests).
 *
 * @param {{ platform?: string, arch?: string }} [opts]
 * @returns {PlatformTarget}
 */
export function detectPlatform(opts = {}) {
  const platform = opts.platform ?? process.platform;
  const arch = opts.arch ?? process.arch;

  if (!SUPPORTED_PLATFORMS.has(platform)) {
    throw new Error(
      `cusa: unsupported platform '${platform}'. Supported: darwin, linux, win32.`,
    );
  }
  if (!SUPPORTED_ARCHES.has(arch)) {
    throw new Error(
      `cusa: unsupported architecture '${arch}'. Supported: x64, arm64.`,
    );
  }

  const target = `${platform}-${arch}`;
  const exe = platform === "win32" ? "cusa-tui.exe" : "cusa-tui";
  const rustTriple = rustTripleFor(platform, arch);

  return { platform, arch, target, exe, rustTriple };
}

/**
 * Parse a canonical `platform-arch` slug back into a PlatformTarget.
 * Used by `cusa download-binary --target=<slug>`.
 *
 * @param {string} slug
 * @returns {PlatformTarget}
 */
export function parseTarget(slug) {
  const dash = slug.indexOf("-");
  if (dash === -1) {
    throw new Error(
      `cusa: invalid --target '${slug}'. Expected e.g. 'darwin-arm64'.`,
    );
  }
  const platform = slug.slice(0, dash);
  const arch = slug.slice(dash + 1);
  return detectPlatform({ platform, arch });
}

function rustTripleFor(platform, arch) {
  if (platform === "darwin") {
    return arch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin";
  }
  if (platform === "linux") {
    return arch === "arm64"
      ? "aarch64-unknown-linux-gnu"
      : "x86_64-unknown-linux-gnu";
  }
  // win32
  return arch === "arm64"
    ? "aarch64-pc-windows-msvc"
    : "x86_64-pc-windows-msvc";
}
