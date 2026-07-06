// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Skills discovery (SPEC-030, SPEC-031).
//
// Walks two roots recursively — `~/.cursor/skills/` (user) and
// `<cwd>/.cursor/skills/` (project) — collecting every `SKILL.md` and
// parsing its YAML front matter. Same-id skills prefer the project
// version. `id` is the POSIX-normalised relative directory path.

import { readdir, readFile, stat } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";

import type { SkillSource } from "../rpc/schema.js";

export interface DiscoveredSkill {
  id: string;
  name: string;
  description: string;
  path: string;
  sizeBytes: number;
  source: SkillSource;
  body: string;
}

export interface DiscoverOptions {
  cwd: string;
  userRoot?: string;
  projectRoot?: string;
  /** Emit non-fatal warnings (e.g. malformed frontmatter). */
  onWarn?: (msg: string) => void;
}

export interface DiscoverResult {
  skills: DiscoveredSkill[];
  warnings: string[];
}

/**
 * Discover skills from user + project roots. Roots that don't exist are
 * silently skipped.
 */
export async function discoverSkills(
  opts: DiscoverOptions,
): Promise<DiscoverResult> {
  const warnings: string[] = [];
  const emitWarn = (msg: string) => {
    warnings.push(msg);
    opts.onWarn?.(msg);
  };
  const userRoot = opts.userRoot ?? path.join(homedir(), ".cursor", "skills");
  const projectRoot =
    opts.projectRoot ?? path.join(opts.cwd, ".cursor", "skills");

  const byId = new Map<string, DiscoveredSkill>();
  for (const [root, source] of [
    [userRoot, "user"] as const,
    [projectRoot, "project"] as const,
  ]) {
    const found = await walkRoot(root, source, emitWarn);
    for (const s of found) {
      // Project skills win over same-id user skills.
      if (source === "project" || !byId.has(s.id)) {
        byId.set(s.id, s);
      }
    }
  }
  const skills = [...byId.values()].sort((a, b) => a.id.localeCompare(b.id));
  return { skills, warnings };
}

async function walkRoot(
  root: string,
  source: SkillSource,
  emitWarn: (msg: string) => void,
): Promise<DiscoveredSkill[]> {
  let entries: string[] = [];
  try {
    entries = await recursiveList(root);
  } catch {
    return [];
  }
  const out: DiscoveredSkill[] = [];
  for (const filePath of entries) {
    if (path.basename(filePath) !== "SKILL.md") continue;
    let text: string;
    let sizeBytes: number;
    try {
      const s = await stat(filePath);
      sizeBytes = s.size;
      text = await readFile(filePath, "utf8");
    } catch (err) {
      emitWarn(`could not read ${filePath}: ${(err as Error).message}`);
      continue;
    }
    const parsed = parseSkillFile(text);
    if (!parsed) {
      emitWarn(`${filePath}: missing or malformed frontmatter; skipping`);
      continue;
    }
    const relDir = path.relative(root, path.dirname(filePath));
    const id = relDir.split(path.sep).filter(Boolean).join("/") || parsed.name;
    out.push({
      id,
      name: parsed.name,
      description: parsed.description,
      path: filePath,
      sizeBytes,
      source,
      body: parsed.body,
    });
  }
  return out;
}

async function recursiveList(root: string): Promise<string[]> {
  const out: string[] = [];
  const stack: string[] = [root];
  while (stack.length > 0) {
    const dir = stack.pop()!;
    let entries: import("node:fs").Dirent[];
    try {
      entries = await readdir(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const e of entries) {
      const full = path.join(dir, e.name);
      if (e.isDirectory()) {
        stack.push(full);
      } else if (e.isFile()) {
        out.push(full);
      }
    }
  }
  return out;
}

// ---------- Frontmatter parser (YAML-subset) -----------------------------

export interface ParsedSkill {
  name: string;
  description: string;
  body: string;
}

/**
 * Parse a SKILL.md file. Accepts leading YAML frontmatter delimited by
 * `---` lines with flat `key: value` entries. Everything after the
 * closing `---` is the body. Returns `null` on missing/malformed
 * frontmatter.
 *
 * We deliberately do not depend on `js-yaml`; the spec explicitly notes
 * "no external deps. ... Any deeper YAML → skip with a warning".
 */
export function parseSkillFile(text: string): ParsedSkill | null {
  const norm = text.replace(/^\uFEFF/, "");
  const lines = norm.split(/\r?\n/);
  let i = 0;
  // Optional leading blank lines.
  while (i < lines.length && lines[i]!.trim() === "") i++;
  if (i >= lines.length || lines[i]!.trim() !== "---") return null;
  i++;
  const fm: Record<string, string> = {};
  while (i < lines.length && lines[i]!.trim() !== "---") {
    const raw = lines[i]!;
    if (raw.trim().length === 0) {
      i++;
      continue;
    }
    // Only flat `key: value` lines. Anything else → malformed.
    const m = /^([A-Za-z_][\w-]*)\s*:\s*(.*)$/.exec(raw);
    if (!m) return null;
    const key = m[1]!.trim();
    let value = m[2]!.trim();
    // Trim surrounding quotes on scalar values.
    if (
      (value.startsWith('"') && value.endsWith('"') && value.length >= 2) ||
      (value.startsWith("'") && value.endsWith("'") && value.length >= 2)
    ) {
      value = value.slice(1, -1);
    }
    fm[key] = value;
    i++;
  }
  if (i >= lines.length) return null; // no closing ---
  const body = lines.slice(i + 1).join("\n").trim();
  const name = fm["name"];
  if (!name) return null;
  return {
    name,
    description: fm["description"] ?? "",
    body,
  };
}
