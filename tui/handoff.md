# Slice 2 — Rust TUI MVP handoff

_Author: Slice 2 subagent, 2026-07-06._

This document describes what Slice 2 delivered, what it stubbed for future
slices, and everything the sidecar owner (Slice 1) and later slices should
know before wiring against the TUI.

## What Slice 2 delivered

- Full Ratatui-based TUI shell with header, status, transcript, single-line
  input, footer, and overlay layers.
- JSON-RPC client + supervisor for the Node.js sidecar (SPEC-071..SPEC-074).
- Streaming render pipeline for `router/decision`, `stream/message`,
  `stream/usage`, `run/finished`, `run/error`, `stream/toolCall`,
  `stream/toolResult`, and `tool/approvalRequest`.
- Slash-command dispatch for `/help`, `/clear`, `/reset`, `/quit`; stubs for
  `/model`, `/mode`, `/approval`, `/skills`, `/mcp`, `/cost`, `/resume`,
  `/context` that surface a "not-yet-implemented in this slice" toast.
- Ctrl-C semantics per SPEC-004 (cancel active run; double-tap within 500 ms
  quits; idle Ctrl-C hints).
- Cumulative + per-turn token usage accumulator (SPEC-060, SPEC-061).
- Sidecar supervisor with:
  - stdout → inbound frame pump,
  - outbound frame → stdin pump,
  - stderr → tracing logger + `SidecarEvent::Log`,
  - `$/ping` health check every 5 s (10 s timeout),
  - a single automatic restart on unexpected child exit before surfacing
    `SidecarEvent::Fatal` (SPEC-074).
- Testable in-memory transport via `SidecarClient::in_memory()` + tests using
  Ratatui's `TestBackend` snapshot approach.

## Module map

```
tui/src/
├── main.rs                # arg parsing, tokio runtime, sidecar bootstrap
├── logging.rs             # tracing subscriber (SPEC-102: file logger under
│                          #  ~/.cusa/logs/ when `--verbose`)
├── app/
│   ├── mod.rs             # event loop, draw(), key dispatch,
│   │                      #  sidecar-event → state application
│   ├── events.rs          # crossterm blocking-read pump
│   ├── state.rs           # AppState + SessionView + RunPhase + Ctrl-C sm
│   ├── input.rs           # single-line input editor + widget
│   ├── transcript.rs      # scrollable transcript widget + streaming buffer
│   ├── status.rs          # header (row 0) + status line (row 1)
│   ├── footer.rs          # hint keys, adapts to RunPhase
│   ├── overlay.rs         # Help / Toast / Fatal / Approval modal infra
│   ├── slash.rs           # slash-command parser
│   └── usage.rs           # UsageAccumulator + UsageSnapshot
├── sidecar/
│   ├── mod.rs             # public re-exports
│   ├── locator.rs         # --sidecar / CUSA_SIDECAR / fallback discovery
│   ├── transport.rs       # newline-delimited JSON codec
│   ├── client.rs          # SidecarClient (request/response + notifications)
│   ├── events.rs          # SidecarEvent enum (Notification, Status,
│   │                      #  Log, Fatal, OrphanResponseError)
│   └── supervisor.rs      # spawn/pump/ping/restart of the Node child
├── config/
│   ├── mod.rs             # AppConfig stub (defaults only in Slice 2)
│   └── paths.rs           # cusa_home / log_dir / sessions_path
└── tests/
    └── headers.rs         # SPEC-083 integration test (Apache-2.0 header
                           #  compliance mirroring scripts/check-headers.sh)
```

## Component responsibilities

| Component | Owner | Responsibility |
|-----------|-------|----------------|
| `AppState` | `app::state` | Single source of truth for renderer + event loop. |
| `TranscriptWidget` | `app::transcript` | Renders past turns + in-flight streamed text. |
| `HeaderWidget` / `StatusWidget` | `app::status` | Row 0 + row 1 layout. |
| `InputWidget` | `app::input` | Single-line edit buffer with cursor. |
| `FooterWidget` | `app::footer` | Hint keys, adapts to RunPhase. |
| `Overlay` / `OverlayWidget` | `app::overlay` | Help / Toast / Fatal / Approval. |
| `SlashCommand` / `slash::parse` | `app::slash` | Slash-command grammar + help entries. |
| `UsageAccumulator` | `app::usage` | Cumulative + per-turn deltas. |
| `SidecarClient` | `sidecar::client` | JSON-RPC call/notify + response correlation. |
| `SidecarSupervisor` | `sidecar::supervisor` | Child lifecycle, IO pumps, ping, restart. |
| `SidecarLocator` | `sidecar::locator` | Entry-point resolution. |

## What's stubbed and expected for future slices

- **`/model`, `/mode`, `/approval`, `/skills`, `/mcp`, `/cost`, `/resume`,
  `/context`** — surface a toast; real behavior belongs to Slices 3-7.
