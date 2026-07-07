// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SessionManager: owns Cursor SDK agents (via SdkAdapter), streams events
// out as RPC notifications, and mediates tool-approval bridging.
//
// SPEC IDs relevant here:
// - SPEC-001: streaming assistant deltas as `stream/message`.
// - SPEC-004: `session/cancel` → run.cancel() with a 3 s settle window.
// - SPEC-060/061: cumulative + per-turn token usage accounting.
// - SPEC-071: hosts `@cursor/sdk`.
// - SPEC-100: never returns the API key on the wire.

import { randomUUID } from "node:crypto";

import { approvalPolicy, shouldEnableSandbox } from "../approval/policy.js";
import { readApiKey, type ApiKeySource } from "../config/apiKey.js";
import { Router, type RouteContext } from "../router/index.js";
import {
  Method,
  RpcErrorCode,
  type ApprovalDecision,
  type ApprovalMode,
  type ContextSetStrategyParams,
  type McpListParams,
  type McpListResult,
  type McpToggleParams,
  type McpToggleResult,
  type ModelInfo,
  type Ok,
  type SessionCancelParams,
  type SessionCreateParams,
  type SessionCreateResult,
  type SessionDisposeParams,
  type SessionResumeParams,
  type SessionResumeResult,
  type SessionSendParams,
  type SessionSendResult,
  type SessionSetApprovalModeParams,
  type SessionSetApprovalModeResult,
  type SkillInfo,
  type SkillsListParams,
  type SkillsListResult,
  type SkillsSetEnabledParams,
  type TokenUsage,
  type ToolApprovalResponseParams,
} from "../rpc/schema.js";
import { UsageAccumulator, TurnUsageTracker } from "../usage/accumulator.js";
import { McpManager, type McpServerConfigMap } from "../mcp/index.js";
import { SkillsManager } from "../skills/index.js";
import { ContextManager } from "../context/index.js";
import type {
  AgentHandle,
  SdkAdapter,
  SendOptions,
  TurnHandle,
} from "./sdkAdapter.js";

const CANCEL_SETTLE_MS = 3000;

/**
 * Default cap for `agent.send()` — the network call that establishes a run
 * with the Cursor API. Without a bound, an unreachable backend leaves the
 * caller of `session/send` waiting forever (the TUI shows an endless
 * spinner; see issue #5).
 */
export const DEFAULT_SEND_TIMEOUT_MS = 60_000;

export interface NotifyFn {
  (method: string, params: unknown): void;
}

export interface SessionManagerOptions {
  adapter: SdkAdapter;
  notify: NotifyFn;
  log?: (level: "info" | "warn" | "error", msg: string) => void;
  readApiKey?: typeof readApiKey;
  now?: () => number;
  /**
   * Optional router. When omitted, a Router with built-in defaults is
   * used (rules only, no LLM classifier).
   */
  router?: Router;
  skills?: SkillsManager;
  mcp?: McpManager;
  /**
   * Context manager (SPEC-090..093). When omitted, a default instance
   * with manual injection ON and no summarizer client is used.
   */
  context?: ContextManager;
  /**
   * Cap (ms) for `agent.send()` — the run-establishing network call to the
   * Cursor API. Defaults to `DEFAULT_SEND_TIMEOUT_MS`. Tests inject small
   * values to exercise the timeout path deterministically.
   */
  sendTimeoutMs?: number;
}

interface PendingApproval {
  requestId: string;
  resolve: (decision: ApprovalDecision) => void;
}

interface SessionState {
  sessionId: string;
  agent: AgentHandle;
  approvalMode: ApprovalMode;
  usage: UsageAccumulator;
  enabledSkillIds: string[];
  mcpOverrides: unknown;
  cwd: string;
  defaultModel: string | undefined;
  currentModel: string | undefined;
  activeRun: ActiveRunState | null;
  /** Tool names for which the user selected "always" this session. */
  alwaysApprovedTools: Set<string>;
  /**
   * Pending approval requests, keyed by requestId. Session-scoped (not
   * run-scoped) on purpose: gating is observational, so a fast run can
   * emit `run/finished` before the user's response arrives — an "always"
   * decision must still seed `alwaysApprovedTools` in that case instead
   * of being silently discarded.
   */
  pendingApprovals: Map<string, PendingApproval>;
  /** Enabled MCP server ids (subset of mcp/list). */
  enabledMcpServerIds: Set<string> | null;
  /** Cached one-time observational-approval warning flag. */
  observationalWarnEmitted: boolean;
}

