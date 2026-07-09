// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// JSON-RPC 2.0 schema shared between `cusa-tui` (Rust) and `cusa-sidecar`
// (Node.js/TypeScript). Types here are the source of truth; the TypeScript
// mirror in `sidecar/src/rpc/schema.ts` MUST be kept in sync manually.
//
// Envelope conventions:
// - Requests: { jsonrpc: "2.0", id: number|string, method, params }
// - Responses: { jsonrpc: "2.0", id, result | error }
// - Notifications: { jsonrpc: "2.0", method, params } (no id)
//
// This crate models the *payloads*. Framing over stdio uses newline-delimited
// JSON (LSP/Codex-style Content-Length headers are considered future work).

#![deny(missing_debug_implementations)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: &str = "0.1";

// ---------- Envelope ------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request<P> {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<P>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response<R> {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification<P> {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<P>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    Num(i64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl RpcError {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    // cusa-defined range: -32000 to -32099
    pub const SIDECAR_STARTUP: i64 = -32000;
    pub const AGENT_ERROR: i64 = -32001;
    pub const RUN_CANCELLED: i64 = -32002;
    pub const NO_API_KEY: i64 = -32003;
    pub const SDK_UNSUPPORTED: i64 = -32004;
}

// ---------- Method names --------------------------------------------------

pub mod method {
    // Requests
    pub const INITIALIZE: &str = "initialize";
    pub const SHUTDOWN: &str = "shutdown";

    pub const MODELS_LIST: &str = "models/list";

    pub const SESSION_CREATE: &str = "session/create";
    pub const SESSION_SEND: &str = "session/send";
    pub const SESSION_CANCEL: &str = "session/cancel";
    pub const SESSION_RESUME: &str = "session/resume";
    pub const SESSION_DISPOSE: &str = "session/dispose";
    pub const SESSION_SET_APPROVAL_MODE: &str = "session/setApprovalMode";

    pub const SKILLS_LIST: &str = "skills/list";
    pub const SKILLS_SET_ENABLED: &str = "skills/setEnabled";

    pub const MCP_LIST: &str = "mcp/list";
    pub const MCP_TOGGLE: &str = "mcp/toggle";

    pub const CONTEXT_SET_STRATEGY: &str = "context/setStrategy";

    pub const TOOL_APPROVAL_RESPONSE: &str = "tool/approvalResponse";

    // Notifications (server -> client)
    pub const ROUTER_DECISION: &str = "router/decision";
    pub const STREAM_MESSAGE: &str = "stream/message";
    pub const STREAM_TOOL_CALL: &str = "stream/toolCall";
    pub const STREAM_TOOL_RESULT: &str = "stream/toolResult";
    pub const STREAM_USAGE: &str = "stream/usage";
    pub const TOOL_APPROVAL_REQUEST: &str = "tool/approvalRequest";
    pub const TOOL_APPROVAL_RESULT: &str = "tool/approvalResult";
    pub const RUN_FINISHED: &str = "run/finished";
    pub const RUN_ERROR: &str = "run/error";
    pub const LOG: &str = "log";
}

// ---------- Domain enums --------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalMode {
    Suggest,
    AutoEdit,
    FullAuto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Approve,
    Deny,
    Always,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RouterSource {
    Rule,
    Llm,
    Override,
    Fallback,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Finished,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SettingSource {
    User,
    Project,
    Local,
}

// ---------- Payloads: requests -------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub sidecar_version: String,
    pub sdk_version: String,
    pub node_version: String,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub streaming: bool,
    pub cancel: bool,
    pub resume: bool,
    pub sandbox: bool,
    pub mcp: bool,
    pub skills: bool,
    pub router_llm: bool,
    /// SPEC-093: true when the Cursor SDK is detected to retain
    /// conversation history natively; sidecar disables manual injection
    /// when this is on. Optional in the wire format so older sidecars
    /// (pre-Phase E) that never sent the field still parse cleanly.
    #[serde(default)]
    pub native_conversation_retention: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsListResult {
    pub models: Vec<ModelInfo>,
}

/// A single model parameter value (Cursor SDK `ModelParameterValue`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelParameterValue {
    pub id: String,
    pub value: String,
}

/// Cursor SDK `ModelSelection` — model id plus optional per-model params
/// (e.g. reasoning effort, fast mode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ModelParameterValue>,
}

impl ModelSelection {
    pub fn id_only(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            params: Vec::new(),
        }
    }
}