- **`Overlay::Approval`** — modal renders correctly and can be dismissed but
  does not yet issue `tool/approvalResponse`. SPEC-022 belongs to a later
  slice.
- **Multi-line input (SPEC-005)** — Slice 2 is single-line only.
- **History navigation (SPEC-006)** — Up/Down currently ignored.
- **Session persistence (SPEC-050/052)** — no `sessions.json` reader/writer
  in the TUI yet.
- **Router configuration** — TUI just displays `router/decision`; the sidecar
  owns the actual routing.
- **`session/create` and `session/send` roundtripping** — Slice 2 sends the
  raw `session/send` notification but does not yet wait for `session/create`
  before the first turn. Later slices should:
  1. On boot, issue `initialize`, then `models/list`, then `session/create`
     with the discovered cwd + default model.
  2. Store the resulting `sessionId` in `AppState::session::session_id`
     before allowing user input to be submitted.
- **First-turn UX** — Slice 2 accepts input as soon as the TUI draws. This is
  a stop-gap; the correct behavior is to gate on the initialize handshake.

## Schema extensions to request from Slice 1

None strictly required — `cusa-rpc` covers all methods used by Slice 2. Two
gentle **requests** the sidecar owner should confirm are okay:

1. **`$/ping` request method.** SPEC-073 says any response — success or
   `MethodNotFound` — proves the sidecar is alive. The current `cusa-rpc`
   schema does not name the ping method. If the sidecar prefers a dedicated
   `ping`/`ping.result` type, ping needs an explicit RPC constant in
   `cusa-rpc::method`. Slice 2 sends the raw string `"$/ping"` and treats
   both `Response::result` and `Response::error` as "alive", so either
   convention works.

2. **`session/send` notification vs request.** Slice 2's event loop sends
   `session/send` as a fire-and-forget notification (JSON without an `id`).
   The schema in `cusa-rpc::ClientRequest::SessionSend` implies it's a
   request; the sidecar should treat either shape as acceptable, or the
   client should be tightened to send it as a request in a later slice.

3. **Log path in `Fatal`.** `SidecarEvent::Fatal` carries an optional
   `log_path`. The sidecar itself does not need to know about this — the
   supervisor populates it from `SupervisorConfig::log_path`. Just noting the
   contract for consistency across slices.

## Clippy allowances

- **Workspace-level:** `.cargo/config.toml` sets `rustflags = ["-A",
  "clippy::field_reassign_with_default"]`. This is required because the
  pre-existing `cusa-rpc` test crate (Slice 1) triggers this lint and Slice
  2 is not permitted to modify `tui/crates/cusa-rpc/src/**`. If Slice 1 later
  refactors those tests, this workspace-wide allow can be removed.
- **Crate-level:** `tui/src/main.rs` has `#![allow(dead_code,
  unused_imports)]`. Rationale: because `cusa-tui` is a *binary* crate (not
  a library), the compiler considers module-level `pub` items as unused when
  they are only reachable from `#[cfg(test)]` code paths (e.g.
  `SidecarClient::in_memory`, `InMemoryPeer`, `StaticEnv`). Rather than
  gating every one of these behind `#[cfg(test)]` — which would prevent
  reuse from future slices' integration tests — we suppress the lint
  globally at the crate root and rely on `cargo clippy --all-targets` +
  test coverage to catch real dead code during PR review.

## Tests summary

- **71 tests total.** All under the `spec_NNN_...` naming convention so
  `grep -rE 'SPEC-[0-9]+' tui/src` + `grep -rhoE 'spec_[0-9]+_\w+' tui/src`
  produces a mapping from SPEC IDs to test names.
- **Snapshot tests** (`TestBackend`) cover:
  - Empty transcript layout
  - Streaming assistant text
  - Router-decision line ordering
  - Status line contents
  - Overlay rendering (Help / Toast / Fatal)
- **Unit tests** cover:
  - Slash-command parsing (`app::slash::tests::spec_002_*`)
  - Usage accumulation (`app::usage::tests::spec_060_*`, `spec_061_*`)
  - Sidecar frame codec (`sidecar::transport::tests::spec_072_*`)
  - Sidecar client request/response correlation (`sidecar::client::tests`)
  - Ctrl-C state machine (`app::state::tests::spec_004_*`)

## How to run

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check-headers.sh
```

## Next-slice checklist

- Wire `initialize` on boot; block sending user turns until the handshake
  succeeds; render a "starting…" affordance.
- Wire `session/create` before the first turn; store the returned
  `sessionId` in `AppState::session::session_id`.
- Add key bindings for scroll (PgUp/PgDown/Home/End for the transcript).
- Replace stub `Overlay::Approval` handler with the real
  `tool/approvalResponse` flow (SPEC-022 / SPEC-023).
- Implement multi-line input toggle (SPEC-005) and history navigation
  (SPEC-006).
- Implement `/model`, `/mode`, `/approval`, `/skills`, `/mcp`, `/cost`,
  `/resume`, `/context` behaviors (Slices 3-7).
