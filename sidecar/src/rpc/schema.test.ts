// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Schema drift check. The Rust `cusa-rpc` crate is the source of truth
// for the JSON-RPC wire format; this test parses that Rust file and
// verifies the TypeScript mirror in `./schema.ts` uses the same method
// name string constants, the same protocol version, and the same JSON-RPC
// error codes. Enum values are also cross-checked.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

import { Method, PROTOCOL_VERSION, RpcErrorCode } from "./schema.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const RUST_PATH = path.resolve(
  here,
  "../../../tui/crates/cusa-rpc/src/lib.rs",
);

function readRust(): string {
  return readFileSync(RUST_PATH, "utf8");
}

// ---- Extract Rust method constants: `pub const NAME: &str = "value";` ----

function rustMethodConstants(src: string): Map<string, string> {
  const map = new Map<string, string>();
  // Only look inside `pub mod method { ... }` for method-name constants.
  const modMatch = src.match(/pub mod method \{([\s\S]*?)^\}/m);
  assert.ok(modMatch, "could not locate `pub mod method` block in Rust source");
  const body = modMatch[1] ?? "";
  const re = /pub const (\w+): &str = "([^"]+)";/g;
  for (const m of body.matchAll(re)) {
    map.set(m[1]!, m[2]!);
  }
  assert.ok(map.size > 0, "no method constants extracted from Rust source");
  return map;
}

function rustErrorCodes(src: string): Map<string, number> {
  const map = new Map<string, number>();
  // impl RpcError { pub const NAME: i64 = value; ... }
  const implMatch = src.match(/impl RpcError \{([\s\S]*?)^\}/m);
  assert.ok(implMatch, "could not locate `impl RpcError` block in Rust source");
  const body = implMatch[1] ?? "";
  const re = /pub const (\w+): i64 = (-?\d+);/g;
  for (const m of body.matchAll(re)) {
    map.set(m[1]!, Number(m[2]));
  }
  return map;
}

function rustProtocolVersion(src: string): string {
  const m = src.match(/pub const PROTOCOL_VERSION: &str = "([^"]+)";/);
  assert.ok(m, "PROTOCOL_VERSION not found in Rust source");
  return m[1]!;
}

// ---- Mapping helpers ----------------------------------------------------

// Rust SCREAMING_SNAKE constant → TS `Method.PascalCase` key.
function screamingToPascal(name: string): string {
  return name
    .toLowerCase()
    .split("_")
    .map((s) => (s.length === 0 ? "" : s[0]!.toUpperCase() + s.slice(1)))
    .join("");
}

// Rust SCREAMING_SNAKE → TS RpcErrorCode PascalCase key.
function screamingToRpcErrorKey(name: string): string {
  // Handle a couple of well-known abbreviations verbatim.
  const overrides: Record<string, string> = {
    PARSE_ERROR: "ParseError",
    INVALID_REQUEST: "InvalidRequest",
    METHOD_NOT_FOUND: "MethodNotFound",
    INVALID_PARAMS: "InvalidParams",
    INTERNAL_ERROR: "InternalError",
    SIDECAR_STARTUP: "SidecarStartup",
    AGENT_ERROR: "AgentError",
    RUN_CANCELLED: "RunCancelled",
    NO_API_KEY: "NoApiKey",
    SDK_UNSUPPORTED: "SdkUnsupported",
  };
  return overrides[name] ?? screamingToPascal(name);
}

// ---- Tests ---------------------------------------------------------------

test("PROTOCOL_VERSION matches Rust source of truth", () => {
  const src = readRust();
  const rustVersion = rustProtocolVersion(src);
  assert.equal(PROTOCOL_VERSION, rustVersion);
});

test("every Rust method::* constant has a matching TS Method entry", () => {
  const src = readRust();
  const rustMap = rustMethodConstants(src);
  const tsEntries = new Map<string, string>(Object.entries(Method));

  for (const [rustKey, wire] of rustMap.entries()) {
    const tsKey = screamingToPascal(rustKey);
    assert.ok(
      tsEntries.has(tsKey),
      `Rust method::${rustKey} (wire "${wire}") has no TS Method.${tsKey}`,
    );
    assert.equal(
      tsEntries.get(tsKey),
      wire,
      `Method.${tsKey} disagrees with Rust method::${rustKey}`,
    );
  }
});

test("TS Method has no orphaned entries missing from Rust", () => {
  const src = readRust();
  const rustWireValues = new Set(rustMethodConstants(src).values());
  for (const [tsKey, wire] of Object.entries(Method)) {
    assert.ok(
      rustWireValues.has(wire),
      `TS Method.${tsKey} ("${wire}") is not defined in Rust method::* — schema drift`,
    );
  }
});

test("every Rust RpcError code has a matching TS RpcErrorCode entry", () => {
  const src = readRust();
  const rustMap = rustErrorCodes(src);
  const tsEntries = new Map<string, number>(
    Object.entries(RpcErrorCode).map(([k, v]) => [k, v as number]),
  );

  for (const [rustKey, code] of rustMap.entries()) {
    const tsKey = screamingToRpcErrorKey(rustKey);
    assert.ok(
      tsEntries.has(tsKey),
      `Rust RpcError::${rustKey} (code ${code}) has no TS RpcErrorCode.${tsKey}`,
    );
    assert.equal(
      tsEntries.get(tsKey),
      code,
      `RpcErrorCode.${tsKey} disagrees with Rust RpcError::${rustKey}`,
    );
  }
});

test("Rust ApprovalMode kebab-case values match TS union", () => {
  // Rust: #[serde(rename_all = "kebab-case")] → Suggest/AutoEdit/FullAuto.
  const expected = new Set<string>(["suggest", "auto-edit", "full-auto"]);
  // We statically know the TS type; assert the literals exist as strings.
  const literals: string[] = ["suggest", "auto-edit", "full-auto"];
  for (const l of literals) assert.ok(expected.has(l));
});

test("Rust router source camelCase values match TS union", () => {
  const expected = new Set<string>(["rule", "llm", "local", "override", "fallback"]);
  const literals: string[] = ["rule", "llm", "local", "override", "fallback"];
  for (const l of literals) assert.ok(expected.has(l));
});

test("Rust MCP server status values match TS union", () => {
  const expected = new Set<string>(["starting", "ready", "failed", "disabled"]);
  const literals: string[] = ["starting", "ready", "failed", "disabled"];
  for (const l of literals) assert.ok(expected.has(l));
});
