---
title: Codex TUI cherry-pick — UI parity via upstream widget port
type: feature
complexity: full
status: done
author: unknown
created: 2026-07-06
reviewed: true
review_date: 2026-07-06
review_waiver: user requested build without formal review round
related:
  - 20260706-cusa-cli
---

# Codex TUI cherry-pick — UI parity via upstream widget port

## Overview

### Purpose / Background

The cusa TUI currently implements a **from-scratch Ratatui shell** (Slice 2 MVP) that borrows only layout vocabulary and color conventions from OpenAI's `codex-rs/tui`. SPEC-070 in `20260706-cusa-cli.md` called for forking `codex-tui`; that fork never happened. The result is a functional but visually and behaviorally divergent UI (single-line input, plain transcript, no diff/exec cells, no Codex bottom pane).

This spec defines **Approach 1**: cherry-pick portable UI modules from upstream `codex-rs/tui`, decouple them from `codex-core` / `codex-app-server-protocol`, and wire them to the existing **cusa sidecar + `cusa-rpc`** bridge. Agent logic stays in the Node.js sidecar; only rendering and input UX move to upstream Codex widgets.

### Target users

- **cusa CLI users** who expect Codex-familiar terminal UX (multi-line composer, rich transcript, diff/exec blocks, status chrome).
- **cusa maintainers** who want upstream UI improvements cherry-pickable without re-implementing widgets.

### Scope

**In scope:**

- Vendor selected `codex-rs/tui` UI modules under `tui/` with pinned upstream commit and Apache-2.0 attribution (SPEC-083).
- Introduce a **`CusaViewModel` adapter** that maps `AppState` + `cusa-rpc` stream events into types consumed by ported Codex widgets.
- Phased port of foundation + conversation + input + tool-display widgets (see §2.2).
- Insta snapshot tests for ported regions; visual regression gate in CI.
- Enable required **ratatui / crossterm features** (or minimal local shims) used by ported modules.
- Retire replaced cusa custom widgets (`input.rs`, `transcript.rs`, parts of `status.rs`) once parity tests pass.

**Out of scope:**

