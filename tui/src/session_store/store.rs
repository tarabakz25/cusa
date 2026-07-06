// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// On-disk store for `~/.cusa/sessions.json` (SPEC-050).
//
// * All I/O is best-effort: read errors reset the in-memory cache to
//   empty (a corrupt file must not brick the TUI); write errors surface
//   as `anyhow::Error` but the caller is free to ignore them.
// * Writes use the classic tmp-file + rename pattern so a crash mid-write
//   never leaves an unreadable half-written file.
// * On Unix the file is created with mode `0600` (SPEC-050 requirement).
//   On Windows we fall back to the default ACL — the mode check test is
//   gated behind `cfg(unix)`.
//
// The store is intentionally small and does zero locking: the TUI is
// single-writer per cusa process, and the cross-process case is rare enough
// that "last writer wins" is acceptable.

use crate::session_store::types::{SessionDelta, StoredSession};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// File-level wrapper — allows a future forward-compatible upgrade path
/// (`version`, plus optional `by_cwd` denormalization).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FileDoc {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    sessions: Vec<StoredSession>,
}

fn default_version() -> u32 {
    1
}

/// Thin handle to `~/.cusa/sessions.json`. Cheap to construct; each method
/// re-reads the file to survive concurrent edits (rare but possible).
#[derive(Debug, Clone)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    /// Construct a store rooted at `path`. Callers typically use
    /// [`SessionStore::for_config`] instead of naming a path directly.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Default store rooted at `~/.cusa/sessions.json`.
    pub fn for_config() -> Self {
        Self::new(crate::config::sessions_path())
    }

    /// Return every stored session whose `cwd` matches `cwd`, newest first.
    pub fn list_for_cwd(&self, cwd: &str) -> Vec<StoredSession> {
        let mut all = self.load().sessions;
        all.retain(|s| s.cwd == cwd);
        all.sort_by_key(|s| std::cmp::Reverse(s.last_used_at));
        all
    }

    /// Return every stored session, newest first.
    pub fn list_all(&self) -> Vec<StoredSession> {
        let mut all = self.load().sessions;
        all.sort_by_key(|s| std::cmp::Reverse(s.last_used_at));
        all
    }

    /// Record a new session, or refresh an existing entry with the same
    /// `agent_id`. Idempotent.
    pub fn record_new(&self, session: StoredSession) -> Result<()> {
        let mut doc = self.load();
        // Replace or push.
        if let Some(existing) = doc.sessions.iter_mut().find(|s| s.agent_id == session.agent_id) {
            *existing = session;
        } else {
            doc.sessions.push(session);
        }
        self.save(&doc)
    }

    /// Apply a delta to the stored session with `agent_id`. Silently
    /// ignores unknown ids.
    pub fn update(&self, agent_id: &str, delta: SessionDelta) -> Result<()> {
        let mut doc = self.load();
        if let Some(existing) = doc.sessions.iter_mut().find(|s| s.agent_id == agent_id) {
            delta.apply(existing);
            self.save(&doc)?;
        }
        Ok(())
    }

    /// Remove the entry with `agent_id`. No-op if the id is absent.
    pub fn remove(&self, agent_id: &str) -> Result<()> {
        let mut doc = self.load();
        let before = doc.sessions.len();
        doc.sessions.retain(|s| s.agent_id != agent_id);
        if doc.sessions.len() != before {
            self.save(&doc)?;
        }
        Ok(())
    }

    /// Path of the underlying file. Useful for tests and diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // ---- internal ----

    fn load(&self) -> FileDoc {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return FileDoc::default();
        };
        serde_json::from_str::<FileDoc>(&text).unwrap_or_default()
    }

    fn save(&self, doc: &FileDoc) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent dir {}", parent.display()))?;
        }
        let tmp = self.tmp_path();
        {
            let body = serde_json::to_string_pretty(doc).context("serialize sessions doc")?;
            write_secret_file(&tmp, body.as_bytes())
                .with_context(|| format!("write {}", tmp.display()))?;
        }
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("rename {} -> {}", tmp.display(), self.path.display()))?;
        Ok(())
    }

    fn tmp_path(&self) -> PathBuf {
        let mut file = self
            .path
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_else(|| std::ffi::OsString::from("sessions.json"));
        file.push(".tmp");
        self.path
            .parent()
            .map(|p| p.join(&file))
            .unwrap_or_else(|| PathBuf::from(file))
    }
}

