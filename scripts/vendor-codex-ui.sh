#!/usr/bin/env bash
# Copyright 2026 cusa contributors
# SPDX-License-Identifier: Apache-2.0
#
# SPEC-103: Cherry-pick allowlisted OpenAI codex-rs/tui UI modules into
# tui/vendor/codex-ui/ at a pinned upstream git ref.
#
# Usage:
#   scripts/vendor-codex-ui.sh <git-sha-or-branch>
#   scripts/vendor-codex-ui.sh --help

set -euo pipefail

UPSTREAM_REPO="https://github.com/openai/codex"
UPSTREAM_SRC="codex-rs/tui/src"
VENDOR_REL="tui/vendor/codex-ui"

# P0 foundation allowlist — extend in later phases (see tui/vendor/codex-ui/README.md).
P0_ROOT_ENTRIES=(
  custom_terminal
  style.rs
  ui_consts.rs
  terminal_palette.rs
  color.rs
  wrapping.rs
  width.rs
  text_formatting.rs
)

# Subdirectories required by P0 root modules (e.g. wrapping.rs → render::line_utils).
P0_SUBDIRS=(
  render
)

# P1 composer allowlist (SPEC-106) — textarea + key hints + paste burst (subset).
P1_ROOT_ENTRIES=(
  key_hint.rs
  keymap.rs
)

P1_SUBDIRS=(
  bottom_pane
)

# P2 transcript allowlist (SPEC-107) — history_cell pipeline + markdown/streaming.
P2_ROOT_ENTRIES=(
  thread_transcript.rs
  markdown.rs
  markdown_render.rs
  markdown_text_merge.rs
  markdown_stream.rs
  table_detect.rs
  terminal_hyperlinks.rs
  insert_history.rs
  transcript_reflow.rs
)

P2_SUBDIRS=(
  history_cell
  markdown_render
  streaming
)

PROVENANCE_LINE="// Vendored from openai/codex codex-rs/tui — see UPSTREAM"
APACHE_BLOCK=$'// Copyright OpenAI\n// SPDX-License-Identifier: Apache-2.0\n//'

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
vendor_dir="$repo/$VENDOR_REL"
workdir=""

cleanup() {
  if [ -n "$workdir" ] && [ -d "$workdir" ]; then
    rm -rf "$workdir"
  fi
}
trap cleanup EXIT

log() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<EOF
Usage: $(basename "$0") <git-sha-or-branch>

Clone (sparse) ${UPSTREAM_REPO} at the given ref and copy the P0 foundation
allowlist from ${UPSTREAM_SRC}/ into ${VENDOR_REL}/.

Writes ${VENDOR_REL}/UPSTREAM with repo URL, resolved SHA, and import date.
Prepends OpenAI provenance and Apache-2.0 markers on vendored .rs files when
missing. Safe to re-run (idempotent).

Examples:
  $(basename "$0") main
  $(basename "$0") be33f80bc65159c094ecd06bf155afa3061ce23d
EOF
}

has_marker() {
  local file="$1"
  local head
  head="$(head -n 40 "$file" 2>/dev/null || true)"
  if printf "%s" "$head" | grep -qF "SPDX-License-Identifier: Apache-2.0"; then
    return 0
  fi
  if printf "%s" "$head" | grep -qF "Licensed under the Apache License, Version 2.0"; then
    return 0
  fi
  return 1
}

has_provenance() {
  local file="$1"
  local head
  head="$(head -n 40 "$file" 2>/dev/null || true)"
  if printf "%s" "$head" | grep -qF "openai/codex"; then
    return 0
  fi
  if printf "%s" "$head" | grep -qF "Vendored from"; then
    return 0
  fi
  return 1
}

prepend_headers() {
  local file="$1"
  local tmp provenance_needed=0 apache_needed=0

  if ! has_provenance "$file"; then
    provenance_needed=1
  fi
  if ! has_marker "$file"; then
    apache_needed=1
  fi
  if [ "$provenance_needed" = "0" ] && [ "$apache_needed" = "0" ]; then
    return 0
  fi

  tmp="$(mktemp)"
  {
    if [ "$provenance_needed" = "1" ]; then
      printf '%s\n' "$PROVENANCE_LINE"
      printf '\n'
    fi
    if [ "$apache_needed" = "1" ]; then
      printf '%s\n' "$APACHE_BLOCK"
      printf '\n'
    fi
    cat "$file"
  } >"$tmp"
  mv "$tmp" "$file"
}

resolve_upstream_entry() {
  local upstream_root="$1"
  local entry="$2"
  local path=""

  if [ -f "$upstream_root/$entry" ]; then
    path="$upstream_root/$entry"
  elif [ -f "$upstream_root/${entry%.rs}.rs" ]; then
    path="$upstream_root/${entry%.rs}.rs"
  elif [ -d "$upstream_root/${entry%.rs}" ]; then
    path="$upstream_root/${entry%.rs}"
  elif [ -d "$upstream_root/$entry" ]; then
    path="$upstream_root/$entry"
  else
    return 1
  fi
  printf '%s' "$path"
}

copy_tree() {
  local src="$1"
  local dest="$2"
  if [ -d "$src" ]; then
    mkdir -p "$dest"
    # rsync is not guaranteed; use cp -R for portability.
    cp -R "$src/." "$dest/"
  else
    mkdir -p "$(dirname "$dest")"
    cp "$src" "$dest"
  fi
}

