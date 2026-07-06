// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// cusa-tui entry point (SPEC-070).
//
// Responsibilities:
//   1. Parse CLI args.
//   2. Initialize tracing (SPEC-102, gated by `--verbose`).
//   3. Resolve the sidecar entry point (`--sidecar` / `CUSA_SIDECAR` /
//      fallback discovery).
//   4. Spawn the sidecar under supervision (SPEC-073).
//   5. Enter the Ratatui event loop.
//
// The bulk of the TUI logic lives in `crate::app` and `crate::sidecar`; this
// file is intentionally slim.

// SPEC-070 fork attribution: files under this tree derive their layout
// vocabulary (chat REPL structure, status/transcript/input panes, slash
// commands) from OpenAI's `codex-rs/tui` (Apache-2.0). The rendering is
// re-implemented on top of `ratatui` 0.29.
//
// SPEC-083 license-header compliance: every file under `tui/src/**` must
// carry an Apache-2.0 header; `bash scripts/check-headers.sh` verifies
// this at CI time and the integration test at `tui/tests/headers.rs`
// mirrors that check in-process (`spec_083_every_rust_file_carries_apache_header`).

use anyhow::{Context, Result};
use clap::Parser;
use cusa_tui::{app, logging, session_store, sidecar};
use serde_json::Value;

/// cusa — Cursor-SDK-powered coding CLI with transparent auto-mode.
#[derive(Debug, Parser)]
#[command(name = "cusa", version, about)]
struct Cli {
    /// Resume a specific agent id (SPEC-052).
    #[arg(long)]
    resume: Option<String>,

    /// Enable verbose logging to `~/.cusa/logs/` (SPEC-102).
    #[arg(long)]
    verbose: bool,

    /// Path to the Node.js sidecar entry (defaults to bundled sidecar).
    #[arg(long, env = "CUSA_SIDECAR")]
    sidecar: Option<String>,

    /// Override SDK setting sources; comma-separated of `user,project,local`.
    #[arg(long)]
    setting_sources: Option<String>,

    /// Inline MCP overrides file (JSON).
    #[arg(long)]
    mcp: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let log_path = logging::init(cli.verbose)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move { run(cli, log_path).await })
}

async fn run(mut cli: Cli, log_path: Option<std::path::PathBuf>) -> Result<()> {
    let cwd = std::env::current_dir()?.display().to_string();

    // SPEC-041: parse `--mcp <file>` inline overrides before we touch the
    // sidecar. A malformed file must exit 2 with a clear error.
    let mcp_overrides = match load_mcp_overrides(cli.mcp.as_deref()) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("cusa: --mcp: {err:#}");
            std::process::exit(2);
        }
    };

    // SPEC-051: BEFORE spawning the sidecar, offer to resume a session for
    // this cwd if we have any. Falling back to "New session" on Esc keeps
    // behavior identical to older versions.
    let session_store = session_store::SessionStore::for_config();
    let candidates = session_store.list_for_cwd(&cwd);
    let resume_choice = if !candidates.is_empty() {
        Some(app::startup::run_blocking(candidates, &cwd)?)
    } else {
        None
    };

    // SPEC-101: when no key is configured, prompt before handshake so
    // `make run-tui` and first-time `cusa` launches work without a
    // separate `cusa login` step.
    if let Err(err) = app::login::ensure_api_key() {
        eprintln!("cusa: {err:#}");
        std::process::exit(1);
    }

    let locator = sidecar::resolve_sidecar_entry(
        cli.sidecar.as_deref(),
        &sidecar::locator::StdEnv,
        std::env::current_exe().ok().as_deref(),
    )?;
    let mut cfg = sidecar::SupervisorConfig::new(locator, std::path::PathBuf::from(&cwd));
    cfg.log_path = log_path.clone();
    // SPEC-102 (sidecar half): with `--verbose`, hand the sidecar its own
    // rotating-log file next to the TUI log. Passed via `CUSA_LOG_FILE` at
    // spawn time (see `supervisor::build_command`).
    cfg.sidecar_log_path = log_path.as_ref().and_then(|p| {
        p.parent()
            .map(|dir| dir.join(format!("cusa-sidecar-{}.log", std::process::id())))
    });

    let supervisor = sidecar::SidecarSupervisor::spawn(cfg).await?;
    let (client, events) = supervisor.into_parts();

    let mut state = app::state::AppState::new(cwd.clone());
    state.session_store = Some(session_store);
    state.mcp_overrides = mcp_overrides.clone();

    // Apply the chooser outcome to `cli` + `state` before we call handshake.
    let mut chosen_stored: Option<session_store::StoredSession> = None;
    if let Some(outcome) = resume_choice.as_ref() {
        if let Some(agent_id) = app::startup::apply_choice_to_state(&mut state, outcome) {
            cli.resume = Some(agent_id);
            if let app::startup::ChooserOutcome::Resume(stored) = outcome {
                chosen_stored = Some(stored.clone());
            }
        }
    }

    // Handshake: initialize → (resume | create). Must complete before the
    // event loop lets the user submit prompts (SPEC-001, SPEC-050..053).
    if let Err(err) = handshake(&client, &cli, &cwd, &mut state).await {
        eprintln!("cusa: handshake failed: {err:#}");
        eprintln!(
            "hint: ensure CURSOR_API_KEY is set (or run `cusa login`), and \
             that `@cursor/sdk` is installed in the sidecar."
        );
        client.request_shutdown();
        return Err(err);
    }

    // SPEC-050: record the (possibly fresh) session on disk after a
    // successful handshake so the next launch's chooser can list it.
    persist_after_handshake(&state, chosen_stored.as_ref());

    app::run_event_loop(&mut state, client, events).await
}