interface ActiveRunState {
  runId: string;
  turn: TurnHandle;
  turnUsage: TurnUsageTracker;
  effectiveModel: string | undefined;
  /** Text of the user prompt that started this run. */
  userPrompt: string;
  /** Assistant deltas accumulated for the ConversationHistory. */
  assistantBuffer: string[];
  /** Human-readable tool-call summary lines for the completed turn. */
  toolCallsSummary: string[];
  /** Track tool names + arg previews between call-start and call-end. */
  toolCallInfo: Map<string, { name: string; argPreview: string }>;
}

/**
 * Thrown by SessionManager methods to produce a typed RPC error.
 */
export class SessionRpcError extends Error {
  constructor(
    public readonly code: number,
    message: string,
    public readonly data?: unknown,
  ) {
    super(message);
    this.name = "SessionRpcError";
  }
}

export class SessionManager {
  private readonly adapter: SdkAdapter;
  private readonly notify: NotifyFn;
  private readonly log: (level: "info" | "warn" | "error", msg: string) => void;
  private readonly readKey: typeof readApiKey;
  private readonly sessions = new Map<string, SessionState>();
  private modelsCache: ModelInfo[] | null = null;
  private readonly router: Router;
  private readonly skills: SkillsManager;
  private readonly mcp: McpManager;
  private readonly context: ContextManager;
  private readonly sendTimeoutMs: number;

  constructor(opts: SessionManagerOptions) {
    this.adapter = opts.adapter;
    this.notify = opts.notify;
    this.log = opts.log ?? (() => {});
    this.readKey = opts.readApiKey ?? readApiKey;
    this.router = opts.router ?? new Router({ log: this.log });
    this.skills = opts.skills ?? new SkillsManager({ log: this.log });
    this.mcp = opts.mcp ?? new McpManager({ log: this.log });
    this.context = opts.context ?? new ContextManager({ log: this.log });
    this.sendTimeoutMs = opts.sendTimeoutMs ?? DEFAULT_SEND_TIMEOUT_MS;
  }

  // -------- API key -----------------------------------------------------

  private cachedKey: ApiKeySource | null = null;
  private keyChecked = false;

  private async requireApiKey(): Promise<string> {
    if (!this.keyChecked) {
      this.cachedKey = await this.readKey();
      this.keyChecked = true;
    }
    if (!this.cachedKey) {
      throw new SessionRpcError(
        RpcErrorCode.NoApiKey,
        "CURSOR_API_KEY is not set. Run `cusa login` or export CURSOR_API_KEY.",
      );
    }
    return this.cachedKey.key;
  }

  // -------- models/list -------------------------------------------------

  async listModels(): Promise<{ models: ModelInfo[] }> {
    if (this.modelsCache) return { models: this.modelsCache };
    // Resolve the key here (env or ~/.cusa/config.toml) and hand it to the
    // adapter explicitly. Relying on the SDK's env-var fallback breaks the
    // config-file path and surfaces as "models/list failed" (SPEC-016).
    const apiKey = await this.requireApiKey();
    try {
      const models = await this.adapter.listModels(apiKey);
      this.modelsCache = models;
      return { models };
    } catch (err) {
      throw this.wrapAgentError(err, "models/list failed");
    }
  }

  // -------- session/create ---------------------------------------------