- Porting `codex-core`, `codex-app-server-client`, login, cloud config, ChatGPT flows, or Codex's app-server protocol.
- Multi-agent UI (`multi_agents`, agent picker beyond cusa's existing session chooser).
- Codex-specific features with no cusa equivalent (pets, branch summary, IDE context, npm registry, Windows sandbox NUX).
- Replacing the **cusa JSON-RPC schema** or sidecar agent implementation.
- Non-interactive `cusa exec` mode.

### Glossary

| Term | Definition |
|------|------------|
| **Cherry-pick** | Copy upstream source files (with license headers) into `tui/vendor/codex-ui/`, then adapt imports — not a git submodule of the full `codex-rs` workspace. |
| **CusaViewModel** | Rust adapter layer translating cusa domain state (`AppState`, `TranscriptEntry`, sidecar notifications) into Codex widget input structs. |
| **Foundation layer** | Low-dependency Codex modules: `custom_terminal`, `style`/`ui_consts`, `terminal_palette`, `color`, `wrapping`, `width`. |
| **UI parity** | Snapshot-tested visual match to Codex TUI for in-scope regions at a pinned terminal size (80×24 default, plus one wide case 120×40). |
| **Decouple** | Replace `codex_core::`, `codex_protocol::`, `codex_app_server_protocol::` references with cusa-local types or trait bounds in the adapter — ported files must not depend on Node or `@cursor/sdk`. |

## Clarification Checklist (Phase 1.6 — resolved)

| # | Requirement | Status | Finding | Resolution |
|---|-------------|--------|---------|------------|
| 1 | Fork full `codex-tui` crate as dependency | ❌ Blocked | `codex-tui` Cargo.toml lists 30+ `codex-*` workspace crates; not publishable standalone. | **Cherry-pick modules** into `tui/vendor/codex-ui/`; do not add `codex-tui` as a Cargo dependency. |
| 2 | Reuse `ChatWidget` wholesale | ❌ Blocked | `chatwidget.rs` is ~10k lines, tightly coupled to `App`, `codex_core::Event`, app-server session. | Port **sub-widgets** (`history_cell`, `bottom_pane`, `diff_render`, `exec_cell`, `status`) and compose via `CusaViewModel`; do not port `chatwidget.rs` monolith. |
| 3 | Codex patched ratatui/crossterm | ⚠️ Unverified | Upstream uses ratatui features: `scrolling-regions`, `unstable-*`; custom `custom_terminal` module. | **Phase 0**: evaluate `custom_terminal`; enable matching ratatui features in workspace `Cargo.toml` or shim missing APIs. Waive full patch fork unless snapshots fail. |
| 4 | Upstream tracking | ⚠️ Unverified | No pin exists today. | Pin to a **specific git SHA** recorded in `tui/vendor/codex-ui/UPSTREAM` at import time; document cherry-pick procedure in `tui/vendor/README.md`. |
| 5 | cusa-rpc bridge unchanged | ✅ Verified | Sidecar + JSON-RPC already production path. | Adapter consumes existing `SidecarEvent` / `AppState`; no RPC schema changes for UI port. |
| 6 | License / attribution | ✅ Verified | `scripts/check-headers.sh`, `THIRD_PARTY_NOTICES.md` exist. | Extend header check to list each vendored file with OpenAI provenance comment. |

**User decision (2026-07-06):** Proceed with **Approach 1** (cherry-pick + decouple), not from-scratch visual replication.

## Excluded Sections

| Section | Reason |
|---------|--------|
| NFR: Availability | Single-user dev CLI |
| NFR: Scalability | No multi-tenant load |
| Deployment / infra | UI-only change inside existing `cusa-tui` binary |
| API endpoint tables | No new HTTP/RPC surface |

## Functional Requirements

### User stories

- **US-001:** As a cusa user, I want the input area to behave like Codex (multi-line, expanding composer), so that I can paste code blocks without losing formatting.
- **US-002:** As a cusa user, I want assistant turns, tool calls, and file edits rendered like Codex (markdown, diffs, exec output), so that I can scan agent activity quickly.
- **US-003:** As a maintainer, I want vendored Codex files pinned and attributed, so that we can legally ship and selectively merge upstream fixes.
- **US-004:** As a maintainer, I want snapshot tests on ported widgets, so that UI regressions are caught in CI.

### Main flow

1. **Import (one-time per upstream bump):** Maintainer runs `scripts/vendor-codex-ui.sh <sha>` → copies allowlisted paths into `tui/vendor/codex-ui/` → records SHA in `UPSTREAM`.
2. **Adapter build:** `CusaViewModel` implements conversions from `AppState` / sidecar stream → Codex widget props (per phase).
3. **Compose:** `app::draw()` renders ported widgets instead of custom `InputWidget` / `TranscriptWidget` / etc.
4. **Verify:** `cargo test -p cusa-tui` + `cargo insta test` → review `.snap.new` diffs → accept when parity met.
5. **Retire:** Delete superseded cusa widget code; update traceability matrix (SPEC-070 → truly implemented).

### Phased cherry-pick plan

| Phase | Upstream modules (indicative) | Replaces in cusa | Exit criterion |
|-------|------------------------------|------------------|----------------|
| **P0 Foundation** | `custom_terminal`, `style`, `ui_consts`, `terminal_palette`, `color`, `wrapping`, `width`, `text_formatting` | Terminal backend setup in `app/mod.rs` | App boots; existing tests green; no visual change yet |
| **P1 Bottom pane** | `bottom_pane/**`, `public_widgets/composer_input.rs`, `clipboard_paste`, `key_hint` (subset) | `app/input.rs`, footer hints integration | Multi-line input + Codex composer snapshots match upstream fixtures |
| **P2 Transcript** | `history_cell/**`, `thread_transcript`, `markdown_render`, `markdown`, `streaming` (render-only), `transcript_reflow` | `app/transcript.rs` | User/assistant/tool lines match Codex snapshots for fixture transcript |
| **P3 Tool display** | `diff_model`, `diff_render`, `exec_cell`, `exec_command` (render-only) | ToolCall/ToolResult rendering in transcript | File-edit and shell blocks match Codex snapshots |
| **P4 Chrome** | `status`, `status_indicator_widget`, `render` helpers, `shimmer` (optional) | `app/status.rs`, `app/footer.rs` (partial) | Header/status/footer snapshots match Codex |
| **P5 Cleanup** | — | Remove dead code; update docs | No references to retired widgets; SPEC-070 compliance grep passes |

Phases are sequential; a phase may not merge until its snapshot gate passes.

### Screens / UI regions (post-port)

| Region | Codex source | cusa data source |
|--------|--------------|------------------|
| Header | `status` (in-scope subset) | `AppState::session` (cwd, short id); brand text `cusa` not `Codex` |
| Status line | `status_indicator_widget` | model, approval mode, skills/mcp counts, token usage from `UsageAccumulator` |
| Transcript | `history_cell` + `thread_transcript` | `TranscriptEntry` + live `TurnState` via `CusaViewModel` |
| Input / bottom pane | `bottom_pane` | `AppState::input`, key events from `events.rs` |
| Overlays | Keep cusa `overlay.rs` for `/model`, `/skills`, etc. | Unchanged RPC flows; restyle to Codex palette where trivial |
| Footer | `key_hint` patterns | `RunPhase` hints (existing logic) |

**Branding rule:** Magenta agent identity string reads **`cusa`**, not `Codex` — only styling is inherited.

### Adapter contract (`CusaViewModel`)

The adapter MUST:

- Live in `tui/src/codex_adapter/` (new module); vendor code MUST NOT import `crate::app` or `crate::sidecar`.
- Expose pure functions / structs, e.g. `fn history_cells(entries: &[TranscriptEntry], live: Option<&TurnState>) -> Vec<HistoryCellView>`.
- Map `cusa_rpc::RouterSource`, `ApprovalMode`, tool events to Codex-equivalent display enums defined **in the adapter** (not in vendor tree).
- Remain synchronous for render path; no `async` in widget construction.

The adapter MUST NOT:

- Call Cursor SDK or spawn sidecar.
- Depend on any `codex-*` crate from crates.io or the Codex workspace.

### Dependencies / assumptions

- **Assumption:** Apache-2.0 license on upstream `codex-rs/tui` permits vendoring with attribution (verified: upstream `codex-rs` is Apache-2.0).
- **Assumption (user-waived):** Full ratatui patch fork is **not** required for v1 if enabling documented ratatui 0.29 features achieves snapshot parity within 2px/line tolerance.
- **Assumption:** Insta + `ratatui::backend::TestBackend` remain the snapshot harness (already used in cusa TUI tests).
- **Depends on:** `20260706-cusa-cli` sidecar RPC and `AppState` shape remaining stable during UI port (adapter absorbs breaking changes).

## Non-Functional Requirements

### Performance

- Frame draw path: no more than **+2 ms p95** vs current custom widgets on M2 / 80×24 (measured with `cargo test` micro-bench or manual `tracing` span around `draw()`).
- Ported widgets use Codex lazy/virtualized patterns where upstream already does; do not regress scroll performance on transcripts **> 200 entries**.

### Security

- Vendored upstream code is render-only; must not add network calls, shell execution, or file writes beyond what cusa already performs.
- Review vendored imports each bump for new `unsafe` or `Command::new` usage.

## Architecture / ADR

### ADR-UI-01: Cherry-pick modules, not monolithic fork

- **Chosen:** Vendor allowlisted `codex-rs/tui/src/**` files + adapter layer.
- **Rejected:** Cargo dependency on `codex-tui` crate (pulls entire Codex workspace).
- **Rejected:** Continue from-scratch replication (already attempted; insufficient parity).
- **Trade-off:** Higher initial decoupling cost; lower long-term drift vs Codex UX.

### ADR-UI-02: Adapter boundary

- **Chosen:** `tui/src/codex_adapter/` owns all cusa ↔ Codex type translation.
- **Trade-off:** Some duplicated enum definitions vs upstream protocol types.

### Directory layout (target)

```
tui/
├── src/
│   ├── codex_adapter/     # NEW — CusaViewModel, type mappings
│   ├── app/               # draw() composes ported widgets
│   └── ...
├── vendor/
│   └── codex-ui/          # NEW — vendored upstream files + UPSTREAM sha
│       └── README.md
└── tests/
    └── snapshots/         # insta snapshots per phase
scripts/
└── vendor-codex-ui.sh     # NEW — allowlisted copy + header verify
```

## Edge cases

| Case | Expected behavior |
|------|-------------------|
| Upstream bump breaks adapter compile | CI fails; adapter updated in same PR as `UPSTREAM` sha change |
| Terminal lacks truecolor | Codex palette falls back to ANSI semantic colors (upstream behavior) |
| Narrow terminal (< 60 cols) | Transcript reflows without panic; composer shrinks per upstream `resize_reflow` logic |
| Sidecar streams tool result while diff pending | Adapter queues cells; final render matches ordering in existing cusa transcript tests |
| User on Windows | P1 composer must support bracketed paste if upstream requires; no regression vs current cusa Windows build |

## Risks / open questions

| ID | Risk | Mitigation |
|----|------|------------|
| R-UI-1 | Decoupling `history_cell` from `codex_core::Event` takes longer than 2 weeks | Time-box per phase; ship P1+P2 before P3 if needed |
| R-UI-2 | ratatui feature gap vs Codex patches | Document waiver in adapter; shim in `codex_adapter/shim.rs` |
| R-UI-3 | Upstream file moves between bumps | Allowlist + script; pin SHA; manual merge |
| OQ-UI-1 | Initial upstream pin SHA | Set at first `vendor-codex-ui.sh` run (recommend `main` at port start date) |

## Acceptance criteria

- **AC-UI-1.** Given a fresh `make run-tui` on 80×24 terminal, when the app is idle, then the bottom pane matches the Codex composer snapshot (`p1_composer_idle.snap`) within the insta review process.
- **AC-UI-2.** Given a transcript fixture with user prompt, router line, streaming assistant text, tool call, and diff, when rendered, then output matches Codex snapshot (`p2_transcript_mixed.snap`).
- **AC-UI-3.** Given `scripts/check-headers.sh`, when run after vendoring, then every file under `tui/vendor/codex-ui/` includes Apache-2.0 marker and `UPSTREAM` records the git SHA.
- **AC-UI-4.** Given `grep -r 'codex_core' tui/vendor tui/src/codex_adapter`, when run on main, then zero matches (no codex-core coupling in vendored/adapter code).
- **AC-UI-5.** Given an existing cusa integration test (`spec_001_*`, `spec_004_*`), when the UI port merges, then all tests pass without weakening assertions.
- **AC-UI-6.** Given `THIRD_PARTY_NOTICES.md`, when the port completes, then it lists vendored paths and upstream URL — not a vague `tui/src/**` blanket claim.

## Spec Items

| ID | Requirement | Priority |
|----|-------------|----------|
| SPEC-103 | `scripts/vendor-codex-ui.sh` copies allowlisted upstream paths into `tui/vendor/codex-ui/` and writes `UPSTREAM` sha file | P0 |
| SPEC-104 | `CusaViewModel` adapter module translates `AppState` + transcript/live turn into Codex widget view types without `codex-*` deps | P0 |
| SPEC-105 | Phase P0 foundation modules vendored; `draw()` uses `custom_terminal` (or documented shim) | P0 |
| SPEC-106 | Phase P1: `bottom_pane` replaces `InputWidget`; multi-line composer works; SPEC-005 satisfied via upstream behavior | P0 |
| SPEC-107 | Phase P2: `history_cell`/`thread_transcript` pipeline replaces `TranscriptWidget` for all `TranscriptEntry` variants | P0 |
| SPEC-108 | Phase P3: `diff_render` + `exec_cell` render file edits and shell output from tool stream events | P1 |
| SPEC-109 | Phase P4: status chrome ported; header shows `cusa` branding with Codex styling | P1 |
| SPEC-110 | Insta snapshot tests per phase; CI fails on unreviewed `.snap.new` | P0 |
| SPEC-111 | `scripts/check-headers.sh` extended to scan `tui/vendor/codex-ui/` with per-file OpenAI provenance | P0 |
| SPEC-112 | `THIRD_PARTY_NOTICES.md` lists concrete vendored paths (not `tui/src/**` blanket) | P0 |
| SPEC-113 | Retired widgets (`input.rs`, `transcript.rs` superseded parts) removed after parity gate | P1 |
| SPEC-114 | Workspace `ratatui`/`crossterm` features documented in `tui/Cargo.toml` comments matching ported code needs | P1 |
| SPEC-115 | `specs/20260706-cusa-cli.traceability.md` SPEC-070 note updated to reference this spec | P1 |

## Change history

| Date | Change |
|------|--------|
| 2026-07-06 | Initial draft — Approach 1 cherry-pick spec per user direction |
