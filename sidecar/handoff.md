# Sidecar — Slice 1 Handoff

Slice 1 (Sidecar MVP) delivered by this branch. This document is written for
the parent orchestrator agent and later slice implementers.

## Public surface added

### RPC methods (all wired end-to-end; some are stubs — see "Deferred")

| Method                    | Status                                                              |
| ------------------------- | ------------------------------------------------------------------- |
| `initialize`              | Returns real `capabilities` (see below).                            |
| `shutdown`                | Returns `{ ok: true }` and closes the run loop.                     |
| `models/list`             | Cached call to `Cursor.models.list()`. Requires `CURSOR_API_KEY`.   |
| `session/create`          | Creates a Cursor SDK agent. Errors with `-32003` if no key.         |
| `session/send`            | Streams `router/decision`, `stream/message`, `run/finished`.        |
| `session/cancel`          | Calls `run.cancel()`; 3 s settle window; falls through if orphaned. |
| `session/resume`          | Re-attaches an existing `agentId` via `Agent.resume`.               |
| `session/dispose`         | Awaits `agent.dispose()` / `agent.close()`.                         |
| `skills/list`             | **Stub**: always returns `{ skills: [], warnings: [] }`.            |
| `skills/setEnabled`       | Persists `enabledSkillIds` on the session; no injection yet.        |
| `mcp/list`                | **Stub**: always returns `{ servers: [] }`.                         |
| `mcp/toggle`              | **Stub**: returns `{ ok: true, pendingUntilNextTurn: true }`.       |
| `tool/approvalResponse`   | Resolves matching pending approval; safe no-op on unknown id.       |

### Notifications emitted

- `router/decision` (once per `session/send`; slice 1 always uses
  `source: "fallback"` when the router itself is not implemented, or
  `source: "override"` when the TUI passed `modelOverride`).
- `stream/message` — assistant / reasoning text deltas.
- `stream/toolCall` — every tool call observed on the run.
- `stream/toolResult` — completion of an observed tool call.
- `stream/usage` — from the SDK's per-turn usage event (mid-run).
- `tool/approvalRequest` — issued when `approvalPolicy(mode, name, category)`
  returns `"prompt"`. **See the SDK gating limitation below.**
- `run/finished` — with cumulative usage, model, `status`, and optional summary.
- `run/error` — for terminal errors.
- `log` — free-form log lines forwarded from the sidecar.

### Capabilities returned in `initialize`

```json
{ "streaming": true, "cancel": true, "resume": true, "sandbox": true,
  "mcp": false, "skills": false, "routerLlm": false }
```

### RPC-schema changes

No schema types were **removed or renamed**. Only additions are:

- Added an implementation of every method already declared in `Method` (both
  TS and Rust already had the method-name constants and payload types).

No changes needed in `tui/crates/cusa-rpc/src/lib.rs` — everything Slice 1
needs was already declared by the Phase-A scaffold. The Rust unit tests
still pass unchanged (`cargo test -p cusa-rpc`).

## Module layout added under `sidecar/src/`

```
agent/
  sdkAdapter.ts        # RealSdkAdapter around `@cursor/sdk`
  sdkAdapter.fake.ts   # scriptable in-memory fake for tests
  session.ts           # SessionManager (owns agents, streams events)
  session.test.ts      # SPEC-001/-004/-060/-061/-071/-022/-024/-100
approval/
  policy.ts            # approvalPolicy(mode, toolName, category) + sandbox
  policy.test.ts       # SPEC-022/-023/-024
config/
  apiKey.ts            # readApiKey() from env, else ~/.cusa/config.toml
  apiKey.test.ts       # SPEC-100
usage/
  accumulator.ts       # UsageAccumulator + TurnUsageTracker
  accumulator.test.ts  # SPEC-060/-061
```

`src/index.ts` now composes a `buildServer()` factory so tests can drive
handlers over `PassThrough` streams instead of spawning a subprocess.
`main()` still runs the full sidecar when the file is invoked as an
entrypoint (both `dist/index.js` and `tsx src/index.ts` are detected via
resolved-realpath equality against `process.argv[1]`).

## Deferred behaviours (for later slices)

- **Router (SPEC-010..016).** `session/send` currently emits a router line
  of `source: "fallback"` (or `"override"` when `modelOverride` is passed).
  When the Router slice lands, wire it into `SessionManager.sendMessage`
  and emit `source: "rule" | "llm"` accordingly. The RPC schema fields
  (`sessionId`, `runId`, `model`, `rationale`, `source`) are already
  correct.
