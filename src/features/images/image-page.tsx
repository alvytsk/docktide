import { useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useNavigate, useParams } from "@tanstack/react-router";
import { ChevronLeft } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import { ErrorPanel } from "@/components/error-panel";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ConnectionPanel } from "@/features/connection/connection-panel";
import { useConnectionStatus } from "@/features/connection/use-connection-status";
import { shortImageId } from "@/features/container-details/format";
import { useContainers } from "@/features/containers/use-containers";
import { imageRows, usedByText } from "./list";
import { ImageContext, imagesQueryKey, useImage, useImages } from "./use-images";
import { useRemoveImages } from "./use-remove-images";
import { UsageBadge } from "./usage-badge";

const tabClass =
  "-mb-px border-b-2 border-transparent px-1 pb-2 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring data-[status=active]:border-foreground data-[status=active]:font-medium data-[status=active]:text-foreground";

function Loading() {
  return (
    <div aria-busy className="flex flex-col gap-6">
      <Skeleton className="h-7 w-64" />
      <Skeleton className="h-5 w-40" />
      {[0, 1, 2].map((section) => (
        <div key={section} className="flex flex-col gap-2">
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-4 w-96" />
        </div>
      ))}
    </div>
  );
}

function Details({ id, connected }: { id: string; connected: boolean }) {
  const image = useImage(id, connected);
  const images = useImages(connected);
  const containers = useContainers(connected);
  const { remove, busy } = useRemoveImages();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data, error, isError, isFetching, refetch } = image;

  const row = useMemo(
    () =>
      data && images.data && containers.data
        ? imageRows(images.data, containers.data).find((r) => r.id === data.details.id)
        : undefined,
    [data, images.data, containers.data],
  );

  if (isError && error.kind === "notFound") {
    return (
      <section aria-live="polite" className="flex flex-col items-start gap-3">
        <h1 className="text-base font-semibold">This image no longer exists.</h1>
        <Link to="/images" className="text-sm underline underline-offset-4">
          Back to images
        </Link>
      </section>
    );
  }
  if (isError) {
    return <ErrorPanel error={error} retrying={isFetching} onRetry={() => void refetch()} />;
  }
  if (!data) return <Loading />;

  const { details } = data;
  const removeAndLeave = async () => {
    if (!row) return;
    await remove([row]);
    // `remove` refreshed the list; leave only if the image is really gone.
    const left = queryClient.getQueryData<{ id: string }[]>(imagesQueryKey);
    if (left && !left.some((i) => i.id === details.id)) void navigate({ to: "/images" });
  };

  return (
    <>
      <header className="flex flex-wrap items-start gap-x-6 gap-y-3">
        <div className="flex min-w-0 flex-col gap-1.5">
          <div className="flex flex-wrap items-baseline gap-3">
            <h1 className="truncate text-xl font-semibold tracking-tight">
              {details.repoTags[0] ?? "<none>"}
            </h1>
            <span className="font-mono text-xs text-muted-foreground">
              {shortImageId(details.id)}
            </span>
          </div>
          {row && <UsageBadge usage={row.usage} />}
        </div>
        <div className="ml-auto">
          {row && row.usage.kind !== "unused" ? (
            <Tooltip>
              <TooltipTrigger render={<span tabIndex={0} className="inline-block" />}>
                <Button variant="outline" size="sm" disabled tabIndex={-1}>
                  Remove
                </Button>
              </TooltipTrigger>
              <TooltipContent>{usedByText(row.usage.containers)}</TooltipContent>
            </Tooltip>
          ) : (
            <Button
              variant="outline"
              size="sm"
              disabled={!row || busy}
              onClick={() => void removeAndLeave()}
            >
              Remove
            </Button>
          )}
        </div>
      </header>
      <nav aria-label="Image sections" className="flex gap-5 border-b">
        <Link to="/images/$id/overview" params={{ id }} className={tabClass}>
          Overview
        </Link>
        <Link to="/images/$id/inspect" params={{ id }} className={tabClass}>
          Inspect
        </Link>
      </nav>
      <ImageContext value={data}>
        <Outlet />
      </ImageContext>
    </>
  );
}

/** Details, with Overview and Inspect tabs (spec M4 FR-5). */
export function ImagePage() {
  const { id } = useParams({ from: "/images/$id" });
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
        to="/images"
        className="-ml-1 flex w-fit items-center gap-1 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChevronLeft className="size-4" aria-hidden />
        Back to images
      </Link>
      {body}
    </main>
  );
}
