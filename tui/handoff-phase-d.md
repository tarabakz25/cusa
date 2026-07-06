# Phase D — TUI-side features handoff

_Author: Phase D subagent, 2026-07-06._

Phase D bundles the TUI half of three slices — Slice 3 (Router UX), Slice 4
(Approval UX), Slice 5 (Skills + MCP overlays). This document lists what
was shipped, the new keybindings, and any items that block real end-to-end
use with a live sidecar.

## Repo state

- Build: `cargo build --workspace` ✔
- Tests: `cargo test --workspace` = **138 tests pass** (44 cusa-rpc, 93
  cusa-tui unit, 1 headers integration).
- Clippy: `cargo clippy --workspace --all-targets -- -D warnings` ✔
- Headers: `bash scripts/check-headers.sh` ✔

## Slash commands

The grammar in `app::slash::parse` now recognizes these forms:

| Command                | Parsed variant                     | Behavior                                                                 |
|------------------------|------------------------------------|--------------------------------------------------------------------------|
| `/model`               | `Model(None)`                      | Opens the model picker overlay (fires `models/list` on first open).      |
| `/model <id>`          | `Model(Some(id))`                  | Sets sticky manual override; status line shows `[override]`.             |
| `/model auto`          | `Model(Some("auto"))`              | Clears the override; toast "auto mode restored".                         |
| `/approval`            | `Approval(None)`                   | Cycles mode _and_ opens the picker overlay so it can be confirmed.       |
| `/approval <name>`     | `Approval(Some(name))`             | Sets mode directly. Accepts `suggest`, `auto-edit`, `full-auto`.         |
| `/mode`, `/mode <name>`| `Mode(_)`                          | Alias for `/approval`.                                                   |
| `/skills`              | `Skills`                           | Opens the skills toggle overlay (fires `skills/list`).                   |
| `/mcp`                 | `Mcp`                              | Opens the MCP server list overlay (fires `mcp/list`).                    |
| `/cost`, `/resume`, `/context` | stubs, still toast-only    | Deferred to later slices per the original plan.                          |
| `/help`, `/clear`, `/reset`, `/quit` | unchanged                | Slice 2 semantics preserved.                                             |

Argument variants are `Option<String>` for Model/Approval/Mode; Skills and
Mcp take no args (a trailing arg is ignored). `is_stub()` now returns
`true` only for Cost / Resume / Context.

## Key bindings

### Global (no overlay)

| Key   | Action                                                              |
|-------|---------------------------------------------------------------------|
| Tab   | **SPEC-021**: cycle approval mode (suggest → auto-edit → full-auto). |
| Ctrl-C| SPEC-004: unchanged (cancel run / double-tap quit).                 |
| Enter | Submit prompt or slash command.                                     |
| ←/→/Home/End/Backspace | Slice 2 input editing.                                   |

### Approval modal (SPEC-022, SPEC-023)

| Key  | Decision   | Sends                                                        |
|------|------------|--------------------------------------------------------------|
| `y`  | approve    | `tool/approvalResponse { decision: "approve" }`              |
| `n`  | deny       | `tool/approvalResponse { decision: "deny" }`                 |
| `a`  | always     | approve + insert into per-session `always_approved_tools`    |
| `Esc`| dismiss    | acts as deny + closes modal                                  |

`a` populates `AppState::always_approved_tools`. On subsequent
`tool/approvalRequest` for the same tool name, the TUI **auto-approves
locally without opening the modal** and appends a dim
`shell_exec: always (auto)` note to the transcript (`TranscriptEntry::ToolDecision`).

### Model picker (SPEC-016)

| Key   | Action                                              |
|-------|-----------------------------------------------------|
| ↑ / ↓ | Move cursor.                                        |
| Enter | Apply the selected id (or clear override if `auto`).|
| Esc   | Cancel without changing state.                      |

`models/list` responses are cached on `AppState::models_cache` so the
overlay is instant after the first open.

### Approval picker (SPEC-021)

| Key       | Action                                     |
|-----------|--------------------------------------------|
| `1/2/3`   | Pick suggest / auto-edit / full-auto.      |
| ↑ / ↓     | Move cursor.                               |
| Enter     | Apply selection.                           |
| Esc       | Cancel.                                    |

### Skills overlay (SPEC-032)