  async createSession(
    params: SessionCreateParams,
  ): Promise<SessionCreateResult> {
    const apiKey = await this.requireApiKey();
    const approvalMode: ApprovalMode = params.approvalMode ?? "suggest";
    // Compose the initial MCP server map so the SDK gets a coherent
    // creation-time config; every session/send re-passes it because
    // inline mcpServers replace creation-time servers per SDK docs.
    const composedMcp = await this.mcp.compose({
      cwd: params.cwd,
      inline: params.mcpOverrides,
    });
    try {
      const agent = await this.adapter.createAgent({
        cwd: params.cwd,
        model: params.model,
        approvalMode,
        settingSources: params.settingSources ?? ["user", "project"],
        mcpOverrides: composedMcp,
        apiKey,
      });
      const sessionId = `sess_${randomUUID()}`;
      const state: SessionState = {
        sessionId,
        agent,
        approvalMode,
        usage: new UsageAccumulator(),
        enabledSkillIds: params.enabledSkillIds ?? [],
        mcpOverrides: params.mcpOverrides,
        cwd: params.cwd,
        defaultModel: params.model,
        currentModel: agent.model ?? params.model,
        activeRun: null,
        alwaysApprovedTools: new Set(),
        pendingApprovals: new Map(),
        enabledMcpServerIds: null, // null = "all enabled"
        observationalWarnEmitted: false,
      };
      this.sessions.set(sessionId, state);
      this.context.registerSession(sessionId);
      this.assertSandboxCoupling(approvalMode);
      return {
        sessionId,
        agentId: agent.agentId,
        model: state.currentModel ?? "",
      };
    } catch (err) {
      if (err instanceof SessionRpcError) throw err;
      throw this.wrapAgentError(err, "session/create failed");
    }
  }

  // -------- session/send -----------------------------------------------

  async sendMessage(
    params: SessionSendParams,
  ): Promise<SessionSendResult> {
    const session = this.getSession(params.sessionId);
    if (session.activeRun) {
      throw new SessionRpcError(
        RpcErrorCode.AgentError,
        "a run is already active for this session (queue_mode=reject)",
      );
    }
    await this.requireApiKey();

    // Route this turn. When the caller passed `modelOverride`, we honour
    // that (per-call sticky override); otherwise we delegate to the
    // Router pipeline.
    const routeCtx: RouteContext = { prompt: params.text };
    if (session.currentModel !== undefined) {
      routeCtx.currentModel = session.currentModel;
    }
    if (session.defaultModel !== undefined) {
      routeCtx.defaultModel = session.defaultModel;
    }
    if (params.modelOverride !== undefined) {
      routeCtx.sessionManualModel = params.modelOverride;
    }
    if (session.enabledSkillIds.length > 0) {
      routeCtx.enabledSkills = [...session.enabledSkillIds];
    }
    const decision = await this.router.route(routeCtx);
    const effectiveModel = decision.model;

    // Build the system-context block (skills + conversation history) and
    // compose the per-turn MCP server map (inline replaces creation-time).
    const systemContext = await this.buildSystemContext(session);
    const mcpForTurn = await this.mcp.composeForTurn({
      cwd: session.cwd,
      inline: session.mcpOverrides,
      enabledIds: session.enabledMcpServerIds,
    });

    let turn: TurnHandle;
    let runId: string;

    // Events can start flowing the moment the adapter's turn exists —
    // potentially before `session.activeRun` is assigned below (the gap
    // spans several microtasks). Buffer them until the run is established
    // so early deltas / tool calls are never silently dropped. If the send
    // fails for good, flip to dropping so a ghost run that keeps streaming
    // can never grow the buffer for its whole lifetime.
    let earlyEvents: Array<import("./sdkAdapter.js").TurnEvent> | null = [];
    let dropTurnEvents = false;

    const sendOptions: SendOptions = {
      modelOverride: effectiveModel,
      onEvent: (event) => {
        if (dropTurnEvents) return;
        if (earlyEvents !== null) {
          earlyEvents.push(event);
          return;
        }
        this.dispatchTurnEvent(session, event);
      },
    };
    if (systemContext !== undefined) sendOptions.systemContext = systemContext;
    if (mcpForTurn !== undefined) sendOptions.mcpServers = mcpForTurn;

    try {
      turn = await this.sendWithStaleRunRecovery(
        session,
        params.text,
        sendOptions,
      );
      runId = turn.runId;
    } catch (err) {
      dropTurnEvents = true;
      earlyEvents = null;
      if (err instanceof SessionRpcError) throw err;
      throw this.wrapAgentError(err, "session/send failed");
    }

    const active: ActiveRunState = {
      runId,
      turn,
      turnUsage: new TurnUsageTracker(),
      effectiveModel,
      userPrompt: params.text,
      assistantBuffer: [],
      toolCallsSummary: [],
      toolCallInfo: new Map(),
    };
    session.activeRun = active;

    // Emit router/decision so the TUI has a decision line to render.
    this.notify(Method.RouterDecision, {
      sessionId: session.sessionId,
      runId,
      model: effectiveModel,
      rationale: decision.rationale,
      source: decision.source,
    });

    // Flush events that raced ahead of run establishment (keeping them
    // behind router/decision), then switch the handler to direct dispatch.
    const buffered = earlyEvents;
    earlyEvents = null;
    for (const event of buffered) {
      this.dispatchTurnEvent(session, event);
    }

    // Fire and forget: consume the turn and emit finish/error events.
    // We intentionally do not await here — the sidecar returns runId to the
    // TUI immediately; final settlement is signalled via run/finished.
    void this.awaitTurn(session, active);

    return { runId };
  }

