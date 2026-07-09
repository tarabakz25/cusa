// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// SdkAdapter: thin abstraction over `@cursor/sdk` used by SessionManager.
// The real adapter forwards to `Cursor` / `Agent` from the installed SDK;
// tests inject `FakeSdkAdapter` (see sdkAdapter.fake.ts) to avoid the
// network. Keep this interface small and free of Cursor-SDK types so the
// session layer never leaks vendor types into RPC payloads.

import type {
  ApprovalMode,
  ModelInfo,
  ModelParameterDefinition,
  ModelSelection,
  SettingSource,
  ToolCategory,
  TokenUsage,
} from "../rpc/schema.js";

export type {
  ApprovalMode,
  ModelInfo,
  ModelParameterDefinition,
  ModelSelection,
  SettingSource,
  ToolCategory,
  TokenUsage,
};

export interface CreateAgentOptions {
  cwd: string;
  model?: ModelSelection;
  approvalMode: ApprovalMode;
  settingSources?: SettingSource[];
  mcpOverrides?: unknown;
  apiKey: string;
}

export interface ResumeAgentOptions {
  cwd: string;
  approvalMode: ApprovalMode;
  mcpOverrides?: unknown;
  apiKey: string;
}

export interface SendOptions {
  modelOverride?: ModelSelection;
  systemContext?: string;
  /**
   * Composed MCP server map for this send. Per the SDK docs, when
   * `mcpServers` is passed to `send()` it fully replaces the creation-
   * time set; the sidecar re-passes the composed map every turn.
   */
  mcpServers?: Record<string, unknown>;
  onEvent: (event: TurnEvent) => void;
  signal?: AbortSignal;
}

export interface AgentHandle {
  readonly agentId: string;
  readonly model: string | undefined;
  send(text: string, opts: SendOptions): Promise<TurnHandle>;
  dispose(): Promise<void>;
}

export interface TurnHandle {
  readonly runId: string;
  readonly model: string | undefined;
  readonly supportsCancel: boolean;
  cancel(): Promise<void>;
  wait(): Promise<TurnResult>;
}

export interface TurnResult {
  status: "finished" | "cancelled" | "error";
  usage?: TokenUsage;
  model?: string;
  resultSummary?: string;
  errorMessage?: string;
}

export type TurnEvent =
  | { kind: "text-delta"; delta: string; textKind?: "assistant" | "reasoning" }
  | {
      kind: "tool-call";
      callId: string;
      name: string;
      category: ToolCategory;
      args: unknown;
    }
  | {
      kind: "tool-result";
      callId: string;
      ok: boolean;
      outputPreview?: string;
      error?: string;
    }
  | { kind: "usage"; usage: TokenUsage }
  | { kind: "warning"; message: string };

export interface SdkAdapter {
  /**
   * List models available to the given API key. The key must be passed
   * explicitly: `Cursor.models.list()` falls back to `process.env`'s
   * CURSOR_API_KEY when omitted, which silently breaks keys resolved
   * from `~/.cusa/config.toml` (SPEC-016 / SPEC-100).
   */
  listModels(apiKey: string): Promise<ModelInfo[]>;
  createAgent(opts: CreateAgentOptions): Promise<AgentHandle>;
  resumeAgent(agentId: string, opts: ResumeAgentOptions): Promise<AgentHandle>;
  /**
   * Cancel every non-terminal run recorded for `agentId` in the SDK's
   * *local agent store* and return how many were cancelled (issue #5
   * follow-up).
   *
   * Background: the local store persists `agent.activeRunId`; `send()`
   * refuses with "Agent <id> already has active run" while that run's
   * record is non-terminal, and the SDK has no staleness sweep. A run
   * whose process died or whose network stream hung therefore wedges the
   * agent forever. Cancelling the stale record via the store clears
   * `activeRunId` (verified against @cursor/sdk 1.0.x: the detached
   * `run.cancel()` marks the run `cancelled` and resets the agent row to
   * `idle`/`activeRunId: null`), after which `send()` works again.
   *
   * `cwd` must match the agent's persisted cwd — the local store scopes
   * agent lookup by workspace and `listRuns` fails with "agent not
   * found" otherwise.
   */
  cancelStaleRuns(agentId: string, opts: { cwd: string }): Promise<number>;
}

// ---------- Real adapter (thin wrapper around @cursor/sdk) ---------------

/**
 * Build a real SdkAdapter backed by `@cursor/sdk`. The heavy imports are
 * dynamic so that unit tests that inject the fake adapter never pull the
 * SDK into their module graph.
 */
export async function createRealSdkAdapter(): Promise<SdkAdapter> {
  const sdk = await import("@cursor/sdk");
  return new RealSdkAdapter(sdk);
}

type CursorSdk = typeof import("@cursor/sdk");
type SdkAgent = Awaited<ReturnType<CursorSdk["Agent"]["create"]>>;
type SdkRun = Awaited<ReturnType<SdkAgent["send"]>>;

class RealSdkAdapter implements SdkAdapter {
  constructor(private readonly sdk: CursorSdk) {}

