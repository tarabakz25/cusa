// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// JSON-RPC framing over stdio (SPEC-072).
//
// Newline-delimited JSON: each frame is exactly one JSON value followed by a
// single `\n`. This module owns just the codec — reading and writing raw
// `serde_json::Value`s to/from something that implements
// `AsyncBufRead`/`AsyncWrite`. The higher-level request-response correlation
// lives in `sidecar::client`.

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, AsyncRead};

/// A single JSON frame arriving from the sidecar.
///
/// We keep it as a raw `Value` at the transport layer so the client can
/// discriminate between requests, responses and notifications by inspecting
/// the shape (presence of `id`, `method`, `result`, `error`).
#[derive(Debug, Clone)]
pub struct Frame(pub Value);

impl Frame {
    pub fn into_value(self) -> Value {
        self.0
    }
}

/// Read the next newline-delimited JSON frame. Returns `Ok(None)` on EOF.
pub async fn read_frame<R>(reader: &mut BufReader<R>) -> Result<Option<Frame>>
where
    R: AsyncRead + Unpin,
{
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .await
        .context("read from sidecar stdout")?;
    if n == 0 {
        return Ok(None);
    }
    let trimmed = line.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        // Ignore blank lines rather than failing parse; they can appear
        // from CRLF terminals.
        return Ok(Some(Frame(Value::Null)));
    }
    let v: Value = serde_json::from_str(trimmed)
        .with_context(|| format!("parse json frame: {trimmed:.200}"))?;
    Ok(Some(Frame(v)))
}

/// Write a JSON value as a single newline-terminated frame.
pub async fn write_frame<W>(writer: &mut W, value: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut buf = serde_json::to_vec(value).context("serialize frame")?;
    buf.push(b'\n');
    writer.write_all(&buf).await.context("write frame")?;
    writer.flush().await.context("flush frame")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cusa_rpc::{
        method, Notification, Request, RequestId, Response, RpcError, StreamMessageParams,
        StreamTextKind,
    };
    use tokio::io::BufReader;

    #[tokio::test]
    async fn spec_072_round_trip_notification() {
        let notif: Notification<StreamMessageParams> = Notification {
            jsonrpc: "2.0".into(),
            method: method::STREAM_MESSAGE.into(),
            params: Some(StreamMessageParams {
                run_id: "r1".into(),
                delta: "hello".into(),
                kind: StreamTextKind::Assistant,
            }),
        };
        let v = serde_json::to_value(&notif).unwrap();

        let _ = v; // First half unused; the second block below is the real test.

        let (a, mut b) = tokio::io::duplex(4096);
        let mut reader = BufReader::new(a);
        let v2 = serde_json::to_value(&notif).unwrap();
        tokio::spawn(async move {
            write_frame(&mut b, &v2).await.unwrap();
        });
        let frame = read_frame(&mut reader).await.unwrap().unwrap();
        let back: Notification<StreamMessageParams> = serde_json::from_value(frame.into_value())
            .expect("deserialize");
        assert_eq!(back.method, method::STREAM_MESSAGE);
        assert_eq!(back.params.unwrap().delta, "hello");
    }

    #[tokio::test]
    async fn spec_072_round_trip_response() {
        let resp: Response<serde_json::Value> = Response {
            jsonrpc: "2.0".into(),
            id: RequestId::Num(7),
            result: Some(serde_json::json!({ "ok": true })),
            error: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        let (a, mut b) = tokio::io::duplex(4096);
        let mut reader = BufReader::new(a);
        tokio::spawn(async move {
            write_frame(&mut b, &v).await.unwrap();
        });
        let frame = read_frame(&mut reader).await.unwrap().unwrap();
        let val = frame.into_value();
        assert_eq!(val["id"], 7);
        assert_eq!(val["result"]["ok"], true);
    }

    #[tokio::test]
    async fn spec_072_round_trip_error_response() {
        let resp: Response<serde_json::Value> = Response {
            jsonrpc: "2.0".into(),
            id: RequestId::Str("abc".into()),
            result: None,
            error: Some(RpcError {
                code: RpcError::AGENT_ERROR,
                message: "nope".into(),
                data: None,
            }),
        };
        let v = serde_json::to_value(&resp).unwrap();
        let (a, mut b) = tokio::io::duplex(4096);
        let mut reader = BufReader::new(a);
        tokio::spawn(async move {
            write_frame(&mut b, &v).await.unwrap();
        });
        let frame = read_frame(&mut reader).await.unwrap().unwrap();
        let val = frame.into_value();
        assert_eq!(val["id"], "abc");
        assert_eq!(val["error"]["code"], -32001);
    }

    #[tokio::test]
    async fn spec_072_round_trip_request() {
        let req: Request<serde_json::Value> = Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Num(1),
            method: method::INITIALIZE.into(),
            params: Some(serde_json::json!({ "protocolVersion": "0.1" })),
        };
        let v = serde_json::to_value(&req).unwrap();
        let (a, mut b) = tokio::io::duplex(4096);
        let mut reader = BufReader::new(a);
        tokio::spawn(async move {
            write_frame(&mut b, &v).await.unwrap();
        });
        let frame = read_frame(&mut reader).await.unwrap().unwrap();
        let val = frame.into_value();
        assert_eq!(val["method"], "initialize");
        assert_eq!(val["id"], 1);
    }

    #[tokio::test]
    async fn spec_072_eof_returns_none() {
        let (a, b) = tokio::io::duplex(4096);
        drop(b);
        let mut reader = BufReader::new(a);
        let frame = read_frame(&mut reader).await.unwrap();
        assert!(frame.is_none());
    }
}
