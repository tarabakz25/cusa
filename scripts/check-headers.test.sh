#!/usr/bin/env bash
# Copyright 2026 cusa contributors
# SPDX-License-Identifier: Apache-2.0
#
# Tests for scripts/check-headers.sh (SPEC-083).
# Zero dependencies — pure bash, uses BATS-style TAP-ish output.
#
# The strategy: build a scratch directory that looks like the real repo
# root (with `tui/src/` and `tui/crates/cusa-rpc/src/`), populate it with
# controlled fixtures, cd into it, then run the script and assert on the
# exit code + output. This works because check-headers.sh resolves its
# FORKED_PATHS relative to $PWD.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK_HEADERS="$SCRIPT_DIR/check-headers.sh"
FAILED=0
PASSED=0

trap 'cleanup' EXIT
SCRATCH=""
cleanup() {
  if [ -n "$SCRATCH" ] && [ -d "$SCRATCH" ]; then
    rm -rf "$SCRATCH"
  fi
}

pass() { PASSED=$((PASSED + 1)); echo "ok - $*"; }
fail() { FAILED=$((FAILED + 1)); echo "not ok - $*" >&2; }

new_scratch() {
  local root
  root="$(mktemp -d -t cusa-check-headers-XXXXXX)"
  mkdir -p "$root/tui/src" "$root/tui/crates/cusa-rpc/src"
  printf "%s" "$root"
}

with_spdx_header() {
  cat <<'EOF'
// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// A test file.
fn main() {}
EOF
}

with_apache_prose_header() {
  cat <<'EOF'
// Copyright 2026 cusa contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
fn main() {}
EOF
}

without_any_header() {
  cat <<'EOF'
// No license header here.
// Just some code.
fn main() {}
EOF
}

# ---- test: --help exits 0 and prints usage ------------------------------
test_help_flag() {
  local out
  out="$("$CHECK_HEADERS" --help 2>&1)" || { fail "--help exited non-zero"; return; }
  if printf "%s" "$out" | grep -q "Usage:"; then
    pass "--help prints usage and exits 0"
  else
    fail "--help output missing usage; got: $out"
  fi
}

# ---- test: --list mode enumerates files without validating -------------
test_list_mode() {
  SCRATCH="$(new_scratch)"
  printf "%s" "$(without_any_header)" > "$SCRATCH/tui/src/no_header.rs"
  printf "%s" "$(with_spdx_header)" > "$SCRATCH/tui/crates/cusa-rpc/src/ok.rs"
  local out
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" --list 2>&1)" \
    || { fail "--list exited non-zero"; return; }
  if printf "%s" "$out" | grep -q "tui/src/no_header.rs" \
     && printf "%s" "$out" | grep -q "tui/crates/cusa-rpc/src/ok.rs"; then
    pass "--list enumerates both fixture files"
  else
    fail "--list missed files; got: $out"
  fi
}

# ---- test: unknown flag returns exit 2 ----------------------------------
test_unknown_flag() {
  local status=0
  "$CHECK_HEADERS" --bogus >/dev/null 2>&1 || status=$?
  if [ "$status" = "2" ]; then
    pass "unknown flag exits with status 2"
  else
    fail "unknown flag: expected exit 2, got $status"
  fi
}

# ---- test: all files carry SPDX marker → exit 0 -------------------------
test_all_spdx_headers_pass() {
  SCRATCH="$(new_scratch)"
  printf "%s" "$(with_spdx_header)" > "$SCRATCH/tui/src/a.rs"
  printf "%s" "$(with_spdx_header)" > "$SCRATCH/tui/crates/cusa-rpc/src/b.rs"
  local out status=0
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" = "0" ] && printf "%s" "$out" | grep -q "check-headers: OK"; then
    pass "all-SPDX fixture passes"
  else
    fail "all-SPDX fixture: status=$status output=$out"
  fi
}

