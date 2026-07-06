// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// JSON-RPC client with request/response correlation (SPEC-072).
//
// `SidecarClient` is the sole handle the app uses to talk to the sidecar. It
// exposes:
//   * `call(method, params)` — send a request, await the response
//   * `notify(method, params)` — send a notification (no reply)
//   * a paired `SidecarEvent` receiver (returned separately) that delivers
//     notifications, status transitions, log lines, and fatal errors
//
// The client is **transport-agnostic**: it emits outbound frames on an mpsc
// sender that any transport (real child process or a test peer) can consume.
// Inbound frames arrive on a companion mpsc that the client's `dispatch_loop`
// drains, routing responses to per-request oneshots and notifications to the
// app's event channel.
//
// For tests, `SidecarClient::in_memory()` returns a client wired to an
// `InMemoryPeer` that simulates the sidecar's side of the pipe.

use crate::sidecar::events::SidecarEvent;
use anyhow::{anyhow, bail, Context, Result};
use cusa_rpc::{RequestId, RpcError, ServerNotification};
use serde_json::Value;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Outcome of a JSON-RPC `call`. Either `result` or `error` is populated;
/// both being `None` is impossible in a well-formed reply.
#[derive(Debug, Clone)]
pub struct CallOutcome {
    pub result: Option<Value>,
    pub error: Option<RpcError>,
}

impl CallOutcome {
    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }

    /// Return the successful result, or convert the RPC error into an
    /// `anyhow::Error`. Timeouts / dropped transport surface separately via
    /// the outer `Result` returned from `call`.
    pub fn into_result(self) -> Result<Option<Value>> {
        if let Some(err) = self.error {
            return Err(anyhow!("rpc error {}: {}", err.code, err.message));
        }
        Ok(self.result)
    }
}

/// Frame the client asks the transport to send.
#[derive(Debug)]
pub enum OutboundFrame {
    /// A JSON value to be serialized and framed by the transport.
    Value(Value),
    /// Ask the transport to shut down cleanly (used on app quit).
    Shutdown,
}

type Pending =
    Arc<Mutex<std::collections::HashMap<RequestId, oneshot::Sender<Result<CallOutcome>>>>>;

/// The primary sidecar handle. Hold one per app.
#[derive(Debug, Clone)]
pub struct SidecarClient {
    outbound_tx: mpsc::UnboundedSender<OutboundFrame>,
    pending: Pending,
    next_id: Arc<AtomicI64>,
}

impl SidecarClient {
    /// Send a request and await the response. `call` is cancel-safe — if the
    /// caller drops the returned future, the pending slot is cleaned up on
    /// the next matching response (harmless leak, bounded by concurrency).
    pub async fn call(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<CallOutcome> {
        let id = RequestId::Num(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id.clone(), tx);
        }

        let mut req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(p) = params {
            req["params"] = p;
        }
        self.outbound_tx
            .send(OutboundFrame::Value(req))
            .map_err(|_| anyhow!("sidecar transport dropped"))?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(inner)) => inner,
            Ok(Err(_canceled)) => {
                bail!("sidecar dropped response for {method}");
            }
            Err(_timeout) => {
                self.pending.lock().await.remove(&id);
                bail!("sidecar call {method} timed out after {timeout:?}");
            }
        }
    }

    /// Health check (SPEC-073). Any answered frame — success or error —
    /// proves the sidecar is alive; only a timeout or dropped transport
    /// counts as failure.
    pub async fn ping(&self, timeout: Duration) -> Result<()> {
        let _ = self.call("$/ping", None, timeout).await?;
        Ok(())
    }

    /// Send a notification. No reply is expected.
    pub fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let mut n = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(p) = params {
            n["params"] = p;
        }
        self.outbound_tx
            .send(OutboundFrame::Value(n))
            .map_err(|_| anyhow!("sidecar transport dropped"))
    }

    /// Ask the transport to shut down. Idempotent.
    pub fn request_shutdown(&self) {
        let _ = self.outbound_tx.send(OutboundFrame::Shutdown);
    }

    /// Construct a client + its supervisor plumbing. Returns the client, the
    /// outbound frame receiver (the transport pumps from this to the child's
    /// stdin), the inbound frame sender (the transport pumps child stdout
    /// into this), and the app event receiver.
    pub fn new_paired() -> (
        Self,
        mpsc::UnboundedReceiver<OutboundFrame>,
        mpsc::UnboundedSender<Value>,
        mpsc::UnboundedSender<SidecarEvent>,
        mpsc::UnboundedReceiver<SidecarEvent>,
    ) {
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<OutboundFrame>();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel::<Value>();
        let (events_tx, events_rx) = mpsc::unbounded_channel::<SidecarEvent>();
        let pending: Pending = Arc::new(Mutex::new(Default::default()));
        let client = SidecarClient {
            outbound_tx,
            pending: pending.clone(),
            next_id: Arc::new(AtomicI64::new(1)),
        };
        tokio::spawn(dispatch_loop(inbound_rx, events_tx.clone(), pending));
        (client, outbound_rx, inbound_tx, events_tx, events_rx)
    }

    /// Test-only: construct a client wired to an in-memory peer.
    pub fn in_memory() -> (Self, InMemoryPeer) {
        let (client, outbound_rx, inbound_tx, events_tx, events_rx) = Self::new_paired();
        let peer = InMemoryPeer {
            outbound_rx,
            inbound_tx,
            events_tx,
            events_rx,
        };
        (client, peer)
    }
}

