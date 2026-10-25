import { RotateCw } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { useLoadedContainer } from "@/features/container-details/use-container";
import { LogView } from "./log-view";
import { useLogStream, type StreamStatus } from "./use-log-stream";

function Footer({ children }: { children: ReactNode }) {
  return (
    <div
      aria-live="polite"
      className="flex items-center gap-3 border-t px-3 py-2 text-sm text-muted-foreground"
    >
      {children}
    </div>
  );
}

function StatusFooter({
  status,
  running,
  onRetry,
}: {
  status: StreamStatus;
  running: boolean;
  onRetry: () => void;
}) {
  const retry = (
    <Button variant="outline" size="sm" onClick={onRetry}>
      <RotateCw />
      Retry
    </Button>
  );
  if (status.kind === "disconnected") {
    return (
      <Footer>
        Docker is not reachable.
        {retry}
      </Footer>
    );
  }
  if (status.kind === "error") {
    return (
      <Footer>
        <span className="text-foreground">{status.error.message}</span>
        {status.error.hint}
        {retry}
      </Footer>
    );
  }
  // A running container reopens as soon as the details show its new run.
  if (status.kind === "ended" && !running) {
    return <Footer>Container stopped. Logs resume when it starts.</Footer>;
  }
  return null;
}

/** Logs: tail plus follow, filter, and resume (spec M1 §5.4). */
export function LogsTab() {
  const { details } = useLoadedContainer();
  const running = details.state === "running";
  const stream = useLogStream(details.id, details.startedAt, running);
  return (
    <LogView
      buffer={stream.buffer}
      status={stream.status}
      tail={stream.tail}
      onTail={stream.setTail}
      onClear={stream.clear}
      footer={<StatusFooter status={stream.status} running={running} onRetry={stream.retry} />}
    />
  );
}
