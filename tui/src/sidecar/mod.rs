// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Sidecar orchestration: process spawn, framing, JSON-RPC client,
// health-ping supervision, and app-event dispatch (SPEC-071..SPEC-074).
//
// The public API surface for the app is limited to:
//   * [`SidecarClient`] — send requests / notifications
//   * [`SidecarEvent`] — inbound events (notifications, status, fatal)
//   * [`SidecarSupervisor`] — spawns and supervises the Node child
//   * [`SidecarLocator`] — locates the sidecar entry point
//
// Split into submodules so tests can exercise each layer independently.

pub mod client;
pub mod events;
pub mod locator;
pub mod supervisor;
pub mod transport;

pub use client::{CallOutcome, InMemoryPeer, OutboundFrame, SidecarClient};
pub use events::{SidecarEvent, SidecarStatus};
pub use locator::{resolve_sidecar_entry, SidecarLocator};
pub use supervisor::{SidecarSupervisor, SupervisorConfig};
