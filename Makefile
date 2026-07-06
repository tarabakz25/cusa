# Copyright 2026 cusa contributors
# SPDX-License-Identifier: Apache-2.0
#
# Top-level Makefile for cusa. Thin, discoverable wrapper around the
# existing tools (cargo, npm, tsc, bash scripts). Every target is
# `.PHONY` — this repo has no filesystem-tracked build products at the
# root that Make needs to reason about.
#
# Quick tour:
#     make            # help
#     make setup      # install every workspace's node deps
#     make build      # cargo build (debug) + sidecar tsc build
#     make test       # run every suite (rust + sidecar + npm + shell)
#     make release    # scripts/build-release.sh (host platform)

SHELL      := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

REPO_ROOT   := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
SIDECAR_DIR := $(REPO_ROOT)/sidecar
NPM_DIR     := $(REPO_ROOT)/npm
TUI_DIR     := $(REPO_ROOT)/tui
SCRIPTS_DIR := $(REPO_ROOT)/scripts

# Resolve cargo through rustup if it isn't on PATH — matches the fallback
# in scripts/run-all-tests.sh so `make test` works in fresh shells.
CARGO ?= $(shell command -v cargo 2>/dev/null || \
                  (command -v rustup >/dev/null 2>&1 && rustup which cargo 2>/dev/null) || \
                  echo cargo)

# When CARGO is an absolute path (common when ~/.cargo/bin is missing),
# prepend its directory so invocations can find `rustc -vV`.
ifneq ($(filter /%,$(CARGO)),)
  export PATH := $(dir $(CARGO))$(PATH)
endif

NODE ?= node
NPM  ?= npm

