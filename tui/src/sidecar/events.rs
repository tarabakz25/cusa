// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// App-facing sidecar events (SPEC-073, SPEC-074).
//
// The supervisor task (see `sidecar::client`) forwards every inbound frame
// into the app in one of these shapes. Keeping this enum small and stable
// means the app's event loop can `select!` on a single mpsc receiver.

use cusa_rpc::{RpcError, ServerNotification};
use std::path::PathBuf;

/// Health/lifecycle state of the sidecar as seen by the supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarStatus {
    /// The supervisor is currently spawning a child process.
    Starting,
    /// The child is up and responding to pings.
    Ready,
    /// The child exited or stopped answering pings; a restart is in flight.
    Down,
    /// A previously-Down child came back up.
    Reconnected,
}

/// Events the supervisor pushes to the app. The app's `AppState` mutates in
/// response to each.
#[derive(Debug)]
pub enum SidecarEvent {
    /// A notification from the sidecar. Requests/responses are correlated
    /// inside the client and are **not** delivered on this channel.
    Notification(ServerNotification),
    /// Supervisor health transition (SPEC-073).
    Status(SidecarStatus),
    /// Line captured from the sidecar's stderr (forwarded to the tracing
    /// subscriber but also surfaced here for the debug overlay).
    Log(String),
    /// The supervisor gave up after retries (SPEC-074). Contains a copyable
    /// log path when logging is enabled.
    Fatal {
        message: String,
        log_path: Option<PathBuf>,
    },
    /// The sidecar returned a JSON-RPC error for a request the app never
    /// registered a pending waiter for. Rarely used but keeps the invariant
    /// that error frames are never silently dropped.
    OrphanResponseError(RpcError),
}