| Key       | Action                                                                  |
|-----------|-------------------------------------------------------------------------|
| ↑ / ↓     | Move cursor.                                                            |
| Space     | Toggle enabled flag on the cursor row.                                  |
| Enter     | Commit: send `skills/setEnabled { sessionId, skillIds }` and close.     |
| Esc       | Cancel without persisting.                                              |

Committing also updates `AppState::session::enabled_skill_ids` and
`skills_count` so the status-line badge (`skills(N)`) updates immediately.

### MCP overlay (SPEC-042)

| Key           | Action                                                              |
|---------------|---------------------------------------------------------------------|
| ↑ / ↓         | Move cursor.                                                        |
| → / Enter     | Toggle expansion of the cursor row (reveal tool list).              |
| Space         | Toggle enabled flag → fires `mcp/toggle`.                           |
| Esc           | Close.                                                              |

Server status is color-coded: green (ready), yellow (starting), red
(failed), dim (disabled). Rows show name · transport · status · [enabled|
disabled]. Expanded rows list each tool by name + description.

## Always-cache scope (SPEC-022)

`AppState::always_approved_tools: HashSet<String>` is:
- **Session-scoped.** Cleared by `AppState::clear_session_caches()`, which
  is called from `/reset` (and can be called by future session-dispose
  paths).
- **Tool-name-keyed.** No per-argument specificity — this matches the spec
  wording ("remember 'always' for the session for this tool name").
- **Additive only.** No UI to remove; a `/reset` is the correct escape.
- **Client-side.** The sidecar is not told about "always" — the TUI simply
  responds `approve` on the next matching `tool/approvalRequest`
  automatically.

## Model override propagation (SPEC-016)

`spawn_send_prompt(client, session_id, text, model_override: Option<String>)`
carries the override on every `session/send`. Callers:

- `submit_input` reads `state.session.manual_model_override.clone()`.
- The override is a string id copied verbatim into the JSON as
  `modelOverride`. It survives across `/clear` (transcript wipe only) and
  is cleared by `/reset` (session dispose) and `/model auto`.

The status line now renders `<model> [override]` in bold yellow when the
override is active.

## Router-decision colorization (SPEC-012)

`TranscriptEntry::RouterDecision` now carries a `source: RouterSource`.
`app::transcript::router_source_style` maps:

| source     | color        | tag        |
|------------|--------------|------------|
| override   | yellow       | `override` |
| rule       | cyan         | `rule`     |
| llm        | magenta      | `llm`      |
| fallback   | dim gray     | `fallback` |

Rendered as `→ <model> · <tag> · <rationale>` with the arrow + model + tag
colored, rationale in dim gray. Snapshot: `spec_012_router_decision_line_uses_source_color_for_rule`.

## New / changed source files

```
tui/src/app/
├── mod.rs              # event loop wiring; Tab hotkey; internal-event dispatch; slash dispatch expansion
├── approval.rs         # NEW — approval overlay logic, always-cache, tool/approvalResponse dispatch
├── skills.rs           # NEW — /skills overlay orchestration + skills/list + skills/setEnabled
├── mcp.rs              # NEW — /mcp overlay orchestration + mcp/list + mcp/toggle
├── model_picker.rs     # NEW — /model picker overlay + models/list cache
├── internal.rs         # NEW — AppInternalEvent channel used by async tasks → event loop
├── overlay.rs          # expanded — ModelPicker, ApprovalPicker, Skills, Mcp; ApprovalOverlay carries request_id + category
├── slash.rs            # widened SlashCommand grammar (Option<String> for Model/Approval/Mode; unit variants for Skills/Mcp)
├── state.rs            # AppState: manual_model_override, enabled_skill_ids, always_approved_tools, models_cache, internal_tx; SessionView additions
├── status.rs           # renders `[override]` badge next to model
├── transcript.rs       # RouterDecision carries source; ToolDecision variant for approvalResult notes
└── footer.rs           # (unchanged behavior; test updated for new on_router_decision signature)
```

Nothing under `tui/crates/cusa-rpc/src/**` was modified — the sidecar
subagent had already appended `ToolApprovalResult` and
`ApprovalResolution`, which the TUI now handles as a dim transcript entry.

## Cross-task plumbing

Overlays that need async responses (`/model`, `/skills`, `/mcp`) all use
the same pattern:

1. Slash-command handler puts a `loading` overlay onscreen.
2. A tokio task is spawned to call the RPC (`tokio::runtime::Handle::try_current`
   is checked so unit tests without a runtime remain safe).
