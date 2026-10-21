import { useQuery } from "@tanstack/react-query";
import { listContainers } from "@/lib/ipc";

export const containersQueryKey = ["containers"] as const;

/** Disabled while disconnected; the connection panel explains why. */
export function useContainers(enabled: boolean) {
  return useQuery({
    queryKey: containersQueryKey,
    queryFn: listContainers,
    enabled,
    // Errors carry a message and hint for the user; retrying would only delay them.
    retry: false,
  });
}
