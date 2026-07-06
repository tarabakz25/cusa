// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Session persistence (SPEC-050, SPEC-051, SPEC-053).
//
// This module owns the on-disk state at `~/.cusa/sessions.json`. Two
// submodules split responsibility:
//
// * `types` — the serde shape (`StoredSession`, `SessionDelta`).
// * `store` — atomic file I/O + directory conventions.
//
// The startup chooser overlay lives in `crate::app::startup`; the code
// paths that actually persist mutations live in `crate::app::mod` (record
// on `session/create`, update on `run/finished`, remove on `/reset`).

pub mod store;
pub mod types;

pub use store::{now_unix, SessionStore};
pub use types::{SessionDelta, StoredSession};
