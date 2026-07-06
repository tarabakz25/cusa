// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Approval-mode → decision matrix. Slice-1 exposes a pure function that
// classifies whether a given (mode, category) combination requires a user
// prompt or is auto-approved. Slice-3 will consume this from an actual
// gating loop; here we only make the decision.

import type { ApprovalMode, ToolCategory } from "../rpc/schema.js";

export type PolicyDecision = "auto-approve" | "prompt";

export interface ApprovalPolicyInput {
  mode: ApprovalMode;
  toolName: string;
  category: ToolCategory;
}

/**
 * Returns `"auto-approve"` when the tool call should run without prompting,
 * `"prompt"` when the TUI needs to gate the call via `tool/approvalRequest`.
 *
 * Semantics (mirrors spec §FR-3):
 * - `full-auto`: everything auto-approved (sandbox is enabled elsewhere).
 * - `auto-edit`: read tools auto-approved; write & shell prompt.
 * - `suggest`:  every non-read tool prompts.
 */
export function approvalPolicy(input: ApprovalPolicyInput): PolicyDecision {
  const { mode, category } = input;
  if (mode === "full-auto") return "auto-approve";
  if (category === "read") return "auto-approve";
  if (category === "write" || category === "shell") return "prompt";
  if (mode === "auto-edit") {
    // Unknown/mcp/other tools in auto-edit stay quiet unless clearly
    // side-effecting. Keep the safe default in suggest.
    return category === "mcp" ? "auto-approve" : "auto-approve";
  }
  // suggest mode default: prompt everything else.
  return "prompt";
}

/**
 * Whether the caller should enable the SDK sandbox on agent creation.
 * SPEC-024 (partial): `full-auto` mode couples to `sandboxOptions.enabled`.
 */
export function shouldEnableSandbox(mode: ApprovalMode): boolean {
  return mode === "full-auto";
}
