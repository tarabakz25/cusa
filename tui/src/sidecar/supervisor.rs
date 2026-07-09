// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Sidecar process supervisor (SPEC-073, SPEC-074).
//
// The supervisor owns:
//   * the Node.js child process,
//   * three IO pumps (stdout -> inbound frames, outbound -> stdin, stderr
//     -> tracing + Log events),
//   * a periodic health-ping task,
//   * a restart-once policy on unexpected child exit.
//
// It emits `SidecarEvent` values onto the app event channel and consumes
// `OutboundFrame`s from the client. Tests do NOT construct a real supervisor
// — they use `SidecarClient::in_memory` and drive events directly.

use crate::sidecar::client::{OutboundFrame, SidecarClient};
use crate::sidecar::events::{SidecarEvent, SidecarStatus};
use crate::sidecar::locator::SidecarLocator;
use crate::sidecar::transport::{read_frame, write_frame};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};

/// Runtime configuration for the supervisor.
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub locator: SidecarLocator,
    /// Working directory for the child process (usually the user's cwd).
    pub cwd: PathBuf,
    /// Health-ping interval. SPEC-073 says 5 s.
    pub ping_interval: Duration,
    /// Timeout for a single ping. SPEC-073 says 10 s.
    pub ping_timeout: Duration,
    /// Optional log path (surfaced in fatal modal per SPEC-074).
    pub log_path: Option<PathBuf>,
    /// Optional rotating-log file for the *sidecar* (SPEC-102, sidecar
    /// half). When set, `spawn_child` exports it as `CUSA_LOG_FILE` so the
    /// sidecar mirrors every `log` notification into that file.
    pub sidecar_log_path: Option<PathBuf>,
}

impl SupervisorConfig {
    pub fn new(locator: SidecarLocator, cwd: PathBuf) -> Self {
        Self {
            locator,
            cwd,
            ping_interval: Duration::from_secs(5),
            ping_timeout: Duration::from_secs(10),
            log_path: None,
            sidecar_log_path: None,
        }
    }
}

/// The supervisor spawns children and returns a fully-wired `SidecarClient`
/// plus the app's `SidecarEvent` receiver.
pub struct SidecarSupervisor {
    pub client: SidecarClient,
    pub events: mpsc::UnboundedReceiver<SidecarEvent>,
}

impl SidecarSupervisor {
    /// Split the supervisor into its parts. Consuming it lets the caller
    /// move `events` into the event loop while keeping `client` for the app.
    pub fn into_parts(self) -> (SidecarClient, mpsc::UnboundedReceiver<SidecarEvent>) {
        (self.client, self.events)
    }
}

impl SidecarSupervisor {
    /// Spawn the sidecar and start supervision. Returns immediately once
    /// the child is up (or produces an error if the initial spawn fails).
    pub async fn spawn(cfg: SupervisorConfig) -> Result<Self> {
        let (client, outbound_rx, inbound_tx, events_tx, events_rx) = SidecarClient::new_paired();

        // Wrap the outbound_rx in a shared cell so restart can install a new
        // stdin writer while re-using the same receiver.
        let outbound_holder = Arc::new(Mutex::new(Some(outbound_rx)));
        let restarts = Arc::new(AtomicI64::new(0));

        // Kick off the supervision task.
        {
            let client = client.clone();
            let events_tx = events_tx.clone();
            let inbound_tx = inbound_tx.clone();
            let cfg = cfg.clone();
            let outbound_holder = outbound_holder.clone();
            let restarts = restarts.clone();
            tokio::spawn(async move {
                supervise(cfg, client, outbound_holder, inbound_tx, events_tx, restarts).await;
            });
        }

        // We intentionally do not wait for the first ping here; the app
        // shows a "starting" status until either `Ready` or `Fatal` arrives.
        let _ = events_tx.send(SidecarEvent::Status(SidecarStatus::Starting));

        // The dispatch loop is already running (spawned in `new_paired`).
        // Keep inbound_tx alive so it doesn't get dropped by the caller.
        // We intentionally leak the sender clone into the supervisor task.
        drop(inbound_tx);

        Ok(Self {
            client,
            events: events_rx,
        })
    }
}