  async listModels(apiKey: string): Promise<ModelInfo[]> {
    const models = await this.sdk.Cursor.models.list({ apiKey });
    return models.map((m) => ({
      id: m.id,
      displayName: m.displayName,
      parameters: m.parameters?.map(mapParameterDefinition),
    }));
  }

  async createAgent(opts: CreateAgentOptions): Promise<AgentHandle> {
    const agent = await this.sdk.Agent.create({
      apiKey: opts.apiKey,
      model: opts.model ? toSdkModelSelection(opts.model) : undefined,
      local: {
        cwd: opts.cwd,
        settingSources: mapSettingSources(opts.settingSources),
        sandboxOptions:
          opts.approvalMode === "full-auto" ? { enabled: true } : undefined,
      },
      mcpServers: mcpServersFrom(opts.mcpOverrides),
    });
    return new RealAgentHandle(agent);
  }

  async resumeAgent(
    agentId: string,
    opts: ResumeAgentOptions,
  ): Promise<AgentHandle> {
    const agent = await this.sdk.Agent.resume(agentId, {
      apiKey: opts.apiKey,
      local: {
        cwd: opts.cwd,
        sandboxOptions:
          opts.approvalMode === "full-auto" ? { enabled: true } : undefined,
      },
      mcpServers: mcpServersFrom(opts.mcpOverrides),
    });
    return new RealAgentHandle(agent);
  }

  async cancelStaleRuns(
    agentId: string,
    opts: { cwd: string },
  ): Promise<number> {
    // Terminal statuses per the SDK's internal predicate ("finished" |
    // "error" | "cancelled" | "expired"). Everything else can hold the
    // agent's `activeRunId` and must be cancelled. The public RunStatus
    // type omits "expired", so compare as strings.
    const terminal = new Set<string>(["finished", "error", "cancelled", "expired"]);
    let cancelled = 0;
    let cursor: string | undefined;
    do {
      const page = await this.sdk.Agent.listRuns(agentId, {
        runtime: "local",
        cwd: opts.cwd,
        ...(cursor === undefined ? {} : { cursor }),
      });
      for (const run of page.items) {
        if (terminal.has(run.status)) continue;
        try {
          // Detached (store-loaded) handles support cancel without a live
          // executor: it is a pure store mutation.
          await run.cancel();
          cancelled += 1;
        } catch {
          // Best-effort: a run that settles concurrently is fine; a run we
          // cannot cancel will resurface as AgentBusy on the retry, which
          // the caller reports with a /reset hint.
        }
      }
      cursor = page.nextCursor;
    } while (cursor !== undefined);
    return cancelled;
  }
}

function mapParameterDefinition(
  p: import("@cursor/sdk").ModelParameterDefinition,
): ModelParameterDefinition {
  return {
    id: p.id,
    displayName: p.displayName,
    values: p.values.map((v) => ({
      value: v.value,
      displayName: v.displayName,
    })),
  };
}

function toSdkModelSelection(
  selection: ModelSelection,
): import("@cursor/sdk").ModelSelection {
  const out: import("@cursor/sdk").ModelSelection = { id: selection.id };
  if (selection.params && selection.params.length > 0) {
    out.params = selection.params.map((p) => ({ id: p.id, value: p.value }));
  }
  return out;
}

function mcpServersFrom(overrides: unknown):
  | Record<string, import("@cursor/sdk").McpServerConfig>
  | undefined {
  if (!overrides || typeof overrides !== "object") return undefined;
  return overrides as Record<string, import("@cursor/sdk").McpServerConfig>;
}

type SdkSettingSource = import("@cursor/sdk").SettingSource;

function mapSettingSources(
  sources: SettingSource[] | undefined,
): SdkSettingSource[] | undefined {
  if (!sources) return undefined;
  // Our RPC schema exposes "user" | "project" | "local"; the SDK's local
  // agent accepts "project" | "user" | "team" | "mdm" | "plugins" | "all".
  // "local" isn't a valid SDK layer — drop it and let the SDK fall back to
  // its default resolver.
  const mapped: SdkSettingSource[] = [];
  for (const s of sources) {
    if (s === "user" || s === "project") mapped.push(s);
  }
  return mapped;
}

class RealAgentHandle implements AgentHandle {
  constructor(private readonly agent: SdkAgent) {}

  get agentId(): string {
    return this.agent.agentId;
  }

  get model(): string | undefined {
    return this.agent.model?.id;
  }

  async send(text: string, opts: SendOptions): Promise<TurnHandle> {
    const prompt = opts.systemContext ? `${opts.systemContext}\n\n${text}` : text;
    const sendArgs: {
      model?: import("@cursor/sdk").ModelSelection;
      onDelta: (args: { update: InteractionUpdate }) => void;
      mcpServers?: Record<string, import("@cursor/sdk").McpServerConfig>;
    } = {
      onDelta: ({ update }) => forwardDelta(update, opts.onEvent),
    };
    if (opts.modelOverride) sendArgs.model = toSdkModelSelection(opts.modelOverride);
    if (opts.mcpServers) {
      sendArgs.mcpServers = opts.mcpServers as Record<
        string,
        import("@cursor/sdk").McpServerConfig
      >;
    }
    const run = await this.agent.send(prompt, sendArgs);
    return new RealTurnHandle(run);
  }

