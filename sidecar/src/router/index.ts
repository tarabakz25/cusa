// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Router — SPEC-010, SPEC-011, SPEC-013, SPEC-014, SPEC-015 + issue #7
// (Super Auto Mode).
//
// Public surface used by SessionManager.sendMessage:
//   const router = new Router({...});
//   const decision = await router.route(ctx);
//
// Pipeline, "auto" mode (legacy, spec §Router pipeline behavior):
//   1. sessionManualModel → source: "override".
//   2. First matching rule → source: "rule".
//   3. LLM classifier (bounded by llm_timeout_ms) → source: "llm".
//   4. Fallback to default_model → source: "fallback".
//
// Pipeline, "super-auto" mode (issue #7; enabled via
// `local_classifier_enabled = true` or `ctx.routerMode`):
//   1. sessionManualModel → "override" (allowlist-exempt; warn only).
//   2. First matching rule → "rule".
//   3. Structural gate (code fence / stack trace / very short) → "rule".
//   4. Local semantic classifier: cosine ≥ θ_high → "local".
//   5. θ_low ≤ cosine < θ_high (ambiguous band ONLY) → cloud LLM → "llm".
//   6. Fallback to default_model → "fallback".
//   Non-override decisions are then resolved to the newest concrete id
//   in the provider-allowlisted catalog (roles A/B in the issue; the
//   embedder — role C — stays pinned).

import { builtInDefaultConfig, loadRouterConfig } from "./config.js";
import { firstMatchingRule, structuralGate } from "./rules.js";
import { classifyWithTimeout, type RouterLlmClient } from "./llm.js";
import {
  LocalEmbeddingClassifier,
  builtInDefaultExemplars,
  createEmbedder,
} from "./localClassifier.js";
import { filterCatalog, providerOf, resolveLatestModel } from "./modelResolver.js";
import type {
  RouteContext,
  RouterConfig,
  RouterDecision,
  RouterMode,
} from "./types.js";