/// One allowed value for a model parameter (from `models/list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelParameterValueOption {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Parameter metadata returned by `models/list` (effort, fast, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelParameterDefinition {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub values: Vec<ModelParameterValueOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default)]
    pub supports_thinking: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ModelParameterDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateParams {
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_mode: Option<ApprovalMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting_sources: Option<Vec<SettingSource>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_overrides: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_skill_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateResult {
    pub session_id: String,
    pub agent_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSendParams {
    pub session_id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_override: Option<ModelSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSendResult {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelParams {
    pub session_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResumeParams {
    pub agent_id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_mode: Option<ApprovalMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_overrides: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_skill_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResumeResult {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDisposeParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSetApprovalModeParams {
    pub session_id: String,
    pub mode: ApprovalMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSetApprovalModeResult {
    pub ok: bool,
    /// True when the SDK was able to apply the approval change live (e.g.
    /// toggle its sandbox). False when only the sidecar's gating logic
    /// was updated — the SDK currently exposes no live-toggle API.
    pub live_sdk_update: bool,
}

/// SPEC-092: force a specific conversation-context strategy on the
/// referenced session. `Auto` resets to the automatic byte-budget picker.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContextStrategy {
    Auto,
    Raw,
    Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSetStrategyParams {
    pub session_id: String,
    pub strategy: ContextStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ok {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsListParams {
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsListResult {
    pub skills: Vec<SkillInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub path: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub source: SkillSource,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    #[default]
    User,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsSetEnabledParams {
    pub session_id: String,
    pub skill_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListResult {
    pub servers: Vec<McpServerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInfo {
    pub id: String,
    pub transport: String,
    pub status: McpServerStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<McpToolInfo>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpServerStatus {
    Starting,
    Ready,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToggleParams {
    pub session_id: String,
    pub server_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToggleResult {
    pub ok: bool,
    pub pending_until_next_turn: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResponseParams {
    pub request_id: String,
    pub decision: ApprovalDecision,
}

// ---------- Payloads: notifications --------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterDecisionParams {
    pub session_id: String,
    pub run_id: String,
    pub model: String,
    pub rationale: String,
    pub source: RouterSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamMessageParams {
    pub run_id: String,
    pub delta: String,
    #[serde(default)]
    pub kind: StreamTextKind,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StreamTextKind {
    #[default]
    Assistant,
    Reasoning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamToolCallParams {
    pub run_id: String,
    pub call_id: String,
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamToolResultParams {
    pub run_id: String,
    pub call_id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamUsageParams {
    pub run_id: String,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_creation_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_model: BTreeMap<String, TokenUsageDelta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageDelta {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequestParams {
    pub request_id: String,
    pub run_id: String,
    pub name: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub category: ToolCategory,
}

/// Observational counterpart to `tool/approvalRequest`. Emitted when the
/// sidecar has already decided whether to auto-approve or prompt; the
/// TUI uses this to reconcile its overlay when the SDK exposes no
/// synchronous interceptor for tool calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResultParams {
    pub request_id: String,
    pub run_id: String,
    pub name: String,
    pub decision: ApprovalResolution,
    /// Always true in this schema revision; kept as a forward-compat flag
    /// for when the SDK grows a real interceptor.
    #[serde(default)]
    pub observed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalResolution {
    AutoApprove,
    Prompt,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolCategory {
    #[default]
    Unknown,
    Read,
    Write,
    Shell,
    Mcp,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedParams {
    pub run_id: String,
    pub status: RunStatus,
    pub usage: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunErrorParams {
    pub run_id: String,
    pub error: RpcError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogParams {
    pub level: LogLevel,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

// ---------- Enum tagging for the TUI's outgoing/incoming pumps ------------

/// Type-erased outgoing frame from the TUI to the sidecar.
///
/// This is a convenience enum that models the union of request bodies. The
/// sidecar mirror in TypeScript keeps a matching discriminated union.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "method", content = "params")]
pub enum ClientRequest {
    #[serde(rename = "initialize")]
    Initialize(InitializeParams),
    #[serde(rename = "shutdown")]
    Shutdown,
    #[serde(rename = "models/list")]
    ModelsList,
    #[serde(rename = "session/create")]
    SessionCreate(SessionCreateParams),
    #[serde(rename = "session/send")]
    SessionSend(SessionSendParams),
    #[serde(rename = "session/cancel")]
    SessionCancel(SessionCancelParams),
    #[serde(rename = "session/resume")]
    SessionResume(SessionResumeParams),
    #[serde(rename = "session/dispose")]
    SessionDispose(SessionDisposeParams),
    #[serde(rename = "session/setApprovalMode")]
    SessionSetApprovalMode(SessionSetApprovalModeParams),
    #[serde(rename = "skills/list")]
    SkillsList(SkillsListParams),
    #[serde(rename = "skills/setEnabled")]
    SkillsSetEnabled(SkillsSetEnabledParams),
    #[serde(rename = "mcp/list")]
    McpList(McpListParams),
    #[serde(rename = "mcp/toggle")]
    McpToggle(McpToggleParams),
    #[serde(rename = "context/setStrategy")]
    ContextSetStrategy(ContextSetStrategyParams),
    #[serde(rename = "tool/approvalResponse")]
    ToolApprovalResponse(ToolApprovalResponseParams),
}

/// Type-erased incoming notification from the sidecar to the TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ServerNotification {
    #[serde(rename = "router/decision")]
    RouterDecision(RouterDecisionParams),
    #[serde(rename = "stream/message")]
    StreamMessage(StreamMessageParams),
    #[serde(rename = "stream/toolCall")]
    StreamToolCall(StreamToolCallParams),
    #[serde(rename = "stream/toolResult")]
    StreamToolResult(StreamToolResultParams),
    #[serde(rename = "stream/usage")]
    StreamUsage(StreamUsageParams),
    #[serde(rename = "tool/approvalRequest")]
    ToolApprovalRequest(ToolApprovalRequestParams),
    #[serde(rename = "tool/approvalResult")]
    ToolApprovalResult(ToolApprovalResultParams),
    #[serde(rename = "run/finished")]
    RunFinished(RunFinishedParams),
    #[serde(rename = "run/error")]
    RunError(RunErrorParams),
    #[serde(rename = "log")]
    Log(LogParams),
}

// ---------- Tests ---------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- Round-trip helpers ---------------------------------------------

    fn rt<T: Serialize + for<'de> Deserialize<'de> + std::fmt::Debug + PartialEq>(v: &T) {
        let s = serde_json::to_string(v).expect("serialize");
        let back: T = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(&back, v, "round-trip mismatch for {s}");
    }

    fn rt_json<T: Serialize + for<'de> Deserialize<'de>>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).expect("to_value")
    }

    // ---- Protocol constants ---------------------------------------------

    #[test]
    fn protocol_version_stable() {
        assert_eq!(PROTOCOL_VERSION, "0.1");
    }

    #[test]
    fn error_codes_stable() {
        // JSON-RPC spec range
        assert_eq!(RpcError::PARSE_ERROR, -32700);
        assert_eq!(RpcError::INVALID_REQUEST, -32600);
        assert_eq!(RpcError::METHOD_NOT_FOUND, -32601);
        assert_eq!(RpcError::INVALID_PARAMS, -32602);
        assert_eq!(RpcError::INTERNAL_ERROR, -32603);
        // cusa-defined range
        assert_eq!(RpcError::SIDECAR_STARTUP, -32000);
        assert_eq!(RpcError::AGENT_ERROR, -32001);
        assert_eq!(RpcError::RUN_CANCELLED, -32002);
        assert_eq!(RpcError::NO_API_KEY, -32003);
        assert_eq!(RpcError::SDK_UNSUPPORTED, -32004);
    }

    #[test]
    fn method_constants_stable() {
        // Guard against accidental renames — TS mirror depends on these.
        assert_eq!(method::INITIALIZE, "initialize");
        assert_eq!(method::SHUTDOWN, "shutdown");
        assert_eq!(method::MODELS_LIST, "models/list");
        assert_eq!(method::SESSION_CREATE, "session/create");
        assert_eq!(method::SESSION_SEND, "session/send");
        assert_eq!(method::SESSION_CANCEL, "session/cancel");
        assert_eq!(method::SESSION_RESUME, "session/resume");
        assert_eq!(method::SESSION_DISPOSE, "session/dispose");
        assert_eq!(method::SKILLS_LIST, "skills/list");
        assert_eq!(method::SKILLS_SET_ENABLED, "skills/setEnabled");
        assert_eq!(method::MCP_LIST, "mcp/list");
        assert_eq!(method::MCP_TOGGLE, "mcp/toggle");
        assert_eq!(method::CONTEXT_SET_STRATEGY, "context/setStrategy");
        assert_eq!(method::TOOL_APPROVAL_RESPONSE, "tool/approvalResponse");
        assert_eq!(method::SESSION_SET_APPROVAL_MODE, "session/setApprovalMode");
        assert_eq!(method::TOOL_APPROVAL_RESULT, "tool/approvalResult");
        assert_eq!(method::ROUTER_DECISION, "router/decision");
        assert_eq!(method::STREAM_MESSAGE, "stream/message");
        assert_eq!(method::STREAM_TOOL_CALL, "stream/toolCall");
        assert_eq!(method::STREAM_TOOL_RESULT, "stream/toolResult");
        assert_eq!(method::STREAM_USAGE, "stream/usage");
        assert_eq!(method::TOOL_APPROVAL_REQUEST, "tool/approvalRequest");
        assert_eq!(method::RUN_FINISHED, "run/finished");
        assert_eq!(method::RUN_ERROR, "run/error");
        assert_eq!(method::LOG, "log");
    }

    // ---- Envelope --------------------------------------------------------

    #[test]
    fn request_id_untagged_variants() {
        let n = serde_json::to_string(&RequestId::Num(42)).unwrap();
        assert_eq!(n, "42");
        let s = serde_json::to_string(&RequestId::Str("abc".into())).unwrap();
        assert_eq!(s, "\"abc\"");

        let back_n: RequestId = serde_json::from_str("7").unwrap();
        assert_eq!(back_n, RequestId::Num(7));
        let back_s: RequestId = serde_json::from_str("\"x\"").unwrap();
        assert_eq!(back_s, RequestId::Str("x".into()));
    }

    #[test]
    fn request_envelope_omits_none_params() {
        let r: Request<serde_json::Value> = Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Num(1),
            method: method::SHUTDOWN.into(),
            params: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["method"], "shutdown");
        assert!(v.get("params").is_none(), "params must be omitted when None");
    }

    #[test]
    fn response_envelope_carries_result_or_error() {
        let ok: Response<Ok> = Response {
            jsonrpc: "2.0".into(),
            id: RequestId::Num(9),
            result: Some(Ok { ok: true }),
            error: None,
        };
        let v = serde_json::to_value(&ok).unwrap();
        assert_eq!(v["result"]["ok"], true);
        assert!(v.get("error").is_none());

        let err: Response<serde_json::Value> = Response {
            jsonrpc: "2.0".into(),
            id: RequestId::Str("x".into()),
            result: None,
            error: Some(RpcError {
                code: RpcError::AGENT_ERROR,
                message: "boom".into(),
                data: None,
            }),
        };
        let v = serde_json::to_value(&err).unwrap();
        assert!(v.get("result").is_none());
        assert_eq!(v["error"]["code"], -32001);
    }

    #[test]
    fn rpc_error_data_optional() {
        let e = RpcError {
            code: RpcError::INVALID_PARAMS,
            message: "no".into(),
            data: None,
        };
        let s = serde_json::to_string(&e).unwrap();
        assert!(!s.contains("data"));

        let with = RpcError {
            code: RpcError::AGENT_ERROR,
            message: "err".into(),
            data: Some(json!({ "hint": "check key" })),
        };
        let v = serde_json::to_value(&with).unwrap();
        assert_eq!(v["data"]["hint"], "check key");
    }

    // ---- Enum casing (contract with TS mirror) --------------------------

    #[test]
    fn approval_mode_kebab_case() {
        assert_eq!(serde_json::to_string(&ApprovalMode::Suggest).unwrap(), "\"suggest\"");
        assert_eq!(serde_json::to_string(&ApprovalMode::AutoEdit).unwrap(), "\"auto-edit\"");
        assert_eq!(serde_json::to_string(&ApprovalMode::FullAuto).unwrap(), "\"full-auto\"");
        let back: ApprovalMode = serde_json::from_str("\"auto-edit\"").unwrap();
        assert_eq!(back, ApprovalMode::AutoEdit);
    }

    #[test]
    fn approval_decision_lowercase() {
        assert_eq!(serde_json::to_string(&ApprovalDecision::Approve).unwrap(), "\"approve\"");
        assert_eq!(serde_json::to_string(&ApprovalDecision::Deny).unwrap(), "\"deny\"");
        assert_eq!(serde_json::to_string(&ApprovalDecision::Always).unwrap(), "\"always\"");
    }

    #[test]
    fn log_level_lowercase() {
        for (v, expected) in [
            (LogLevel::Trace, "\"trace\""),
            (LogLevel::Debug, "\"debug\""),
            (LogLevel::Info, "\"info\""),
            (LogLevel::Warn, "\"warn\""),
            (LogLevel::Error, "\"error\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), expected);
        }
    }

    #[test]
    fn router_source_camel_case() {
        assert_eq!(serde_json::to_string(&RouterSource::Rule).unwrap(), "\"rule\"");
        assert_eq!(serde_json::to_string(&RouterSource::Llm).unwrap(), "\"llm\"");
        assert_eq!(serde_json::to_string(&RouterSource::Override).unwrap(), "\"override\"");
        assert_eq!(serde_json::to_string(&RouterSource::Fallback).unwrap(), "\"fallback\"");
    }

    #[test]
    fn run_status_lowercase() {
        assert_eq!(serde_json::to_string(&RunStatus::Finished).unwrap(), "\"finished\"");
        assert_eq!(serde_json::to_string(&RunStatus::Cancelled).unwrap(), "\"cancelled\"");
        assert_eq!(serde_json::to_string(&RunStatus::Error).unwrap(), "\"error\"");
    }

    #[test]
    fn setting_source_camel_case() {
        assert_eq!(serde_json::to_string(&SettingSource::User).unwrap(), "\"user\"");
        assert_eq!(serde_json::to_string(&SettingSource::Project).unwrap(), "\"project\"");
        assert_eq!(serde_json::to_string(&SettingSource::Local).unwrap(), "\"local\"");
    }

    #[test]
    fn skill_source_default_is_user() {
        assert_eq!(SkillSource::default(), SkillSource::User);
        assert_eq!(serde_json::to_string(&SkillSource::User).unwrap(), "\"user\"");
        assert_eq!(serde_json::to_string(&SkillSource::Project).unwrap(), "\"project\"");
    }

    #[test]
    fn mcp_status_camel_case() {
        for (v, expected) in [
            (McpServerStatus::Starting, "\"starting\""),
            (McpServerStatus::Ready, "\"ready\""),
            (McpServerStatus::Failed, "\"failed\""),
            (McpServerStatus::Disabled, "\"disabled\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), expected);
        }
    }

    #[test]
    fn stream_text_kind_default_is_assistant() {
        assert_eq!(StreamTextKind::default(), StreamTextKind::Assistant);
        assert_eq!(
            serde_json::to_string(&StreamTextKind::Reasoning).unwrap(),
            "\"reasoning\""
        );
    }

    #[test]
    fn tool_category_default_is_unknown() {
        assert_eq!(ToolCategory::default(), ToolCategory::Unknown);
        for (v, expected) in [
            (ToolCategory::Unknown, "\"unknown\""),
            (ToolCategory::Read, "\"read\""),
            (ToolCategory::Write, "\"write\""),
            (ToolCategory::Shell, "\"shell\""),
            (ToolCategory::Mcp, "\"mcp\""),
            (ToolCategory::Other, "\"other\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), expected);
        }
    }

    // ---- Payload round-trips --------------------------------------------

    #[test]
    fn initialize_result_roundtrip() {
        let r = InitializeResult {
            protocol_version: PROTOCOL_VERSION.into(),
            sidecar_version: "0.0.1".into(),
            sdk_version: "1.0.23".into(),
            node_version: "20.11.0".into(),
            capabilities: Capabilities {
                streaming: true,
                cancel: true,
                resume: true,
                sandbox: false,
                mcp: true,
                skills: true,
                router_llm: false,
                native_conversation_retention: false,
            },
        };
        let v = rt_json(&r);
        // camelCase confirmation for the TS mirror
        assert_eq!(v["protocolVersion"], "0.1");
        assert_eq!(v["sdkVersion"], "1.0.23");
        assert_eq!(v["capabilities"]["routerLlm"], false);
        assert_eq!(v["capabilities"]["nativeConversationRetention"], false);
        let back: InitializeResult = serde_json::from_value(v).unwrap();
        assert_eq!(back.sidecar_version, "0.0.1");
        assert!(back.capabilities.streaming);
    }

    #[test]
    fn capabilities_default_all_false() {
        let c = Capabilities::default();
        assert!(!c.streaming);
        assert!(!c.cancel);
        assert!(!c.resume);
        assert!(!c.sandbox);
        assert!(!c.mcp);
        assert!(!c.skills);
        assert!(!c.router_llm);
        assert!(!c.native_conversation_retention);
    }

    #[test]
    fn capabilities_native_conversation_retention_is_optional_on_wire() {
        // Older sidecars that never emit the new field must still parse.
        let raw = json!({
            "streaming": true,
            "cancel": true,
            "resume": true,
            "sandbox": false,
            "mcp": true,
            "skills": true,
            "routerLlm": false
        });
        let c: Capabilities = serde_json::from_value(raw).unwrap();
        assert!(!c.native_conversation_retention);
    }

    #[test]
    fn context_set_strategy_lowercase_roundtrip() {
        let p = ContextSetStrategyParams {
            session_id: "s1".into(),
            strategy: ContextStrategy::Summary,
        };
        let v = rt_json(&p);
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["strategy"], "summary");
        for (s, expected) in [
            (ContextStrategy::Auto, "\"auto\""),
            (ContextStrategy::Raw, "\"raw\""),
            (ContextStrategy::Summary, "\"summary\""),
        ] {
            assert_eq!(serde_json::to_string(&s).unwrap(), expected);
        }
    }

    #[test]
    fn session_create_params_omits_optional_and_empty_skills() {
        let p = SessionCreateParams {
            cwd: "/tmp".into(),
            model: None,
            approval_mode: None,
            setting_sources: None,
            mcp_overrides: None,
            enabled_skill_ids: vec![],
        };
        let v = rt_json(&p);
        assert_eq!(v["cwd"], "/tmp");
        assert!(v.get("model").is_none());
        assert!(v.get("approvalMode").is_none());
        assert!(v.get("settingSources").is_none());
        assert!(v.get("mcpOverrides").is_none());
        assert!(v.get("enabledSkillIds").is_none(), "empty vec must be skipped");
    }

    #[test]
    fn session_create_params_includes_populated_fields() {
        let p = SessionCreateParams {
            cwd: "/repo".into(),
            model: Some("composer-2.5".into()),
            approval_mode: Some(ApprovalMode::AutoEdit),
            setting_sources: Some(vec![SettingSource::User, SettingSource::Project]),
            mcp_overrides: Some(json!({"servers": {}})),
            enabled_skill_ids: vec!["one".into(), "two".into()],
        };
        let v = rt_json(&p);
        assert_eq!(v["model"], "composer-2.5");
        assert_eq!(v["approvalMode"], "auto-edit");
        assert_eq!(v["settingSources"], json!(["user", "project"]));
        assert_eq!(v["enabledSkillIds"], json!(["one", "two"]));
    }

    #[test]
    fn session_send_params_roundtrip() {
        let p = SessionSendParams {
            session_id: "s1".into(),
            text: "hello".into(),
            model_override: Some(ModelSelection {
                id: "claude-sonnet-4".into(),
                params: vec![ModelParameterValue {
                    id: "effort".into(),
                    value: "high".into(),
                }],
            }),
        };
        let v = rt_json(&p);
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["modelOverride"]["id"], "claude-sonnet-4");
        assert_eq!(v["modelOverride"]["params"][0]["id"], "effort");
    }

    #[test]
    fn session_resume_result_optional_model() {
        let r = SessionResumeResult {
            session_id: "abc".into(),
            model: None,
        };
        let v = rt_json(&r);
        assert!(v.get("model").is_none());
    }

    #[test]
    fn models_list_result_supports_thinking_default_false() {
        let m: ModelInfo = serde_json::from_value(json!({ "id": "auto" })).unwrap();
        assert_eq!(m.id, "auto");
        assert!(!m.supports_thinking);
        assert!(m.display_name.is_none());
    }

    #[test]
    fn token_usage_accumulates_by_model() {
        let mut u = TokenUsage::default();
        u.input_tokens = 100;
        u.output_tokens = 250;
        u.total_tokens = 350;
        u.by_model.insert(
            "composer-2.5".into(),
            TokenUsageDelta {
                input_tokens: 100,
                output_tokens: 250,
                total_tokens: 350,
            },
        );
        let v = rt_json(&u);
        assert_eq!(v["totalTokens"], 350);
        assert_eq!(v["byModel"]["composer-2.5"]["totalTokens"], 350);
        rt(&u);
    }

    #[test]
    fn token_usage_omits_empty_by_model() {
        let u = TokenUsage::default();
        let v = rt_json(&u);
        assert!(v.get("byModel").is_none());
    }

    #[test]
    fn approval_resolution_kebab_case() {
        assert_eq!(
            serde_json::to_string(&ApprovalResolution::AutoApprove).unwrap(),
            "\"auto-approve\""
        );
        assert_eq!(
            serde_json::to_string(&ApprovalResolution::Prompt).unwrap(),
            "\"prompt\""
        );
        let back: ApprovalResolution = serde_json::from_str("\"auto-approve\"").unwrap();
        assert_eq!(back, ApprovalResolution::AutoApprove);
    }

    #[test]
    fn session_set_approval_mode_roundtrip() {
        let p = SessionSetApprovalModeParams {
            session_id: "s1".into(),
            mode: ApprovalMode::FullAuto,
        };
        let v = rt_json(&p);
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["mode"], "full-auto");

        let r = SessionSetApprovalModeResult {
            ok: true,
            live_sdk_update: false,
        };
        let v = rt_json(&r);
        assert_eq!(v["liveSdkUpdate"], false);
    }

    #[test]
    fn tool_approval_result_roundtrip() {
        let p = ToolApprovalResultParams {
            request_id: "req-1".into(),
            run_id: "run-1".into(),
            name: "write".into(),
            decision: ApprovalResolution::Prompt,
            observed: true,
        };
        let v = rt_json(&p);
        assert_eq!(v["requestId"], "req-1");
        assert_eq!(v["decision"], "prompt");
        assert_eq!(v["observed"], true);
    }

    #[test]
    fn tool_approval_request_default_category() {
        let raw = json!({
            "requestId": "req-1",
            "runId": "run-1",
            "name": "shell_exec",
            "args": { "cmd": "ls" }
        });
        let p: ToolApprovalRequestParams = serde_json::from_value(raw).unwrap();
        assert_eq!(p.category, ToolCategory::Unknown);
    }

    #[test]
    fn skills_list_result_omits_empty_warnings() {
        let r = SkillsListResult {
            skills: vec![],
            warnings: vec![],
        };
        let v = rt_json(&r);
        assert!(v.get("warnings").is_none());
        assert_eq!(v["skills"], json!([]));
    }

    #[test]
    fn skill_info_roundtrip_with_defaults() {
        let raw = json!({
            "id": "foo",
            "name": "Foo",
            "path": "/tmp/foo/SKILL.md"
        });
        let s: SkillInfo = serde_json::from_value(raw).unwrap();
        assert_eq!(s.description, "");
        assert_eq!(s.size_bytes, 0);
        assert_eq!(s.source, SkillSource::User);
    }

    #[test]
    fn mcp_server_info_defaults() {
        let raw = json!({
            "id": "fs",
            "transport": "stdio",
            "status": "ready"
        });
        let s: McpServerInfo = serde_json::from_value(raw).unwrap();
        assert!(s.tools.is_empty());
        assert!(!s.enabled);
        assert!(s.error.is_none());
    }

    #[test]
    fn mcp_toggle_result_roundtrip() {
        let r = McpToggleResult {
            ok: true,
            pending_until_next_turn: true,
        };
        let v = rt_json(&r);
        assert_eq!(v["ok"], true);
        assert_eq!(v["pendingUntilNextTurn"], true);
    }

    #[test]
    fn router_decision_params_camel_case() {
        let p = RouterDecisionParams {
            session_id: "s1".into(),
            run_id: "r1".into(),
            model: "composer-2.5".into(),
            rationale: "fast rule: explain code".into(),
            source: RouterSource::Rule,
        };
        let v = rt_json(&p);
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["runId"], "r1");
        assert_eq!(v["source"], "rule");
    }

    #[test]
    fn stream_message_params_default_kind_assistant() {
        let raw = json!({ "runId": "r1", "delta": "hi" });
        let p: StreamMessageParams = serde_json::from_value(raw).unwrap();
        assert_eq!(p.kind, StreamTextKind::Assistant);
    }

    #[test]
    fn run_finished_params_omits_none_fields() {
        let p = RunFinishedParams {
            run_id: "r1".into(),
            status: RunStatus::Finished,
            usage: TokenUsage::default(),
            model: None,
            result_summary: None,
        };
        let v = rt_json(&p);
        assert!(v.get("model").is_none());
        assert!(v.get("resultSummary").is_none());
    }

    #[test]
    fn stream_tool_result_optional_preview_and_error() {
        let p = StreamToolResultParams {
            run_id: "r1".into(),
            call_id: "c1".into(),
            ok: false,
            output_preview: None,
            error: Some("nope".into()),
        };
        let v = rt_json(&p);
        assert!(v.get("outputPreview").is_none());
        assert_eq!(v["error"], "nope");
    }

    #[test]
    fn log_params_optional_target() {
        let p = LogParams {
            level: LogLevel::Info,
            message: "hi".into(),
            target: None,
        };
        let v = rt_json(&p);
        assert!(v.get("target").is_none());
    }

    // ---- Tagged union serialization -------------------------------------

    #[test]
    fn client_request_shutdown_serializes_as_bare_method() {
        let r = ClientRequest::Shutdown;
        let s = serde_json::to_string(&r).unwrap();
        // Unit variant: `{ "method": "shutdown" }`; content is omitted.
        assert!(s.contains("\"method\":\"shutdown\""));
    }

    #[test]
    fn client_request_session_create_tagged() {
        let r = ClientRequest::SessionCreate(SessionCreateParams {
            cwd: "/tmp".into(),
            model: Some("composer-2.5".into()),
            approval_mode: Some(ApprovalMode::Suggest),
            setting_sources: None,
            mcp_overrides: None,
            enabled_skill_ids: vec![],
        });
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["method"], "session/create");
        assert_eq!(v["params"]["cwd"], "/tmp");
        assert_eq!(v["params"]["approvalMode"], "suggest");
    }

    #[test]
    fn server_notification_all_methods_tagged() {
        let cases: Vec<(ServerNotification, &str)> = vec![
            (
                ServerNotification::RouterDecision(RouterDecisionParams {
                    session_id: "s".into(),
                    run_id: "r".into(),
                    model: "m".into(),
                    rationale: "why".into(),
                    source: RouterSource::Override,
                }),
                "router/decision",
            ),
            (
                ServerNotification::StreamMessage(StreamMessageParams {
                    run_id: "r".into(),
                    delta: "d".into(),
                    kind: StreamTextKind::Reasoning,
                }),
                "stream/message",
            ),
            (
                ServerNotification::StreamToolCall(StreamToolCallParams {
                    run_id: "r".into(),
                    call_id: "c".into(),
                    name: "n".into(),
                    args: json!({}),
                }),
                "stream/toolCall",
            ),
            (
                ServerNotification::StreamToolResult(StreamToolResultParams {
                    run_id: "r".into(),
                    call_id: "c".into(),
                    ok: true,
                    output_preview: Some("ok".into()),
                    error: None,
                }),
                "stream/toolResult",
            ),
            (
                ServerNotification::StreamUsage(StreamUsageParams {
                    run_id: "r".into(),
                    usage: TokenUsage::default(),
                }),
                "stream/usage",
            ),
            (
                ServerNotification::ToolApprovalRequest(ToolApprovalRequestParams {
                    request_id: "req".into(),
                    run_id: "r".into(),
                    name: "n".into(),
                    args: json!({}),
                    category: ToolCategory::Write,
                }),
                "tool/approvalRequest",
            ),
            (
                ServerNotification::ToolApprovalResult(ToolApprovalResultParams {
                    request_id: "req".into(),
                    run_id: "r".into(),
                    name: "n".into(),
                    decision: ApprovalResolution::Prompt,
                    observed: true,
                }),
                "tool/approvalResult",
            ),
            (
                ServerNotification::RunFinished(RunFinishedParams {
                    run_id: "r".into(),
                    status: RunStatus::Cancelled,
                    usage: TokenUsage::default(),
                    model: None,
                    result_summary: None,
                }),
                "run/finished",
            ),
            (
                ServerNotification::RunError(RunErrorParams {
                    run_id: "r".into(),
                    error: RpcError {
                        code: RpcError::AGENT_ERROR,
                        message: "boom".into(),
                        data: None,
                    },
                }),
                "run/error",
            ),
            (
                ServerNotification::Log(LogParams {
                    level: LogLevel::Warn,
                    message: "!".into(),
                    target: Some("router".into()),
                }),
                "log",
            ),
        ];

        for (n, expected) in cases {
            let v = serde_json::to_value(&n).unwrap();
            assert_eq!(v["method"], expected, "wrong tag for {expected}");
            assert!(v.get("params").is_some(), "params missing for {expected}");
        }
    }

    #[test]
    fn client_request_all_tag_names_match_method_constants() {
        // Every variant name maps to a method-name constant. The
        // discriminants encoded here must match the `method` module.
        let cases: Vec<(ClientRequest, &str)> = vec![
            (
                ClientRequest::Initialize(InitializeParams {
                    protocol_version: PROTOCOL_VERSION.into(),
                    client_info: ClientInfo {
                        name: "cusa-tui".into(),
                        version: "0.0.1".into(),
                    },
                }),
                method::INITIALIZE,
            ),
            (ClientRequest::Shutdown, method::SHUTDOWN),
            (ClientRequest::ModelsList, method::MODELS_LIST),
            (
                ClientRequest::SessionCreate(SessionCreateParams {
                    cwd: "/x".into(),
                    model: None,
                    approval_mode: None,
                    setting_sources: None,
                    mcp_overrides: None,
                    enabled_skill_ids: vec![],
                }),
                method::SESSION_CREATE,
            ),
            (
                ClientRequest::SessionSend(SessionSendParams {
                    session_id: "s".into(),
                    text: "t".into(),
                    model_override: None,
                }),
                method::SESSION_SEND,
            ),
            (
                ClientRequest::SessionCancel(SessionCancelParams {
                    session_id: "s".into(),
                    run_id: "r".into(),
                }),
                method::SESSION_CANCEL,
            ),
            (
                ClientRequest::SessionResume(SessionResumeParams {
                    agent_id: "a".into(),
                    cwd: "/x".into(),
                    approval_mode: None,
                    mcp_overrides: None,
                    enabled_skill_ids: vec![],
                }),
                method::SESSION_RESUME,
            ),
            (
                ClientRequest::SessionDispose(SessionDisposeParams {
                    session_id: "s".into(),
                }),
                method::SESSION_DISPOSE,
            ),
            (
                ClientRequest::SessionSetApprovalMode(SessionSetApprovalModeParams {
                    session_id: "s".into(),
                    mode: ApprovalMode::AutoEdit,
                }),
                method::SESSION_SET_APPROVAL_MODE,
            ),
            (
                ClientRequest::SkillsList(SkillsListParams { cwd: "/x".into() }),
                method::SKILLS_LIST,
            ),
            (
                ClientRequest::SkillsSetEnabled(SkillsSetEnabledParams {
                    session_id: "s".into(),
                    skill_ids: vec![],
                }),
                method::SKILLS_SET_ENABLED,
            ),
            (
                ClientRequest::McpList(McpListParams {
                    session_id: "s".into(),
                }),
                method::MCP_LIST,
            ),
            (
                ClientRequest::McpToggle(McpToggleParams {
                    session_id: "s".into(),
                    server_id: "srv".into(),
                    enabled: true,
                }),
                method::MCP_TOGGLE,
            ),
            (
                ClientRequest::ContextSetStrategy(ContextSetStrategyParams {
                    session_id: "s".into(),
                    strategy: ContextStrategy::Raw,
                }),
                method::CONTEXT_SET_STRATEGY,
            ),
            (
                ClientRequest::ToolApprovalResponse(ToolApprovalResponseParams {
                    request_id: "req".into(),
                    decision: ApprovalDecision::Approve,
                }),
                method::TOOL_APPROVAL_RESPONSE,
            ),
        ];

        for (r, expected) in cases {
            let v = serde_json::to_value(&r).unwrap();
            assert_eq!(v["method"], expected, "wrong tag: {expected}");
        }
    }

    // ---- Notification framing (real-world payloads) ---------------------

    #[test]
    fn notification_frame_round_trip() {
        let n: Notification<StreamMessageParams> = Notification {
            jsonrpc: "2.0".into(),
            method: method::STREAM_MESSAGE.into(),
            params: Some(StreamMessageParams {
                run_id: "r1".into(),
                delta: "hi".into(),
                kind: StreamTextKind::Assistant,
            }),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"method\":\"stream/message\""));
        let back: Notification<StreamMessageParams> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.params.unwrap().delta, "hi");
    }
}