  /**
   * `agent.send()` with automatic stale-run recovery (issue #5 follow-up).
   *
   * The SDK's local agent store persists `agent.activeRunId`, and `send()`
   * refuses with "Agent <id> already has active run" while that run's
   * record is non-terminal. The store has no staleness sweep, so a run
   * whose process died mid-stream (crash, kill) or whose network phase
   * hung (e.g. our own send timeout below — the SDK writes the run record
   * *before* opening the stream) wedges the agent permanently: every
   * subsequent send fails.
   *
   * SessionManager itself tracks at most one live run per session (the
   * `activeRun` guard at the top of `sendMessage`), so when the SDK
   * reports "busy" while we track no run, the recorded run is stale by
   * definition. Recovery: cancel the stale record(s) in the store — which
   * clears `activeRunId` — and retry the send exactly once.
   */
  private async sendWithStaleRunRecovery(
    session: SessionState,
    text: string,
    sendOptions: SendOptions,
  ): Promise<TurnHandle> {
    try {
      return await this.sendWithTimeout(session, text, sendOptions);
    } catch (err) {
      if (!isAgentBusyError(err)) throw err;
      this.log(
        "warn",
        `agent ${session.agent.agentId} reports an active run but none is tracked — ` +
          "cancelling stale run(s) in the local store and retrying",
      );
      let cancelledCount: number;
      try {
        cancelledCount = await this.adapter.cancelStaleRuns(
          session.agent.agentId,
          { cwd: session.cwd },
        );
      } catch (recoveryErr) {
        const detail =
          recoveryErr instanceof Error
            ? recoveryErr.message
            : String(recoveryErr);
        throw new SessionRpcError(
          RpcErrorCode.AgentError,
          `session/send failed: ${(err as Error).message} — automatic ` +
            `stale-run recovery failed (${detail}); start a fresh ` +
            'session (relaunch cusa and pick "New session")',
        );
      }
      this.log(
        "info",
        `stale-run recovery cancelled ${cancelledCount} run(s); retrying send`,
      );
      try {
        return await this.sendWithTimeout(session, text, sendOptions);
      } catch (retryErr) {
        if (isAgentBusyError(retryErr)) {
          throw new SessionRpcError(
            RpcErrorCode.AgentError,
            `session/send failed: ${(retryErr as Error).message} — the ` +
              "agent is still busy after stale-run recovery (another " +
              "process may be driving it); start a fresh session " +
              '(relaunch cusa and pick "New session")',
          );
        }
        throw retryErr;
      }
    }
  }