main() {
  local ref="${1:-}"

  case "$ref" in
    -h|--help|help)
      usage
      exit 0
      ;;
    "")
      usage >&2
      die "missing required argument: <git-sha-or-branch>"
      ;;
  esac

  command -v git >/dev/null 2>&1 || die "git not found on PATH"

  local upstream_root resolved_sha import_date
  workdir="$(mktemp -d -t cusa-vendor-codex-XXXXXX)"

  log "cloning ${UPSTREAM_REPO} (sparse ${UPSTREAM_SRC}) at ${ref}"
  git clone --quiet --depth 1 --filter=blob:none --sparse "$UPSTREAM_REPO" "$workdir/codex"
  (
    cd "$workdir/codex"
    git fetch --quiet --depth 1 origin "$ref" || die "could not fetch ref: ${ref}"
    git checkout --quiet FETCH_HEAD
    git sparse-checkout set "$UPSTREAM_SRC"
  )

  upstream_root="$workdir/codex/$UPSTREAM_SRC"
  [ -d "$upstream_root" ] || die "upstream path missing after sparse checkout: ${UPSTREAM_SRC}"

  resolved_sha="$(cd "$workdir/codex" && git rev-parse HEAD)"
  import_date="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  log "resolved upstream SHA: ${resolved_sha}"
  mkdir -p "$vendor_dir"

  local entry src rel dest
  for entry in "${P0_ROOT_ENTRIES[@]}"; do
    src="$(resolve_upstream_entry "$upstream_root" "$entry")" \
      || die "allowlisted path not found in upstream: ${entry} (under ${UPSTREAM_SRC}/)"
    if [ -d "$src" ]; then
      rel="${entry%/}"
      rel="${rel%.rs}"
      dest="$vendor_dir/$rel"
    else
      dest="$vendor_dir/$(basename "$src")"
    fi
    log "copy ${UPSTREAM_SRC}/$(basename "$src") → ${VENDOR_REL}/$(basename "$dest")"
    rm -rf "$dest"
    copy_tree "$src" "$dest"
  done

  for entry in "${P0_SUBDIRS[@]}"; do
    src="$upstream_root/$entry"
    [ -e "$src" ] || die "required subdirectory not found in upstream: ${entry}/"
    dest="$vendor_dir/$entry"
    log "copy ${UPSTREAM_SRC}/${entry}/ → ${VENDOR_REL}/${entry}/"
    rm -rf "$dest"
    copy_tree "$src" "$dest"
  done

  for entry in "${P1_ROOT_ENTRIES[@]}"; do
    src="$(resolve_upstream_entry "$upstream_root" "$entry")" \
      || die "P1 allowlisted path not found in upstream: ${entry} (under ${UPSTREAM_SRC}/)"
    dest="$vendor_dir/$(basename "$src")"
    log "copy ${UPSTREAM_SRC}/$(basename "$src") → ${VENDOR_REL}/$(basename "$dest")"
    rm -rf "$dest"
    copy_tree "$src" "$dest"
  done

  for entry in "${P1_SUBDIRS[@]}"; do
    src="$upstream_root/$entry"
    [ -e "$src" ] || die "P1 subdirectory not found in upstream: ${entry}/"
    dest="$vendor_dir/$entry"
    log "copy ${UPSTREAM_SRC}/${entry}/ (composer subset) → ${VENDOR_REL}/${entry}/"
    rm -rf "$dest"
    mkdir -p "$dest"
    cp "$src/textarea.rs" "$dest/"
    cp "$src/paste_burst.rs" "$dest/"
    mkdir -p "$dest/textarea"
    cp "$src/textarea/vim.rs" "$dest/textarea/"
  done

  for entry in "${P2_ROOT_ENTRIES[@]}"; do
    src="$(resolve_upstream_entry "$upstream_root" "$entry")" \
      || die "P2 allowlisted path not found in upstream: ${entry} (under ${UPSTREAM_SRC}/)"
    dest="$vendor_dir/$(basename "$src")"
    log "copy ${UPSTREAM_SRC}/$(basename "$src") → ${VENDOR_REL}/$(basename "$dest")"
    rm -rf "$dest"
    copy_tree "$src" "$dest"
  done

  for entry in "${P2_SUBDIRS[@]}"; do
    src="$upstream_root/$entry"
    [ -e "$src" ] || die "P2 subdirectory not found in upstream: ${entry}/"
    dest="$vendor_dir/$entry"
    log "copy ${UPSTREAM_SRC}/${entry}/ → ${VENDOR_REL}/${entry}/"
    rm -rf "$dest"
    copy_tree "$src" "$dest"
  done

  log "writing ${VENDOR_REL}/UPSTREAM"
  cat >"$vendor_dir/UPSTREAM" <<EOF
repo=${UPSTREAM_REPO}
sha=${resolved_sha}
date=${import_date}
ref=${ref}
paths=${UPSTREAM_SRC}
EOF

  log "ensuring Apache-2.0 + provenance headers on vendored .rs files"
  while IFS= read -r -d '' f; do
    prepend_headers "$f"
  done < <(find "$vendor_dir" -type f -name '*.rs' -print0)

  log "done — vendored P0 foundation + P1 composer + P2 transcript into ${VENDOR_REL}/ at ${resolved_sha}"
}

main "$@"
