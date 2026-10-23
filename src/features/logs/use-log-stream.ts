import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import type { IpcError } from "@/bindings/IpcError";
import { connectionQueryKey } from "@/features/connection/use-connection-status";
import { isConnectionLevel } from "@/lib/errors";
import { closeLogs, openLogs } from "@/lib/ipc";
import { LogBuffer } from "./log-buffer";

export const TAILS = [100, 1000, 10_000, null] as const;
/** Lines to fetch when opening; `null` is all. */
export type LogTail = (typeof TAILS)[number];

export type StreamStatus =
  | { kind: "opening" }
  | { kind: "live" }
  /** Docker closed the stream: the container stopped or was removed. */
  | { kind: "ended" }
  | { kind: "disconnected" }
  | { kind: "error"; error: IpcError };

/**
 * The lines and settings of the most recently viewed container's logs. Kept
 * outside React so they survive the tab unmounting (another tab, or the
 * connection panel while Docker is unreachable); a remount resumes after the
 * last line instead of fetching the tail again.
 */
class LogSession {
  tail: LogTail = 1000;
  buffer = new LogBuffer();
  status: StreamStatus = { kind: "opening" };
  /** The container's `startedAt` when the stream opened, to detect a restart. */
  openedFor: string | null = null;

  constructor(readonly id: string) {}

  /** Opens the stream; the returned function closes it. `onChange` runs after every change. */
  open(startedAt: string | null, onChange: () => void, onConnectionLost: () => void) {
    const { buffer } = this;
    let closed = false;
    let subscription: number | null = null;
    const update = (status: StreamStatus) => {
      this.status = status;
      onChange();
    };
    const fail = (error: IpcError) => {
      if (error.kind === "notFound") {
        // The details page shows the removed state once its query refetches.
        update({ kind: "ended" });
      } else if (isConnectionLevel(error.kind)) {
        onConnectionLost();
        update({ kind: "disconnected" });
      } else {
        update({ kind: "error", error });
      }
    };

    this.openedFor = startedAt;
    update({ kind: "opening" });
    openLogs(this.id, this.tail, buffer.lastTimestamp, (event) => {
      if (closed) return;
      switch (event.type) {
        case "lines":
          buffer.append(event.lines);
          this.status = { kind: "live" };
          break;
        case "gap":
          buffer.gap(event.dropped);
          break;
        case "ended":
          this.status = { kind: "ended" };
          break;
        case "error":
          fail(event.error);
          return;
      }
      onChange();
    }).then(
      (id) => {
        if (closed) {
          void closeLogs(id);
          return;
        }
        subscription = id;
        if (this.status.kind === "opening") update({ kind: "live" });
      },
      (error: IpcError) => !closed && fail(error),
    );

    return () => {
      closed = true;
      if (subscription !== null) void closeLogs(subscription);
    };
  }

  /** A new tail starts over with an empty view. */
  setTail(tail: LogTail) {
    this.tail = tail;
    this.buffer = new LogBuffer();
  }
}

let current: LogSession | null = null;

function sessionFor(id: string): LogSession {
  if (current?.id !== id) current = new LogSession(id);
  return current;
}

/** Re-renders at most once per animation frame, however many events arrive. */
function useFrameRender() {
  const [, setFrame] = useState(0);
  const pending = useRef(false);
  return useCallback(() => {
    if (pending.current) return;
    pending.current = true;
    requestAnimationFrame(() => {
      pending.current = false;
      setFrame((n) => n + 1);
    });
  }, []);
}

/**
 * Follows the logs of container `id` while mounted (spec M1 §5.4). Reopens
 * after the last shown line when the container starts again (`startedAt`
 * changes), on Retry, and on remount; a tail change starts over.
 */
export function useLogStream(id: string, startedAt: string | null, running: boolean) {
  const session = sessionFor(id);
  const render = useFrameRender();
  const queryClient = useQueryClient();
  const [generation, setGeneration] = useState(0);
  const reopen = useCallback(() => setGeneration((n) => n + 1), []);
  // Read when the stream opens; a new value alone is not a reason to reopen.
  const currentStart = useEffectEvent(() => startedAt);
  const connectionLost = useEffectEvent(
    () => void queryClient.invalidateQueries({ queryKey: connectionQueryKey }),
  );

  // A restart ends the stream. Once the details show the new run (a new
  // `startedAt` while running), the key changes and the stream reopens.
  const { status } = session;
  const run = status.kind === "ended" && running ? startedAt : (session.openedFor ?? startedAt);

  useEffect(
    () => session.open(currentStart(), render, connectionLost),
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- `generation` and `run` are reopen triggers
    [session, generation, run, render],
  );

  const setTail = useCallback(
    (tail: LogTail) => {
      session.setTail(tail);
      reopen();
    },
    [session, reopen],
  );

  const clear = useCallback(() => {
    session.buffer.clear();
    render();
  }, [session, render]);

  return { buffer: session.buffer, status, tail: session.tail, setTail, clear, retry: reopen };
}
