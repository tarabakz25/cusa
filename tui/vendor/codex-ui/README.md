# Vendored Codex TUI UI modules

Cherry-picked source from [openai/codex](https://github.com/openai/codex)
`codex-rs/tui/src/`. These files are copied (not submodule-linked) so cusa can
adapt imports without pulling the full Codex workspace.

Upstream pin is recorded in [`UPSTREAM`](./UPSTREAM).

## Cherry-pick procedure

1. Choose an upstream commit (usually `main` or a tagged release) from
   https://github.com/openai/codex/commits/main
2. From the repo root, run:

   ```bash
   bash scripts/vendor-codex-ui.sh <git-sha-or-branch>
   ```

3. Review the diff under `tui/vendor/codex-ui/` — resolve any import or API
   drift in `tui/src/codex_adapter/` (not in this tree).
4. Run compliance checks:

   ```bash
   bash scripts/check-headers.sh
   ```

5. Commit the updated vendor tree, `UPSTREAM`, and any adapter fixes together.

Re-running the script at the same ref is idempotent.

## P0 allowlist (foundation phase)

Copied by `scripts/vendor-codex-ui.sh` today:

| Path | Notes |
|------|-------|
| `custom_terminal.rs` | Ratatui terminal backend (derived from upstream ratatui) |
| `style.rs` | Semantic styles |
| `ui_consts.rs` | Layout constants |
| `terminal_palette.rs` | Truecolor / ANSI palette |
| `color.rs` | Color utilities |
| `wrapping.rs` | Word wrap with URL heuristics |
| `width.rs` | Narrow-terminal width guards |
| `text_formatting.rs` | Unicode width / capitalization helpers |
| `render/` | Required by `wrapping.rs` (`render::line_utils`) |

Later phases (P1–P4) extend the allowlist in the vendor script — see
`specs/20260706-codex-tui-cherry-pick.md`.

## Bumping upstream

1. Run `scripts/vendor-codex-ui.sh <new-sha>`.
2. Confirm `UPSTREAM` records the new SHA and import date.
3. Update `THIRD_PARTY_NOTICES.md` if the vendored path set changed.
4. Fix `codex_adapter` and run `cargo test -p cusa-tui` before merging.

## License

Upstream `codex-rs` is Apache-2.0. Vendored `.rs` files retain upstream headers
where present; the vendor script prepends provenance and SPDX markers when
missing. See the root [`THIRD_PARTY_NOTICES.md`](../../../THIRD_PARTY_NOTICES.md).
