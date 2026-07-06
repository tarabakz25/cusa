#!/usr/bin/env node
// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// npm-installed `cusa` shim (SPEC-080..082, SPEC-101, SPEC-102, R-7).
//
// Responsibilities:
//   - Enforce Node.js >= 20 (SPEC-082).
//   - Handle Node-side subcommands that don't need the TUI:
//       * `cusa login`             (SPEC-101)
//       * `cusa download-binary`   (SPEC-080, R-6 mitigation)
//       * `cusa --version` / `-V`  (never crashes even if the binary is
//         absent; the TUI has its own version, but the npm shim answering
//         with the package version is sufficient for smoke tests.)
//   - Locate the prebuilt TUI binary for the current platform and spawn it,
//     forwarding argv and env. `CUSA_SIDECAR` is set to the bundled sidecar
//     entry so the TUI does not need to guess.
//   - On `--verbose`, ensure `$CUSA_HOME/logs/` exists with mode 0700
//     (SPEC-102).
//   - On Windows, rewrite `--approval=full-auto` to `--approval=auto-edit`
//     (R-7).

import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { parseSubcommandArgs, rewriteFullAutoForWindows } from "../lib/args.js";
import { cusaHomeFromEnv, downloadBinary } from "../lib/download.js";
import { argvIsVerbose, ensureLogDir } from "../lib/logs.js";
import {
  readSecret,
  readStdin,
  writeApiKey,
} from "../lib/login.js";
import { enforceNode } from "../lib/node.js";
import { detectPlatform } from "../lib/platform.js";

async function main() {
  enforceNode();

  const here = path.dirname(fileURLToPath(import.meta.url));
  const pkgRoot = path.resolve(here, "..");
  const pkgVersion = readVersion(pkgRoot);

  const rawArgs = process.argv.slice(2);
  const sub = rawArgs[0];

  if (sub === "--version" || sub === "-V") {
    process.stdout.write(`cusa ${pkgVersion}\n`);
    return;
  }
  if (sub === "login") {
    return handleLogin(rawArgs.slice(1));
  }
  if (sub === "download-binary") {
    return handleDownloadBinary(rawArgs.slice(1), pkgVersion);
  }

  return runTui({ pkgRoot, pkgVersion, argv: rawArgs });
}

function runTui({ pkgRoot, pkgVersion, argv }) {
  const { argv: adjustedArgv, warning } = rewriteFullAutoForWindows({
    argv,
    platform: process.platform,
  });
  if (warning) process.stderr.write(`${warning}\n`);

  if (argvIsVerbose(adjustedArgv)) {
    try {
      ensureLogDir();
    } catch (err) {
      process.stderr.write(
        `cusa: could not prepare log dir (${err.message}); continuing without --verbose logs.\n`,
      );
    }
  }

  const tui = resolveBinary(pkgRoot);
  if (!tui) {
    process.stderr.write(
      "cusa: native TUI binary not found for this platform.\n" +
        "      Try: cusa download-binary\n" +
        "      Or:  set CUSA_TUI=/path/to/cusa-tui to point at a local build.\n",
    );
    process.exit(1);
  }
  const sidecar = resolveSidecar(pkgRoot);

  const env = {
    ...process.env,
    CUSA_SIDECAR: sidecar,
    CUSA_NPM_VERSION: pkgVersion,
  };

  const child = spawn(tui, adjustedArgv, { stdio: "inherit", env });
  child.on("exit", (code, signal) => {
    if (signal) process.kill(process.pid, signal);
    else process.exit(code ?? 0);
  });
  child.on("error", (err) => {
    process.stderr.write(`cusa: failed to spawn TUI: ${err.message}\n`);
    process.exit(1);
  });
}

function resolveBinary(pkgRoot) {
  if (process.env.CUSA_TUI) {
    return process.env.CUSA_TUI;
  }
  let detected;
  try {
    detected = detectPlatform();
  } catch (err) {
    process.stderr.write(`${err.message}\n`);
    return null;
  }
  const local = path.join(pkgRoot, "binaries", detected.target, detected.exe);
  if (existsSync(local)) return local;

  const cached = path.join(
    cusaHomeFromEnv(),
    "bin",
    detected.target,
    detected.exe,
  );
  if (existsSync(cached)) return cached;

  return null;
}

function resolveSidecar(pkgRoot) {
  if (process.env.CUSA_SIDECAR) return process.env.CUSA_SIDECAR;
  return path.join(pkgRoot, "sidecar", "dist", "index.js");
}

// ---------------------------------------------------------------------------
// `cusa login`
// ---------------------------------------------------------------------------

async function handleLogin(argv) {
  let parsed;
  try {
    parsed = parseSubcommandArgs(argv, {
      boolean: ["stdin", "force-windows"],
      string: ["key"],
    });
  } catch (err) {
    process.stderr.write(`${err.message}\n`);
    process.exit(2);
    return;
  }
  const { flags } = parsed;

  let key;
  if (typeof flags.key === "string") {
    key = flags.key;
  } else if (flags.stdin) {
    key = (await readStdin()).trim();
    if (key.length === 0) {
      process.stderr.write("cusa login: --stdin was empty\n");
      process.exit(2);
      return;
    }
  } else {
    if (!process.stdin.isTTY) {
      process.stderr.write(
        "cusa login: stdin is not a TTY. Pass --stdin or --key <value>.\n",
      );
      process.exit(2);
      return;
    }
    key = (await readSecret("Cursor API key: ")).trim();
    if (key.length === 0) {
      process.stderr.write("cusa login: empty key; aborting.\n");
      process.exit(2);
      return;
    }
  }

  try {
    const result = writeApiKey({
      apiKey: key,
      forceWindows: Boolean(flags["force-windows"]),
    });
    process.stdout.write(
      `cusa: wrote API key to ${result.path} (mode 0${(result.mode & 0o777).toString(8)})\n`,
    );
  } catch (err) {
    process.stderr.write(`${err.message}\n`);
    process.exit(1);
  }
}

// ---------------------------------------------------------------------------
// `cusa download-binary`
// ---------------------------------------------------------------------------

async function handleDownloadBinary(argv, pkgVersion) {
  let parsed;
  try {
    parsed = parseSubcommandArgs(argv, {
      boolean: ["force"],
      string: ["target", "version"],
    });
  } catch (err) {
    process.stderr.write(`${err.message}\n`);
    process.exit(2);
    return;
  }
  const { flags } = parsed;
  try {
    const result = await downloadBinary({
      version: typeof flags.version === "string" ? flags.version : pkgVersion,
      target: typeof flags.target === "string" ? flags.target : undefined,
      force: Boolean(flags.force),
      logger: (msg) => process.stdout.write(`${msg}\n`),
    });
    if (result.cached) {
      process.stdout.write(`cusa: already installed at ${result.path}\n`);
    } else {
      process.stdout.write(
        `cusa: installed ${result.target} binary at ${result.path}\n`,
      );
    }
  } catch (err) {
    process.stderr.write(`cusa download-binary: ${err.message}\n`);
    process.exit(1);
  }
}

function readVersion(pkgRoot) {
  try {
    const pkgPath = path.join(pkgRoot, "package.json");
    if (existsSync(pkgPath)) {
      return JSON.parse(readFileSync(pkgPath, "utf8")).version;
    }
  } catch {
    /* ignore */
  }
  return "unknown";
}

main().catch((err) => {
  process.stderr.write(`cusa: ${err.stack || err.message || err}\n`);
  process.exit(1);
});
