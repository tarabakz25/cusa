// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Feature-detect Cursor SDK's native local-context retention (SPEC-093).
//
// Strategy:
//   - Inspect the `@cursor/sdk` package.json version + walk its `.d.ts`
//     files for a `retainConversation` (or similar) flag on Agent /
//     LocalAgent options.
//   - Detection is best-effort: unrecognized SDK versions default to
//     `false` and the sidecar continues to inject history manually.
//
// The final mode blends the detection with the user's config:
//   - `conversation.mode = "auto"`   → detection result.
//   - `conversation.mode = "manual"` → force manual (detection ignored).
//   - `conversation.mode = "native"` → trust the SDK (detection ignored).

import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";

export type ConversationMode = "auto" | "manual" | "native";

/**
 * Signals we scan the SDK `.d.ts` files for. Any hit → we treat the SDK
 * as offering native retention. Kept generous on purpose: the exact flag
 * name is unknown until the fix ships.
 */
const DETECTION_PATTERNS: RegExp[] = [
  /\bretainConversation\b/,
  /\bretain_conversation\b/,
  /\bconversationRetention\b/,
  /\bpreserveHistory\b/,
  /\bkeepConversation\b/,
];

/** Filenames inside the SDK dist that are most likely to declare the flag. */
const CANDIDATE_FILES = [
  "agent/options.d.ts",
  "options.d.ts",
  "agent.d.ts",
  "index.d.ts",
];

export interface FeatureDetectOptions {
  /** Override the entry-point resolution (tests). */
  resolveEntry?: () => string | null;
  /** Override file reads (tests). */
  readFileImpl?: (p: string) => Promise<string>;
  /** Override the extra patterns (tests). */
  patterns?: readonly RegExp[];
}

export interface FeatureDetectResult {
  /** True when the SDK exposes a native retention flag. */
  nativeRetention: boolean;
  /** Free-form reason string surfaced in the `log` notification. */
  reason: string;
  /** The signal that matched (empty when detection failed). */
  matchedPattern?: string;
  /** Path we searched inside the SDK, when available. */
  searchedIn?: string;
}

/**
 * Run detection. Never throws — resolution failures degrade to
 * `nativeRetention: false`.
 */
export async function detectNativeConversationRetention(
  opts: FeatureDetectOptions = {},
): Promise<FeatureDetectResult> {
  const patterns = opts.patterns ?? DETECTION_PATTERNS;
  const entryPath = opts.resolveEntry
    ? opts.resolveEntry()
    : defaultResolveSdkEntry();
  if (!entryPath) {
    return {
      nativeRetention: false,
      reason: "@cursor/sdk not resolvable; assuming manual injection",
    };
  }
  const sdkRoot = findSdkRoot(entryPath);
  if (!sdkRoot) {
    return {
      nativeRetention: false,
      reason: "could not locate @cursor/sdk root",
      searchedIn: entryPath,
    };
  }
  const distEsm = path.join(sdkRoot, "dist", "esm");
  const reader = opts.readFileImpl ?? ((p) => readFile(p, "utf8"));
  for (const rel of CANDIDATE_FILES) {
    const abs = path.join(distEsm, rel);
    let text: string;
    try {
      text = await reader(abs);
    } catch {
      continue;
    }
    for (const rx of patterns) {
      if (rx.test(text)) {
        return {
          nativeRetention: true,
          reason: `native retention signal '${rx.source}' found in ${rel}`,
          matchedPattern: rx.source,
          searchedIn: distEsm,
        };
      }
    }
  }
  return {
    nativeRetention: false,
    reason: "no native retention signals found in @cursor/sdk types",
    searchedIn: distEsm,
  };
}

/**
 * Resolve `conversation.mode` into a boolean: `true` when the sidecar
 * should skip manual history injection.
 */
export function shouldUseNativeRetention(
  mode: ConversationMode,
  detection: FeatureDetectResult,
): { useNative: boolean; reason: string } {
  switch (mode) {
    case "manual":
      return {
        useNative: false,
        reason: "conversation.mode=manual (user override); manual injection on",
      };
    case "native":
      return {
        useNative: true,
        reason: "conversation.mode=native (user override); trusting SDK",
      };
    case "auto":
    default:
      return {
        useNative: detection.nativeRetention,
        reason: `conversation.mode=auto; detection: ${detection.reason}`,
      };
  }
}

function defaultResolveSdkEntry(): string | null {
  try {
    const require = createRequire(import.meta.url);
    return require.resolve("@cursor/sdk");
  } catch {
    return null;
  }
}

function findSdkRoot(entry: string): string | null {
  let dir = path.dirname(entry);
  for (let i = 0; i < 6; i++) {
    if (path.basename(dir) === "sdk" && path.basename(path.dirname(dir)) === "@cursor") {
      return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return null;
}
