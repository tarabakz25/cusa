// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// TypeScript mirror of `tui/crates/cusa-rpc/src/lib.rs`. Keep the two in
// sync manually — the Rust crate is the source of truth for wire format.
// A drift test lives in `sidecar/src/rpc/schema.test.ts`.

export const PROTOCOL_VERSION = "0.1";

// ---------- Envelope ------------------------------------------------------

export type RequestId = number | string;

export interface Request<M extends string = string, P = unknown> {
  jsonrpc: "2.0";
  id: RequestId;
  method: M;
  params?: P;
}

export interface Response<R = unknown> {
  jsonrpc: "2.0";
  id: RequestId;
  result?: R;
  error?: RpcError;
}

export interface Notification<M extends string = string, P = unknown> {
  jsonrpc: "2.0";
  method: M;
  params?: P;
}

export interface RpcError {
  code: number;
  message: string;
  data?: unknown;
}

export const RpcErrorCode = {
  ParseError: -32700,
  InvalidRequest: -32600,
  MethodNotFound: -32601,
  InvalidParams: -32602,
  InternalError: -32603,

  // cusa-defined range: -32000 to -32099
  SidecarStartup: -32000,
  AgentError: -32001,
  RunCancelled: -32002,
  NoApiKey: -32003,
  SdkUnsupported: -32004,
} as const;

export type RpcErrorCode = (typeof RpcErrorCode)[keyof typeof RpcErrorCode];

// ---------- Method names --------------------------------------------------

export const Method = {
  Initialize: "initialize",
  Shutdown: "shutdown",

  ModelsList: "models/list",

  SessionCreate: "session/create",
  SessionSend: "session/send",
  SessionCancel: "session/cancel",
  SessionResume: "session/resume",
  SessionDispose: "session/dispose",

  SessionSetApprovalMode: "session/setApprovalMode",

  SkillsList: "skills/list",
  SkillsSetEnabled: "skills/setEnabled",

  McpList: "mcp/list",
  McpToggle: "mcp/toggle",

  ContextSetStrategy: "context/setStrategy",

  ToolApprovalResponse: "tool/approvalResponse",

  RouterDecision: "router/decision",
  StreamMessage: "stream/message",
  StreamToolCall: "stream/toolCall",
  StreamToolResult: "stream/toolResult",
  StreamUsage: "stream/usage",
  ToolApprovalRequest: "tool/approvalRequest",
  ToolApprovalResult: "tool/approvalResult",
  RunFinished: "run/finished",
  RunError: "run/error",
  Log: "log",
} as const;

export type Method = (typeof Method)[keyof typeof Method];

// ---------- Domain enums --------------------------------------------------

export type ApprovalMode = "suggest" | "auto-edit" | "full-auto";
export type ApprovalDecision = "approve" | "deny" | "always";
export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";
export type RouterSource = "rule" | "llm" | "local" | "override" | "fallback";
export type RunStatus = "finished" | "cancelled" | "error";
export type SettingSource = "user" | "project" | "local";
export type SkillSource = "user" | "project";
export type McpServerStatus = "starting" | "ready" | "failed" | "disabled";
export type StreamTextKind = "assistant" | "reasoning";
export type ToolCategory = "unknown" | "read" | "write" | "shell" | "mcp" | "other";

// ---------- Payloads: requests -------------------------------------------

export interface InitializeParams {
  protocolVersion: string;
  clientInfo: ClientInfo;
}

export interface ClientInfo {
  name: string;
  version: string;
}

export interface InitializeResult {
  protocolVersion: string;
  sidecarVersion: string;
  sdkVersion: string;
  nodeVersion: string;
  capabilities: Capabilities;
}

export interface Capabilities {
  streaming: boolean;
  cancel: boolean;
  resume: boolean;
  sandbox: boolean;
  mcp: boolean;
  skills: boolean;
  routerLlm: boolean;
  /**
   * SPEC-093: true when the underlying Cursor SDK is detected to retain
   * conversation history natively (e.g. exposes a `retainConversation`
   * option), which lets the sidecar disable the manual history
   * injection. Defaults to `false` for the 1.0.x SDK line.
   */
  nativeConversationRetention: boolean;
}

export interface ModelsListResult {
  models: ModelInfo[];
}

export interface ModelParameterValue {
  id: string;
  value: string;
}

/** Cursor SDK `ModelSelection` — model id plus optional params (effort, fast, …). */
export interface ModelSelection {
  id: string;
  params?: ModelParameterValue[];
}

export interface ModelParameterValueOption {
  value: string;
  displayName?: string;
}

export interface ModelParameterDefinition {
  id: string;
  displayName?: string;
  values: ModelParameterValueOption[];
}

export interface ModelInfo {
  id: string;
  displayName?: string;
  provider?: string;
  supportsThinking?: boolean;
  parameters?: ModelParameterDefinition[];
}

export interface SessionCreateParams {
  cwd: string;
  model?: string;
  approvalMode?: ApprovalMode;
  settingSources?: SettingSource[];
  mcpOverrides?: unknown;
  enabledSkillIds?: string[];
}

export interface SessionCreateResult {
  sessionId: string;
  agentId: string;
  model: string;
}

export interface SessionSendParams {
  sessionId: string;
  text: string;
  modelOverride?: ModelSelection;
}

export interface SessionSendResult {
  runId: string;
}

export interface SessionCancelParams {
  sessionId: string;
  runId: string;
}

export interface SessionResumeParams {
  agentId: string;
  cwd: string;
  approvalMode?: ApprovalMode;
  mcpOverrides?: unknown;
  enabledSkillIds?: string[];
}

export interface SessionResumeResult {
  sessionId: string;
  model?: string;
}

export interface SessionDisposeParams {
  sessionId: string;
}

