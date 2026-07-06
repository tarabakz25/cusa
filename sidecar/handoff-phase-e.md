# Sidecar — Phase E Handoff

Phase E delivers the sidecar-side context workaround (SPEC-090..093), inline
MCP override normalisation for `--mcp` (SPEC-041), and the sidecar half of
`--verbose` rotating logs (SPEC-102). It also flips
`initialize.capabilities.nativeConversationRetention` on/off based on a
best-effort SDK type inspection.

## RPC changes (all additive)

### New method

| Method                | Request                                                                        | Result |
| --------------------- | ------------------------------------------------------------------------------ | ------ |
| `context/setStrategy` | `{ sessionId: string, strategy: "auto" \| "raw" \| "summary" }`                | `Ok`   |

- Rust: `cusa_rpc::method::CONTEXT_SET_STRATEGY`, `ContextStrategy` enum
  (`lowercase` serde), `ContextSetStrategyParams`.
- TS mirror: `Method.ContextSetStrategy`, `ContextStrategy`,
  `ContextSetStrategyParams`.
- Wire is stable / unchanged for every pre-existing method. Both the
  TypeScript drift test (`sidecar/src/rpc/schema.test.ts`) and the Rust
  round-trip / method-constant tests still pass.

### `Capabilities` gains one field

`nativeConversationRetention: bool` (`camelCase`, default `false`,
`#[serde(default)]` on Rust so older sidecars keep parsing cleanly).

Reported by the initialize handler from the current `ContextManager`
state, which is set during startup by `bootstrapContext()`.

### `session/create.mcpOverrides` payload shape (SPEC-041)

`mcpOverrides` now accepts three shapes (schema unchanged — the field
was already `unknown`; only the parser widened):

1. `{ mcpServers: { id: config, ... } }`
2. bare `{ id: config, ... }` map
3. **NEW**: array of server-diff objects `{ id, enabled?, config? }`
   - `enabled: false` drops the server from the inline layer
   - missing `config` skips with a warning

Layering unchanged: inline > project > user.

## New modules

```
sidecar/src/context/
  format.ts             renderRaw / renderSummary / xmlEscape / rawRenderByteSize
  history.ts            ConversationHistory (per-session ring)
  strategy.ts           pickStrategy() + DEFAULT_BYTE_BUDGET / DEFAULT_RAW_TURNS
  summarizer.ts         Summarizer + buildSummarizerPrompt + REGROW_THRESHOLD
  feature_detect.ts     detectNativeConversationRetention + shouldUseNativeRetention
  index.ts              ContextManager (public API used by SessionManager)
  format.test.ts               (SPEC-090)
  history.test.ts              (SPEC-090/091)
  strategy.test.ts             (SPEC-091/092)
  summarizer.test.ts           (SPEC-091)
  feature_detect.test.ts       (SPEC-093)
  session_integration.test.ts  (SPEC-090/091/092/093 end-to-end)

sidecar/src/config/
  conversation.ts       loadConversationConfig / parseConversationSection
  conversation.test.ts

sidecar/src/logging/
  rotate.ts             RotatingLogger + formatLine / formatStamp
  rotate.test.ts        (SPEC-102)
```

## Wire-in points

- **`SessionManager.sendMessage`** — now calls
  `context.buildContext(sessionId)` and prepends its output to the
  `systemContext` after skills. The current turn's user text is NOT
  included in the built context — it goes as the fresh prompt (per
  spec).
- **`SessionManager.awaitTurn`** — after `run/finished`, appends the
  completed turn `{ userPrompt, assistantText, toolCallsSummary,
  model }` onto the session's `ConversationHistory`. Assistant text is
  built from the streamed deltas; tool summaries are built from
  paired `tool-call` + `tool-result` events.
- **`SessionManager.disposeSession`** — clears the session's
  `ConversationHistory`.
- **`SessionManager.setContextStrategy(params)`** — handles
  `context/setStrategy`.
- **`buildServer()`** — instantiates a shared `ContextManager`, wires
  `RotatingLogger` from `CUSA_LOG_FILE`, and mirrors every `Method.Log`
  notification into that file when set.
- **`main()`** — calls `bootstrapContext()` after `buildServer()` and
  before `server.run()`. That step (a) reads `[conversation]` from
  `~/.cusa/config.toml`, (b) runs SDK detection, (c) resolves
  `mode = "auto" | "manual" | "native"` into a boolean and calls
  `contextManager.setUseNative(bool)`, (d) emits a single `log` line
  summarising the decision so the TUI can render an inline banner.

## Defaults

| Knob                     | Value        | Config key                    |
| ------------------------ | ------------ | ----------------------------- |
| `conversation.mode`      | `"auto"`     | `mode`                        |
| Raw window size          | 6 turns      | `raw_turns`                   |
| Byte budget              | 32 KiB       | `byte_budget`                 |
| Summarizer timeout       | 8000 ms      | `summarizer_timeout_ms`       |
| Summarizer model         | `composer-2.5` | `summarizer_model`          |
| Summary tail (raw)       | 2 turns      | (constant)                    |
| Re-summarise threshold   | 25 % growth  | (constant, `REGROW_THRESHOLD`) |
| Rotate log at            | 10 MiB       | (constant, `DEFAULT_ROTATE_BYTES`) |
| Backups kept             | 3            | (constant, `DEFAULT_BACKUP_COUNT`) |