3. On response, the task sends an `AppInternalEvent` on `state.internal_tx`.
4. The event loop `select!`s a third channel and calls
   `app::apply_internal_event`, which populates the overlay.

Tests can drive this by calling `apply_internal_event` directly — no
runtime required.

## Schema additions we relied on

The sidecar subagent's parallel work has already added everything we need
in `cusa-rpc`:

- `RouterSource` (Rule / Llm / Override / Fallback) on `RouterDecisionParams`.
- `ToolCategory` on `ToolApprovalRequestParams`.
- `SessionSendParams::model_override`.
- `SkillsListResult` + `SkillsSetEnabledParams`.
- `McpListResult` + `McpToggleParams` + `McpToggleResult`.
- `ToolApprovalResult` + `ApprovalResolution` (observational).

**Items we still need from the sidecar (not blocking Phase D but noted):**

1. **Approval-mode persistence method** — `/approval` currently updates
   `AppState::session::approval_mode` locally only. The spec's Slice 4
   deliverable mentions "persist the change into the sidecar by calling a
   new RPC method" (e.g. `session/setApprovalMode`). If the sidecar
   subagent has not landed one, mode changes revert on `session/resume`.
   Once the method exists, wire it into `set_approval_mode` in `app::mod`.

2. **Session-scoped approval mode read-back** — no schema change needed;
   the SDK-driven sidecar could optionally emit a
   `session/approvalModeChanged` notification so external drivers stay in
   sync. Nice to have.

## Testing summary

Every new/edited SPEC has a `spec_NNN_...`-named test:

- **SPEC-012**: `spec_012_router_decision_line_uses_source_color_for_rule`
- **SPEC-016**:
  - `spec_016_slash_model_id_sets_override_and_next_send_carries_it`
  - `spec_016_slash_model_auto_clears_override`
  - `spec_016_model_picker_populates_from_internal_event`
  - `spec_016_model_picker_snapshot_lists_models`
  - `spec_016_parse_model_variants`
  - `spec_016_status_line_shows_override_marker`
- **SPEC-021**:
  - `spec_021_slash_approval_cycles_modes`
  - `spec_021_tab_key_cycles_modes_when_no_overlay`
  - `spec_021_parse_approval_and_mode_variants`
  - `spec_021_approval_picker_snapshot_lists_three_modes`
- **SPEC-022**:
  - `spec_022_approval_prompt_sends_approve_on_y`
  - `spec_022_approval_prompt_sends_deny_on_n`
  - `spec_022_approval_always_marks_tool_and_auto_approves_next_call`
  - `spec_022_approval_overlay_renders_category_and_hints`
- **SPEC-023**: `spec_023_auto_edit_prompts_on_shell_tool_category`
- **SPEC-032**:
  - `spec_032_slash_skills_calls_list_and_renders_toggles`
  - `spec_032_toggling_a_skill_and_confirming_persists_via_set_enabled`
  - `spec_032_skills_overlay_lists_toggles`
- **SPEC-042**:
  - `spec_042_slash_mcp_calls_list_and_renders_servers_with_transport`
  - `spec_042_expanded_row_shows_tool_list`
  - `spec_042_mcp_overlay_renders_servers_with_transport_and_status`
  - `spec_042_mcp_expanded_row_shows_tool_list`
- **SPEC-032/042 parser**: `spec_032_042_parse_skills_and_mcp`

## Clippy allowances

None new. The workspace-level `-A clippy::field_reassign_with_default`
from Slice 2 is retained (still triggered by the pre-existing `cusa-rpc`
tests we cannot touch).

## Follow-ups that block real E2E use

1. **Approval-mode persistence.** Until the sidecar exposes a
   `session/setApprovalMode` (or similar) RPC, `/approval` and `Tab` only
   change the TUI's local state. The sidecar keeps whatever mode was set
   at `session/create` time. This is a one-line addition in
   `set_approval_mode`.
2. **Model override propagation confidence.** The TUI now sends
   `modelOverride` on every `session/send`. Confirm the sidecar's
   `session/send` handler actually consumes it and short-circuits the
   router when present (SPEC-016 sidecar half).
3. **Skills warnings channel.** `SkillsListResult::warnings` is displayed
   inline in the overlay but not persisted anywhere. If SPEC-034
   (16 KiB cap) surfaces truncation, it lands via this channel.
