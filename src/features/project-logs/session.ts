import type { ContainerSummary } from "@/bindings/ContainerSummary";
import type { IpcError } from "@/bindings/IpcError";
import { LogBuffer } from "@/features/logs/log-buffer";
import type { LogTail, StreamStatus } from "@/features/logs/use-log-stream";
import { isConnectionLevel } from "@/lib/errors";
import { closeLogs, openLogs } from "@/lib/ipc";
import { tagFor } from "./tags";

export type LogsIpc = { open: typeof openLogs; close: typeof closeLogs };

export type SourceInfo = { service: string; tag: string };

type Hooks = { onEnded(): void; onConnectionLost(): void };

/** One container's open stream, with what the reopen rule needs. */
type Stream = {
  close: () => void;
  /** `Date.now()` when Docker ended the stream. */
  endedAt: number | null;
  /** The containers result this stream was reopened for, so it reopens once per result. */
  reopenedFor: number | null;
};

/**
 * The lines and settings of the most recently viewed project's logs (spec M3
 * FR-2): one stream per member container, merged into one buffer by
 * timestamp. Kept outside React like the Logs tab's session, so leaving and
 * returning resumes after each container's last line.
 */
export class ProjectLogSession {
  tail: LogTail = 1000;
  buffer = new LogBuffer();
  /** Services whose lines are hidden from the view. */
  hidden = new Set<string>();
  /** Container id → service and tag; kept for removed containers, whose lines stay. */
  sources = new Map<string, SourceInfo>();
  streams = new Map<string, StreamStatus>();
  version = 0;
  private open = new Map<string, Stream>();
  private members: ContainerSummary[] = [];
  private onChange = () => {};
  private hooks: Hooks = { onEnded() {}, onConnectionLost() {} };

  constructor(
    readonly project: string,
    private readonly ipc: LogsIpc = { open: openLogs, close: closeLogs },
  ) {}

  /** Attaches the callbacks; the returned function closes every stream. `onChange` runs after every change. */
  start(onChange: () => void, hooks: Hooks): () => void {
    this.onChange = onChange;
    this.hooks = hooks;
    return () => {
      for (const id of this.open.keys()) this.forget(id);
    };
  }

  /**
   * Applies a containers query result fetched at `fetchedAt`: opens a stream
   * for each new member, closes those of removed ones, and reopens an ended
   * stream once its container shows as running in a result fetched after the end.
   */
  sync(members: ContainerSummary[], fetchedAt: number) {
    this.members = members;
    const ids = new Set(members.map((c) => c.id));
    for (const id of this.open.keys()) if (!ids.has(id)) this.forget(id);
    for (const c of members) {
      this.sources.set(c.id, { service: c.compose?.service ?? c.name, tag: tagFor(c, members) });
      const stream = this.open.get(c.id);
      if (!stream) {
        this.connect(c.id);
      } else if (
        stream.endedAt !== null &&
        c.state === "running" &&
        fetchedAt > stream.endedAt &&
        stream.reopenedFor !== fetchedAt
      ) {
        this.connect(c.id).reopenedFor = fetchedAt;
      }
    }
    this.changed();
  }

  retry(id: string) {
    if (this.open.has(id)) this.connect(id);
  }

  /** A new tail starts over with an empty view. */
  setTail(tail: LogTail) {
    this.tail = tail;
    this.buffer = new LogBuffer();
    for (const id of this.open.keys()) this.connect(id);
    this.changed();
  }

  toggleService(service: string) {
    if (!this.hidden.delete(service)) this.hidden.add(service);
    this.changed();
  }

  clear() {
    this.buffer.clear();
    this.changed();
  }

  /** Services with a member or with lines in the buffer, sorted: one chip each. */
  services(): string[] {
    const services = new Set(this.members.map((c) => this.sources.get(c.id)!.service));
    for (const entry of this.buffer.entries) {
      const service = this.sources.get(entry.source)?.service;
      if (service !== undefined) services.add(service);
    }
    // A fresh array, and the lib target lacks toSorted.
    // oxlint-disable-next-line unicorn/no-array-sort
    return [...services].sort();
  }

  private changed() {
    this.version++;
    this.onChange();
  }

  private forget(id: string) {
    this.open.get(id)?.close();
    this.open.delete(id);
    this.streams.delete(id);
  }

  /** Opens (or reopens) container `id`'s stream, resuming after its last line when it has one. */
  private connect(id: string): Stream {
    this.open.get(id)?.close();
    const { buffer, ipc } = this;
    let closed = false;
    let subscription: number | null = null;
    const stream: Stream = {
      close: () => {
        closed = true;
        if (subscription !== null) void ipc.close(subscription);
      },
      endedAt: null,
      reopenedFor: null,
    };
    this.open.set(id, stream);
    const update = (status: StreamStatus) => {
      this.streams.set(id, status);
      this.changed();
    };
    const end = () => {
      stream.endedAt = Date.now();
      update({ kind: "ended" });
      this.hooks.onEnded();
    };
    const fail = (error: IpcError) => {
      if (error.kind === "notFound") {
        end();
      } else if (isConnectionLevel(error.kind)) {
        this.hooks.onConnectionLost();
        update({ kind: "disconnected" });
      } else {
        update({ kind: "error", error });
      }
    };

    update({ kind: "opening" });
    ipc
      .open(id, this.tail, buffer.resumePoint(id), (event) => {
        if (closed) return;
        switch (event.type) {
          case "lines":
            buffer.append(event.lines, id);
            this.streams.set(id, { kind: "live" });
            break;
          case "gap":
            buffer.gap(event.dropped, id);
            break;
          case "ended":
            end();
            return;
          case "error":
            fail(event.error);
            return;
        }
        this.changed();
      })
      .then(
        (sub) => {
          if (closed) {
            void ipc.close(sub);
            return;
          }
          subscription = sub;
          if (this.streams.get(id)?.kind === "opening") update({ kind: "live" });
        },
        (error: IpcError) => !closed && fail(error),
      );
    return stream;
  }
}

let current: ProjectLogSession | null = null;

/** The session of `project`; opening another project replaces it. */
export function projectSession(project: string): ProjectLogSession {
  if (current?.project !== project) current = new ProjectLogSession(project);
  return current;
}
