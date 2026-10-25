import { RotateCw } from "lucide-react";
import type { ReactNode } from "react";
import { ErrorPanel } from "@/components/error-panel";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ConnectionPanel } from "@/features/connection/connection-panel";
import { useConnectionStatus } from "@/features/connection/use-connection-status";
import { useContainerFilter } from "@/stores/container-filter";
import { ContainerTable } from "./container-table";
import { groupContainers } from "./groups";
import type { StateFilter } from "./list";
import { useContainers } from "./use-containers";

const STATE_FILTERS: { value: StateFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "running", label: "Running" },
  { value: "stopped", label: "Stopped" },
];

function Notice({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-3 rounded-lg border border-dashed px-6 py-12 text-sm">
      <p className="font-medium">{title}</p>
      {children}
    </div>
  );
}

function ContainerList({ enabled }: { enabled: boolean }) {
  const { data, error, isError, isFetching, refetch } = useContainers(enabled);
  const filter = useContainerFilter();
  const list = data && groupContainers(data, filter);
  const retry = () => void refetch();

  let body: ReactNode;
  if (isError) {
    body = <ErrorPanel error={error} retrying={isFetching} onRetry={retry} />;
  } else if (!data || !list) {
    body = <ContainerTable />;
  } else if (data.length === 0) {
    body = <Notice title="No containers yet" />;
  } else if (list.shownCount === 0) {
    body = (
      <Notice title="No containers match">
        <Button variant="outline" size="sm" onClick={filter.clear}>
          Clear filters
        </Button>
      </Notice>
    );
  } else {
    body = <ContainerTable list={list} />;
  }

  return (
    <>
      <div className="flex flex-wrap items-center gap-3">
        <Input
          type="search"
          aria-label="Filter containers"
          placeholder="Filter by name, image, project, or ID"
          value={filter.text}
          onChange={(event) => filter.setText(event.target.value)}
          className="w-72"
        />
        <ToggleGroup
          aria-label="State"
          variant="outline"
          size="sm"
          spacing={0}
          value={[filter.state]}
          onValueChange={([value]) => {
            const next = STATE_FILTERS.find((f) => f.value === value);
            if (next) filter.setState(next.value);
          }}
        >
          {STATE_FILTERS.map(({ value, label }) => (
            <ToggleGroupItem
              key={value}
              value={value}
              className="text-muted-foreground aria-pressed:bg-foreground aria-pressed:text-background"
            >
              {label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <span className="ml-auto text-sm text-muted-foreground tabular-nums">
          {data && list && `${list.shownCount} of ${data.length}`}
        </span>
        <Button
          variant="outline"
          size="icon-sm"
          aria-label="Refresh"
          disabled={!enabled || isFetching}
          onClick={retry}
        >
          <RotateCw className={isFetching ? "motion-safe:animate-spin" : undefined} />
        </Button>
      </div>
      {body}
    </>
  );
}

export function ContainersPage() {
  const { status } = useConnectionStatus();
  return (
    <main id="containers" className="flex min-w-0 flex-1 flex-col gap-6 px-10 py-8">
      <h1 className="text-xl font-semibold tracking-tight">Containers</h1>
      {status?.state === "disconnected" ? (
        <ConnectionPanel error={status.error} endpoint={status.endpoint} />
      ) : (
        <ContainerList enabled={status?.state === "connected"} />
      )}
    </main>
  );
}
