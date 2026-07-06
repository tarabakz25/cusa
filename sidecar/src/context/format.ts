// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Rendering helpers for the local-context workaround (SPEC-090..092).
//
// The Cursor local-agent runtime discards inter-turn conversation
// history (Risk R-1). The sidecar prepends a compact XML-tagged replay
// or summary to the system context so the model can distinguish
// previous turns from the fresh prompt.
//
// Two rendering shapes are exposed:
//   - `renderRaw(turns)`               → last-N-turns replay (SPEC-090).
//   - `renderSummary(summary, tail)`   → rolling summary + last-2 turns
//                                        raw (SPEC-091).

export interface ConversationTurn {
  /** Monotonically increasing per-session. Used by summarizer caching. */
  readonly index: number;
  /** User prompt that started the turn. */
  readonly userPrompt: string;
  /** Assistant text (concatenated deltas), post-turn. */
  readonly assistantText: string;
  /** Human-readable summary of tool calls: `["wrote /a (12 lines)", ...]`. */
  readonly toolCallsSummary: readonly string[];
  /** Model id the assistant answered with, when known. */
  readonly model?: string;
}

/**
 * Escape the four XML metachars we emit. We do NOT round-trip through
 * a real XML parser — the model just needs to see the structure — so
 * the escape rules stay minimal.
 */
export function xmlEscape(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * SPEC-090: render the last-N-turns replay as an XML-tagged block. Each
 * turn contributes one `<turn role="user">`, one `<turn role="assistant"
 * model="...">`, and one `<turn role="tool" name="...">...</turn>` per
 * tool-call summary line.
 *
 * Empty input → empty string (caller decides whether to omit entirely
 * or keep the surrounding wrapper).
 */
export function renderRaw(turns: readonly ConversationTurn[]): string {
  if (turns.length === 0) return "";
  const lines: string[] = ["<conversation>"];
  for (const turn of turns) {
    lines.push(`  <turn role="user">${xmlEscape(turn.userPrompt)}</turn>`);
    if (turn.assistantText.length > 0) {
      const modelAttr = turn.model
        ? ` model="${xmlEscape(turn.model)}"`
        : "";
      lines.push(
        `  <turn role="assistant"${modelAttr}>${xmlEscape(
          turn.assistantText,
        )}</turn>`,
      );
    }
    for (const summary of turn.toolCallsSummary) {
      const parsed = parseToolSummary(summary);
      const nameAttr = parsed.name.length > 0
        ? ` name="${xmlEscape(parsed.name)}"`
        : "";
      lines.push(
        `  <turn role="tool"${nameAttr}>${xmlEscape(parsed.body)}</turn>`,
      );
    }
  }
  lines.push("</conversation>");
  return lines.join("\n");
}

/**
 * SPEC-091: render the summary block ("older turns collapsed") plus the
 * raw tail (default: last 2 turns) as a single `<conversation>` block.
 * The summary is emitted as a `<summary>...</summary>` element followed
 * by the raw tail turns in normal order.
 */
export function renderSummary(
  summary: string,
  tail: readonly ConversationTurn[],
): string {
  const cleanSummary = summary.trim();
  if (cleanSummary.length === 0 && tail.length === 0) return "";
  const lines: string[] = ["<conversation>"];
  if (cleanSummary.length > 0) {
    lines.push(`  <summary>${xmlEscape(cleanSummary)}</summary>`);
  }
  for (const turn of tail) {
    lines.push(`  <turn role="user">${xmlEscape(turn.userPrompt)}</turn>`);
    if (turn.assistantText.length > 0) {
      const modelAttr = turn.model
        ? ` model="${xmlEscape(turn.model)}"`
        : "";
      lines.push(
        `  <turn role="assistant"${modelAttr}>${xmlEscape(
          turn.assistantText,
        )}</turn>`,
      );
    }
    for (const s of turn.toolCallsSummary) {
      const parsed = parseToolSummary(s);
      const nameAttr = parsed.name.length > 0
        ? ` name="${xmlEscape(parsed.name)}"`
        : "";
      lines.push(
        `  <turn role="tool"${nameAttr}>${xmlEscape(parsed.body)}</turn>`,
      );
    }
  }
  lines.push("</conversation>");
  return lines.join("\n");
}

/**
 * Total UTF-8 byte cost of the raw rendering — used by the strategy
 * picker to decide between raw and summary without a full render pass.
 * We approximate by rendering, since the XML boilerplate is small and
 * accurate size matters more than saving a few micros.
 */
export function rawRenderByteSize(turns: readonly ConversationTurn[]): number {
  if (turns.length === 0) return 0;
  return new TextEncoder().encode(renderRaw(turns)).length;
}

/**
 * Parse a tool-summary line into a `name` and a `body`. The stored
 * format is loosely structured — callers typically produce entries like
 * `"write wrote /path (12 lines)"` — so we take the first token as the
 * tool name and the rest as the body. Lines that don't start with a
 * recognisable identifier are emitted with an empty name.
 */
function parseToolSummary(line: string): { name: string; body: string } {
  const trimmed = line.trim();
  if (trimmed.length === 0) return { name: "", body: "" };
  const m = /^([A-Za-z_][A-Za-z0-9_-]*)\s+(.*)$/.exec(trimmed);
  if (!m) return { name: "", body: trimmed };
  return { name: m[1]!, body: m[2] ?? "" };
}
