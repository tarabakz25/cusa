// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Router — SPEC-010, SPEC-011, SPEC-013, SPEC-014, SPEC-015.
//
// Public surface used by SessionManager.sendMessage:
//   const router = new Router({...});
//   const decision = await router.route(ctx);
//
// Pipeline (spec §Router pipeline behavior):
//   1. sessionManualModel → source: "override".
//   2. First matching rule → source: "rule".
//   3. LLM classifier (bounded by llm_timeout_ms) → source: "llm".
//   4. Fallback to default_model → source: "fallback".

import { builtInDefaultConfig, loadRouterConfig } from "./config.js";
import { firstMatchingRule } from "./rules.js";
import { classifyWithTimeout, type RouterLlmClient } from "./llm.js";
import type { RouteContext, RouterConfig, RouterDecision } from "./types.js";

export type { RouteContext, RouterConfig, RouterDecision } from "./types.js";
export type { RouterLlmClient } from "./llm.js";

export interface RouterOptions {
  /** Initial parsed config. Defaults to `builtInDefaultConfig()`. */
  initialConfig?: RouterConfig;
  /** Optional LLM classifier client. Absent → LLM path is skipped. */
  llmClient?: RouterLlmClient;
  /** Log sink for hot-reload + fallback notices. */
  log?: (level: "info" | "warn" | "error", message: string) => void;
}

export class Router {
  private config: RouterConfig;
  private readonly log: (level: "info" | "warn" | "error", message: string) => void;
  private readonly llmClient?: RouterLlmClient;

  constructor(opts: RouterOptions = {}) {
    this.config = opts.initialConfig ?? builtInDefaultConfig();
    this.log = opts.log ?? (() => {});
    if (opts.llmClient !== undefined) {
      this.llmClient = opts.llmClient;
    }
  }

  /** Atomically replace the current config. Used by hot-reload. */
  updateConfig(next: RouterConfig): void {
    this.config = next;
  }

  currentConfig(): RouterConfig {
    return this.config;
  }

  /** Whether the LLM classifier path is available for this router. */
  llmAvailable(): boolean {
    return this.config.llmEnabled && this.llmClient !== undefined;
  }

  async route(ctx: RouteContext): Promise<RouterDecision> {
    // 1. Sticky manual override wins.
    if (ctx.sessionManualModel && ctx.sessionManualModel.length > 0) {
      return {
        model: ctx.sessionManualModel,
        rationale: "manual override",
        source: "override",
      };
    }

    // 2. Rules — first match wins.
    const rule = firstMatchingRule(ctx, this.config.rules);
    if (rule) {
      return {
        model: rule.model,
        rationale: rule.rationale,
        source: "rule",
      };
    }

    // 3. LLM classifier, bounded by llm_timeout_ms (SPEC-015).
    if (this.llmAvailable()) {
      const candidateModels = collectCandidateModels(this.config, ctx);
      const parsed = await classifyWithTimeout(this.llmClient!, {
        userPrompt: ctx.prompt,
        classifierModel: this.config.llmClassifierModel,
        candidateModels,
        timeoutMs: this.config.llmTimeoutMs,
      });
      if (parsed) {
        return {
          model: parsed.model,
          rationale: parsed.rationale,
          source: "llm",
        };
      }
      // Timeout / parse failure — fall through to default.
      const fallbackModel = ctx.currentModel ?? this.config.defaultModel;
      const rationale = "router-llm timeout; falling back";
      this.log("warn", rationale);
      return {
        model: fallbackModel,
        rationale,
        source: "fallback",
      };
    }

    // 4. No LLM available → default.
    return {
      model: ctx.currentModel ?? this.config.defaultModel,
      rationale: "no rule match",
      source: "fallback",
    };
  }
}

/**
 * Distinct candidate models we advertise to the classifier — every model
 * referenced by a rule, plus the default model, plus the current session
 * model (if any).
 */
function collectCandidateModels(
  cfg: RouterConfig,
  ctx: RouteContext,
): string[] {
  const set = new Set<string>();
  set.add(cfg.defaultModel);
  for (const r of cfg.rules) set.add(r.model);
  if (ctx.currentModel) set.add(ctx.currentModel);
  return Array.from(set);
}

/**
 * Convenience factory used at sidecar startup. Runs the async config
 * load, then constructs a Router. Callers usually swap in an LLM client
 * separately when the SDK adapter is available.
 */
export async function createRouterFromDisk(opts: {
  llmClient?: RouterLlmClient;
  log?: (level: "info" | "warn" | "error", message: string) => void;
  configPath?: string;
}): Promise<Router> {
  const loadOpts: Parameters<typeof loadRouterConfig>[0] = {};
  if (opts.log !== undefined) loadOpts.log = opts.log;
  if (opts.configPath !== undefined) loadOpts.configPath = opts.configPath;
  const loaded = await loadRouterConfig(loadOpts);
  const routerOpts: RouterOptions = { initialConfig: loaded.config };
  if (opts.llmClient !== undefined) routerOpts.llmClient = opts.llmClient;
  if (opts.log !== undefined) routerOpts.log = opts.log;
  return new Router(routerOpts);
}