# ANSI color helpers (guarded so `make` in a dumb terminal still works).
ifneq (,$(findstring xterm,$(TERM)))
  CYAN  := \033[36m
  BOLD  := \033[1m
  DIM   := \033[2m
  RESET := \033[0m
else
  CYAN  :=
  BOLD  :=
  DIM   :=
  RESET :=
endif

## help: Show this help text (auto-generated from '## target: description' lines).
.PHONY: help
help:
	@printf "$(BOLD)cusa$(RESET) — Cursor-SDK-powered coding CLI\n"
	@printf "$(DIM)Usage:$(RESET) make $(CYAN)<target>$(RESET)\n\n"
	@awk 'BEGIN {FS = ":.*## "} /^## [a-zA-Z0-9_.-]+:.*/ { \
	    sub("^## ", "", $$0); split($$0, a, ":"); \
	    printf "  $(CYAN)%-16s$(RESET) %s\n", a[1], substr($$0, length(a[1])+3); \
	}' $(MAKEFILE_LIST)
	@printf "\n$(DIM)Env overrides:$(RESET) CARGO, NODE, NPM, SKIP_RUST=1, SKIP_SIDECAR=1, SKIP_NPM=1, SKIP_SCRIPTS=1\n"

# ---------------------------------------------------------------------------
# Setup & install
# ---------------------------------------------------------------------------

## setup: Install node_modules for sidecar/ and npm/ (uses `npm ci` when a lockfile exists).
.PHONY: setup
setup: setup-sidecar setup-npm

.PHONY: setup-sidecar
setup-sidecar:
	@printf "$(CYAN)==>$(RESET) sidecar deps\n"
	@cd $(SIDECAR_DIR) && \
	  if [ -f package-lock.json ]; then $(NPM) ci; else $(NPM) install; fi

.PHONY: setup-npm
setup-npm:
	@printf "$(CYAN)==>$(RESET) npm wrapper deps\n"
	@cd $(NPM_DIR) && \
	  if [ -f package-lock.json ]; then $(NPM) ci; \
	  elif [ -f package.json ]; then \
	    if grep -q '"dependencies"\|"devDependencies"' package.json; then $(NPM) install; \
	    else printf "$(DIM)    (no dependencies declared)$(RESET)\n"; fi; \
	  fi

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

## build: Debug build of everything (cargo build + tsc).
.PHONY: build
build: build-rust build-sidecar

## build-rust: cargo build --all (debug profile).
.PHONY: build-rust
build-rust:
	@printf "$(CYAN)==>$(RESET) cargo build --all\n"
	@$(CARGO) build --all --manifest-path $(REPO_ROOT)/Cargo.toml

## build-sidecar: Compile TypeScript sidecar to sidecar/dist/.
.PHONY: build-sidecar
build-sidecar:
	@printf "$(CYAN)==>$(RESET) tsc (sidecar)\n"
	@cd $(SIDECAR_DIR) && $(NPM) run --silent build

## build-release: Release build for the host platform (scripts/build-release.sh).
.PHONY: build-release
build-release:
	@$(SCRIPTS_DIR)/build-release.sh

release: build-release  ## alias for build-release
.PHONY: release

# ---------------------------------------------------------------------------
# Test suites
# ---------------------------------------------------------------------------

## test: Run every test suite (rust + sidecar + npm + shell).
.PHONY: test
test:
	@$(SCRIPTS_DIR)/run-all-tests.sh

## test-rust: Rust workspace tests only.
.PHONY: test-rust
test-rust:
	@printf "$(CYAN)==>$(RESET) cargo test --all\n"
	@$(CARGO) test --all --manifest-path $(REPO_ROOT)/Cargo.toml

## test-tui-snapshots: SPEC-110 insta snapshot gate (`*.snap.new` = failure until reviewed).
.PHONY: test-tui-snapshots
test-tui-snapshots:
	@printf "$(CYAN)==>$(RESET) cargo test -p cusa-tui --test snapshots\n"
	@$(CARGO) test -p cusa-tui --test snapshots --manifest-path $(REPO_ROOT)/Cargo.toml
	@if find $(TUI_DIR)/tests/snapshots -name '*.snap.new' -print -quit 2>/dev/null | grep -q .; then \
	  printf "$(BOLD)error:$(RESET) unreviewed snapshot diffs — run \`cargo insta review -p cusa-tui\` or delete *.snap.new\n"; \
	  find $(TUI_DIR)/tests/snapshots -name '*.snap.new'; \
	  exit 1; \
	fi

## test-sidecar: Sidecar TypeScript tests (unit + drift + integration).
.PHONY: test-sidecar
test-sidecar:
	@printf "$(CYAN)==>$(RESET) sidecar tests\n"
	@cd $(SIDECAR_DIR) && $(NPM) test --silent

## test-npm: npm wrapper tests (platform, download, postinstall, login, ...).
.PHONY: test-npm
test-npm:
	@printf "$(CYAN)==>$(RESET) npm wrapper tests\n"
	@cd $(NPM_DIR) && $(NPM) test --silent

## test-scripts: Bash tests for scripts/check-headers.sh.
.PHONY: test-scripts
test-scripts:
	@printf "$(CYAN)==>$(RESET) shell script tests\n"
	@bash $(SCRIPTS_DIR)/check-headers.test.sh

# ---------------------------------------------------------------------------
# Static checks
# ---------------------------------------------------------------------------

## check: Fast static checks (typecheck + cargo check + attribution headers).
.PHONY: check
check: typecheck cargo-check check-headers

## typecheck: TypeScript --noEmit for the sidecar.
.PHONY: typecheck
typecheck:
	@printf "$(CYAN)==>$(RESET) tsc --noEmit (sidecar)\n"
	@cd $(SIDECAR_DIR) && $(NPM) run --silent typecheck

## cargo-check: cargo check --all.
.PHONY: cargo-check
cargo-check:
	@printf "$(CYAN)==>$(RESET) cargo check --all\n"
	@$(CARGO) check --all --manifest-path $(REPO_ROOT)/Cargo.toml

## fmt: Format Rust code (cargo fmt --all).
.PHONY: fmt
fmt:
	@printf "$(CYAN)==>$(RESET) cargo fmt --all\n"
	@$(CARGO) fmt --all --manifest-path $(REPO_ROOT)/Cargo.toml

## fmt-check: Verify formatting without writing (cargo fmt --all -- --check).
.PHONY: fmt-check
fmt-check:
	@printf "$(CYAN)==>$(RESET) cargo fmt --all -- --check\n"
	@$(CARGO) fmt --all --manifest-path $(REPO_ROOT)/Cargo.toml -- --check

## clippy: cargo clippy --all-targets -- -D warnings.
.PHONY: clippy
clippy:
	@printf "$(CYAN)==>$(RESET) cargo clippy --all-targets -- -D warnings\n"
	@$(CARGO) clippy --all-targets --manifest-path $(REPO_ROOT)/Cargo.toml -- -D warnings

## check-headers: Verify Apache-2.0 attribution on forked files (SPEC-083).
.PHONY: check-headers
check-headers:
	@$(SCRIPTS_DIR)/check-headers.sh

## lint: All fast lints (fmt-check + clippy + typecheck + check-headers).
.PHONY: lint
lint: fmt-check clippy typecheck check-headers

# ---------------------------------------------------------------------------
# Dev shortcuts
# ---------------------------------------------------------------------------

## dev-sidecar: Run the sidecar in watch mode (tsx watch).
.PHONY: dev-sidecar
dev-sidecar:
	@cd $(SIDECAR_DIR) && $(NPM) run dev

## run-tui: cargo run the TUI (uses bundled sidecar via CUSA_SIDECAR).
.PHONY: run-tui
run-tui: build-sidecar
	@CUSA_SIDECAR=$(SIDECAR_DIR)/dist/index.js \
	  $(CARGO) run --manifest-path $(REPO_ROOT)/Cargo.toml -p cusa-tui

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

## clean: Remove build artefacts (target/, sidecar/dist/, dist/, npm/binaries/).
.PHONY: clean
clean:
	@printf "$(CYAN)==>$(RESET) cleaning build artefacts\n"
	@rm -rf $(REPO_ROOT)/target
	@rm -rf $(SIDECAR_DIR)/dist
	@rm -rf $(REPO_ROOT)/dist
	@rm -rf $(NPM_DIR)/binaries

## distclean: `clean` plus every node_modules/.
.PHONY: distclean
distclean: clean
	@printf "$(CYAN)==>$(RESET) removing node_modules/\n"
	@rm -rf $(SIDECAR_DIR)/node_modules
	@rm -rf $(NPM_DIR)/node_modules

# ---------------------------------------------------------------------------
# CI convenience
# ---------------------------------------------------------------------------

## ci: What CI runs — lint + test.
.PHONY: ci
ci: lint test
