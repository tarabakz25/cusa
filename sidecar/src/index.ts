// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// cusa-sidecar entrypoint (Slice 1 — Sidecar MVP).
//
// Wires JSON-RPC methods to the SessionManager, which owns @cursor/sdk
// through SdkAdapter. Bootstraps the SDK adapter lazily so a missing API
// key doesn't block `initialize`.

import process from "node:process";

import {
  createRealSdkAdapter,
  type SdkAdapter,
} from "./agent/sdkAdapter.js";
import {
  SessionManager,
  SessionRpcError,
} from "./agent/session.js";
import {
  Method,
  PROTOCOL_VERSION,
  type ContextSetStrategyParams,
  type InitializeResult,
  type LogParams,
  type McpListParams,
  type McpToggleParams,
  type SessionCancelParams,
  type SessionCreateParams,
  type SessionDisposeParams,
  type SessionResumeParams,
  type SessionSendParams,
  type SessionSetApprovalModeParams,
  type SkillsListParams,
  type SkillsSetEnabledParams,
  type ToolApprovalResponseParams,
} from "./rpc/schema.js";
import { RpcServer, RpcMethodError } from "./rpc/server.js";
import { RotatingLogger } from "./logging/rotate.js";
import { loadConversationConfig } from "./config/conversation.js";
import {
  ContextManager,
  detectNativeConversationRetention,
  shouldUseNativeRetention,
} from "./context/index.js";

const SIDECAR_VERSION = "0.0.1";
const LOG_FILE_ENV = "CUSA_LOG_FILE";

export interface BuildServerOptions {
  adapter?: SdkAdapter;
  sidecarVersion?: string;
  sdkVersionResolver?: () => Promise<string>;
  /**
   * Optional context manager. When omitted a default one is created with
   * manual injection ON; the bootstrap step in `main()` refines it based
   * on config + SDK detection.
   */
  context?: ContextManager;
  /** Rotating log path override (defaults to `process.env.CUSA_LOG_FILE`). */
  logFilePath?: string;
}

/**
 * Compose an `RpcServer` with every method handler wired in. Exposed for
 * tests so they can drive the server without spawning a real subprocess.
 */
export function buildServer(opts: BuildServerOptions = {}): {
  server: RpcServer;
  sessions: SessionManager;
  fileLogger: RotatingLogger | null;
  contextManager: ContextManager;
} {
  const server = new RpcServer();
  // SPEC-102: sidecar mirrors every `log` notification to the file the
  // TUI opened via `CUSA_LOG_FILE`. The env var is not part of the RPC
  // contract; it is set at spawn time.
  const logFilePath =
    opts.logFilePath ?? process.env[LOG_FILE_ENV] ?? undefined;
  const fileLogger =
    logFilePath && logFilePath.length > 0
      ? new RotatingLogger({ filePath: logFilePath })
      : null;

  const notifyWithMirror = (method: string, params: unknown): void => {
    server.notify(method, params);
    if (fileLogger && method === Method.Log) {
      const p = params as LogParams;
      // Fire-and-forget: rotate handles chaining internally.
      void fileLogger.write({
        level: p.level,
        message: p.message,
        target: p.target,
      });
    }
  };

  const contextManager = opts.context ?? new ContextManager({
    log: (level, message) =>
      notifyWithMirror(Method.Log, {
        level,
        message,
        target: "sidecar/context",
      }),
  });

  const sessions = new SessionManager({
    adapter:
      opts.adapter ??
      lazyAdapter(async () => createRealSdkAdapter()),
    notify: notifyWithMirror,
    log: (level, message) => {
      notifyWithMirror(Method.Log, {
        level,
        message,
        target: "sidecar",
      });
    },
    context: contextManager,
  });

  const version = opts.sidecarVersion ?? SIDECAR_VERSION;
  const resolveSdkVersion = opts.sdkVersionResolver ?? defaultSdkVersion;

  server.on(Method.Initialize, async () => {
    const result: InitializeResult = {
      protocolVersion: PROTOCOL_VERSION,
      sidecarVersion: version,
      sdkVersion: await resolveSdkVersion(),
      nodeVersion: process.versions.node,
      capabilities: {
        streaming: true,
        cancel: true,
        resume: true,
        sandbox: true,
        mcp: true,
        skills: true,
        // Default router config enables the LLM classifier path; the
        // sidecar upgrades or degrades this at runtime via the router
        // config file. We advertise true here so the TUI can render a
        // "router: llm" hint on first launch.
        routerLlm: true,
        // SPEC-093: reflect the resolved retention decision. Filled in
        // by `bootstrapContext()` in `main()`; for tests / raw
        // `buildServer()` calls it defaults to whatever the current
        // ContextManager already thinks.
        nativeConversationRetention: contextManager.isNative(),
      },
    };
    return result;
  });

  server.on(Method.Shutdown, () => ({ ok: true }));

  server.on(Method.ModelsList, () => call(sessions.listModels()));

  server.on(Method.SessionCreate, (params) =>
    call(sessions.createSession(params as SessionCreateParams)),
  );
  server.on(Method.SessionSend, (params) =>
    call(sessions.sendMessage(params as SessionSendParams)),
  );
  server.on(Method.SessionCancel, (params) =>
    call(sessions.cancelRun(params as SessionCancelParams)),
  );
  server.on(Method.SessionResume, (params) =>
    call(sessions.resumeSession(params as SessionResumeParams)),
  );
  server.on(Method.SessionDispose, (params) =>
    call(sessions.disposeSession(params as SessionDisposeParams)),
  );
  server.on(Method.SessionSetApprovalMode, (params) =>
    call(
      Promise.resolve(
        sessions.setApprovalMode(params as SessionSetApprovalModeParams),
      ),
    ),
  );

  server.on(Method.SkillsList, (params) =>
    call(sessions.listSkills(params as SkillsListParams)),
  );
  server.on(Method.SkillsSetEnabled, (params) =>
    call(
      Promise.resolve(
        sessions.setSkillsEnabled(params as SkillsSetEnabledParams),
      ),
    ),
  );

  server.on(Method.McpList, (params) =>
    call(sessions.listMcp(params as McpListParams)),
  );
  server.on(Method.McpToggle, (params) =>
    call(
      Promise.resolve(sessions.toggleMcp(params as McpToggleParams)),
    ),
  );

  server.on(Method.ContextSetStrategy, (params) =>
    call(
      Promise.resolve(
        sessions.setContextStrategy(params as ContextSetStrategyParams),
      ),
    ),
  );

  server.on(Method.ToolApprovalResponse, (params) =>
    call(
      Promise.resolve(
        sessions.handleApprovalResponse(params as ToolApprovalResponseParams),
      ),
    ),
  );

  return { server, sessions, fileLogger, contextManager };
}

