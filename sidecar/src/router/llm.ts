// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Router LLM classifier (SPEC-013 fallback path, SPEC-015 timeout).
//
// The classifier is an interface so tests can inject fakes; the real
// implementation forwards to `@cursor/sdk`'s one-shot `Agent.prompt`.

/**
 * Client that runs a single classification prompt against a model and
 * returns the raw text output. Implementations must honour `signal` and
 * throw when the signal aborts.
 */
export interface RouterLlmClient {
  classify(input: RouterLlmInput): Promise<string>;
}

export interface RouterLlmInput {
  /** Prompt payload — includes the classifier system + user prompt. */
  prompt: string;
  /** Model to run the classifier on. */
  model: string;
  /** Abort signal for hard-timeout enforcement. */
  signal: AbortSignal;
}

/**
 * Parsed classifier output. `null` means "invalid / unusable output".
 */
export interface ClassifierResult {
  model: string;
  rationale: string;
}

/**
 * Build the deterministic classifier prompt. We keep it short so the
 * classifier LLM can turn around inside the 5 s budget.
 */
export function buildClassifierPrompt(
  userPrompt: string,
  candidateModels: readonly string[],
): string {
  const models = candidateModels.length > 0
    ? candidateModels.join(", ")
    : "composer-2.5, claude-sonnet-4";
  return [
    "You are a task-router for a coding assistant.",
    "Pick the best model for the user's prompt from the candidates below.",
    "Respond with ONLY a JSON object of the form:",
    '  {"model": "<id>", "rationale": "<short reason>"}',
    "No other text.",
    "",
    `Candidates: ${models}`,
    "",
    "User prompt:",
    userPrompt,
  ].join("\n");
}

/**
 * Parse the classifier's raw text output into a `ClassifierResult`.
 * Tolerates common LLM chattiness: extracts the first `{ ... }` block.
 */
export function parseClassifierOutput(text: string): ClassifierResult | null {
  const trimmed = text.trim();
  const start = trimmed.indexOf("{");
  const end = trimmed.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  const jsonSlice = trimmed.slice(start, end + 1);
  try {
    const obj = JSON.parse(jsonSlice) as unknown;
    if (
      typeof obj !== "object" ||
      obj === null ||
      typeof (obj as { model?: unknown }).model !== "string"
    ) {
      return null;
    }
    const model = (obj as { model: string }).model.trim();
    const rationale =
      typeof (obj as { rationale?: unknown }).rationale === "string"
        ? (obj as { rationale: string }).rationale.trim()
        : "router-llm choice";
    if (model.length === 0) return null;
    return { model, rationale };
  } catch {
    return null;
  }
}

/**
 * Run a classifier under a hard-timeout budget. Resolves to a parsed
 * `ClassifierResult` on success, `null` on failure or timeout.
 */
export async function classifyWithTimeout(
  client: RouterLlmClient,
  args: {
    userPrompt: string;
    classifierModel: string;
    candidateModels: readonly string[];
    timeoutMs: number;
  },
): Promise<ClassifierResult | null> {
  const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | null = setTimeout(() => {
    controller.abort();
  }, args.timeoutMs);
  try {
    const raw = await client.classify({
      prompt: buildClassifierPrompt(args.userPrompt, args.candidateModels),
      model: args.classifierModel,
      signal: controller.signal,
    });
    return parseClassifierOutput(raw);
  } catch {
    return null;
  } finally {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }
}