/// Read a JSON file and validate it decodes into a `serde_json::Value`.
/// Returns `Ok(None)` when no path was passed.
fn load_mcp_overrides(path: Option<&str>) -> Result<Option<Value>> {
    let Some(p) = path else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(p)
        .with_context(|| format!("read {p}: file missing or unreadable"))?;
    let v: Value =
        serde_json::from_str(&text).with_context(|| format!("parse {p}: invalid JSON"))?;
    Ok(Some(v))
}

/// Persist the just-created / resumed session so the chooser can list it
/// next launch. Failures are logged but never fatal.
fn persist_after_handshake(
    state: &app::state::AppState,
    resumed: Option<&session_store::StoredSession>,
) {
    let (Some(store), Some(agent_id)) =
        (state.session_store.as_ref(), state.session.agent_id.as_ref())
    else {
        return;
    };
    let now = session_store::now_unix();
    if let Some(prev) = resumed {
        let mut updated = prev.clone();
        updated.last_used_at = now;
        updated.model = state.session.model.clone();
        updated.approval_mode = state.session.approval_mode;
        updated.enabled_skill_ids = state.session.enabled_skill_ids.clone();
        updated.mcp_overrides = state.mcp_overrides.clone();
        if let Err(err) = store.record_new(updated) {
            tracing::warn!(target: "session_store", ?err, "record_new (resume) failed");
        }
    } else {
        let entry = session_store::StoredSession {
            agent_id: agent_id.clone(),
            cwd: state.session.cwd.clone(),
            model: state.session.model.clone(),
            approval_mode: state.session.approval_mode,
            enabled_skill_ids: state.session.enabled_skill_ids.clone(),
            mcp_overrides: state.mcp_overrides.clone(),
            created_at: now,
            last_used_at: now,
            turns: 0,
        };
        if let Err(err) = store.record_new(entry) {
            tracing::warn!(target: "session_store", ?err, "record_new (fresh) failed");
        }
    }
}

/// Boot-time handshake: `initialize`, then `session/create` or
/// `session/resume` depending on `--resume`.
async fn handshake(
    client: &sidecar::SidecarClient,
    cli: &Cli,
    cwd: &str,
    state: &mut app::state::AppState,
) -> Result<()> {
    use std::time::Duration;

    let init_params = serde_json::json!({
        "protocolVersion": cusa_rpc::PROTOCOL_VERSION,
        "clientInfo": {
            "name": "cusa-tui",
            "version": env!("CARGO_PKG_VERSION"),
        }
    });
    let _init = client
        .call(
            cusa_rpc::method::INITIALIZE,
            Some(init_params),
            Duration::from_secs(15),
        )
        .await?
        .into_result()?;

    let setting_sources: Option<Vec<&str>> = cli
        .setting_sources
        .as_ref()
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .or(Some(vec!["user", "project"]));

    let approval_mode_str = match state.session.approval_mode {
        cusa_rpc::ApprovalMode::Suggest => "suggest",
        cusa_rpc::ApprovalMode::AutoEdit => "auto-edit",
        cusa_rpc::ApprovalMode::FullAuto => "full-auto",
    };

    let outcome = if let Some(agent_id) = &cli.resume {
        let mut params = serde_json::json!({
            "agentId": agent_id,
            "cwd": cwd,
            "approvalMode": approval_mode_str,
        });
        if !state.session.enabled_skill_ids.is_empty() {
            params["enabledSkillIds"] =
                serde_json::to_value(&state.session.enabled_skill_ids)
                    .unwrap_or(Value::Array(vec![]));
        }
        if let Some(mcp) = &state.mcp_overrides {
            params["mcpOverrides"] = mcp.clone();
        }
        client
            .call(
                cusa_rpc::method::SESSION_RESUME,
                Some(params),
                Duration::from_secs(30),
            )
            .await?
            .into_result()?
    } else {
        let mut params = serde_json::json!({
            "cwd": cwd,
            "settingSources": setting_sources,
            "approvalMode": approval_mode_str,
        });
        if !state.session.enabled_skill_ids.is_empty() {
            params["enabledSkillIds"] =
                serde_json::to_value(&state.session.enabled_skill_ids)
                    .unwrap_or(Value::Array(vec![]));
        }
        if let Some(mcp) = &state.mcp_overrides {
            params["mcpOverrides"] = mcp.clone();
        }
        client
            .call(
                cusa_rpc::method::SESSION_CREATE,
                Some(params),
                Duration::from_secs(30),
            )
            .await?
            .into_result()?
    };

    let Some(result) = outcome else {
        anyhow::bail!("sidecar returned empty result for session bootstrap");
    };
    if let Some(session_id) = result.get("sessionId").and_then(|v| v.as_str()) {
        state.session.session_id = Some(session_id.to_string());
    }
    if let Some(agent_id) = result.get("agentId").and_then(|v| v.as_str()) {
        state.session.agent_id = Some(agent_id.to_string());
    }
    if let Some(model) = result.get("model").and_then(|v| v.as_str()) {
        state.session.model = model.to_string();
    }
    Ok(())
}