Config file lives at `~/.cusa/config.toml`:

```toml
[conversation]
mode = "auto"                # "auto" | "manual" | "native"
raw_turns = 6
byte_budget = 32768
summarizer_timeout_ms = 8000
summarizer_model = "composer-2.5"
```

## SDK feature-detect result

Runtime check against the installed `@cursor/sdk` 1.0.23:

```json
{
  "nativeRetention": false,
  "reason": "no native retention signals found in @cursor/sdk types",
  "searchedIn": "<node_modules>/@cursor/sdk/dist/esm"
}
```

Interpretation: the SDK does **not** currently expose
`retainConversation` / `history` / any equivalent flag on `AgentOptions`
or `LocalAgentOptions`. The sidecar therefore ships with **manual
history injection ON** by default. When Cursor lands the fix, the
detection block re-runs at every sidecar startup — no user action
required beyond restarting `cusa`.

Users can force either mode via `conversation.mode = "manual"` or
`conversation.mode = "native"` in `~/.cusa/config.toml`.

## Summarizer client wiring

`ContextManager` accepts a `RouterLlmClient` in its constructor. The
Router already exposes the same `RouterLlmClient` interface; hooking
that same client into `ContextManager` gives the sidecar a working
summarizer without a second SDK path.

If no client is wired, the strategy picker still selects `"summary"`
when the byte budget is exceeded — but `buildContext` degrades to raw
replay with a `log` warning (kept observable so the TUI can nudge the
user). Wire the client via `contextManager.setSummarizerClient(client)`
whenever the SDK adapter becomes available.

## Rotating log format

`CUSA_LOG_FILE=/abs/path/cusa-sidecar.log` — the sidecar mirrors every
`log` RPC notification (level + optional target + message) into this
file with format:

```
2026-07-06T15:00:00.000Z INFO [sidecar/context] conversation retention: manual injection on — …
```

On >10 MiB the live file is renamed to
`cusa-sidecar.YYYYMMDD-HHMMSS.log` and a fresh handle is opened; the
prune step retains the 3 newest backups by mtime.

## Test rollup

- `sidecar/`: **161 tests pass** (was 97 at Phase D end — +64 new
  tests). All new test names include their SPEC id.
- `cargo test --workspace`: **163 tests pass** (was 139; +24 in the
  cusa-rpc crate from the new schema drift assertions and the
  `context/setStrategy` variant).
- `bash scripts/check-headers.sh`: **31 files scanned, OK**.
- `sidecar/`: `npm run build` + `npm run typecheck` clean.

## Cross-agent notes for the TUI subagent

1. **`--mcp <file.json>`** — the TUI-side parser can hand either an
   `mcpServers` object, a bare map, or an array of
   `{ id, enabled?, config? }` diffs to `session/create.mcpOverrides`.
   All three are normalised by `sidecar/src/mcp/loader.ts`.
2. **`--verbose`** — the TUI should set `CUSA_LOG_FILE=<abs path>`
   before spawning the sidecar. The sidecar prints its ready banner
   to stderr regardless; the file mirror is additive.
3. **`context/setStrategy`** — clients call this method to implement
   `/context strategy=raw|summary|auto`. Idempotent, always `Ok`. Bad
   sessionIds return `InvalidParams`.
4. **`capabilities.nativeConversationRetention`** — the TUI can hide
   the `/context` command / hint the user with "SDK now retains
   history natively" when this is `true`. Today it will always report
   `false` unless the user explicitly sets `mode = "native"` in
   config.

## Known limits / follow-ups

- **`cargo clippy --workspace --all-targets -- -D warnings`** currently
  emits a handful of style warnings inside `tui/src/**` (which Phase E
  is not allowed to touch). The `cusa-rpc` crate — the only Rust file
  I edited — passes clippy cleanly. The parallel TUI subagent needs
  to clean those up before the workspace CI gate goes hard.
- **MCP hot-reload** (`fs.watch` on `.cursor/mcp.json`) — deferred as
  "nice-to-have". The router config watcher already demonstrates the
  pattern; if wanted, drop into `sidecar/src/mcp/index.ts` mirroring
  `sidecar/src/router/config.ts`.
- **`context/setStrategy` observability** — the sidecar accepts the
  strategy but does not emit a confirmation notification. The TUI can
  render its own toast; if we want a sidecar-authoritative log line,
  add a `Method.Log` emit inside `setContextStrategy`.
- **Summarizer regeneration cost** — `Summarizer` caches by
  `lastIndex` + window bytes; re-summarizing on ≥25% growth is a
  single LLM turn. If a session drifts into >1 MB of history the
  summarizer may still be expensive; that's for a future slice
  (`compact_after_bytes` config).