export interface Ok {
  ok: boolean;
}

export interface SkillsListParams {
  cwd: string;
}

export interface SkillsListResult {
  skills: SkillInfo[];
  warnings?: string[];
}

export interface SkillInfo {
  id: string;
  name: string;
  description: string;
  path: string;
  sizeBytes: number;
  source: SkillSource;
}

export interface SkillsSetEnabledParams {
  sessionId: string;
  skillIds: string[];
}

export interface McpListParams {
  sessionId: string;
}

export interface McpListResult {
  servers: McpServerInfo[];
}

export interface McpServerInfo {
  id: string;
  transport: string;
  status: McpServerStatus;
  tools?: McpToolInfo[];
  enabled: boolean;
  error?: string;
}

export interface McpToolInfo {
  name: string;
  description?: string;
}

export interface McpToggleParams {
  sessionId: string;
  serverId: string;
  enabled: boolean;
}

export interface McpToggleResult {
  ok: boolean;
  pendingUntilNextTurn: boolean;
}

export interface ToolApprovalResponseParams {
  requestId: string;
  decision: ApprovalDecision;
}

export interface SessionSetApprovalModeParams {
  sessionId: string;
  mode: ApprovalMode;
}

export interface SessionSetApprovalModeResult {
  ok: boolean;
  /** True when the SDK could apply the mode live; false when only the
   *  sidecar's gating logic was updated (SDK cannot toggle sandbox live). */
  liveSdkUpdate: boolean;
}

/**
 * SPEC-092: force a specific conversation-context strategy on a session,
 * overriding the automatic byte-budget-driven raw/summary switcher until
 * the caller resets to `"auto"`.
 */
export type ContextStrategy = "auto" | "raw" | "summary";

export interface ContextSetStrategyParams {
  sessionId: string;
  strategy: ContextStrategy;
}

// ---------- Payloads: notifications --------------------------------------

export interface RouterDecisionParams {
  sessionId: string;
  runId: string;
  model: string;
  rationale: string;
  source: RouterSource;
}

export interface StreamMessageParams {
  runId: string;
  delta: string;
  kind?: StreamTextKind;
}

export interface StreamToolCallParams {
  runId: string;
  callId: string;
  name: string;
  args: unknown;
}

export interface StreamToolResultParams {
  runId: string;
  callId: string;
  ok: boolean;
  outputPreview?: string;
  error?: string;
}

export interface StreamUsageParams {
  runId: string;
  usage: TokenUsage;
}

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens?: number;
  cacheCreationTokens?: number;
  reasoningTokens?: number;
  totalTokens: number;
  byModel?: Record<string, TokenUsageDelta>;
}

export interface TokenUsageDelta {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
}

export interface ToolApprovalRequestParams {
  requestId: string;
  runId: string;
  name: string;
  args: unknown;
  category?: ToolCategory;
}

/**
 * Observational counterpart to `tool/approvalRequest`. Emitted when the
 * sidecar has already made an approval decision for a tool call (either
 * auto-approved or forced-prompt); the TUI uses this to reconcile its
 * gate overlay when the SDK exposes no synchronous interceptor to actually
 * block the call.
 */
export interface ToolApprovalResultParams {
  requestId: string;
  runId: string;
  name: string;
  decision: ApprovalResolution;
  /** Always true in slice-D; kept as a forward-compat flag. */
  observed: boolean;
}

export type ApprovalResolution = "auto-approve" | "prompt";

export interface RunFinishedParams {
  runId: string;
  status: RunStatus;
  usage: TokenUsage;
  model?: string;
  resultSummary?: string;
}

export interface RunErrorParams {
  runId: string;
  error: RpcError;
}

export interface LogParams {
  level: LogLevel;
  message: string;
  target?: string;
}

// ---------- Method → payload map (compile-time helper) -------------------

export interface RequestMap {
  [Method.Initialize]: { params: InitializeParams; result: InitializeResult };
  [Method.Shutdown]: { params: undefined; result: Ok };

  [Method.ModelsList]: { params: undefined; result: ModelsListResult };

  [Method.SessionCreate]: { params: SessionCreateParams; result: SessionCreateResult };
  [Method.SessionSend]: { params: SessionSendParams; result: SessionSendResult };
  [Method.SessionCancel]: { params: SessionCancelParams; result: Ok };
  [Method.SessionResume]: { params: SessionResumeParams; result: SessionResumeResult };
  [Method.SessionDispose]: { params: SessionDisposeParams; result: Ok };
  [Method.SessionSetApprovalMode]: {
    params: SessionSetApprovalModeParams;
    result: SessionSetApprovalModeResult;
  };

  [Method.SkillsList]: { params: SkillsListParams; result: SkillsListResult };
  [Method.SkillsSetEnabled]: { params: SkillsSetEnabledParams; result: Ok };

  [Method.McpList]: { params: McpListParams; result: McpListResult };
  [Method.McpToggle]: { params: McpToggleParams; result: McpToggleResult };

  [Method.ContextSetStrategy]: {
    params: ContextSetStrategyParams;
    result: Ok;
  };

  [Method.ToolApprovalResponse]: { params: ToolApprovalResponseParams; result: Ok };
}

export interface NotificationMap {
  [Method.RouterDecision]: RouterDecisionParams;
  [Method.StreamMessage]: StreamMessageParams;
  [Method.StreamToolCall]: StreamToolCallParams;
  [Method.StreamToolResult]: StreamToolResultParams;
  [Method.StreamUsage]: StreamUsageParams;
  [Method.ToolApprovalRequest]: ToolApprovalRequestParams;
  [Method.ToolApprovalResult]: ToolApprovalResultParams;
  [Method.RunFinished]: RunFinishedParams;
  [Method.RunError]: RunErrorParams;
  [Method.Log]: LogParams;
}
