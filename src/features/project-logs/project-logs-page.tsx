import { Link, useParams } from "@tanstack/react-router";
import { ArrowLeft, RotateCw } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { ErrorPanel } from "@/components/error-panel";
import { Button } from "@/components/ui/button";
import { Toggle } from "@/components/ui/toggle";
import { ConnectionPanel } from "@/features/connection/connection-panel";
import { useConnectionStatus } from "@/features/connection/use-connection-status";
import { useContainers } from "@/features/containers/use-containers";
import { Footer } from "@/features/logs/logs-tab";
import { LogView } from "@/features/logs/log-view";
import type { StreamStatus } from "@/features/logs/use-log-stream";
import type { ProjectLogSession } from "./session";
import { serviceColor } from "./tags";
import { useProjectLogs } from "./use-project-logs";

const linkClass = "text-sm underline underline-offset-4";

function RetryButton({ onClick }: { onClick: () => void }) {
  return (
    <Button variant="outline" size="sm" onClick={onClick}>
      <RotateCw />
      Retry
    </Button>
  );
}

/** Live while any stream is, opening while any is, ended otherwise. */
function overallStatus(statuses: StreamStatus[], loaded: boolean): StreamStatus {
  if (statuses.some((s) => s.kind === "live")) return { kind: "live" };
  if (!loaded || statuses.some((s) => s.kind === "opening")) return { kind: "opening" };
  return { kind: "ended" };
}

/** The first matching state of spec M3 §5.2's table that has a footer. */
function StatusFooter({
  session,
  project,
  members,
  retry,
}: {
  session: ProjectLogSession;
  project: string;
  members: ContainerSummary[];
  retry: (id: string) => void;
}) {
  const streams = [...session.streams];
  const failed = streams.find(([, s]) => s.kind === "error");
  if (failed) {
    const [id, status] = failed;
    return (
      <Footer>
        <span className="text-foreground">
          Logs for {session.sources.get(id)?.tag} failed:{" "}
          {status.kind === "error" && status.error.message}
        </span>
        <RetryButton onClick={() => retry(id)} />
      </Footer>
    );
  }
  const disconnected = streams.filter(([, s]) => s.kind === "disconnected").map(([id]) => id);
  if (disconnected.length > 0) {
    return (
      <Footer>
        Docker is not reachable.
        <RetryButton onClick={() => disconnected.forEach(retry)} />
      </Footer>
    );
  }
  if (members.length === 0) {
    return (
      <Footer>
        No containers in project {project}.
        <Link to="/containers" className={linkClass}>
          Back to containers
        </Link>
      </Footer>
    );
  }
  if (members.every((c) => c.state !== "running")) {
    return <Footer>No containers running. Logs resume when one starts.</Footer>;
  }
  return null;
}

function ProjectLogs({
  project,
  members,
  fetchedAt,
}: {
  project: string;
  members: ContainerSummary[] | undefined;
  fetchedAt: number;
}) {
  const { session, retry, setTail, clear, toggleService } = useProjectLogs(
    project,
    members,
    fetchedAt,
  );
  const { buffer, sources } = session;

  if (members?.length === 0 && buffer.entries.length === 0) {
    return (
      <section aria-live="polite" className="flex flex-col items-start gap-3">
        <p className="text-sm font-medium">No containers in project {project}.</p>
        <Link to="/containers" className={linkClass}>
          Back to containers
        </Link>
      </section>
    );
  }

  const tag = (source: string) => sources.get(source)?.tag ?? "";
  const service = (source: string) => sources.get(source)?.service ?? "";
  // Tags are padded to the longest, plus " |".
  const tagWidth = Math.max(0, ...[...sources.values()].map((s) => s.tag.length)) + 2;

  return (
    <LogView
      buffer={buffer}
      status={overallStatus([...session.streams.values()], members !== undefined)}
      tail={session.tail}
      onTail={setTail}
      onClear={clear}
      hidden={session.hidden.size > 0 ? (source) => session.hidden.has(service(source)) : undefined}
      renderTag={(source) => (
        <span
          className="shrink-0 whitespace-pre"
          style={{ width: `${tagWidth}ch`, color: serviceColor(service(source)) }}
        >
          {`${tag(source)} |`}
        </span>
      )}
      copyPrefix={(source) => `${tag(source)} | `}
      gapText={(entry) =>
        `${entry.dropped.toLocaleString("en")} lines skipped from ${tag(entry.source)} (output too fast)`
      }
      toolbarExtra={session.services().map((name) => (
        <Toggle
          key={name}
          variant="outline"
          size="sm"
          pressed={!session.hidden.has(name)}
          onPressedChange={() => toggleService(name)}
          className="not-aria-pressed:opacity-50"
          style={{ color: serviceColor(name) }}
        >
          {name}
        </Toggle>
      ))}
      footer={
        members && (
          <StatusFooter session={session} project={project} members={members} retry={retry} />
        )
      }
    />
  );
}

/** One Compose project's merged logs (spec M3 §5.2). */
export function ProjectLogsPage() {
  const { project } = useParams({ from: "/projects/$project/logs" });
  const { status } = useConnectionStatus();
  const { data, dataUpdatedAt, error, isError, isFetching, refetch } = useContainers(
    status?.state === "connected",
  );
  // A new array only for a new result, so the logs sync once per result.
  const members = useMemo(
    () => data?.filter((c) => c.compose?.project === project),
    [data, project],
  );

  let body: ReactNode;
  if (status?.state === "disconnected") {
    // Unmounts the logs, which closes every stream; a reconnect resumes them.
    body = <ConnectionPanel error={status.error} endpoint={status.endpoint} />;
  } else if (isError && !data) {
    body = <ErrorPanel error={error} retrying={isFetching} onRetry={() => void refetch()} />;
  } else {
    body = <ProjectLogs project={project} members={members} fetchedAt={dataUpdatedAt} />;
  }

  const running = members?.filter((c) => c.state === "running").length;
  return (
    <main className="flex min-w-0 flex-1 flex-col gap-6 overflow-y-auto px-10 py-8">
      <header className="flex flex-wrap items-baseline gap-3">
        <h1 className="truncate text-xl font-semibold tracking-tight">{project}</h1>
        {members && (
          <span className="text-sm text-muted-foreground tabular-nums">
            {running}/{members.length} running
          </span>
        )}
        <Link
          to="/containers"
          className="ml-auto flex items-center gap-1 self-center text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ArrowLeft className="size-4" aria-hidden />
          Containers
        </Link>
      </header>
      {body}
    </main>
  );
}
