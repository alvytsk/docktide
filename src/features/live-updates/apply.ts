import type { QueryClient } from "@tanstack/react-query";
import type { LiveUpdate } from "@/bindings/LiveUpdate";
import { connectionQueryKey } from "@/features/connection/use-connection-status";
import { containerQueryKey, containersQueryKey } from "@/features/containers/use-containers";
import { imagesQueryKey } from "@/features/images/use-images";

/** Refetches what a hint from the events supervisor says may have changed (spec M1 FR-2). */
export async function applyLiveUpdate(queryClient: QueryClient, update: LiveUpdate) {
  switch (update.type) {
    case "connected":
      // Events during the gap are lost, so anything may be stale.
      return queryClient.invalidateQueries();
    case "disconnected":
      return queryClient.invalidateQueries({ queryKey: connectionQueryKey });
    case "containersChanged":
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: containersQueryKey }),
        ...update.ids.map((id) =>
          queryClient.invalidateQueries({ queryKey: containerQueryKey(id) }),
        ),
      ]);
      return;
    case "imagesChanged":
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: imagesQueryKey }),
        queryClient.invalidateQueries({ queryKey: ["image"] }),
      ]);
  }
}