# ---- test: Apache prose marker accepted ---------------------------------
test_apache_prose_header_accepted() {
  SCRATCH="$(new_scratch)"
  printf "%s" "$(with_apache_prose_header)" > "$SCRATCH/tui/src/a.rs"
  local out status=0
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" = "0" ]; then
    pass "Apache prose header accepted"
  else
    fail "Apache prose header rejected; status=$status output=$out"
  fi
}

# ---- test: missing header → non-zero exit + lists the file --------------
test_missing_header_detected() {
  SCRATCH="$(new_scratch)"
  printf "%s" "$(with_spdx_header)" > "$SCRATCH/tui/src/ok.rs"
  printf "%s" "$(without_any_header)" > "$SCRATCH/tui/src/bad.rs"
  local out status=0
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" != "0" ] \
     && printf "%s" "$out" | grep -q "bad.rs" \
     && printf "%s" "$out" | grep -q "missing an Apache-2.0 header"; then
    pass "missing header is detected"
  else
    fail "missing header not detected; status=$status output=$out"
  fi
}

# ---- test: marker beyond line 40 is NOT accepted ------------------------
test_header_beyond_40_lines_rejected() {
  SCRATCH="$(new_scratch)"
  {
    for _ in $(seq 1 41); do printf "// filler line\n"; done
    printf "// SPDX-License-Identifier: Apache-2.0\n"
    printf "fn main() {}\n"
  } > "$SCRATCH/tui/src/late.rs"
  local out status=0
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" != "0" ] && printf "%s" "$out" | grep -q "late.rs"; then
    pass "header past line 40 is rejected"
  else
    fail "header-past-40 should fail; status=$status output=$out"
  fi
}

# ---- test: non-.rs/.ts/.js files are ignored ---------------------------
test_ignores_non_source_files() {
  SCRATCH="$(new_scratch)"
  printf "%s" "$(without_any_header)" > "$SCRATCH/tui/src/README.md"
  printf "%s" "$(without_any_header)" > "$SCRATCH/tui/src/config.toml"
  local out status=0
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" = "0" ]; then
    pass "non-source files (.md, .toml) are ignored"
  else
    fail "non-source files not ignored; status=$status output=$out"
  fi
}

# ---- test: .ts and .js files are also validated ------------------------
test_typescript_and_javascript_files() {
  SCRATCH="$(new_scratch)"
  printf "// No header at all\nconst x = 1;\n" > "$SCRATCH/tui/src/bad.ts"
  local out status=0
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" != "0" ] && printf "%s" "$out" | grep -q "bad.ts"; then
    pass ".ts files are scanned"
  else
    fail ".ts files not scanned; status=$status output=$out"
  fi
}

# ---- test: empty forked directories exit cleanly -----------------------
test_empty_forked_directories() {
  SCRATCH="$(new_scratch)"
  # both fixture dirs exist but are empty
  local out status=0
  out="$(cd "$SCRATCH" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" = "0" ] && printf "%s" "$out" | grep -q "0 files scanned"; then
    pass "empty forked dirs pass with count=0"
  else
    fail "empty forked dirs: status=$status output=$out"
  fi
}

# ---- test: the real repo checkout must currently pass -------------------
test_repo_currently_passes() {
  local repo out status=0
  repo="$(cd "$SCRIPT_DIR/.." && pwd)"
  out="$(cd "$repo" && "$CHECK_HEADERS" 2>&1)" || status=$?
  if [ "$status" = "0" ]; then
    pass "current repo checkout passes (regression guard)"
  else
    fail "repo checkout fails check-headers; status=$status output=$out"
  fi
}

# ---- run everything ------------------------------------------------------
main() {
  echo "1..11"
  test_help_flag
  test_list_mode
  test_unknown_flag
  test_all_spdx_headers_pass
  test_apache_prose_header_accepted
  test_missing_header_detected
  test_header_beyond_40_lines_rejected
  test_ignores_non_source_files
  test_typescript_and_javascript_files
  test_empty_forked_directories
  test_repo_currently_passes
  echo "# passed=$PASSED failed=$FAILED"
  if [ "$FAILED" -ne 0 ]; then
    exit 1
  fi
}

main "$@"
