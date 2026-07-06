# cusa

Cursor-SDK-powered coding CLI with transparent auto-mode. `cusa` is a
terminal coding agent that pairs the Cursor SDK's multi-provider model access
with a Codex-styled TUI and a **deterministic, per-turn model router** —
replacing Cursor's opaque built-in auto with a router you can read, edit, and
override.

> Status: pre-alpha. Working name `cusa` may be renamed before v1.

See [`specs/20260706-cusa-cli.md`](specs/20260706-cusa-cli.md) for the
authoritative specification.

## Highlights

- **Transparent auto mode.** Every turn shows the model name and a one-line
  rationale before streaming starts.
- **Codex-styled TUI.** Rust + Ratatui, forked from OpenAI's Codex TUI
  (Apache-2.0).
- **Node.js sidecar hosts `@cursor/sdk`.** JSON-RPC over stdio between TUI
  and sidecar.
- **Cursor-format skills** loaded from `~/.cursor/skills/**/SKILL.md`.
- **Layered MCP config** matching Cursor precedence.
- **Codex-style approval modes**: `suggest`, `auto-edit`, `full-auto`.
- **Local sessions** stored in `~/.cusa/sessions.json`; resume by picking or
  `--resume <agentId>`.
- **Cumulative + per-turn token telemetry** in the status line and `/cost`.

## Requirements

- Node.js **≥ 20**
- macOS (arm64/x64), Linux (x64/arm64), or Windows (x64)
- A Cursor account and API key (`CURSOR_API_KEY`)

## Install (planned)

```sh
npm install -g cusa
```

The npm package's `postinstall` step downloads the prebuilt TUI binary for
your platform and pins the sidecar's `@cursor/sdk` version.

## Repo layout

```
.
├── tui/               # Rust TUI (Ratatui) — forked from codex-tui
│   ├── Cargo.toml
│   ├── src/
│   └── crates/
│       └── cusa-rpc/  # JSON-RPC schema, shared with sidecar
├── sidecar/           # Node.js sidecar hosting @cursor/sdk
│   ├── package.json
│   ├── tsconfig.json
│   └── src/
├── npm/               # npm-publish artifact (postinstall + shim)
│   ├── package.json
│   ├── bin/
│   └── postinstall.js
├── scripts/
│   └── check-headers.sh   # SPEC-083: verify fork license headers
├── specs/             # Spec-driven development sources
├── LICENSE            # Apache-2.0
├── THIRD_PARTY_NOTICES.md
└── README.md
```

## Development

```sh
# Sidecar
cd sidecar
npm install
npm run build

# TUI
cd tui
cargo build

# End-to-end (run TUI, which spawns the sidecar)
./tui/target/debug/cusa-tui --sidecar ./sidecar/dist/index.js
```

Detailed slice-by-slice implementation notes live in
[`specs/20260706-cusa-cli.md`](specs/20260706-cusa-cli.md).

## Attribution

Portions of the TUI are derived from OpenAI's Codex Rust TUI. See
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

## License

Apache License 2.0 — see [`LICENSE`](LICENSE).
