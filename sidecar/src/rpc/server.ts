// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Minimal newline-delimited JSON-RPC 2.0 server bound to a duplex stream
// (typically process.stdin / process.stdout). Framing: one JSON value per
// line, `\n` terminated.
//
// This is intentionally small; feature-specific method wiring lives in
// `sidecar/src/index.ts` and later in `sidecar/src/agent/*`.

import type { Readable, Writable } from "node:stream";

import {
  Method,
  RpcErrorCode,
  type Notification,
  type Request,
  type RequestId,
  type Response,
  type RpcError,
} from "./schema.js";

export type MethodHandler = (
  params: unknown,
  ctx: HandlerContext,
) => Promise<unknown> | unknown;

export interface HandlerContext {
  sendNotification: <M extends string>(method: M, params: unknown) => void;
  requestId: RequestId;
  method: string;
}

export interface RpcServerOptions {
  input?: Readable;
  output?: Writable;
  log?: (line: string) => void;
}

export class RpcServer {
  private handlers = new Map<string, MethodHandler>();
  private readonly input: Readable;
  private readonly output: Writable;
  private readonly log: (line: string) => void;
  private buffer = "";
  private closed = false;
  private stop: (() => void) | null = null;

  constructor(opts: RpcServerOptions = {}) {
    this.input = opts.input ?? process.stdin;
    this.output = opts.output ?? process.stdout;
    this.log = opts.log ?? ((line) => process.stderr.write(line + "\n"));
  }

  on<M extends string>(method: M, handler: MethodHandler): this {
    this.handlers.set(method, handler);
    return this;
  }

  notify<M extends string>(method: M, params: unknown): void {
    const frame: Notification<M> = { jsonrpc: "2.0", method, params: params as never };
    this.writeFrame(frame);
  }

  async run(): Promise<void> {
    this.input.setEncoding("utf8");
    return new Promise<void>((resolve) => {
      const done = () => {
        if (this.closed) return;
        this.closed = true;
        this.stop = null;
        resolve();
      };
      this.stop = done;
      this.input.on("data", (chunk: string) => this.onData(chunk));
      this.input.on("close", done);
      this.input.on("end", done);
      this.input.on("error", (err) => {
        this.log(`stdin error: ${(err as Error).message}`);
        done();
      });
    });
  }

  private onData(chunk: string): void {
    this.buffer += chunk;
    let idx: number;
    // eslint-disable-next-line no-cond-assign
    while ((idx = this.buffer.indexOf("\n")) >= 0) {
      const line = this.buffer.slice(0, idx).trim();
      this.buffer = this.buffer.slice(idx + 1);
      if (line.length === 0) continue;
      void this.dispatchLine(line);
    }
  }

  private async dispatchLine(line: string): Promise<void> {
    let msg: unknown;
    try {
      msg = JSON.parse(line);
    } catch (err) {
      this.log(`parse error: ${(err as Error).message}`);
      this.writeError(null, {
        code: RpcErrorCode.ParseError,
        message: "invalid JSON",
      });
      return;
    }
    if (!isRequest(msg)) {
      this.log(`ignoring non-request frame: ${line.slice(0, 200)}`);
      return;
    }
    const handler = this.handlers.get(msg.method);
    if (!handler) {
      this.writeError(msg.id, {
        code: RpcErrorCode.MethodNotFound,
        message: `method not found: ${msg.method}`,
      });
      return;
    }
    try {
      const result = await handler(msg.params, {
        sendNotification: (m, p) => this.notify(m, p),
        requestId: msg.id,
        method: msg.method,
      });
      this.writeResult(msg.id, result);
      if (msg.method === Method.Shutdown) {
        this.stop?.();
      }
    } catch (err) {
      const e = err as Error & { code?: number; data?: unknown };
      this.writeError(msg.id, {
        code: typeof e.code === "number" ? e.code : RpcErrorCode.InternalError,
        message: e.message || "unknown error",
        data: e.data,
      });
    }
  }

  private writeResult(id: RequestId, result: unknown): void {
    const frame: Response = { jsonrpc: "2.0", id, result };
    this.writeFrame(frame);
  }

  private writeError(id: RequestId | null, error: RpcError): void {
    if (id === null) {
      this.log(`unattached error: ${JSON.stringify(error)}`);
      return;
    }
    const frame: Response = { jsonrpc: "2.0", id, error };
    this.writeFrame(frame);
  }

  private writeFrame(frame: unknown): void {
    try {
      const line = JSON.stringify(frame);
      this.output.write(line + "\n");
    } catch (err) {
      this.log(`write error: ${(err as Error).message}`);
    }
  }
}

function isRequest(x: unknown): x is Request {
  if (typeof x !== "object" || x === null) return false;
  const o = x as Record<string, unknown>;
  return o.jsonrpc === "2.0" && typeof o.method === "string" && "id" in o;
}

export class RpcMethodError extends Error {
  readonly code: number;
  readonly data?: unknown;

  constructor(code: number, message: string, data?: unknown) {
    super(message);
    this.name = "RpcMethodError";
    this.code = code;
    this.data = data;
  }
}
