// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SPEC-101: `cusa login` writes the API key to `~/.cusa/config.toml` with
// mode 0600. Node-side only — the sidecar reads the file at runtime to talk
// to Cursor.  Simple hand-rolled TOML rewriter so the published package
// avoids a toml dependency.

import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import readline from "node:readline";

import { cusaHomeFromEnv } from "./download.js";

/**
 * Persist an API key. Creates `$CUSA_HOME/config.toml` if it does not exist,
 * or edits the existing `[api]` section in place.
 *
 * @param {{
 *   apiKey: string,
 *   cusaHome?: string,
 *   forceWindows?: boolean,
 *   platform?: string,
 * }} opts
 * @returns {{ path: string, mode: number }}
 */
export function writeApiKey(opts) {
  const apiKey = (opts.apiKey ?? "").trim();
  if (apiKey.length === 0) {
    throw new Error("cusa: refusing to write an empty API key");
  }
  const platform = opts.platform ?? process.platform;
  if (platform === "win32" && !opts.forceWindows) {
    throw new Error(
      "cusa: login on Windows is not fully supported (0600 mode is a no-op).\n" +
        "      Re-run with --force-windows if you understand the limitation.",
    );
  }
  const cusaHome = opts.cusaHome ?? cusaHomeFromEnv();
  const configPath = path.join(cusaHome, "config.toml");

  mkdirSync(cusaHome, { recursive: true, mode: 0o755 });

  let existing = "";
  try {
    existing = readFileSync(configPath, "utf8");
  } catch (err) {
    if (err && err.code !== "ENOENT") throw err;
  }
  const next = renderConfig(existing, apiKey);

  writeFileSync(configPath, next, { mode: 0o600 });
  if (platform !== "win32") {
    chmodSync(configPath, 0o600);
  }
  return { path: configPath, mode: 0o600 };
}

/**
 * Given the current file contents (possibly empty), return the new contents
 * with `api_key` set inside the `[api]` section.
 *
 * Round-trip goal: preserve any unrelated sections, comments, and formatting.
 *
 * @param {string} current
 * @param {string} apiKey
 * @returns {string}
 */
export function renderConfig(current, apiKey) {
  const value = tomlString(apiKey);
  if (current.trim().length === 0) {
    return `[api]\napi_key = ${value}\n`;
  }
  const lines = current.split(/\r?\n/);
  const sectionStart = findSectionStart(lines, "api");
  if (sectionStart === -1) {
    const trailingBlank = lines[lines.length - 1] === "";
    const prefix = trailingBlank ? current : `${current}\n`;
    return `${prefix}\n[api]\napi_key = ${value}\n`;
  }
  const sectionEnd = findSectionEnd(lines, sectionStart);
  let replaced = false;
  for (let i = sectionStart + 1; i < sectionEnd; i++) {
    const trimmed = lines[i].trimStart();
    if (/^api_key\s*=/.test(trimmed)) {
      const leading = lines[i].slice(0, lines[i].length - trimmed.length);
      lines[i] = `${leading}api_key = ${value}`;
      replaced = true;
      break;
    }
  }
  if (!replaced) {
    lines.splice(sectionEnd, 0, `api_key = ${value}`);
  }
  let result = lines.join("\n");
  if (!result.endsWith("\n")) result += "\n";
  return result;
}

function findSectionStart(lines, name) {
  const target = `[${name}]`;
  for (let i = 0; i < lines.length; i++) {
    const t = lines[i].trim();
    if (t === target) return i;
  }
  return -1;
}

function findSectionEnd(lines, start) {
  for (let i = start + 1; i < lines.length; i++) {
    const t = lines[i].trim();
    if (t.startsWith("[") && t.endsWith("]")) return i;
  }
  return lines.length;
}

/**
 * Emit a TOML basic-string literal. We escape the four characters that a
 * naive round-trip through TOML would misinterpret. Cursor API keys are
 * ASCII in practice, so this is sufficient.
 *
 * @param {string} s
 */
export function tomlString(s) {
  const escaped = s
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/\n/g, "\\n")
    .replace(/\r/g, "\\r")
    .replace(/\t/g, "\\t");
  return `"${escaped}"`;
}

/**
 * Read a single line from a TTY without echoing keystrokes.
 *
 * @param {string} prompt
 * @param {{
 *   input?: NodeJS.ReadStream,
 *   output?: NodeJS.WriteStream,
 * }} [opts]
 */
export function readSecret(prompt, opts = {}) {
  const input = opts.input ?? process.stdin;
  const output = opts.output ?? process.stdout;
  return new Promise((resolve, reject) => {
    const rl = readline.createInterface({ input, output, terminal: true });
    const stdinIsTTY = Boolean(input.isTTY);
    if (stdinIsTTY) {
      // Suppress echo by hooking rl._writeToOutput; standard trick.
      /** @type {any} */ (rl)._writeToOutput = (str) => {
        if (str.startsWith(prompt)) output.write(str);
        else if (str.includes("\n")) output.write("\n");
        // else swallow keystrokes
      };
    }
    rl.question(prompt, (answer) => {
      rl.close();
      resolve(answer);
    });
    rl.on("error", reject);
  });
}

/**
 * Read all of stdin as UTF-8 text.
 *
 * @param {NodeJS.ReadStream} [stdin]
 */
export function readStdin(stdin = process.stdin) {
  return new Promise((resolve, reject) => {
    let data = "";
    stdin.setEncoding("utf8");
    stdin.on("data", (c) => (data += c));
    stdin.once("end", () => resolve(data));
    stdin.once("error", reject);
  });
}
