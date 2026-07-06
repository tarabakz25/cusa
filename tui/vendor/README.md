# Vendored Codex TUI modules

Cherry-picked UI modules from [OpenAI `codex-rs/tui`](https://github.com/openai/codex/tree/main/codex-rs/tui) live under `codex-ui/`.

## Import procedure

1. Run `scripts/vendor-codex-ui.sh <upstream-sha>` (SPEC-103).
2. Re-apply decoupling patches: replace `codex_*` imports with `crate::codex_adapter` shims.
3. Record the SHA in `codex-ui/UPSTREAM`.
4. Run `cargo test -p cusa-tui`.

P0 foundation modules: `custom_terminal`, `style`, `ui_consts`, `terminal_palette`, `color`, `width`, `text_formatting`.
