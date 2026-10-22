import { Link, Outlet, useParams } from "@tanstack/react-router";
import { ChevronLeft } from "lucide-react";
import type { ReactNode } from "react";
import { ErrorPanel } from "@/components/error-panel";
import { Skeleton } from "@/components/ui/skeleton";
import { ConnectionPanel } from "@/features/connection/connection-panel";
import { useConnectionStatus } from "@/features/connection/use-connection-status";
import { ContainerActions } from "@/features/containers/container-actions";
import { StateBadge } from "@/features/containers/state-badge";
import { useContainers } from "@/features/containers/use-containers";
import { ContainerContext, useContainer } from "./use-container";

// `data-status="active"` is set by the router's Link on the current tab.
const tabClass =
  "-mb-px border-b-2 border-transparent px-1 pb-2 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring data-[status=active]:border-foreground data-[status=active]:font-medium data-[status=active]:text-foreground";

function Tabs({ id }: { id: string }) {
  return (
    <nav aria-label="Container sections" className="flex gap-5 border-b">
      <Link to="/containers/$id/overview" params={{ id }} className={tabClass}>
        Overview
      </Link>
      <Link to="/containers/$id/logs" params={{ id }} className={tabClass}>
        Logs
      </Link>
      <Link to="/containers/$id/inspect" params={{ id }} className={tabClass}>
        Inspect
      </Link>
    </nav>
  );
}

function Loading() {
  return (
    <div aria-busy className="flex flex-col gap-6">
      <Skeleton className="h-7 w-64" />
      <Skeleton className="h-5 w-40" />
      {[0, 1, 2].map((section) => (
        <div key={section} className="flex flex-col gap-2">
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-4 w-96" />
          <Skeleton className="h-4 w-80" />
        </div>
      ))}
    </div>
  );
}

function Details({ id, connected }: { id: string; connected: boolean }) {
  const { data, error, isError, isFetching, refetch } = useContainer(id, connected);
  // Docker's status text ("Up 3 hours (healthy)") is only in the list.
  const status = useContainers(connected).data?.find((c) => c.id === id)?.status;

  if (isError && error.kind === "notFound") {
    return (
      <section aria-live="polite" className="flex flex-col items-start gap-3">
        <h1 className="text-base font-semibold">This container no longer exists.</h1>
        <Link to="/containers" className="text-sm underline underline-offset-4">
          Back to containers
        </Link>
      </section>
    );
  }
  if (isError) {
    return <ErrorPanel error={error} retrying={isFetching} onRetry={() => void refetch()} />;
  }
  if (!data) return <Loading />;

  const { details } = data;
  return (
    <>
      <header className="flex flex-wrap items-start gap-x-6 gap-y-3">
        <div className="flex min-w-0 flex-col gap-1.5">
          <div className="flex flex-wrap items-baseline gap-3">
            <h1 className="truncate text-xl font-semibold tracking-tight">{details.name}</h1>
            <span className="font-mono text-xs text-muted-foreground">
              {details.id.slice(0, 12)}
            </span>
          </div>
          <div className="flex items-center gap-3 text-sm text-muted-foreground">
            <StateBadge state={details.state} />
            {status}
          </div>
        </div>
        <div className="ml-auto">
          <ContainerActions container={details} variant="header" />
        </div>
      </header>
      <Tabs id={id} />
      <ContainerContext value={data}>
        <Outlet />
      </ContainerContext>
    </>
  );
}

/** Details, with Overview and Inspect tabs (spec M1 §5.3). */
export function ContainerPage() {
  const { id } = useParams({ from: "/containers/$id" });
  const { status } = useConnectionStatus();

  let body: ReactNode;
  if (status?.state === "disconnected") {
    body = <ConnectionPanel error={status.error} endpoint={status.endpoint} />;
  } else {
    body = <Details id={id} connected={status?.state === "connected"} />;
  }

  return (
    <main className="flex min-w-0 flex-1 flex-col gap-6 overflow-y-auto px-10 py-8">
      <Link
        to="/containers"
        className="-ml-1 flex w-fit items-center gap-1 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChevronLeft className="size-4" aria-hidden />
        Containers
      </Link>
      {body}
    </main>
  );
}
