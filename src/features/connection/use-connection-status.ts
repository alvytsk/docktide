import { useQuery } from "@tanstack/react-query";
import type { ConnectionStatus } from "@/bindings/ConnectionStatus";
import { getConnectionStatus } from "@/lib/ipc";

export const connectionQueryKey = ["connection"] as const;

export function useConnectionStatus() {
  const query = useQuery({
    queryKey: connectionQueryKey,
    queryFn: getConnectionStatus,
    // The one accepted poll in M0; the events stream replaces it in M1.
    refetchInterval: 10_000,
    refetchIntervalInBackground: true,
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
