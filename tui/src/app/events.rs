// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Crossterm event pipeline.
//
// Crossterm's `read()` blocks. We run it on a `spawn_blocking` task so the
// tokio runtime stays responsive; each event is forwarded onto an mpsc
// channel that the event loop selects on.

use crossterm::event::{self, Event as CtEvent, MouseEventKind};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// TUI-level event. Widened later to include timers and internal signals.
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// Raw terminal event.
    Term(CtEvent),
    /// Coalesced wheel notches: positive scrolls up, negative scrolls down.
    Wheel(isize),
}

fn wheel_notches(evt: &CtEvent) -> Option<isize> {
    match evt {
        CtEvent::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(1),
            MouseEventKind::ScrollDown => Some(-1),
            _ => None,
        },
        _ => None,
    }
}

/// Spawn a blocking task that reads crossterm events and forwards them.
pub fn spawn_input(tx: mpsc::UnboundedSender<TuiEvent>) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        while let Ok(evt) = event::read() {
            let Some(mut notches) = wheel_notches(&evt) else {
                if tx.send(TuiEvent::Term(evt)).is_err() {
                    break;
                }
                continue;
            };

            loop {
                match event::poll(Duration::from_millis(8)) {
                    Ok(true) => match event::read() {
                        Ok(next) => {
                            if let Some(next_notches) = wheel_notches(&next) {
                                notches = notches.saturating_add(next_notches);
                            } else {
                                if notches != 0 && tx.send(TuiEvent::Wheel(notches)).is_err() {
                                    break;
                                }
                                if tx.send(TuiEvent::Term(next)).is_err() {
                                    break;
                                }
                                break;
                            }
                        }
                        Err(_) => break,
                    },
                    Ok(false) => {
                        if notches != 0 && tx.send(TuiEvent::Wheel(notches)).is_err() {
                            break;
                        }
                        break;
                    }
                    Err(_) => break,
                }
            }
            if tx.is_closed() {
                break;
            }
        }
    })
}
