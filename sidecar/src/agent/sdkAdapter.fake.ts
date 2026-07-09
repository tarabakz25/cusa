// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// In-memory fake SdkAdapter used by tests. Every real network / SDK call
// is replaced with a deterministic scripted turn so that assertions can
// drive event sequences precisely.

import type {
  AgentHandle,
  CreateAgentOptions,
  ResumeAgentOptions,
  SdkAdapter,
  SendOptions,
  TurnEvent,
  TurnHandle,
  TurnResult,
} from "./sdkAdapter.js";
import type { ModelInfo, ModelSelection } from "../rpc/schema.js";

export type TurnScript = {
  events: TurnEvent[];
  /**
   * Delay (in ms) between events. Defaults to 0. Tests occasionally set
   * this to observe streaming interleaving.
   */
  delayMs?: number;
  /**
   * Final result returned by `turn.wait()`.
   */
  result?: TurnResult;
  /**
   * When true, `turn.supportsCancel` reports true and `cancel()` resolves
   * the wait with `status: "cancelled"`.
   */
  supportsCancel?: boolean;
  /**
   * When set, the wait promise never resolves on its own; only cancel or
   * signal abort will settle it. Handy for cancel-behavior tests.
   */
  hangUntilCancel?: boolean;
};

export interface FakeAdapterState {
  models: ModelInfo[];
  /** API keys passed to `listModels`, in call order. */
  listModelsKeys: string[];
  nextAgentId: number;
  nextRunId: number;
  createCalls: CreateAgentOptions[];
  resumeCalls: Array<{ agentId: string; opts: ResumeAgentOptions }>;
  disposedAgentIds: string[];
  sendCalls: Array<{
    agentId: string;
    text: string;
    modelOverride?: ModelSelection;
    systemContext?: string;
    mcpServers?: Record<string, unknown>;
  }>;
  /** Arguments of every `cancelStaleRuns` call, in call order. */
  cancelStaleRunsCalls: Array<{ agentId: string; cwd: string }>;
}

export class FakeSdkAdapter implements SdkAdapter {
  readonly state: FakeAdapterState = {
    models: [
      {
        id: "composer-2.5",
        displayName: "Composer 2.5",
        parameters: [
          {
            id: "effort",
            displayName: "Effort",
            values: [
              { value: "low", displayName: "Low" },
              { value: "medium", displayName: "Medium" },
              { value: "high", displayName: "High" },
            ],
          },
          {
            id: "fast",
            displayName: "Fast",
            values: [
              { value: "false", displayName: "Off" },
              { value: "true", displayName: "On" },
            ],
          },
        ],
      },
      { id: "claude-sonnet-4", displayName: "Claude Sonnet 4" },
    ],
    listModelsKeys: [],
    nextAgentId: 1,
    nextRunId: 1,
    createCalls: [],
    resumeCalls: [],
    disposedAgentIds: [],
    sendCalls: [],
    cancelStaleRunsCalls: [],
  };

  /**
   * Queue of scripts the next `send()` calls will play back. If empty, the
   * fake replies with an empty successful turn.
   */
  scripts: TurnScript[] = [];

  /**
   * When true, the next `send()` call never resolves on its own — it only
   * settles (with a rejection) once the caller aborts `SendOptions.signal`.
   * Used to exercise the session-level send timeout (issue #5).
   */
  hangNextSend = false;

  /**
   * When > 0, the next N `send()` calls reject with the SDK's local-store
   * busy error ("Agent <id> already has active run") before any turn is
   * created — mirroring @cursor/sdk, whose store guard runs before the
   * network phase. Decremented per rejected send.
   */
  busyNextSends = 0;

  /** Value `cancelStaleRuns` reports as the number of cancelled runs. */
  staleRunsToCancel = 1;

  /** When true, `cancelStaleRuns` rejects (simulates a broken store). */
  failCancelStaleRuns = false;

  script(...s: TurnScript[]): void {
    this.scripts.push(...s);
  }

  async cancelStaleRuns(
    agentId: string,
    opts: { cwd: string },
  ): Promise<number> {
    this.state.cancelStaleRunsCalls.push({ agentId, cwd: opts.cwd });
    if (this.failCancelStaleRuns) {
      throw new Error("fake local agent store unavailable");
    }
    return this.staleRunsToCancel;
  }

  async listModels(apiKey: string): Promise<ModelInfo[]> {
    this.state.listModelsKeys.push(apiKey);
    return this.state.models;
  }

