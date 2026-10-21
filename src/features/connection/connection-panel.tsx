import type { EndpointInfo } from "@/bindings/EndpointInfo";
import type { IpcError } from "@/bindings/IpcError";
import { ErrorPanel } from "@/components/error-panel";
import { describeSource } from "./describe";
import { useConnectionStatus } from "./use-connection-status";

type Props = { error: IpcError; endpoint: EndpointInfo | null };

/** Explains why Docktide cannot reach Docker and how to fix it. */
export function ConnectionPanel({ error, endpoint }: Props) {
  const { refetch, isFetching } = useConnectionStatus();
  return (
    <ErrorPanel error={error} retrying={isFetching} onRetry={() => void refetch()}>
      {endpoint && (
        <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-sm">
          <dt className="text-muted-foreground">Endpoint</dt>
          <dd className="font-mono text-[0.8rem] break-all">{endpoint.uri}</dd>
          <dt className="text-muted-foreground">Source</dt>
          <dd>{describeSource(endpoint)}</dd>
        </dl>
      )}
    </ErrorPanel>
  );
}
