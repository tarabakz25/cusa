#!/usr/bin/env bash
# Copyright 2026 cusa contributors
# SPDX-License-Identifier: Apache-2.0
#
# scripts/build-release.sh — dev-only helper. Builds the sidecar (TypeScript
# → JS) and the Rust TUI in release mode, then stages the TUI binary at
# `npm/binaries/<target>/`. The `scripts/prepack.js` step then bundles
# everything into the publish tarball.
#
# This does NOT publish. It also does NOT run inside postinstall — the
# published package only *downloads* the prebuilt binary; only maintainers
# ever run this locally.
#
# Prereqs (macOS/Linux):
#   - node >= 20
#   - rustup + cargo, with the host toolchain installed
#   - jq (optional — used only for pretty logs)
#
# Env overrides:
#   - CUSA_RUSTUP_PREFIX  path prepended to $PATH before running cargo.
#                         Defaults to /opt/homebrew/opt/rustup/bin.
#   - CUSA_TARGET_DIR     override for the Rust target dir (default: repo
#                         root's `target/`).

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"

log() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------- platform + target ----------------------------------------------
node_uname_platform() {
  case "$(uname -s)" in
    Darwin) echo "darwin" ;;
    Linux)  echo "linux" ;;
    MINGW*|MSYS*|CYGWIN*) echo "win32" ;;
    *) die "unsupported OS: $(uname -s)" ;;
  esac
}
node_uname_arch() {
  case "$(uname -m)" in
    arm64|aarch64) echo "arm64" ;;
    x86_64|amd64)  echo "x64" ;;
    *) die "unsupported arch: $(uname -m)" ;;
  esac
}
platform="$(node_uname_platform)"
arch="$(node_uname_arch)"
target="${platform}-${arch}"
exe_name="cusa-tui"
[ "$platform" = "win32" ] && exe_name="cusa-tui.exe"
log "target: $target ($exe_name)"

# ---------- 1. sidecar -----------------------------------------------------
log "building sidecar"
(
  cd "$repo/sidecar"
  if [ -f package-lock.json ]; then
    npm ci
  else
    npm install
  fi
  npm run build
)
[ -f "$repo/sidecar/dist/index.js" ] || die "sidecar build produced no dist/index.js"

# ---------- 2. Rust TUI ----------------------------------------------------
log "building Rust TUI (release)"
if [ -n "${CUSA_RUSTUP_PREFIX:-}" ]; then
  export PATH="$CUSA_RUSTUP_PREFIX:$PATH"
elif [ -d "/opt/homebrew/opt/rustup/bin" ]; then
  export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
fi
command -v cargo >/dev/null 2>&1 || die "cargo not found on PATH"

target_dir="${CUSA_TARGET_DIR:-$repo/target}"
(
  cd "$repo/tui"
  CARGO_TARGET_DIR="$target_dir" cargo build --release
)

# ---------- 3. stage into npm/binaries/<target> ----------------------------
dest_dir="$repo/npm/binaries/$target"
mkdir -p "$dest_dir"
found=""
for candidate in \
  "$target_dir/release/$exe_name" \
  "$target_dir/release/cusa-tui" \
  "$target_dir/release/tui" ; do
  if [ -f "$candidate" ]; then
    found="$candidate"
    break
  fi
done
[ -n "$found" ] || die "could not find built binary under $target_dir/release/"

cp -f "$found" "$dest_dir/$exe_name"
chmod 0755 "$dest_dir/$exe_name"
log "staged $found → $dest_dir/$exe_name"

# ---------- 4. release tarball + sha256 ------------------------------------
release_dir="$repo/dist/release"
mkdir -p "$release_dir"
tar_name="cusa-tui-${target}.tar.gz"
(
  cd "$dest_dir"
  tar -czf "$release_dir/$tar_name" "$exe_name"
)
(
  cd "$release_dir"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$tar_name" > "$tar_name.sha256"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$tar_name" > "$tar_name.sha256"
  else
    die "no sha256 tool found (need sha256sum or shasum)"
  fi
)
log "release artifacts:"
ls -la "$release_dir" | sed 's/^/    /'

log "done. Upload $release_dir/*.tar.gz{,.sha256} to the GitHub Release."
