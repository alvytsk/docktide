import { useMutation, useQueryClient } from "@tanstack/react-query";
import { connectionQueryKey } from "@/features/connection/use-connection-status";
import { isConnectionLevel } from "@/lib/errors";
import { runContainerAction } from "@/lib/ipc";
import { toastError } from "@/lib/toast";
import type { ContainerAction } from "./list";
import { containersQueryKey } from "./use-containers";

/** One mutation per row, so a pending action only blocks its own row. */
export function useContainerAction(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (action: ContainerAction) => runContainerAction(action, id),
    onError: toastError,
    // Returning the refetch keeps the row pending until the list shows the new state.
    onSettled: (_data, error) =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: containersQueryKey }),
        error &&
          isConnectionLevel(error.kind) &&
          queryClient.invalidateQueries({ queryKey: connectionQueryKey }),
      ]),
  });
}