/// Drain inbound frames, routing responses to oneshots and notifications to
/// the app event channel. Runs until the inbound channel closes.
pub async fn dispatch_loop(
    mut inbound_rx: mpsc::UnboundedReceiver<Value>,
    events_tx: mpsc::UnboundedSender<SidecarEvent>,
    pending: Pending,
) {
    while let Some(frame) = inbound_rx.recv().await {
        let has_id = frame.get("id").is_some();
        let has_method = frame.get("method").is_some();
        let has_result_or_error = frame.get("result").is_some() || frame.get("error").is_some();
        match (has_id, has_method, has_result_or_error) {
            (true, false, true) | (true, true, true) => {
                let Some(id) = extract_id(&frame) else {
                    let _ = events_tx.send(SidecarEvent::OrphanResponseError(RpcError {
                        code: RpcError::INVALID_REQUEST,
                        message: "response missing id".into(),
                        data: Some(frame),
                    }));
                    continue;
                };
                let taken = pending.lock().await.remove(&id);
                let outcome = CallOutcome {
                    result: frame.get("result").cloned(),
                    error: extract_error(&frame),
                };
                if let Some(tx) = taken {
                    let _ = tx.send(Ok(outcome));
                } else if let Some(err) = outcome.error {
                    let _ = events_tx.send(SidecarEvent::OrphanResponseError(err));
                }
            }
            (false, true, false) | (false, true, true) => {
                match serde_json::from_value::<ServerNotification>(frame.clone()) {
                    Ok(n) => {
                        if events_tx.send(SidecarEvent::Notification(n)).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        tracing::warn!(target: "sidecar", "unknown notification: {frame}");
                    }
                }
            }
            _ => {
                tracing::warn!(target: "sidecar", "invalid frame shape: {frame}");
            }
        }
    }
}

fn extract_id(frame: &Value) -> Option<RequestId> {
    match frame.get("id")? {
        Value::Number(n) => n.as_i64().map(RequestId::Num),
        Value::String(s) => Some(RequestId::Str(s.clone())),
        _ => None,
    }
}

fn extract_error(frame: &Value) -> Option<RpcError> {
    let e = frame.get("error")?;
    let code = e
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or(RpcError::INTERNAL_ERROR);
    let message = e
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("(no message)")
        .to_string();
    let data = e.get("data").cloned();
    Some(RpcError {
        code,
        message,
        data,
    })
}

/// In-memory peer used by tests. It represents *the sidecar's side* of the
/// pipe: it consumes what the TUI writes and produces what the TUI reads.
#[derive(Debug)]
pub struct InMemoryPeer {
    outbound_rx: mpsc::UnboundedReceiver<OutboundFrame>,
    inbound_tx: mpsc::UnboundedSender<Value>,
    events_tx: mpsc::UnboundedSender<SidecarEvent>,
    pub events_rx: mpsc::UnboundedReceiver<SidecarEvent>,
}

impl InMemoryPeer {
    /// Wait for the next outbound frame written by the client.
    pub async fn expect_frame(&mut self) -> OutboundFrame {
        self.outbound_rx
            .recv()
            .await
            .expect("client dropped outbound sender")
    }

    /// Try to receive an outbound frame without blocking.
    pub fn try_recv_outbound(&mut self) -> Option<OutboundFrame> {
        self.outbound_rx.try_recv().ok()
    }

    /// Push an inbound frame as if the sidecar emitted it.
    pub fn emit(&self, frame: Value) {
        let _ = self.inbound_tx.send(frame);
    }

    /// Emit a typed server notification (wraps `emit`).
    pub fn emit_notification(&self, n: ServerNotification) {
        let mut v = serde_json::to_value(n).expect("serialize notification");
        v["jsonrpc"] = Value::String("2.0".into());
        self.emit(v);
    }

    /// Directly push a supervisor-style event into the app event channel.
    pub fn push_event(&self, evt: SidecarEvent) {
        let _ = self.events_tx.send(evt);
    }

    /// Convenience: build a response frame for a specific request id.
    pub fn respond_ok(&self, id: RequestId, result: Value) {
        self.emit(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }));
    }

    /// Convenience: build an error response.
    pub fn respond_err(&self, id: RequestId, code: i64, message: &str) {
        self.emit(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }));
    }
}

