// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Crossterm event pipeline.
//
// Crossterm's `read()` blocks. We run it on a `spawn_blocking` task so the
// tokio runtime stays responsive; each event is forwarded onto an mpsc
// channel that the event loop selects on.

use crossterm::event::{self, Event as CtEvent};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// TUI-level event. Widened later to include timers and internal signals.
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// Raw terminal event.
    Term(CtEvent),
}

/// Spawn a blocking task that reads crossterm events and forwards them.
pub fn spawn_input(tx: mpsc::UnboundedSender<TuiEvent>) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        while let Ok(evt) = event::read() {
            if tx.send(TuiEvent::Term(evt)).is_err() {
                break;
            }
        }
    })
}
