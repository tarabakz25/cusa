#!/usr/bin/env bash
# Copyright 2026 cusa contributors
# SPDX-License-Identifier: Apache-2.0
#
# Runs every test suite in the repository. Intended for local pre-push
# checks and CI. Fails fast on the first suite failure.
#
# Suites executed:
#   1. cargo test --all           (Rust crates: cusa-tui, cusa-rpc)
#   2. sidecar: node --test tsx    (@cursor/sdk sidecar unit + integration)
#   3. npm:     node --test        (npm wrapper: platform, node-version,
#                                   postinstall, login, args, download, logs,
#                                   prepack)
#   4. scripts: check-headers.test (SPEC-083 attribution guard)
#
# Environment:
#   SKIP_RUST=1     — skip cargo suite (useful when rustup isn't on PATH).
#   SKIP_SIDECAR=1  — skip sidecar suite.
#   SKIP_NPM=1      — skip npm suite.
#   SKIP_SCRIPTS=1  — skip shell suite.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

BLUE=$'\033[34m'
GREEN=$'\033[32m'
RED=$'\033[31m'
DIM=$'\033[2m'
RESET=$'\033[0m'

banner() { printf "\n%s══ %s ══%s\n" "$BLUE" "$1" "$RESET"; }
ok()    { printf "%s✔%s %s\n"       "$GREEN" "$RESET" "$1"; }
fail()  { printf "%s✖%s %s\n"       "$RED"   "$RESET" "$1" >&2; }
note()  { printf "%s%s%s\n"         "$DIM"   "$1" "$RESET"; }

any_failed=0
run_suite() {
  local name="$1"; shift
  banner "$name"
  if "$@"; then
    ok "$name passed"
  else
    fail "$name failed"
    any_failed=1
  fi
}

# 1. Rust workspace ---------------------------------------------------------
run_rust() {
  local cargo=""
  if command -v cargo >/dev/null 2>&1; then
    cargo="$(command -v cargo)"
  elif [ -x "$HOME/.cargo/bin/cargo" ]; then
    cargo="$HOME/.cargo/bin/cargo"
  elif command -v rustup >/dev/null 2>&1; then
    cargo="$(rustup which cargo 2>/dev/null || true)"
  fi
  if [ -z "$cargo" ] || ! [ -x "$cargo" ]; then
    fail "cargo not found — install via rustup or set SKIP_RUST=1"
    return 1
  fi
  note "using $cargo"
  (cd "$REPO_ROOT" && "$cargo" test --all --quiet)
}

# 2. Sidecar (Node + tsx) ---------------------------------------------------
run_sidecar() {
  if ! command -v node >/dev/null 2>&1; then
    fail "node not found on PATH"
    return 1
  fi
  (cd "$REPO_ROOT/sidecar" && npm test --silent)
}

# 3. npm wrapper ------------------------------------------------------------
run_npm() {
  (cd "$REPO_ROOT/npm" && npm test --silent)
}

# 4. Shell scripts ----------------------------------------------------------
run_scripts() {
  bash "$REPO_ROOT/scripts/check-headers.test.sh"
}

# Dispatch -----------------------------------------------------------------

[ "${SKIP_RUST:-0}"    = "1" ] || run_suite "rust workspace"     run_rust
[ "${SKIP_SIDECAR:-0}" = "1" ] || run_suite "sidecar (node)"     run_sidecar
[ "${SKIP_NPM:-0}"     = "1" ] || run_suite "npm wrapper (node)" run_npm
[ "${SKIP_SCRIPTS:-0}" = "1" ] || run_suite "scripts (bash)"     run_scripts

banner "summary"
if [ "$any_failed" -eq 0 ]; then
  ok "all suites passed"
  exit 0
else
  fail "one or more suites failed"
  exit 1
fi