  async createAgent(opts: CreateAgentOptions): Promise<AgentHandle> {
    this.state.createCalls.push(opts);
    const agentId = `fake-agent-${this.state.nextAgentId++}`;
    return new FakeAgentHandle(this, agentId, opts.model);
  }

  async resumeAgent(
    agentId: string,
    opts: ResumeAgentOptions,
  ): Promise<AgentHandle> {
    this.state.resumeCalls.push({ agentId, opts });
    return new FakeAgentHandle(this, agentId, undefined);
  }
}

class FakeAgentHandle implements AgentHandle {
  constructor(
    private readonly adapter: FakeSdkAdapter,
    public readonly agentId: string,
    private _model: ModelSelection | undefined,
  ) {}

  get model(): string | undefined {
    return this._model?.id;
  }

  async send(text: string, opts: SendOptions): Promise<TurnHandle> {
    this.adapter.state.sendCalls.push({
      agentId: this.agentId,
      text,
      ...(opts.modelOverride === undefined
        ? {}
        : { modelOverride: opts.modelOverride }),
      ...(opts.systemContext === undefined
        ? {}
        : { systemContext: opts.systemContext }),
      ...(opts.mcpServers === undefined
        ? {}
        : { mcpServers: opts.mcpServers }),
    });
    if (this.adapter.busyNextSends > 0) {
      this.adapter.busyNextSends -= 1;
      throw new Error(`Agent ${this.agentId} already has active run`);
    }
    if (this.adapter.hangNextSend) {
      this.adapter.hangNextSend = false;
      return new Promise<TurnHandle>((_, reject) => {
        const abort = () =>
          reject(new Error("fake send aborted via SendOptions.signal"));
        if (opts.signal?.aborted) {
          abort();
          return;
        }
        opts.signal?.addEventListener("abort", abort, { once: true });
      });
    }
    const script = this.adapter.scripts.shift() ?? {
      events: [],
      result: { status: "finished" },
    };
    const runId = `fake-run-${this.adapter.state.nextRunId++}`;
    const effectiveModel = opts.modelOverride?.id ?? this._model?.id;
    if (opts.modelOverride) {
      this._model = opts.modelOverride;
    }
    return new FakeTurnHandle(runId, effectiveModel, script, opts);
  }

  async dispose(): Promise<void> {
    this.adapter.state.disposedAgentIds.push(this.agentId);
  }
}

class FakeTurnHandle implements TurnHandle {
  private cancelled = false;
  private waitResolve: ((r: TurnResult) => void) | null = null;
  private playPromise: Promise<void>;

  constructor(
    public readonly runId: string,
    public readonly model: string | undefined,
    private readonly script: TurnScript,
    private readonly opts: SendOptions,
  ) {
    this.playPromise = this.play();
  }

  get supportsCancel(): boolean {
    return this.script.supportsCancel ?? true;
  }

  async cancel(): Promise<void> {
    this.cancelled = true;
    // Give play() a tick to observe the flag; then settle wait if it has
    // already parked on the hang-point.
    await Promise.resolve();
    if (this.waitResolve) {
      this.waitResolve({ status: "cancelled" });
      this.waitResolve = null;
    }
  }

  async wait(): Promise<TurnResult> {
    await this.playPromise;
    if (this.cancelled) return { status: "cancelled" };
    if (this.script.hangUntilCancel) {
      return new Promise<TurnResult>((resolve) => {
        // Re-check after we parked; cancel may have arrived first.
        if (this.cancelled) {
          resolve({ status: "cancelled" });
          return;
        }
        this.waitResolve = resolve;
      });
    }
    return this.script.result ?? { status: "finished" };
  }

  private async play(): Promise<void> {
    // Defer so that events never fire synchronously before `send()` has
    // returned to the caller and the session bookkeeping has been recorded.
    await Promise.resolve();
    const delay = this.script.delayMs ?? 0;
    for (const event of this.script.events) {
      if (this.cancelled) return;
      if (this.opts.signal?.aborted) return;
      if (delay > 0) {
        await new Promise<void>((r) => setTimeout(r, delay));
      } else {
        // Yield each tick so ordering is observable in tests.
        await Promise.resolve();
      }
      try {
        this.opts.onEvent(event);
      } catch {
        /* consumer bugs shouldn't stop the fake */
      }
    }
  }
}