/**
 * SPEC-093: resolve `conversation.mode`, run SDK detection, and update
 * the given ContextManager accordingly. Returns the resolved decision
 * so `main()` can log it once at startup.
 */
export async function bootstrapContext(opts: {
  contextManager: ContextManager;
  log: (level: "info" | "warn" | "error", msg: string) => void;
  configPath?: string;
}): Promise<{ useNative: boolean; reason: string }> {
  const loadArgs: Parameters<typeof loadConversationConfig>[0] = {
    log: opts.log,
  };
  if (opts.configPath !== undefined) loadArgs.configPath = opts.configPath;
  const loaded = await loadConversationConfig(loadArgs);
  const detection = await detectNativeConversationRetention();
  const decision = shouldUseNativeRetention(loaded.config.mode, detection);
  opts.contextManager.setUseNative(decision.useNative);
  return decision;
}

/**
 * Wrap a SessionManager call so `SessionRpcError` is translated to the
 * `RpcMethodError` shape the server expects.
 */
async function call<T>(p: Promise<T>): Promise<T> {
  try {
    return await p;
  } catch (err) {
    if (err instanceof SessionRpcError) {
      throw new RpcMethodError(err.code, err.message, err.data);
    }
    throw err;
  }
}

/**
 * Adapter proxy that defers SDK loading until the first method that needs
 * it. Keeps `initialize` snappy and lets tests inject their own adapter.
 */
function lazyAdapter(factory: () => Promise<SdkAdapter>): SdkAdapter {
  let real: Promise<SdkAdapter> | null = null;
  const get = () => (real ??= factory());
  return {
    async listModels() {
      return (await get()).listModels();
    },
    async createAgent(opts) {
      return (await get()).createAgent(opts);
    },
    async resumeAgent(id, opts) {
      return (await get()).resumeAgent(id, opts);
    },
  };
}

async function defaultSdkVersion(): Promise<string> {
  try {
    const { createRequire } = await import("node:module");
    const require = createRequire(import.meta.url);
    const entry = require.resolve("@cursor/sdk");
    const { readFileSync } = await import("node:fs");
    const path = await import("node:path");
    let dir = path.dirname(entry);
    for (let i = 0; i < 5; i++) {
      const candidate = path.join(dir, "package.json");
      try {
        const pkg = JSON.parse(readFileSync(candidate, "utf8")) as {
          name?: string;
          version?: string;
        };
        if (pkg.name === "@cursor/sdk" && typeof pkg.version === "string") {
          return pkg.version;
        }
      } catch {
        /* walk up */
      }
      const parent = path.dirname(dir);
      if (parent === dir) break;
      dir = parent;
    }
    return "unknown";
  } catch {
    return "unavailable";
  }
}

export async function main(): Promise<void> {
  const { server, contextManager } = buildServer();
  // Resolve conversation.mode + SDK detection asynchronously; the
  // initialize handler will read the ContextManager state at request
  // time so it always reports the current retention decision.
  const decision = await bootstrapContext({
    contextManager,
    log: (level, message) =>
      server.notify(Method.Log, {
        level,
        message,
        target: "sidecar/context",
      }),
  });
  server.notify(Method.Log, {
    level: "info",
    message: `conversation retention: ${
      decision.useNative ? "native (skip injection)" : "manual injection on"
    } — ${decision.reason}`,
    target: "sidecar/context",
  });
  process.stderr.write(`cusa-sidecar ${SIDECAR_VERSION} ready\n`);
  await server.run();
}

async function runAsEntrypoint(): Promise<boolean> {
  if (!process.argv[1]) return false;
  const { pathToFileURL, fileURLToPath } = await import("node:url");
  const { realpathSync } = await import("node:fs");
  try {
    const argvReal = realpathSync(process.argv[1]);
    const metaReal = realpathSync(fileURLToPath(import.meta.url));
    return argvReal === metaReal;
  } catch {
    // Fall back to a href compare which handles the non-symlink case.
    return import.meta.url === pathToFileURL(process.argv[1]).href;
  }
}

// Only run when invoked as an entrypoint (not when imported by tests).
if (await runAsEntrypoint()) {
  main().catch((err) => {
    process.stderr.write(
      `cusa-sidecar fatal: ${(err as Error).stack ?? err}\n`,
    );
    process.exit(1);
  });
}
