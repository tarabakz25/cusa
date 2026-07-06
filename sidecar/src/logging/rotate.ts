// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Rotating file logger for the sidecar (SPEC-102, sidecar half).
//
// The TUI passes `CUSA_LOG_FILE=<absolute path>` when it starts the
// sidecar with `--verbose`. Every `log` RPC notification is mirrored
// into that file. On rotation trigger (default: file grows past
// 10 MiB), the current file is renamed to
// `<stem>.<yyyymmdd-hhmmss>.log` and a fresh handle is opened. Only the
// most recent `keep` backups are retained; older ones are unlinked.

import { existsSync, mkdirSync, statSync } from "node:fs";
import { open, readdir, rename, stat, unlink } from "node:fs/promises";
import path from "node:path";

import type { LogLevel } from "../rpc/schema.js";

export const DEFAULT_ROTATE_BYTES = 10 * 1024 * 1024;
export const DEFAULT_BACKUP_COUNT = 3;

export interface RotatingLoggerOptions {
  filePath: string;
  /** Rotate when the file exceeds this many bytes. Defaults to 10 MiB. */
  rotateBytes?: number;
  /** Retain this many backups (excluding the live file). Defaults to 3. */
  backupCount?: number;
  /** Injectable clock for deterministic filename stamps in tests. */
  now?: () => Date;
}

export interface LogEntry {
  level: LogLevel;
  message: string;
  target?: string | undefined;
  ts?: Date;
}

/**
 * Append-only rotating logger. All I/O is async but serialised through
 * an internal queue so callers can `fire-and-forget` from a `log`
 * notification handler without interleaving.
 */
export class RotatingLogger {
  private readonly filePath: string;
  private readonly rotateBytes: number;
  private readonly backupCount: number;
  private readonly now: () => Date;
  private currentBytes = 0;
  private chain: Promise<void> = Promise.resolve();
  private closed = false;

  constructor(opts: RotatingLoggerOptions) {
    this.filePath = opts.filePath;
    this.rotateBytes = opts.rotateBytes ?? DEFAULT_ROTATE_BYTES;
    this.backupCount = opts.backupCount ?? DEFAULT_BACKUP_COUNT;
    this.now = opts.now ?? (() => new Date());
    const dir = path.dirname(this.filePath);
    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true });
    }
    try {
      const s = statSync(this.filePath);
      this.currentBytes = s.size;
    } catch {
      this.currentBytes = 0;
    }
  }

  /**
   * Append a formatted log line. The returned promise resolves once the
   * append (and any rotation it triggers) has finished; callers can
   * ignore it if they don't need ordering with respect to subsequent
   * writes.
   */
  write(entry: LogEntry): Promise<void> {
    if (this.closed) return Promise.resolve();
    const chain = this.chain.then(() => this.doWrite(entry));
    this.chain = chain.catch(() => undefined);
    return chain;
  }

  /** Test hook: current live-file byte count. */
  liveFileBytes(): number {
    return this.currentBytes;
  }

  /** Drain the internal chain. Used to make tests deterministic. */
  drain(): Promise<void> {
    return this.chain;
  }

  /** Mark the logger closed; subsequent writes are dropped. */
  close(): void {
    this.closed = true;
  }

  private async doWrite(entry: LogEntry): Promise<void> {
    const line = formatLine(entry, entry.ts ?? this.now());
    const buf = Buffer.from(line, "utf8");
    if (
      this.currentBytes > 0 &&
      this.currentBytes + buf.byteLength > this.rotateBytes
    ) {
      await this.rotate();
    }
    const handle = await open(this.filePath, "a");
    try {
      await handle.write(buf);
    } finally {
      await handle.close();
    }
    this.currentBytes += buf.byteLength;
  }

  private async rotate(): Promise<void> {
    const stamp = formatStamp(this.now());
    const { dir, name, ext } = parseFilePath(this.filePath);
    const rotated = path.join(dir, `${name}.${stamp}${ext}`);
    try {
      await rename(this.filePath, rotated);
    } catch {
      // If the file cannot be moved (e.g. removed under our feet), just
      // reset the counter and continue.
    }
    this.currentBytes = 0;
    await this.pruneBackups();
  }

  private async pruneBackups(): Promise<void> {
    const { dir, name, ext } = parseFilePath(this.filePath);
    let names: string[];
    try {
      names = await readdir(dir);
    } catch {
      return;
    }
    const prefix = `${name}.`;
    const candidates = names.filter(
      (n) => n.startsWith(prefix) && n.endsWith(ext) && n !== path.basename(this.filePath),
    );
    const withStats: Array<{ full: string; mtime: number }> = [];
    for (const n of candidates) {
      const full = path.join(dir, n);
      try {
        const st = await stat(full);
        withStats.push({ full, mtime: st.mtimeMs });
      } catch {
        /* skip */
      }
    }
    withStats.sort((a, b) => b.mtime - a.mtime);
    const stale = withStats.slice(this.backupCount);
    for (const s of stale) {
      try {
        await unlink(s.full);
      } catch {
        /* best-effort */
      }
    }
  }
}

/** Compact ISO-like stamp: `YYYYMMDD-HHMMSS`. */
export function formatStamp(d: Date): string {
  const pad = (n: number): string => n.toString().padStart(2, "0");
  return (
    `${d.getFullYear()}` +
    `${pad(d.getMonth() + 1)}` +
    `${pad(d.getDate())}` +
    `-${pad(d.getHours())}` +
    `${pad(d.getMinutes())}` +
    `${pad(d.getSeconds())}`
  );
}

/** Format a single log line: `<ISO ts> <level> [target] message\n`. */
export function formatLine(entry: LogEntry, ts: Date): string {
  const target = entry.target ? ` [${entry.target}]` : "";
  return `${ts.toISOString()} ${entry.level.toUpperCase()}${target} ${entry.message}\n`;
}

/**
 * Split a log-file path into `{ dir, name, ext }` where `name` excludes
 * the extension. For `foo.log` this returns `{ dir, name: "foo", ext: ".log" }`.
 */
export function parseFilePath(
  filePath: string,
): { dir: string; name: string; ext: string } {
  const dir = path.dirname(filePath);
  const base = path.basename(filePath);
  const ext = path.extname(base);
  const name = base.slice(0, base.length - ext.length);
  return { dir, name, ext };
}
