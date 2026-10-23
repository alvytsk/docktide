import { useMutation, useQueryClient } from "@tanstack/react-query";
import { runContainerAction } from "@/lib/ipc";
import { toastError } from "@/lib/toast";
import type { ContainerAction } from "./list";
import { containerQueryKey, containersQueryKey } from "./use-containers";

/** One mutation per row, so a pending action only blocks its own row. */
export function useContainerAction(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (action: ContainerAction) => runContainerAction(action, id),
    onError: toastError,
    // Returning the refetch keeps the buttons pending until the list and the
    // details page show the new state. Connection-level errors also refresh the
    // connection status (see providers.tsx).
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: containersQueryKey }),
        queryClient.invalidateQueries({ queryKey: containerQueryKey(id) }),
      ]),
  });
}
