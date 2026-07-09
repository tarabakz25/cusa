# cusa

## What This Is

Terminal coding agent pairing a Rust/Ratatui TUI with a Node.js sidecar that hosts `@cursor/sdk`. The TUI and sidecar communicate via JSON-RPC over stdio. A deterministic per-turn model router replaces Cursor's opaque auto mode. Authoritative design: `specs/20260706-cusa-cli.md`.

## Tech Stack

- **Rust 1.85+** — TUI (`tui/`, Ratatui, tokio, crossterm); workspace crate `cusa-rpc`
- **Node.js ≥ 20** — sidecar (`sidecar/`, TypeScript ESM, `@cursor/sdk`)
- **npm wrapper** — publish artifact in `npm/` (postinstall downloads TUI binary)
- **Make** — top-level orchestration over cargo, npm, and bash scripts

## Build & Run

- Setup: `make setup`
- Build: `make build` (cargo debug + sidecar tsc)
- Dev sidecar: `make dev-sidecar`
- Run TUI: `make run-tui` (sets `CUSA_SIDECAR` to `sidecar/dist/index.js`)
- Test: `make test`
- Lint: `make lint` (fmt-check, clippy, typecheck, check-headers)
- CI locally: `make ci`

Sidecar only: `cd sidecar && npm install && npm run build`
Manual TUI: `./tui/target/debug/cusa-tui --sidecar ./sidecar/dist/index.js`

## Project Structure

- `tui/` — Rust TUI binary (`cusa-tui`); vendored Codex UI under `vendor/codex-ui/`
- `tui/crates/cusa-rpc/` — shared JSON-RPC schema (Rust + sidecar drift tests)
- `sidecar/src/` — agent session, router, MCP, skills, RPC server
- `npm/` — global install shim and postinstall
- `scripts/` — `run-all-tests.sh`, `build-release.sh`, `check-headers.sh`
- `specs/` — spec-driven development sources

## Code Style

- SPDX Apache-2.0 header on new source files (`Copyright 2026 cusa contributors`)
- Rust: `cargo fmt --all`; clippy with `-D warnings`; inline `#[cfg(test)]` modules
- TypeScript: ESM imports with `.ts` extensions; `node:test` + `node:assert/strict`
- Forked Codex TUI files must retain attribution (enforced by `make check-headers`)

## Testing

- Command: `make test` (rust + sidecar + npm + shell suites)
- Rust snapshots: `make test-tui-snapshots`; review with `cargo insta review -p cusa-tui`
- Sidecar: `cd sidecar && npm test` — co-located `*.test.ts` next to source
- Skip suites via env: `SKIP_RUST=1`, `SKIP_SIDECAR=1`, `SKIP_NPM=1`, `SKIP_SCRIPTS=1`

## Conventions

- Spec IDs (e.g. SPEC-083, SPEC-110) reference `specs/`; read before large changes
- Sessions stored at `~/.cusa/sessions.json`; skills from `~/.cursor/skills/**/SKILL.md`
- API key via `CURSOR_API_KEY` env — never commit keys or `.env` values
- Branch naming: `feat/`, `fix/` prefixes observed in recent history

## Do Not Do

- Commit secrets, API keys, or machine-specific paths
- Edit generated output (`target/`, `sidecar/dist/`, `npm/binaries/`)
- Remove or strip Apache-2.0 headers on forked Codex files
- Commit unreviewed TUI snapshot diffs (`*.snap.new` under `tui/tests/snapshots/`)
- Invent build/test commands not present in the Makefile or `package.json` scripts
- Treat pre-alpha install (`npm install -g cusa`) as shipped until release workflow is enabled

## Current Goal

<!-- Update when starting focused work -->
