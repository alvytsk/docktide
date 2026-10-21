import { useQuery } from "@tanstack/react-query";
import type { ConnectionStatus } from "@/bindings/ConnectionStatus";
import { getConnectionStatus } from "@/lib/ipc";

export const connectionQueryKey = ["connection"] as const;

export function useConnectionStatus() {
  const query = useQuery({
    queryKey: connectionQueryKey,
    queryFn: getConnectionStatus,
    // No poll: live updates refetch on connection changes (spec M1 FR-2), and
    // focus picks up an endpoint changed while Docktide was in the background.
    refetchOnWindowFocus: true,
    // Failures are part of the status value; retrying would only delay it.
    retry: false,
  });
  // The command itself only rejects on IPC-level failures.
  const status: ConnectionStatus | undefined = query.isError
    ? { state: "disconnected", endpoint: null, error: query.error }
    : query.data;
  return { status, isFetching: query.isFetching, refetch: query.refetch };
}
