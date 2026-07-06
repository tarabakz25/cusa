// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// On-disk shape of `~/.cusa/sessions.json` entries (SPEC-050, SPEC-053).
//
// One `StoredSession` per resumable agent id. The file lives at
// `~/.cusa/sessions.json` and is rewritten atomically on every mutation by
// `session_store::store::SessionStore`.
//
// Fields are deliberately serde-`camelCase` to match the JSON convention the
// rest of the protocol uses, and every optional/collection field defaults so
// older on-disk versions load without an error.

use cusa_rpc::ApprovalMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One resumable session entry. `agent_id` uniquely identifies the row.
///
/// The `cwd` field is the *repository root* the session was created against
/// — SPEC-051's chooser filters by this on startup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredSession {
    /// Cursor SDK agent id that `session/resume` targets.
    pub agent_id: String,
    /// Working directory the session was created in.
    pub cwd: String,
    /// Model that was active when the session was last used. Advisory —
    /// the sidecar's router may still pick a different model for the
    /// first turn after resume.
    pub model: String,
    /// Approval mode captured on the last mutation. Restored on resume
    /// (SPEC-053).
    pub approval_mode: ApprovalMode,
    /// Ids of skills that were enabled the last time the session ran
    /// (SPEC-053).
    #[serde(default)]
    pub enabled_skill_ids: Vec<String>,
    /// Optional per-session MCP override document (SPEC-041/053).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_overrides: Option<Value>,
    /// Unix timestamp (seconds) when this session was first recorded.
    #[serde(default)]
    pub created_at: i64,
    /// Unix timestamp (seconds) of the most recent send/dispose.
    #[serde(default)]
    pub last_used_at: i64,
    /// Number of user turns completed on this session.
    #[serde(default)]
    pub turns: u32,
}

impl StoredSession {
    /// Short 8-char prefix for the chooser row.
    pub fn short_agent_id(&self) -> String {
        if self.agent_id.len() > 8 {
            self.agent_id[..8].to_string()
        } else {
            self.agent_id.clone()
        }
    }
}

/// Field-by-field delta applied to a `StoredSession` when a live session
/// changes state. Fields that are `None` are left untouched.
#[derive(Debug, Clone, Default)]
pub struct SessionDelta {
    pub model: Option<String>,
    pub approval_mode: Option<ApprovalMode>,
    pub enabled_skill_ids: Option<Vec<String>>,
    pub mcp_overrides: Option<Option<Value>>,
    pub last_used_at: Option<i64>,
    pub bump_turns: bool,
}

impl SessionDelta {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_last_used(mut self, ts: i64) -> Self {
        self.last_used_at = Some(ts);
        self
    }

    pub fn bump_turn(mut self) -> Self {
        self.bump_turns = true;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_approval_mode(mut self, mode: ApprovalMode) -> Self {
        self.approval_mode = Some(mode);
        self
    }

    pub fn with_skills(mut self, ids: Vec<String>) -> Self {
        self.enabled_skill_ids = Some(ids);
        self
    }

    pub fn with_mcp_overrides(mut self, overrides: Option<Value>) -> Self {
        self.mcp_overrides = Some(overrides);
        self
    }

    /// Apply the delta to `session` in place.
    pub fn apply(self, session: &mut StoredSession) {
        if let Some(m) = self.model {
            session.model = m;
        }
        if let Some(mode) = self.approval_mode {
            session.approval_mode = mode;
        }
        if let Some(ids) = self.enabled_skill_ids {
            session.enabled_skill_ids = ids;
        }
        if let Some(overrides) = self.mcp_overrides {
            session.mcp_overrides = overrides;
        }
        if let Some(ts) = self.last_used_at {
            session.last_used_at = ts;
        }
        if self.bump_turns {
            session.turns = session.turns.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spec_050_stored_session_round_trips_json() {
        let s = StoredSession {
            agent_id: "agent-xyz-longer-than-eight".into(),
            cwd: "/tmp/repo".into(),
            model: "composer-2.5".into(),
            approval_mode: ApprovalMode::AutoEdit,
            enabled_skill_ids: vec!["skill-a".into(), "skill-b".into()],
            mcp_overrides: Some(json!({"servers": {}})),
            created_at: 1_700_000_000,
            last_used_at: 1_700_000_100,
            turns: 3,
        };
        let text = serde_json::to_string(&s).unwrap();
        let back: StoredSession = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
        // Field casing sanity — must be camelCase for TS compat.
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["agentId"], "agent-xyz-longer-than-eight");
        assert_eq!(v["approvalMode"], "auto-edit");
        assert_eq!(v["enabledSkillIds"], json!(["skill-a", "skill-b"]));
    }

    #[test]
    fn spec_050_short_agent_id_prefix() {
        let s = StoredSession {
            agent_id: "abcdef1234567890".into(),
            cwd: "/x".into(),
            model: "auto".into(),
            approval_mode: ApprovalMode::Suggest,
            enabled_skill_ids: vec![],
            mcp_overrides: None,
            created_at: 0,
            last_used_at: 0,
            turns: 0,
        };
        assert_eq!(s.short_agent_id(), "abcdef12");
    }

    #[test]
    fn spec_050_session_delta_applies_only_populated_fields() {
        let mut base = StoredSession {
            agent_id: "a".into(),
            cwd: "/w".into(),
            model: "auto".into(),
            approval_mode: ApprovalMode::Suggest,
            enabled_skill_ids: vec![],
            mcp_overrides: None,
            created_at: 100,
            last_used_at: 100,
            turns: 0,
        };
        SessionDelta::new()
            .with_last_used(200)
            .bump_turn()
            .apply(&mut base);
        assert_eq!(base.model, "auto");
        assert_eq!(base.last_used_at, 200);
        assert_eq!(base.turns, 1);

        SessionDelta::new()
            .with_model("claude-sonnet-4")
            .with_approval_mode(ApprovalMode::FullAuto)
            .with_skills(vec!["skill".into()])
            .apply(&mut base);
        assert_eq!(base.model, "claude-sonnet-4");
        assert_eq!(base.approval_mode, ApprovalMode::FullAuto);
        assert_eq!(base.enabled_skill_ids, vec!["skill".to_string()]);
    }
}