/// One iteration = one child lifetime. On unexpected exit, `supervise`
/// attempts a single restart before emitting `Fatal` (SPEC-074).
async fn supervise(
    cfg: SupervisorConfig,
    client: SidecarClient,
    outbound_holder: Arc<Mutex<Option<mpsc::UnboundedReceiver<OutboundFrame>>>>,
    inbound_tx: mpsc::UnboundedSender<Value>,
    events_tx: mpsc::UnboundedSender<SidecarEvent>,
    restarts: Arc<AtomicI64>,
) {
    let mut first = true;
    loop {
        let spawn_result = spawn_child(&cfg).await;
        let mut child = match spawn_result {
            Ok(child) => child,
            Err(e) => {
                let _ = events_tx.send(SidecarEvent::Fatal {
                    message: format!("failed to spawn sidecar: {e:#}"),
                    log_path: cfg.log_path.clone(),
                });
                return;
            }
        };

        let outbound_rx = outbound_holder.lock().await.take();
        let Some(outbound_rx) = outbound_rx else {
            tracing::error!("outbound receiver missing on supervisor restart");
            return;
        };

        let stdout = child
            .stdout
            .take()
            .expect("stdout piped");
        let stdin = child
            .stdin
            .take()
            .expect("stdin piped");
        let stderr = child.stderr.take().expect("stderr piped");

        // Fork three IO tasks + one ping task.
        let inbound_tx2 = inbound_tx.clone();
        let read_task = tokio::spawn(async move {
            let mut r = BufReader::new(stdout);
            loop {
                match read_frame(&mut r).await {
                    Ok(Some(frame)) => {
                        if inbound_tx2.send(frame.into_value()).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(target: "sidecar", "read_frame error: {e:#}");
                        break;
                    }
                }
            }
        });

        let write_task = tokio::spawn(async move {
            let mut w = stdin;
            let mut rx = outbound_rx;
            while let Some(frame) = rx.recv().await {
                match frame {
                    OutboundFrame::Value(v) => {
                        if let Err(e) = write_frame(&mut w, &v).await {
                            tracing::warn!(target: "sidecar", "write_frame error: {e:#}");
                            break;
                        }
                    }
                    OutboundFrame::Shutdown => break,
                }
            }
            // Return the receiver on shutdown so restart can reuse it.
            rx
        });

        let events_tx2 = events_tx.clone();
        let stderr_task = tokio::spawn(async move {
            let mut r = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = r.next_line().await {
                tracing::info!(target: "sidecar", "{line}");
                let _ = events_tx2.send(SidecarEvent::Log(line));
            }
        });

        // Health-ping task. Runs until either the child dies or client is
        // dropped.
        let client_ping = client.clone();
        let events_ping = events_tx.clone();
        let ping_interval = cfg.ping_interval;
        let ping_timeout = cfg.ping_timeout;
        let ping_task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(ping_interval);
            ticker.tick().await; // consume the immediate first tick
            loop {
                ticker.tick().await;
                if client_ping.ping(ping_timeout).await.is_err() {
                    let _ = events_ping.send(SidecarEvent::Status(SidecarStatus::Down));
                    break;
                }
            }
        });

        if !first {
            let _ = events_tx.send(SidecarEvent::Status(SidecarStatus::Reconnected));
        } else {
            let _ = events_tx.send(SidecarEvent::Status(SidecarStatus::Ready));
        }
        first = false;

        // Wait for the child to exit.
        let exit_status = match child.wait().await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(target: "sidecar", "child wait error: {e:#}");
                let _ = events_tx.send(SidecarEvent::Fatal {
                    message: format!("sidecar wait failed: {e:#}"),
                    log_path: cfg.log_path.clone(),
                });
                return;
            }
        };

        ping_task.abort();
        stderr_task.abort();
        read_task.abort();
        if let Ok(rx) = write_task.await {
            *outbound_holder.lock().await = Some(rx);
        }

        let _ = events_tx.send(SidecarEvent::Status(SidecarStatus::Down));

        let attempts = restarts.fetch_add(1, Ordering::SeqCst);
        if attempts >= 1 {
            let _ = events_tx.send(SidecarEvent::Fatal {
                message: format!(
                    "sidecar exited unexpectedly ({:?}); giving up after 1 restart",
                    exit_status
                ),
                log_path: cfg.log_path.clone(),
            });
            return;
        }

        // Small backoff before restart.
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn spawn_child(cfg: &SupervisorConfig) -> Result<Child> {
    let mut cmd = build_command(cfg);
    cmd.spawn().with_context(|| {
        format!(
            "spawning sidecar: {} {}",
            cfg.locator.node.display(),
            cfg.locator.entry.display()
        )
    })
}

/// Compose the sidecar `Command`. Split from `spawn_child` so the env /
/// argv wiring is unit-testable without spawning a process.
///
/// SPEC-102 (sidecar half): the sidecar's rotating file logger is armed by
/// the `CUSA_LOG_FILE` env var at spawn time. Before this was wired up the
/// sidecar log file was silently never written (issue #5, item 3).
fn build_command(cfg: &SupervisorConfig) -> Command {
    let mut cmd = Command::new(&cfg.locator.node);
    cmd.arg(&cfg.locator.entry)
        .current_dir(&cfg.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(path) = &cfg.sidecar_log_path {
        cmd.env("CUSA_LOG_FILE", path);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> SupervisorConfig {
        let locator = SidecarLocator {
            node: PathBuf::from("/usr/bin/node"),
            entry: PathBuf::from("/opt/cusa/sidecar/dist/index.js"),
        };
        SupervisorConfig::new(locator, PathBuf::from("/tmp/repo"))
    }

    #[test]
    fn spec_102_build_command_exports_cusa_log_file_when_configured() {
        let mut cfg = test_cfg();
        cfg.sidecar_log_path = Some(PathBuf::from("/tmp/logs/cusa-sidecar-42.log"));
        let cmd = build_command(&cfg);
        let envs: Vec<_> = cmd.as_std().get_envs().collect();
        assert!(
            envs.iter().any(|(k, v)| {
                *k == std::ffi::OsStr::new("CUSA_LOG_FILE")
                    && v.is_some_and(|v| v == std::ffi::OsStr::new("/tmp/logs/cusa-sidecar-42.log"))
            }),
            "CUSA_LOG_FILE should be exported to the sidecar: {envs:?}"
        );
    }

    #[test]
    fn spec_102_build_command_omits_cusa_log_file_by_default() {
        let cfg = test_cfg();
        let cmd = build_command(&cfg);
        let has_env = cmd
            .as_std()
            .get_envs()
            .any(|(k, _)| k == std::ffi::OsStr::new("CUSA_LOG_FILE"));
        assert!(!has_env, "CUSA_LOG_FILE must not be set when logging is off");
    }
}