/// Utility: unix seconds since epoch.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn write_secret_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_secret_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cusa_rpc::ApprovalMode;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_store() -> (SessionStore, PathBuf) {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "cusa-session-store-{}-{}",
            std::process::id(),
            id
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.json");
        (SessionStore::new(path.clone()), path)
    }

    fn make(id: &str, cwd: &str, last: i64) -> StoredSession {
        StoredSession {
            agent_id: id.into(),
            cwd: cwd.into(),
            model: "auto".into(),
            approval_mode: ApprovalMode::Suggest,
            enabled_skill_ids: vec![],
            mcp_overrides: None,
            created_at: last,
            last_used_at: last,
            turns: 0,
        }
    }

    #[test]
    fn spec_050_session_store_round_trips_stored_session_json() {
        let (store, path) = tmp_store();
        let s = StoredSession {
            agent_id: "agent-a".into(),
            cwd: "/tmp/repo".into(),
            model: "composer-2.5".into(),
            approval_mode: ApprovalMode::AutoEdit,
            enabled_skill_ids: vec!["skill".into()],
            mcp_overrides: Some(json!({"servers": {}})),
            created_at: 1_700_000_000,
            last_used_at: 1_700_000_100,
            turns: 2,
        };
        store.record_new(s.clone()).unwrap();
        assert!(path.exists());

        // Round-trip via a fresh handle.
        let fresh = SessionStore::new(path.clone());
        let got = fresh.list_all();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], s);
    }

    #[test]
    fn spec_050_sessions_store_writes_json_atomically() {
        let (store, path) = tmp_store();
        store.record_new(make("a", "/repo", 10)).unwrap();
        // The tmp file must NOT survive a successful write.
        let mut tmp = path.clone();
        let mut name = tmp.file_name().unwrap().to_os_string();
        name.push(".tmp");
        tmp.set_file_name(name);
        assert!(!tmp.exists(), "tmp file should be gone after rename");
        // The real file must be valid JSON with our entry.
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"agentId\""));
        assert!(text.contains("\"a\""));
    }

    #[cfg(unix)]
    #[test]
    fn spec_050_session_store_writes_with_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let (store, path) = tmp_store();
        store.record_new(make("a", "/repo", 10)).unwrap();
        let meta = fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "sessions.json must be 0600, got {mode:o}");
    }

    #[test]
    fn spec_050_list_for_cwd_only_returns_matching_rows_newest_first() {
        let (store, _path) = tmp_store();
        store.record_new(make("a", "/repo", 100)).unwrap();
        store.record_new(make("b", "/repo", 300)).unwrap();
        store.record_new(make("c", "/other", 500)).unwrap();
        store.record_new(make("d", "/repo", 200)).unwrap();

        let got = store.list_for_cwd("/repo");
        let ids: Vec<&str> = got.iter().map(|s| s.agent_id.as_str()).collect();
        assert_eq!(ids, vec!["b", "d", "a"]);
    }

    #[test]
    fn spec_050_update_applies_delta_and_leaves_others_alone() {
        let (store, _path) = tmp_store();
        store.record_new(make("a", "/repo", 100)).unwrap();
        store.record_new(make("b", "/repo", 200)).unwrap();
        store
            .update(
                "a",
                SessionDelta::new()
                    .with_last_used(500)
                    .with_model("claude-sonnet-4")
                    .bump_turn(),
            )
            .unwrap();
        let got = store.list_for_cwd("/repo");
        let a = got.iter().find(|s| s.agent_id == "a").unwrap();
        let b = got.iter().find(|s| s.agent_id == "b").unwrap();
        assert_eq!(a.last_used_at, 500);
        assert_eq!(a.model, "claude-sonnet-4");
        assert_eq!(a.turns, 1);
        assert_eq!(b.last_used_at, 200);
        assert_eq!(b.model, "auto");
    }

    #[test]
    fn spec_050_remove_deletes_only_target_row() {
        let (store, _path) = tmp_store();
        store.record_new(make("a", "/repo", 100)).unwrap();
        store.record_new(make("b", "/repo", 200)).unwrap();
        store.remove("a").unwrap();
        let got = store.list_all();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].agent_id, "b");
        // Removing a missing id is a no-op.
        store.remove("does-not-exist").unwrap();
    }

    #[test]
    fn spec_050_corrupt_file_reads_as_empty() {
        let (store, path) = tmp_store();
        fs::write(&path, b"not json").unwrap();
        assert!(store.list_all().is_empty());
        // And we can still record afterwards.
        store.record_new(make("a", "/repo", 1)).unwrap();
        assert_eq!(store.list_all().len(), 1);
    }

    #[test]
    fn spec_050_record_new_replaces_existing_agent_id() {
        let (store, _path) = tmp_store();
        let mut s = make("a", "/repo", 100);
        store.record_new(s.clone()).unwrap();
        s.model = "changed".into();
        s.last_used_at = 999;
        store.record_new(s.clone()).unwrap();
        let all = store.list_all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].model, "changed");
        assert_eq!(all[0].last_used_at, 999);
    }
}
