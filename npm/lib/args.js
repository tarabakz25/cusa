// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// R-7 mitigation: on Windows, Cursor SDK's local sandbox that backs
// `full-auto` approval mode is not supported. The shim intercepts the flag
// before spawning the TUI and rewrites it to `auto-edit`, printing a
// warning. The TUI enforces the same policy defensively — this just fails
// fast at the process boundary.

const APPROVAL_FLAG = "--approval";
const FULL_AUTO = "full-auto";
const FULL_AUTO_SHORTCUT = "--full-auto";
const REPLACEMENT = "auto-edit";

/**
 * @param {{ argv: string[], platform: string }} opts
 * @returns {{ argv: string[], warning: string | null, rewritten: boolean }}
 */
export function rewriteFullAutoForWindows({ argv, platform }) {
  if (platform !== "win32") {
    return { argv, warning: null, rewritten: false };
  }
  const out = [];
  let rewritten = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === FULL_AUTO_SHORTCUT) {
      out.push(`${APPROVAL_FLAG}=${REPLACEMENT}`);
      rewritten = true;
      continue;
    }
    if (a === `${APPROVAL_FLAG}=${FULL_AUTO}`) {
      out.push(`${APPROVAL_FLAG}=${REPLACEMENT}`);
      rewritten = true;
      continue;
    }
    if (a === APPROVAL_FLAG && argv[i + 1] === FULL_AUTO) {
      out.push(APPROVAL_FLAG, REPLACEMENT);
      rewritten = true;
      i += 1;
      continue;
    }
    out.push(a);
  }
  return {
    argv: out,
    rewritten,
    warning: rewritten
      ? "cusa: `full-auto` approval mode is not supported on Windows " +
        "(Cursor SDK local sandbox is POSIX-only). Falling back to `auto-edit`."
      : null,
  };
}

/**
 * Extract simple `--flag=value` pairs from the tail of an argv slice.
 * Anything unrecognized is returned in `rest`. Used by `cusa login` and
 * `cusa download-binary` — a full parser would be overkill.
 *
 * @param {string[]} args
 * @param {{ boolean?: string[], string?: string[] }} spec
 */
export function parseSubcommandArgs(args, spec = {}) {
  const booleans = new Set(spec.boolean ?? []);
  const strings = new Set(spec.string ?? []);
  /** @type {Record<string, string | boolean>} */
  const flags = {};
  const rest = [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (!a.startsWith("--")) {
      rest.push(a);
      continue;
    }
    const eq = a.indexOf("=");
    const key = eq === -1 ? a.slice(2) : a.slice(2, eq);
    const inline = eq === -1 ? undefined : a.slice(eq + 1);
    if (booleans.has(key)) {
      flags[key] = inline === undefined ? true : inline !== "false";
      continue;
    }
    if (strings.has(key)) {
      if (inline !== undefined) {
        flags[key] = inline;
      } else {
        const nxt = args[i + 1];
        if (nxt === undefined || nxt.startsWith("--")) {
          throw new Error(`cusa: --${key} requires a value`);
        }
        flags[key] = nxt;
        i += 1;
      }
      continue;
    }
    throw new Error(`cusa: unknown flag --${key}`);
  }
  return { flags, rest };
}