/// Utility: pump a supervisor-style outbound queue into an `AsyncWrite`.
///
/// Kept in this module so the real spawner and tests share the same
/// serialization logic.
pub async fn pump_outbound<W>(
    mut rx: mpsc::UnboundedReceiver<OutboundFrame>,
    mut writer: W,
) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use crate::sidecar::transport::write_frame;
    while let Some(frame) = rx.recv().await {
        match frame {
            OutboundFrame::Value(v) => {
                write_frame(&mut writer, &v)
                    .await
                    .context("write outbound frame")?;
            }
            OutboundFrame::Shutdown => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cusa_rpc::{
        method, RouterDecisionParams, RouterSource, ServerNotification, StreamMessageParams,
        StreamTextKind,
    };

    #[tokio::test]
    async fn spec_072_call_correlates_response() {
        let (client, mut peer) = SidecarClient::in_memory();
        let client_clone = client.clone();
        let client_task = tokio::spawn(async move {
            client_clone
                .call(method::MODELS_LIST, None, Duration::from_secs(2))
                .await
        });

        let frame = peer.expect_frame().await;
        let value = match frame {
            OutboundFrame::Value(v) => v,
            OutboundFrame::Shutdown => panic!("unexpected shutdown"),
        };
        assert_eq!(value["method"], method::MODELS_LIST);
        let id = extract_id(&value).expect("request id");
        peer.respond_ok(id, serde_json::json!({ "models": [] }));

        let outcome = client_task.await.unwrap().unwrap();
        assert!(outcome.is_ok());
        let result = outcome.into_result().unwrap().unwrap();
        assert_eq!(result["models"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn spec_072_call_reports_error_response() {
        let (client, mut peer) = SidecarClient::in_memory();
        let call = tokio::spawn(async move {
            client
                .call(method::SESSION_SEND, None, Duration::from_secs(2))
                .await
        });

        let frame = peer.expect_frame().await;
        let value = match frame {
            OutboundFrame::Value(v) => v,
            _ => panic!(),
        };
        let id = extract_id(&value).unwrap();
        peer.respond_err(id, RpcError::AGENT_ERROR, "no api key");

        let outcome = call.await.unwrap().unwrap();
        assert!(!outcome.is_ok());
        let err = outcome.error.unwrap();
        assert_eq!(err.code, RpcError::AGENT_ERROR);
        assert_eq!(err.message, "no api key");
    }

    #[tokio::test]
    async fn spec_072_call_timeout() {
        let (client, _peer) = SidecarClient::in_memory();
        let err = client
            .call(method::INITIALIZE, None, Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn spec_073_ping_accepts_method_not_found_response() {
        let (client, mut peer) = SidecarClient::in_memory();
        let ping = tokio::spawn(async move { client.ping(Duration::from_secs(2)).await });

        let frame = peer.expect_frame().await;
        let value = match frame {
            OutboundFrame::Value(v) => v,
            _ => panic!(),
        };
        assert_eq!(value["method"], "$/ping");
        let id = extract_id(&value).unwrap();
        peer.respond_err(id, RpcError::METHOD_NOT_FOUND, "unknown");
        ping.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn spec_072_notifications_forwarded_to_events() {
        let (_client, mut peer) = SidecarClient::in_memory();
        peer.emit_notification(ServerNotification::RouterDecision(RouterDecisionParams {
            session_id: "s1".into(),
            run_id: "r1".into(),
            model: "composer-2.5".into(),
            rationale: "unit test".into(),
            source: RouterSource::Rule,
        }));
        let evt = peer.events_rx.recv().await.unwrap();
        match evt {
            SidecarEvent::Notification(ServerNotification::RouterDecision(p)) => {
                assert_eq!(p.model, "composer-2.5");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn spec_072_stream_message_notification_dispatched() {
        let (_client, mut peer) = SidecarClient::in_memory();
        peer.emit_notification(ServerNotification::StreamMessage(StreamMessageParams {
            run_id: "r1".into(),
            delta: "hi".into(),
            kind: StreamTextKind::Assistant,
        }));
        let evt = peer.events_rx.recv().await.unwrap();
        match evt {
            SidecarEvent::Notification(ServerNotification::StreamMessage(p)) => {
                assert_eq!(p.delta, "hi");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn spec_072_orphan_error_response_surfaces() {
        let (_client, mut peer) = SidecarClient::in_memory();
        peer.respond_err(RequestId::Num(999), RpcError::AGENT_ERROR, "boom");
        let evt = peer.events_rx.recv().await.unwrap();
        match evt {
            SidecarEvent::OrphanResponseError(e) => {
                assert_eq!(e.code, RpcError::AGENT_ERROR);
                assert_eq!(e.message, "boom");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