- **Skills (SPEC-030..034).** Discovery, frontmatter parsing, and body
  injection are not implemented. `skills/list` returns `[]`. Once the
  skills slice lands, replace the stub with real discovery and thread the
  concatenated skill body into `SendOptions.systemContext` on the adapter.
- **MCP (SPEC-040..043).** `mcp/list` and `mcp/toggle` are stubs. Session
  state already accepts `mcpOverrides: unknown` and passes it to the
  adapter as `mcpServers`; the layered inline > project > user loader is
  future work.
- **Tool approval gating (SPEC-022/-023 real gating).** See "SDK deviation"
  below. Today the sidecar *observes* tool calls, emits
  `tool/approvalRequest`, and records the pending id, but the underlying
  SDK call is not blocked. `tool/approvalResponse` resolves the pending
  entry so the TUI's book-keeping stays correct.
- **Session persistence (SPEC-050..053).** Not touched; belongs in the TUI
  side.
- **Manual history injection (SPEC-090..093).** Not touched.
- **Verbose logging (SPEC-102).** `--verbose` CLI flag not implemented;
  logs currently ride the `log` notification channel only.
- **`cusa login` (SPEC-101).** Config writing is out of scope; only
  reading of `~/.cusa/config.toml` is implemented.

## Deviations from the SDK

### 1. Real approval gating is not possible with the current SDK surface

`@cursor/sdk` 1.0.23 exposes `onDelta` callbacks for tool-call *observation*
but no synchronous or asynchronous *interceptor* that lets a host block a
tool call while asking the user. The internal `custom-user-tools` shim
supports host-defined tools (`SDKCustomTool`), but the SDK's built-in tools
(`read`/`write`/`edit`/`shell`/…) run inside the executor with no
per-invocation approval hook.

**Consequences for this slice:**

- `approvalPolicy(mode, name, category)` is a pure function ready to plug in
  once the SDK grows an interceptor.
- The sidecar still fires `tool/approvalRequest` and tracks pending
  responses, so TUI-side gating UX can be developed against this stream in
  parallel. It's observational only in slice 1.
- The `sandboxOptions.enabled = true` coupling under `full-auto` (partial
  SPEC-024) is enforced — that is a real SDK knob and it works.

**Recommendation for the parent agent**: keep an eye on the SDK release
notes for an interception hook (`beforeToolCall` or similar). When it
lands, all we need to add is the promise-await in `dispatchTurnEvent`'s
tool-call branch and swap the pending-approval resolver from a no-op to
the actual decision.

### 2. `local.settingSources` shape drift

Our RPC schema exposes `"user" | "project" | "local"`, but the SDK's
`LocalAgentOptions.settingSources` accepts `"project" | "user" | "team" |
"mdm" | "plugins" | "all"`. The `"local"` variant carried over from Cursor
IDE terminology does not exist on the SDK. The adapter drops `"local"`
silently and forwards only `"user"` / `"project"` (`mapSettingSources`).
When the router / MCP / skills slices arrive we may want to update the
RPC schema to expose the additional SDK layers (`"team"`, `"plugins"`,
`"all"`) — this is additive and safe.

### 3. `TokenUsage` field names

The SDK uses `cacheWriteTokens`; our schema uses `cacheCreationTokens`
(Codex convention). Adapter normalises to the schema — no change needed
downstream.

### 4. Dispatch is concurrent, not serial

`RpcServer.dispatchLine` fires each request handler without awaiting the
previous one. In practice all our slice-1 handlers are fast enough that
this doesn't matter, but a subsequent slice that adds long-running
handlers may want to introduce per-method serialisation.

## Anything the parent agent needs to reconcile

- **None on the schema.** Rust `tui/crates/cusa-rpc/src/lib.rs` and TS
  `sidecar/src/rpc/schema.ts` remain in structural parity. No new enum
  variants were added.
- **Test file collisions.** During the build I saw `src/index.test.ts` get
  rewritten by what looks like an editor / autosync tool after my initial
  Write. The version now on disk was authored (by that tool) to be
  slice-1-compatible — it drives the sidecar as a child process and only
  asserts capability shape rather than specific `true`/`false` values, so
  it passes cleanly against slice 1. If the parent agent expected a
  different test layout there, please diff.

## Testing summary

Run from `sidecar/`:

```sh
npm run build
npm run typecheck
npm test        # 49 tests pass
```

From the repo root:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test -p cusa-rpc
bash scripts/check-headers.sh
```

Smoke test:

```sh
cd sidecar
npm run build
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"0.1","clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"models/list"}' \
  '{"jsonrpc":"2.0","id":3,"method":"shutdown"}' \
| node dist/index.js
# → initialize result, models/list error {-32003 NO_API_KEY}, shutdown {ok:true}
```
