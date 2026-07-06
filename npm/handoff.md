# cusa npm — Slice 8 handoff

This document describes what Slice 8 delivered inside `npm/**` and
`scripts/**`, what is deferred, and how a maintainer produces a release.

## Delivered

### Shim (`npm/bin/cusa.js`)

- Enforces Node.js ≥ 20 with an actionable message (**SPEC-082**), via
  `lib/node.js` so the enforcement can be unit-tested without spawning a
  child process.
- Resolves the platform-specific TUI binary in this order:
  1. `$CUSA_TUI` (dev override).
  2. `npm/binaries/<target>/<exe>` (developer bundle, produced by
     `scripts/build-release.sh`).
  3. `$CUSA_HOME/bin/<target>/<exe>` (cached download).
- Sub-commands handled without spawning the TUI:
  - `cusa login [--stdin|--key <k>] [--force-windows]` — **SPEC-101**.
  - `cusa download-binary [--force] [--target=<slug>]` — **R-6**.
  - `cusa --version` / `-V` — prints the npm package version. Prints even
    when the binary is missing (i.e. graceful degradation of the shim).
- If argv contains `--verbose` / `-v`, ensures `$CUSA_HOME/logs/` exists at
  mode `0700` (**SPEC-102**) before spawning the TUI. Log writes are the
  TUI's responsibility.
- On Windows, rewrites `--approval=full-auto` / `--full-auto` to
  `--approval=auto-edit` and prints a warning (**R-7**). The TUI enforces
  the same rule defensively; this fails fast at the process boundary.

### Postinstall (`npm/postinstall.js`)

- Skips when told to:
  - `CUSA_SKIP_POSTINSTALL=1`
  - `CI=1` and no `CUSA_ALLOW_CI_DOWNLOAD=1` opt-in
  - A bundled binary already exists under `npm/binaries/<t>/`
  - A cached binary already exists under `$CUSA_HOME/bin/<t>/`
- Otherwise:
  1. `GET` `<base>/v<version>/cusa-tui-<target>.tar.gz.sha256`
  2. `GET` `<base>/v<version>/cusa-tui-<target>.tar.gz`
  3. Verify SHA-256; on mismatch, throw + leave no partial file.
  4. Extract with a small internal tar reader (zlib + POSIX ustar parser).
  5. `chmod 0755` and rename atomically into
     `$CUSA_HOME/bin/<target>/<exe>`.
- On any failure (network, checksum, extraction, mkdir), prints a clear
  recovery hint and exits **0** so `npm install` succeeds. The user can
  rerun `cusa download-binary` later.
- Zero runtime deps. Uses `node:https`, `node:http`, `node:tls`,
  `node:zlib`, `node:crypto`, `node:fs`.

### Proxy support (`lib/download.js`, **R-6**)

- `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` are honored.
- HTTPS through an HTTP proxy is tunneled via `CONNECT` + `tls.connect` on
  the returned socket. HTTP through a proxy uses absolute-URL requests.
- `NO_PROXY` matching mirrors curl: `*`, exact hostname, `.suffix`, and
  bare `suffix` all bypass the proxy.

### Login (`lib/login.js`, **SPEC-101**)

- Writes `$CUSA_HOME/config.toml` at file mode **0600**.
- If `[api]` section exists, replaces the `api_key` line in place;
  otherwise appends a fresh `[api]` block. Other sections are preserved.
- Hand-rolled TOML string escaping — no `toml` dependency, keeping the
  zero-runtime-deps requirement.
- `readSecret` masks TTY echo; `readStdin` reads for `--stdin` use.

### Logs (`lib/logs.js`, **SPEC-102**)

- `ensureLogDir({ cusaHome })` creates the home dir (0755) and
  `$CUSA_HOME/logs/` (0700), then explicit `chmod` to defeat umask.

### prepack (`scripts/prepack.js`, **SPEC-083**)

- Mirrors `sidecar/dist/` → `npm/sidecar/dist/` and copies the sidecar's
  own `package.json` for its runtime deps.
- Copies `THIRD_PARTY_NOTICES.md` + `LICENSE` from the repo root into
  `npm/`.
