// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Skills injection (SPEC-033, SPEC-034).
//
// `buildSkillContext` composes an XML-ish block from the enabled skills.
// Total byte budget is capped (default 16 KiB); once exceeded, remaining
// skills are dropped and a truncation warning is surfaced through the
// provided sink.

import type { DiscoveredSkill } from "./discover.js";

export const DEFAULT_INJECTION_BUDGET_BYTES = 16 * 1024;

export interface BuildContextOptions {
  budgetBytes?: number;
  onWarn?: (msg: string) => void;
}

/**
 * Build the `<skills>...</skills>` block that gets prepended to the
 * system context on every `agent.send()`. When the block would exceed
 * `budgetBytes`, later skills are dropped and `onWarn` is called once.
 */
export function buildSkillContext(
  enabled: readonly DiscoveredSkill[],
  opts: BuildContextOptions = {},
): string {
  if (enabled.length === 0) return "";
  const budget = opts.budgetBytes ?? DEFAULT_INJECTION_BUDGET_BYTES;
  const enc = new TextEncoder();

  const open = "<skills>\n";
  const close = "</skills>";
  const included: string[] = [];
  const dropped: string[] = [];
  let used = enc.encode(open).length + enc.encode(close).length;

  for (const skill of enabled) {
    const entry = renderSkill(skill);
    const size = enc.encode(entry).length;
    if (used + size > budget) {
      dropped.push(skill.id);
      continue;
    }
    included.push(entry);
    used += size;
  }

  if (dropped.length > 0) {
    opts.onWarn?.(
      `skill context truncated at ${budget} bytes; dropped: ${dropped.join(
        ", ",
      )}`,
    );
  }

  return `${open}${included.join("")}${close}`;
}

function renderSkill(s: DiscoveredSkill): string {
  const name = xmlEscape(s.name);
  const desc = xmlEscape(s.description);
  const body = xmlEscape(s.body);
  return `  <skill name="${name}"><description>${desc}</description><body>${body}</body></skill>\n`;
}

function xmlEscape(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
