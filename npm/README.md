# cusa

Terminal coding agent that pairs the Cursor SDK's multi-provider model access
with a Codex-styled TUI and a **deterministic, per-turn model router**.

## Install

```sh
npm install -g cusa
```

The `postinstall` step downloads the prebuilt TUI binary for your platform
from GitHub Releases (SPEC-080), verifies its SHA-256 checksum, and installs
it under `$CUSA_HOME/bin/<target>/`. The bundled Node.js sidecar (which owns
`@cursor/sdk`) ships in the same package.

## Requirements

- Node.js **≥ 20** (SPEC-082)
- macOS (arm64 or x64), Linux (x64 or arm64), or Windows (x64) (SPEC-081)
- `CURSOR_API_KEY` in the environment **or** the key saved via `cusa login`

## First-time setup

Set an API key one of two ways:

```sh
export CURSOR_API_KEY=cursor_...
```

or, interactively:

```sh
cusa login              # prompts on the TTY (input is hidden)
cusa login --stdin      # reads the key from stdin (for scripts / CI)
cusa login --key "..."  # passes the key inline (least secure)
```

`cusa login` writes to `$CUSA_HOME/config.toml` with file mode `0600`
(SPEC-101). On Windows, POSIX file modes don't apply the same way; pass
`--force-windows` to acknowledge the limitation.

## Usage

```sh
cusa                          # start in the current repo
cusa --resume <agentId>       # resume a specific session
cusa --verbose                # write RPC + stream logs to $CUSA_HOME/logs/
cusa --approval=full-auto     # full auto-approve (POSIX only; see below)
cusa download-binary [--force] [--target=darwin-arm64]
                              # re-run the postinstall fetch or install a
                              # different target's binary (useful in
                              # cross-install scenarios / air-gapped hosts)
cusa login [--stdin|--key <k>]  # save the API key to config.toml (0600)
cusa --version                # print the npm shim version
```

Full documentation and slash-command reference: run `cusa` and type `/help`.

## Environment variables

| Variable                    | Purpose                                                                                          |
| --------------------------- | ------------------------------------------------------------------------------------------------ |
| `CURSOR_API_KEY`            | Cursor API key. Read by the sidecar; never printed to the TUI (SPEC-100).                        |
| `CUSA_HOME`                 | Config + cache root. Defaults to `~/.cusa`.                                                      |
| `CUSA_TUI`                  | Absolute path to a local `cusa-tui` binary. Overrides both the bundled and cached binary.        |
| `CUSA_SIDECAR`              | Absolute path to the sidecar entry (`sidecar/dist/index.js`). Handy for local sidecar dev.       |
| `CUSA_NODE`                 | Node interpreter to use for the sidecar (reserved; the TUI reads this).                          |
| `CUSA_SKIP_POSTINSTALL`     | Set to `1` to skip the postinstall binary download entirely.                                     |
| `CUSA_ALLOW_CI_DOWNLOAD`    | Set to `1` to let postinstall download in CI (default: skip when `CI=1`).                        |
| `CUSA_RELEASE_BASE_URL`     | Override the GitHub Releases base URL (for staging / local testing).                             |
| `HTTP_PROXY` / `HTTPS_PROXY`| Standard proxy env vars. The postinstall fetch honors them for corporate networks (R-6).         |
| `NO_PROXY`                  | Comma-separated hostnames to exclude from proxy usage.                                           |

## Platform notes

- **Windows + `full-auto`.** Cursor SDK's local sandbox is POSIX-only. On
  Windows, `--approval=full-auto` (and the `--full-auto` shortcut) is
  auto-downgraded to `--approval=auto-edit` and a warning is printed
  (SPEC risk R-7). The TUI enforces the same rule defensively.
- **Air-gapped / corporate networks.** If postinstall can't reach GitHub, it
  exits `0` with a recovery hint. Rerun with `cusa download-binary` when the
  network is available, or point `CUSA_TUI` at a locally-built binary.

## What this slice ships (Slice 8)

Implemented in this npm package:

- **SPEC-080** — `postinstall` fetch + SHA-256 verify + install to
  `$CUSA_HOME/bin/<target>/<exe>` (0755).
- **SPEC-081** — platform detection (`darwin`/`linux`/`win32` × `arm64`/`x64`).
- **SPEC-082** — Node.js ≥ 20 enforcement in the shim.
- **SPEC-083** — `THIRD_PARTY_NOTICES.md` shipped in the publish tarball.
- **SPEC-101** — `cusa login` writes `$CUSA_HOME/config.toml` at mode `0600`.
- **SPEC-102** — `--verbose` ensures `$CUSA_HOME/logs/` exists with mode `0700`.
- **R-6** — `cusa download-binary`, proxy support, checksum verification.
- **R-7** — `full-auto` downgraded on Windows.

Deferred to later slices (see `npm/handoff.md`): the actual TUI + sidecar
functionality (session store, router, MCP, skills, approvals, telemetry).
This package currently spawns whichever `cusa-tui` binary is available.

## License

Apache-2.0. Portions derived from OpenAI's Codex Rust TUI (Apache-2.0). See
`THIRD_PARTY_NOTICES.md` for attribution.