- Fails loudly if `sidecar/dist/index.js` is missing.
- Fails loudly if `npm/binaries/` is empty, unless
  `CUSA_ALLOW_EMPTY_BINARIES=1` is set (needed for JS-only smoke packs and
  for CI that publishes with binaries hosted on GitHub Releases).

### Build-release script (`scripts/build-release.sh`)

Dev-only. Runs the sidecar build, the Rust TUI release build, stages the
binary under `npm/binaries/<target>/`, and produces a `.tar.gz` +
`.sha256` pair under `dist/release/`. Intended to be run per-target on a
matching host / GH Actions runner.

### Tests (`npm/tests/*.test.js`)

52 tests, all `node --test`, no external test framework, no runtime deps
required to run:

| File                | SPEC IDs                          |
| ------------------- | --------------------------------- |
| `platform.test.js`  | SPEC-081                          |
| `node.test.js`      | SPEC-082                          |
| `download.test.js`  | SPEC-080 (mocked HTTP server)     |
| `login.test.js`     | SPEC-101                          |
| `logs.test.js`      | SPEC-102                          |
| `args.test.js`      | R-7 + shared arg parser           |
| `postinstall.test.js` | SPEC-080/082/101 (subprocess integration) |
| `prepack.test.js`   | SPEC-083                          |

Run with `npm test` inside `npm/`, or `node --test npm/tests/*.test.js`
from the repo root.

### CI stubs (`.github/workflows/*.yml`)

Two files, both gated by `if: false`. Enable by:

1. Reviewing the matrix and secret names (`NPM_TOKEN`).
2. Removing `if: false` on the jobs.
3. Adding `push: tags: v*.*.*` and `pull_request:` triggers back.

The release workflow builds a per-target matrix, uploads tarballs +
`.sha256` files to the GitHub Release, and then publishes the npm package
with `--provenance --access public` and `CUSA_ALLOW_EMPTY_BINARIES=1`.

## Deferred (not in Slice 8)

- Actual TUI + sidecar functionality — Slices 1-7 own these.
- **Bundling the sidecar's runtime deps.** The sidecar package.json still
  lists `@cursor/sdk` as a `dependency`. The outer `cusa` package is
  zero-runtime-deps, so at install time the sidecar's `node_modules` is
  NOT populated. Slice 4/5 owns switching the sidecar build to a bundler
  (e.g. `esbuild --bundle --platform=node`) that produces a single
  self-contained `dist/index.js`. Until then, running the TUI end-to-end
  requires either (a) a maintainer-side `npm ci && npm run build` in
  `sidecar/` before prepack, followed by a post-prepack `npm install`
  under `npm/sidecar/`, or (b) setting `CUSA_SIDECAR=/abs/path/to/dev/sidecar/dist/index.js`.
- Windows `cusa login` with real ACL-based key protection. Today the shim
  writes the same TOML file but the mode is a no-op; the user must pass
  `--force-windows`.
- Postinstall retry / resume for partial downloads. Current failure mode
  is "hint and skip".
- Verifying the tarball is signed. Only SHA-256 (from the sibling
  `.sha256` file that the release job produces) is checked.
- `codesign` / SmartScreen / Gatekeeper attestation for the TUI binary.
  Necessary for a wide release; out of scope for Slice 8.
- Auto-updates. `cusa download-binary` covers the manual path.

## How to run the release script locally

```sh
# 1. Build for this host and stage into npm/binaries/<target>/
scripts/build-release.sh

# 2. (Optional) Dry-run the publish tarball
cd npm
CUSA_ALLOW_EMPTY_BINARIES=1 npm pack --dry-run

# 3. Local end-to-end smoke test of the shim:
node bin/cusa.js --version
CUSA_HOME=/tmp/cusa-test node postinstall.js   # exits 0 even offline
```

For a real release, run `scripts/build-release.sh` on each of {macOS
arm64, macOS x64, Linux x64, Linux arm64, Windows x64} — the tarball
naming (`cusa-tui-<target>.tar.gz`) determines what the postinstall on
end-user machines will fetch. Upload all tarballs + `.sha256` files to
the matching GitHub Release, then publish the npm package.
