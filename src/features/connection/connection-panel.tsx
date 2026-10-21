import { RotateCw } from "lucide-react";
import type { EndpointInfo } from "@/bindings/EndpointInfo";
import type { IpcError } from "@/bindings/IpcError";
import { Button } from "@/components/ui/button";
import { describeSource } from "./describe";
import { useConnectionStatus } from "./use-connection-status";

type Props = { error: IpcError; endpoint: EndpointInfo | null };

/** Explains why Docktide cannot reach Docker and how to fix it. */
export function ConnectionPanel({ error, endpoint }: Props) {
  const { refetch, isFetching } = useConnectionStatus();
  return (
    <section aria-live="polite" className="flex max-w-xl flex-col gap-3">
      <h2 className="text-base font-semibold text-foreground">{error.message}</h2>
      {error.hint && <p className="text-sm leading-relaxed text-muted-foreground">{error.hint}</p>}
      {endpoint && (
        <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-sm">
          <dt className="text-muted-foreground">Endpoint</dt>
          <dd className="font-mono text-[0.8rem] break-all">{endpoint.uri}</dd>
          <dt className="text-muted-foreground">Source</dt>
          <dd>{describeSource(endpoint)}</dd>
        </dl>
      )}
      <div>
        <Button variant="outline" size="sm" disabled={isFetching} onClick={() => void refetch()}>
          <RotateCw className={isFetching ? "motion-safe:animate-spin" : undefined} />
          Retry
        </Button>
      </div>
    </section>
  );
}