  /**
   * Run `agent.send()` under a hard timeout (issue #5). `agent.send()` is a
   * network call into the Cursor API; the SDK exposes no deadline for it, so
   * an unreachable backend would otherwise park `session/send` forever and
   * the TUI would spin with no error. We race the call against a timer and
   * surface a typed RPC error when the budget is exhausted.
   *
   * The adapter-level `SendOptions.signal` is aborted on timeout so adapters
   * that honour it (the fake does; the real SDK currently exposes no signal
   * for `send`) stop early. If the underlying send settles *after* the
   * timeout fired, the ghost run is cancelled best-effort and its rejection
   * is swallowed so it never becomes an unhandled rejection.
   */
  private async sendWithTimeout(
    session: SessionState,
    text: string,
    sendOptions: SendOptions,
  ): Promise<TurnHandle> {
    const controller = new AbortController();
    sendOptions.signal = controller.signal;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let timedOut = false;

    const sendPromise = session.agent.send(text, sendOptions);
    // Reap a late-settling send: if the race below is lost to the timeout,
    // the underlying promise may still settle afterwards. Cancel the ghost
    // run best-effort and swallow any rejection so it never surfaces as an
    // unhandled rejection.
    void sendPromise
      .then((turn) => {
        if (timedOut) void turn.cancel().catch(() => {});
      })
      .catch(() => {});

    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        timedOut = true;
        // Reject *before* aborting: both settle in the same tick, and the
        // race must surface the typed timeout error, not whatever the
        // adapter throws in reaction to the abort.
        reject(
          new SessionRpcError(
            RpcErrorCode.AgentError,
            `session/send failed: agent.send() timed out after ${this.sendTimeoutMs} ms ` +
              "(no response from the Cursor API — check network connectivity / try /model to probe)",
          ),
        );
        controller.abort();
      }, this.sendTimeoutMs);
    });

    try {
      return await Promise.race([sendPromise, timeout]);
    } catch (err) {
      if (timedOut) {
        this.log(
          "warn",
          `agent.send() timed out after ${this.sendTimeoutMs} ms for session ${session.sessionId}`,
        );
      }
      throw err;
    } finally {
      if (timer !== null) clearTimeout(timer);
    }
  }

  private async buildSystemContext(
    session: SessionState,
  ): Promise<string | undefined> {
    let skillsBlock = "";
    if (session.enabledSkillIds.length > 0) {
      skillsBlock = await this.skills.buildContextFor({
        cwd: session.cwd,
        enabledIds: session.enabledSkillIds,
        onWarn: (msg) =>
          this.notify(Method.Log, {
            level: "warn",
            message: msg,
            target: "sidecar/skills",
          }),
      });
    }
    // Conversation-history workaround (SPEC-090..093). The current turn's
    // user text is NOT included; it goes to the SDK as the fresh prompt.
    const built = await this.context.buildContext(session.sessionId);
    const parts: string[] = [];
    if (skillsBlock.length > 0) parts.push(skillsBlock);
    if (built.text.length > 0) parts.push(built.text);
    if (parts.length === 0) return undefined;
    return parts.join("\n\n");
  }

  private dispatchTurnEvent(
    session: SessionState,
    event: import("./sdkAdapter.js").TurnEvent,
  ): void {
    const active = session.activeRun;
    if (!active) return;
    switch (event.kind) {
      case "text-delta":
        if ((event.textKind ?? "assistant") === "assistant") {
          active.assistantBuffer.push(event.delta);
        }
        this.notify(Method.StreamMessage, {
          runId: active.runId,
          delta: event.delta,
          kind: event.textKind ?? "assistant",
        });
        return;
      case "tool-call": {
        active.toolCallInfo.set(event.callId, {
          name: event.name,
          argPreview: previewArgs(event.args),
        });
        this.notify(Method.StreamToolCall, {
          runId: active.runId,
          callId: event.callId,
          name: event.name,
          args: event.args,
        });
        // Policy decision. "always" cache short-circuits future prompts.
        let decision = approvalPolicy({
          mode: session.approvalMode,
          toolName: event.name,
          category: event.category,
        });
        if (
          decision === "prompt" &&
          session.alwaysApprovedTools.has(event.name)
        ) {
          decision = "auto-approve";
        }
        if (decision === "prompt") {
          const requestId = `appr_${randomUUID()}`;
          session.pendingApprovals.set(requestId, {
            requestId,
            resolve: (final: ApprovalDecision) => {
              if (final === "always") {
                session.alwaysApprovedTools.add(event.name);
              }
            },
          });
          this.notify(Method.ToolApprovalRequest, {
            requestId,
            runId: active.runId,
            name: event.name,
            args: event.args,
            category: event.category,
          });
          // Observational path (SDK 1.0.23 exposes no beforeToolCall hook):
          // emit the resolution *now* so the TUI can reconcile that the
          // sidecar did not actually block the call.
          this.notify(Method.ToolApprovalResult, {
            requestId,
            runId: active.runId,
            name: event.name,
            decision: "prompt",
            observed: true,
          });
          this.emitObservationalWarnOnce(session);
        }
        return;
      }
      case "tool-result": {
        const call = active.toolCallInfo.get(event.callId);
        const name = call?.name ?? "tool";
        const argPreview = call?.argPreview ?? "";
        const bodyBits: string[] = [];
        if (argPreview.length > 0) bodyBits.push(argPreview);
        if (event.ok) {
          if (event.outputPreview) bodyBits.push(event.outputPreview.slice(0, 200));
        } else {
          bodyBits.push(`error: ${event.error ?? "unknown"}`);
        }
        active.toolCallsSummary.push(
          `${name} ${bodyBits.join(" — ").trim()}`.trim(),
        );
        this.notify(Method.StreamToolResult, {
          runId: active.runId,
          callId: event.callId,
          ok: event.ok,
          outputPreview: event.outputPreview,
          error: event.error,
        });
        return;
      }
      case "usage":
        active.turnUsage.observe(event.usage);
        this.notify(Method.StreamUsage, {
          runId: active.runId,
          usage: event.usage,
        });
        return;
      case "warning":
        this.notify(Method.Log, {
          level: "warn",
          message: event.message,
          target: "sidecar/agent",
        });
        return;
    }
  }

  private async awaitTurn(
    session: SessionState,
    active: ActiveRunState,
  ): Promise<void> {
    try {
      const result = await active.turn.wait();
      // Cumulative usage: prefer the SDK's final `usage` snapshot; fall
      // back to the accumulated per-event usage.
      const finalUsage: TokenUsage =
        result.usage ?? active.turnUsage.turnDelta();
      const modelId = result.model ?? active.effectiveModel;
      session.usage.add(finalUsage, modelId);
      if (result.status === "error") {
        this.notify(Method.RunError, {
          runId: active.runId,
          error: {
            code: RpcErrorCode.AgentError,
            message: result.errorMessage ?? "run failed",
          },
        });
      }
      if (modelId) session.currentModel = modelId;
      // Record the completed turn onto the ConversationHistory *before*
      // emitting `run/finished`, so any observer that reacts to that
      // notification (tests, downstream consumers) can safely inspect the
      // history and see the just-finished turn. On error we still record
      // so users can retry with the failed turn's context intact.
      const assistantText = active.assistantBuffer.join("");
      this.context.recordTurn(session.sessionId, {
        userPrompt: active.userPrompt,
        assistantText,
        toolCallsSummary: active.toolCallsSummary,
        ...(modelId !== undefined ? { model: modelId } : {}),
      });
      this.notify(Method.RunFinished, {
        runId: active.runId,
        status: result.status,
        usage: finalUsage,
        model: modelId,
        resultSummary: result.resultSummary,
      });
    } catch (err) {
      this.notify(Method.RunError, {
        runId: active.runId,
        error: {
          code: RpcErrorCode.AgentError,
          message: (err as Error).message ?? "unknown run error",
        },
      });
    } finally {
      if (session.activeRun && session.activeRun.runId === active.runId) {
        session.activeRun = null;
      }
    }
  }

  // -------- session/cancel ---------------------------------------------

  async cancelRun(params: SessionCancelParams): Promise<Ok> {
    const session = this.getSession(params.sessionId);
    const active = session.activeRun;
    if (!active || active.runId !== params.runId) {
      // Nothing to cancel — treat as success (idempotent).
      return { ok: true };
    }
    if (!active.turn.supportsCancel) {
      this.notify(Method.Log, {
        level: "warn",
        message: `run ${active.runId} does not support cancel; letting it drain`,
        target: "sidecar/session",
      });
    } else {
      try {
        await active.turn.cancel();
      } catch (err) {
        this.log("warn", `cancel() threw: ${(err as Error).message}`);
      }
    }
    // Wait up to 3 s for the run to settle; if it does not, emit
    // run/finished with status: "cancelled" ourselves.
    const settled = await raceWithTimeout(
      // Wait for `activeRun` to be cleared by awaitTurn().
      new Promise<void>((resolve) => {
        const check = () => {
          if (!session.activeRun || session.activeRun.runId !== active.runId) {
            resolve();
          } else {
            setTimeout(check, 25);
          }
        };
        check();
      }),
      CANCEL_SETTLE_MS,
    );
    if (!settled) {
      this.notify(Method.RunFinished, {
        runId: active.runId,
        status: "cancelled",
        usage: session.usage.snapshot(),
        model: active.effectiveModel,
        resultSummary: "cancelled (sidecar-forced)",
      });
      // Detach so future turns can proceed. The underlying SDK handle is
      // orphaned per spec §Concurrency.
      if (session.activeRun && session.activeRun.runId === active.runId) {
        session.activeRun = null;
      }
    }
    return { ok: true };
  }

  // -------- session/resume ---------------------------------------------

  async resumeSession(
    params: SessionResumeParams,
  ): Promise<SessionResumeResult> {
    const apiKey = await this.requireApiKey();
    const approvalMode = params.approvalMode ?? "suggest";
    const composedMcp = await this.mcp.compose({
      cwd: params.cwd,
      inline: params.mcpOverrides,
    });
    try {
      const agent = await this.adapter.resumeAgent(params.agentId, {
        cwd: params.cwd,
        approvalMode,
        mcpOverrides: composedMcp,
        apiKey,
      });
      const sessionId = `sess_${randomUUID()}`;
      const state: SessionState = {
        sessionId,
        agent,
        approvalMode,
        usage: new UsageAccumulator(),
        enabledSkillIds: params.enabledSkillIds ?? [],
        mcpOverrides: params.mcpOverrides,
        cwd: params.cwd,
        defaultModel: agent.model,
        currentModel: agent.model,
        activeRun: null,
        alwaysApprovedTools: new Set(),
        pendingApprovals: new Map(),
        enabledMcpServerIds: null,
        observationalWarnEmitted: false,
      };
      this.sessions.set(sessionId, state);
      this.context.registerSession(sessionId);
      return { sessionId, model: agent.model };
    } catch (err) {
      throw this.wrapAgentError(err, "session/resume failed");
    }
  }

  // -------- session/dispose --------------------------------------------

  async disposeSession(params: SessionDisposeParams): Promise<Ok> {
    const session = this.sessions.get(params.sessionId);
    if (!session) return { ok: true };
    this.sessions.delete(params.sessionId);
    this.context.disposeSession(params.sessionId);
    try {
      await session.agent.dispose();
    } catch (err) {
      this.log("warn", `dispose() threw: ${(err as Error).message}`);
    }
    return { ok: true };
  }

  // -------- skills -----------------------------------------------------

  async listSkills(params: SkillsListParams): Promise<SkillsListResult> {
    const { skills, warnings } = await this.skills.list(params.cwd);
    const out: SkillInfo[] = skills.map((s) => ({
      id: s.id,
      name: s.name,
      description: s.description,
      path: s.path,
      sizeBytes: s.sizeBytes,
      source: s.source,
    }));
    return { skills: out, warnings };
  }

  setSkillsEnabled(params: SkillsSetEnabledParams): Ok {
    const session = this.getSession(params.sessionId);
    session.enabledSkillIds = [...params.skillIds];
    return { ok: true };
  }

  // -------- MCP --------------------------------------------------------

  async listMcp(params: McpListParams): Promise<McpListResult> {
    const session = this.getSession(params.sessionId);
    const composed = await this.mcp.compose({
      cwd: session.cwd,
      inline: session.mcpOverrides,
    });
    const servers = await this.mcp.list({
      composed,
      enabledIds: session.enabledMcpServerIds,
    });
    return { servers };
  }

  toggleMcp(params: McpToggleParams): McpToggleResult {
    const session = this.getSession(params.sessionId);
    // Materialise the enabled set on first toggle. `null` means "all".
    let ids = session.enabledMcpServerIds;
    if (ids === null) {
      // Best-effort: seed with the composed server ids so an explicit
      // disable actually excludes something.
      ids = new Set();
      const composed = this.mcp.lastComposed();
      if (composed) {
        for (const id of Object.keys(composed)) ids.add(id);
      }
      session.enabledMcpServerIds = ids;
    }
    if (params.enabled) ids.add(params.serverId);
    else ids.delete(params.serverId);
    return { ok: true, pendingUntilNextTurn: true };
  }

  // -------- context/setStrategy ---------------------------------------

  setContextStrategy(params: ContextSetStrategyParams): Ok {
    // Validate the session exists so we surface a clean error.
    this.getSession(params.sessionId);
    this.context.setForcedStrategy(params.sessionId, params.strategy);
    return { ok: true };
  }

  contextManager(): ContextManager {
    return this.context;
  }

  // -------- session/setApprovalMode -----------------------------------

  setApprovalMode(
    params: SessionSetApprovalModeParams,
  ): SessionSetApprovalModeResult {
    const session = this.getSession(params.sessionId);
    const wasFullAuto = session.approvalMode === "full-auto";
    session.approvalMode = params.mode;
    this.assertSandboxCoupling(params.mode);
    const nowFullAuto = params.mode === "full-auto";
    if (wasFullAuto !== nowFullAuto) {
      // The SDK does not currently expose a live sandbox toggle: the
      // sandboxOptions passed at Agent.create() are baked in for that
      // agent handle. Document that limitation via a log line so the
      // TUI can render a hint.
      this.notify(Method.Log, {
        level: "warn",
        message:
          "approval mode changed to " +
          params.mode +
          ": SDK cannot live-toggle sandbox on an existing agent; " +
          "next session/create will apply the new sandbox setting.",
        target: "sidecar/approval",
      });
    }
    return { ok: true, liveSdkUpdate: false };
  }

  private emitObservationalWarnOnce(session: SessionState): void {
    if (session.observationalWarnEmitted) return;
    session.observationalWarnEmitted = true;
    this.notify(Method.Log, {
      level: "warn",
      message:
        "approval gating is observational until the SDK exposes a beforeToolCall hook",
      target: "sidecar/approval",
    });
  }

  // -------- tool/approvalResponse --------------------------------------

  handleApprovalResponse(params: ToolApprovalResponseParams): Ok {
    // Walk every session to find the pending request. In practice the TUI
    // is 1:1 with a session, but the schema does not bind the response to a
    // sessionId — we resolve by requestId only.
    for (const session of this.sessions.values()) {
      const pending = session.pendingApprovals.get(params.requestId);
      if (pending) {
        pending.resolve(params.decision);
        session.pendingApprovals.delete(params.requestId);
        return { ok: true };
      }
    }
    // Unknown request id: treat as no-op so a stale response doesn't crash.
    return { ok: true };
  }

  // -------- helpers -----------------------------------------------------

  private assertSandboxCoupling(mode: ApprovalMode): void {
    if (shouldEnableSandbox(mode)) {
      this.log("info", "approval=full-auto → sandbox enabled");
    }
  }

  private getSession(sessionId: string): SessionState {
    const s = this.sessions.get(sessionId);
    if (!s) {
      throw new SessionRpcError(
        RpcErrorCode.InvalidParams,
        `unknown sessionId: ${sessionId}`,
      );
    }
    return s;
  }

  private wrapAgentError(err: unknown, prefix: string): SessionRpcError {
    const message =
      err instanceof Error ? `${prefix}: ${err.message}` : `${prefix}`;
    return new SessionRpcError(RpcErrorCode.AgentError, message);
  }
}

/**
 * True when `err` is the SDK's "agent busy" rejection. The local-agent
 * path throws a plain `Error` with this message (not `AgentBusyError`,
 * which the cloud path uses for HTTP 409) — match both.
 */
function isAgentBusyError(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  if (err.name === "AgentBusyError") return true;
  return /already has (an )?active run/i.test(err.message);
}

function previewArgs(args: unknown): string {
  if (args === undefined || args === null) return "";
  try {
    // Prefer human-friendly output for well-known shapes.
    if (typeof args === "object" && !Array.isArray(args)) {
      const rec = args as Record<string, unknown>;
      if (typeof rec.path === "string") return rec.path;
      if (typeof rec.command === "string") return rec.command;
      if (typeof rec.file === "string") return rec.file;
    }
    return JSON.stringify(args).slice(0, 160);
  } catch {
    return String(args).slice(0, 160);
  }
}

async function raceWithTimeout(
  p: Promise<void>,
  ms: number,
): Promise<boolean> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const timeout = new Promise<boolean>((resolve) => {
    timer = setTimeout(() => resolve(false), ms);
  });
  const settled = p.then(() => true);
  try {
    return await Promise.race([settled, timeout]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}
