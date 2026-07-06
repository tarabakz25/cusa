#!/usr/bin/env bash
# Copyright 2026 cusa contributors
# SPDX-License-Identifier: Apache-2.0
#
# SPEC-083: verify that files forked from Apache-2.0 upstream projects
# (currently only OpenAI's codex-tui) retain a license/attribution header.
# Fails CI with a non-zero exit code if any file in FORKED_PATHS is missing
# the expected marker.
#
# Header contract: the file must contain either:
#   "SPDX-License-Identifier: Apache-2.0"       or
#   "Licensed under the Apache License, Version 2.0"
# within the first 40 lines.
#
# Usage:
#   scripts/check-headers.sh            # check
#   scripts/check-headers.sh --list     # list files being checked

set -euo pipefail

# Repo-relative paths (globs) to scan. Extend as we cherry-pick more of
# upstream `codex-rs/tui`.
FORKED_PATHS=(
  "tui/src"
  "tui/crates/cusa-rpc/src"
  "tui/vendor/codex-ui"
)

# Paths where we also require OpenAI vendoring provenance (SPEC-111).
VENDORED_PATHS=(
  "tui/vendor/codex-ui"
)

PROVENANCE_MARKERS=(
  "openai/codex"
  "Vendored from"
)

MARKERS=(
  "SPDX-License-Identifier: Apache-2.0"
  "Licensed under the Apache License, Version 2.0"
)

usage() {
  cat <<EOF
Usage: $(basename "$0") [--list]
  --list   Print the files being checked and exit.
EOF
}

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
  --list) LIST_ONLY=1 ;;
  "") LIST_ONLY=0 ;;
  *) usage; exit 2 ;;
esac

files=()
for base in "${FORKED_PATHS[@]}"; do
  if [ -d "$base" ]; then
    while IFS= read -r -d '' f; do
      files+=("$f")
    done < <(find "$base" -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.js' \) -print0)
  fi
done

if [ "$LIST_ONLY" = "1" ]; then
  if [ "${#files[@]}" -gt 0 ]; then
    printf "%s\n" "${files[@]}"
  fi
  exit 0
fi

missing=()
missing_provenance=()
if [ "${#files[@]}" -gt 0 ]; then
  for f in "${files[@]}"; do
    head=$(head -n 40 "$f" 2>/dev/null || true)
    found=0
    for m in "${MARKERS[@]}"; do
      if printf "%s" "$head" | grep -qF "$m"; then
        found=1
        break
      fi
    done
    if [ "$found" = "0" ]; then
      missing+=("$f")
    fi

    for base in "${VENDORED_PATHS[@]}"; do
      case "$f" in
        "$base"/*)
          prov_found=0
          for p in "${PROVENANCE_MARKERS[@]}"; do
            if printf "%s" "$head" | grep -qF "$p"; then
              prov_found=1
              break
            fi
          done
          if [ "$prov_found" = "0" ]; then
            missing_provenance+=("$f")
          fi
          ;;
      esac
    done
  done
fi

if [ "${#missing[@]}" -ne 0 ]; then
  echo "check-headers: the following files are missing an Apache-2.0 header:" >&2
  for f in "${missing[@]}"; do echo "  $f" >&2; done
  echo "" >&2
  echo "Add one of these markers within the first 40 lines:" >&2
  for m in "${MARKERS[@]}"; do echo "  - $m" >&2; done
  exit 1
fi

if [ "${#missing_provenance[@]}" -ne 0 ]; then
  echo "check-headers: the following vendored files are missing OpenAI provenance:" >&2
  for f in "${missing_provenance[@]}"; do echo "  $f" >&2; done
  echo "" >&2
  echo "Add a provenance comment within the first 40 lines, e.g.:" >&2
  echo "  // Vendored from openai/codex codex-rs/tui — see UPSTREAM" >&2
  exit 1
fi

echo "check-headers: OK (${#files[@]} files scanned)"