export type {
  RouteContext,
  RouterConfig,
  RouterDecision,
  RouterMode,
} from "./types.js";
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
  /** Lazily built from config; invalidated on updateConfig (hot reload). */
  private localClassifier: LocalEmbeddingClassifier | null = null;
  private localClassifierBuilt = false;

  constructor(opts: RouterOptions = {}) {
    this.config = opts.initialConfig ?? builtInDefaultConfig();
    this.log = opts.log ?? (() => {});
    if (opts.llmClient !== undefined) {
      this.llmClient = opts.llmClient;
    }
  }

  /**
   * Atomically replace the current config. Used by hot-reload. Also
   * invalidates the exemplar-embedding cache so θ / exemplar / embedder
   * changes take effect on the next turn.
   */
  updateConfig(next: RouterConfig): void {
    this.config = next;
    this.localClassifier = null;
    this.localClassifierBuilt = false;
  }

  currentConfig(): RouterConfig {
    return this.config;
  }

  /** Whether the LLM classifier path is available for this router. */
  llmAvailable(): boolean {
    return this.config.llmEnabled && this.llmClient !== undefined;
  }

  /** Effective mode for a turn: explicit ctx override, else config. */
  modeFor(ctx: RouteContext): RouterMode {
    return (
      ctx.routerMode ??
      (this.config.localClassifierEnabled ? "super-auto" : "auto")
    );
  }

  async route(ctx: RouteContext): Promise<RouterDecision> {
    const superAuto = this.modeFor(ctx) === "super-auto";
    const catalog = superAuto
      ? filterCatalog(ctx.catalogModels ?? [], this.config.allowedProviders)
      : [];

    // 1. Sticky manual override wins. Allowlist-EXEMPT and never
    //    re-resolved: an explicit pin is explicit intent (SPEC-016).
    if (ctx.sessionManualModel && ctx.sessionManualModel.length > 0) {
      if (superAuto) {
        const brand = providerOf(ctx.sessionManualModel);
        const allowed = new Set(this.config.allowedProviders);
        if (allowed.size > 0 && (brand === null || !allowed.has(brand))) {
          this.log(
            "warn",
            `manual override '${ctx.sessionManualModel}' is outside allowed_providers (exempt; honoring it)`,
          );
        }
      }
      return {
        model: ctx.sessionManualModel,
        rationale: "manual override",
        source: "override",
      };
    }

    const finalize = (decision: RouterDecision): RouterDecision =>
      superAuto ? this.resolveDecision(decision, catalog) : decision;

    // 2. Rules — first match wins.
    const rule = firstMatchingRule(ctx, this.config.rules);
    if (rule) {
      return finalize({
        model: rule.model,
        rationale: rule.rationale,
        source: "rule",
      });
    }

    // 3. Structural gate (super-auto only): ~0 ms deterministic catches
    //    that keep obviously-fast turns on the default model.
    if (superAuto) {
      const gate = structuralGate(ctx.prompt);
      if (gate) {
        return finalize({
          model: this.config.defaultModel,
          rationale: gate.rationale,
          source: "rule",
        });
      }
    }

    // 4. Local semantic classifier (super-auto only). When the embedder
    //    is unavailable (unknown pin), degrade to the legacy
    //    rules → LLM → fallback path instead of hard-failing.
    let ambiguous = false;
    let classifier: LocalEmbeddingClassifier | null = null;
    if (superAuto) {
      classifier = this.getLocalClassifier();
      const c = classifier?.classify(ctx.prompt) ?? null;
      if (c) {
        if (c.score >= this.config.thetaHigh) {
          return finalize({
            model: c.model,
            rationale: `${c.rationale} [score=${c.score.toFixed(2)}]`,
            source: "local",
          });
        }
        ambiguous = c.score >= this.config.thetaLow;
      }
      if (classifier && !ambiguous) {
        // Below θ_low: the exemplars have no opinion. Per AC, the cloud
        // LLM only fires inside the θ band — go straight to fallback.
        return finalize({
          model: ctx.currentModel ?? this.config.defaultModel,
          rationale: c
            ? `no semantic match (score=${c.score.toFixed(2)} < theta_low)`
            : "local classifier produced no candidate",
          source: "fallback",
        });
      }
    }

    // 5. LLM classifier, bounded by llm_timeout_ms (SPEC-015). In
    //    super-auto (with a working local classifier) this only runs for
    //    the ambiguous θ band.
    if (this.llmAvailable() && (!superAuto || !classifier || ambiguous)) {
      const candidateModels = collectCandidateModels(this.config, ctx, catalog);
      const parsed = await classifyWithTimeout(this.llmClient!, {
        userPrompt: ctx.prompt,
        classifierModel: superAuto
          ? this.resolveModelId(this.config.llmClassifierModel, catalog)
          : this.config.llmClassifierModel,
        candidateModels,
        timeoutMs: this.config.llmTimeoutMs,
      });
      if (parsed) {
        return finalize({
          model: parsed.model,
          rationale: parsed.rationale,
          source: "llm",
        });
      }
      // Timeout / parse failure — fall through to default.
      const fallbackModel = ctx.currentModel ?? this.config.defaultModel;
      const rationale = "router-llm timeout; falling back";
      this.log("warn", rationale);
      return finalize({
        model: fallbackModel,
        rationale,
        source: "fallback",
      });
    }

    // 6. No LLM available → default.
    return finalize({
      model: ctx.currentModel ?? this.config.defaultModel,
      rationale: "no rule match",
      source: "fallback",
    });
  }

  /**
   * Build (once per config generation) the local classifier. Unknown
   * `embedding_model` → warn once and degrade to rules + LLM.
   */
  private getLocalClassifier(): LocalEmbeddingClassifier | null {
    if (this.localClassifierBuilt) return this.localClassifier;
    this.localClassifierBuilt = true;
    const embedder = createEmbedder(this.config.embeddingModel);
    if (!embedder) {
      this.log(
        "warn",
        `unknown embedding_model '${this.config.embeddingModel}'; local classifier disabled (rules + LLM only)`,
      );
      this.localClassifier = null;
      return null;
    }
    const exemplars =
      this.config.exemplars.length > 0
        ? this.config.exemplars
        : builtInDefaultExemplars;
    this.localClassifier = new LocalEmbeddingClassifier({
      exemplars,
      embedder,
    });
    return this.localClassifier;
  }

  /**
   * Super-auto post-step (roles A/B): map the decision's model (possibly
   * a family alias or a stale sibling id) onto the newest concrete id in
   * the allowlisted catalog. Unresolvable → try the default model before
   * giving up, warn, and never throw (#5: the turn must not stall).
   */
  private resolveDecision(
    decision: RouterDecision,
    catalog: readonly string[],
  ): RouterDecision {
    if (catalog.length === 0) return decision;
    const resolved = this.resolveModelId(decision.model, catalog);
    if (resolved !== decision.model) {
      return { ...decision, model: resolved };
    }
    return decision;
  }

  private resolveModelId(alias: string, catalog: readonly string[]): string {
    if (catalog.length === 0) return alias;
    const r = resolveLatestModel(alias, catalog);
    if (r.warning) this.log("warn", `router: ${r.warning}`);
    if (r.resolved || catalog.includes(r.id)) return r.id;
    // Alias has no family in the allowed catalog. Fall back to the
    // default model (itself resolved) so we never hand the SDK an id the
    // allowlist rejected.
    const fallback = resolveLatestModel(this.config.defaultModel, catalog);
    if (fallback.resolved || catalog.includes(fallback.id)) {
      this.log(
        "warn",
        `router: '${alias}' not resolvable in allowed catalog; using '${fallback.id}'`,
      );
      return fallback.id;
    }
    return alias;
  }
}

/**
 * Distinct candidate models we advertise to the classifier — every model
 * referenced by a rule or exemplar, plus the default model, plus the
 * current session model (if any). In super-auto mode candidates are
 * resolved through the allowlisted catalog so the cloud classifier only
 * ever sees fresh, permitted ids.
 */
function collectCandidateModels(
  cfg: RouterConfig,
  ctx: RouteContext,
  catalog: readonly string[],
): string[] {
  const set = new Set<string>();
  const add = (id: string): void => {
    if (catalog.length === 0) {
      set.add(id);
      return;
    }
    const r = resolveLatestModel(id, catalog);
    if (r.resolved || catalog.includes(r.id)) set.add(r.id);
  };
  add(cfg.defaultModel);
  for (const r of cfg.rules) add(r.model);
  if (catalog.length > 0) {
    const exemplars = cfg.exemplars.length > 0 ? cfg.exemplars : builtInDefaultExemplars;
    for (const e of exemplars) add(e.model);
  }
  if (ctx.currentModel) add(ctx.currentModel);
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
