// Vendored from openai/codex codex-rs/tui — see UPSTREAM

// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//

//! Minimal file-change model used by TUI diff rendering and approval previews.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileChange {
    Add {
        content: String,
    },
    Delete {
        content: String,
    },
    Update {
        unified_diff: String,
        move_path: Option<PathBuf>,
    },
}
