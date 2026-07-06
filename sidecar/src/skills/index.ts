// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Public API for the skills subsystem, used by SessionManager. Owns a
// small in-memory cache keyed by `cwd` so consecutive turns don't
// re-walk the filesystem.

import { discoverSkills, type DiscoveredSkill } from "./discover.js";
import { buildSkillContext } from "./inject.js";

export {
  discoverSkills,
  parseSkillFile,
  type DiscoveredSkill,
  type DiscoverOptions,
  type DiscoverResult,
} from "./discover.js";
export {
  buildSkillContext,
  DEFAULT_INJECTION_BUDGET_BYTES,
  type BuildContextOptions,
} from "./inject.js";

export interface SkillsManagerOptions {
  log?: (level: "info" | "warn" | "error", msg: string) => void;
}

interface CacheEntry {
  skills: DiscoveredSkill[];
  warnings: string[];
  ts: number;
}

const CACHE_TTL_MS = 5_000; // walk again if last discovery was > 5 s ago

export class SkillsManager {
  private readonly log: (level: "info" | "warn" | "error", msg: string) => void;
  private readonly cache = new Map<string, CacheEntry>();

  constructor(opts: SkillsManagerOptions = {}) {
    this.log = opts.log ?? (() => {});
  }

  async list(
    cwd: string,
  ): Promise<{ skills: DiscoveredSkill[]; warnings: string[] }> {
    const cached = this.cache.get(cwd);
    if (cached && Date.now() - cached.ts < CACHE_TTL_MS) {
      return { skills: cached.skills, warnings: cached.warnings };
    }
    const result = await discoverSkills({
      cwd,
      onWarn: (m) => this.log("warn", `skills: ${m}`),
    });
    this.cache.set(cwd, {
      skills: result.skills,
      warnings: result.warnings,
      ts: Date.now(),
    });
    return { skills: result.skills, warnings: result.warnings };
  }

  /**
   * Compose the skill context block for a given set of enabled ids.
   * Skills that don't resolve to a discovered entry are silently
   * skipped (they may have been deleted since the last discovery).
   */
  async buildContextFor(args: {
    cwd: string;
    enabledIds: readonly string[];
    onWarn?: (msg: string) => void;
    budgetBytes?: number;
  }): Promise<string> {
    if (args.enabledIds.length === 0) return "";
    const { skills } = await this.list(args.cwd);
    const byId = new Map(skills.map((s) => [s.id, s]));
    const enabled: DiscoveredSkill[] = [];
    for (const id of args.enabledIds) {
      const s = byId.get(id);
      if (s) enabled.push(s);
    }
    const injectionOpts: import("./inject.js").BuildContextOptions = {};
    if (args.onWarn !== undefined) injectionOpts.onWarn = args.onWarn;
    if (args.budgetBytes !== undefined) {
      injectionOpts.budgetBytes = args.budgetBytes;
    }
    return buildSkillContext(enabled, injectionOpts);
  }
}
