import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useEffectEvent } from "react";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { connectionQueryKey } from "@/features/connection/use-connection-status";
import { containersQueryKey } from "@/features/containers/use-containers";
import { useFrameRender, type LogTail } from "@/features/logs/use-log-stream";
import { projectSession } from "./session";

/**
 * Follows the logs of `project`'s `members` while mounted (spec M3 FR-2).
 * `fetchedAt` is the containers query's `dataUpdatedAt`: each new result is
 * synced once, which opens, closes, and reopens streams as members change.
 */
export function useProjectLogs(
  project: string,
  members: ContainerSummary[] | undefined,
  fetchedAt: number,
) {
  const session = projectSession(project);
  const render = useFrameRender();
  const queryClient = useQueryClient();
  // A result fetched after the end decides whether the stream reopens.
  const ended = useEffectEvent(
    () => void queryClient.invalidateQueries({ queryKey: containersQueryKey }),
  );
  const connectionLost = useEffectEvent(
    () => void queryClient.invalidateQueries({ queryKey: connectionQueryKey }),
  );

  useEffect(
    () => session.start(render, { onEnded: ended, onConnectionLost: connectionLost }),
    [session, render],
  );

  // Only on a new result: a redundant sync could reopen an ended stream twice.
  useEffect(() => {
    if (members) session.sync(members, fetchedAt);
  }, [session, members, fetchedAt]);

  return {
    session,
    retry: (id: string) => session.retry(id),
    setTail: (tail: LogTail) => session.setTail(tail),
    clear: () => session.clear(),
    toggleService: (service: string) => session.toggleService(service),
  };
}