  async dispose(): Promise<void> {
    try {
      await (this.agent as unknown as { [Symbol.asyncDispose]?: () => Promise<void> })
        [Symbol.asyncDispose]?.();
    } catch {
      // fall through to close()
    }
    try {
      this.agent.close();
    } catch {
      /* ignore */
    }
  }
}

class RealTurnHandle implements TurnHandle {
  constructor(private readonly run: SdkRun) {}

  get runId(): string {
    return this.run.id;
  }

  get model(): string | undefined {
    return this.run.model?.id;
  }

  get supportsCancel(): boolean {
    return this.run.supports("cancel");
  }

  async cancel(): Promise<void> {
    if (!this.supportsCancel) return;
    await this.run.cancel();
  }

  async wait(): Promise<TurnResult> {
    const r = await this.run.wait();
    const status: TurnResult["status"] =
      r.status === "finished" || r.status === "cancelled" || r.status === "error"
        ? r.status
        : "error";
    return {
      status,
      usage: r.usage ? normalizeUsage(r.usage) : undefined,
      model: r.model?.id,
      resultSummary: r.result,
      errorMessage: r.error?.message,
    };
  }
}

// ---------- Event translation ------------------------------------------

// The delta-types module is enormous (>600k chars) and we only touch a
// handful of shapes; import via the aggregated type export.
type InteractionUpdate = import("@cursor/sdk").InteractionUpdate;

function forwardDelta(
  update: InteractionUpdate,
  emit: (e: TurnEvent) => void,
): void {
  switch (update.type) {
    case "text-delta":
      emit({ kind: "text-delta", delta: update.text, textKind: "assistant" });
      return;
    case "thinking-delta":
      // The reasoning-delta shape varies; only forward if it has a `text` field.
      if (
        "text" in update &&
        typeof (update as { text?: unknown }).text === "string"
      ) {
        emit({
          kind: "text-delta",
          delta: (update as { text: string }).text,
          textKind: "reasoning",
        });
      }
      return;
    case "tool-call-started": {
      const tc = update.toolCall as { type: string; args?: unknown };
      emit({
        kind: "tool-call",
        callId: update.callId,
        name: tc.type,
        category: toolCategoryFor(tc.type),
        args: tc.args,
      });
      return;
    }
    case "tool-call-completed": {
      const tc = update.toolCall as {
        type: string;
        result?: { status?: string; value?: unknown; error?: unknown };
      };
      const status = tc.result?.status;
      const ok = status === "success";
      emit({
        kind: "tool-result",
        callId: update.callId,
        ok,
        outputPreview: previewOf(tc.result?.value),
        error: ok ? undefined : previewOf(tc.result?.error),
      });
      return;
    }
    case "turn-ended":
      if (update.usage) {
        emit({
          kind: "usage",
          usage: normalizeTurnUsage(update.usage),
        });
      }
      return;
    default:
      // Unhandled variants (token-delta, summary-*, step-*, shell-output-delta,
      // partial-tool-call, user-message-appended) are intentionally dropped
      // in slice 1; later slices can extend.
      return;
  }
}

function toolCategoryFor(name: string): ToolCategory {
  switch (name) {
    case "read":
    case "ls":
    case "glob":
    case "grep":
    case "semSearch":
    case "readLints":
      return "read";
    case "write":
    case "edit":
    case "delete":
      return "write";
    case "shell":
      return "shell";
    case "mcp":
      return "mcp";
    default:
      return "other";
  }
}

function previewOf(v: unknown): string | undefined {
  if (v === undefined || v === null) return undefined;
  if (typeof v === "string") return v.slice(0, 512);
  try {
    return JSON.stringify(v).slice(0, 512);
  } catch {
    return String(v).slice(0, 512);
  }
}

function normalizeUsage(u: {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
  totalTokens: number;
  reasoningTokens?: number;
}): TokenUsage {
  return {
    inputTokens: u.inputTokens,
    outputTokens: u.outputTokens,
    cacheReadTokens: u.cacheReadTokens ?? 0,
    cacheCreationTokens: u.cacheWriteTokens ?? 0,
    reasoningTokens: u.reasoningTokens ?? 0,
    totalTokens: u.totalTokens,
  };
}

function normalizeTurnUsage(u: {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  reasoningTokens?: number;
}): TokenUsage {
  const total =
    u.inputTokens +
    u.outputTokens +
    (u.cacheReadTokens ?? 0) +
    (u.cacheWriteTokens ?? 0);
  return {
    inputTokens: u.inputTokens,
    outputTokens: u.outputTokens,
    cacheReadTokens: u.cacheReadTokens ?? 0,
    cacheCreationTokens: u.cacheWriteTokens ?? 0,
    reasoningTokens: u.reasoningTokens ?? 0,
    totalTokens: total,
  };
}
