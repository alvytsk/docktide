import { RotateCw } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { ErrorPanel } from "@/components/error-panel";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ConnectionPanel } from "@/features/connection/connection-panel";
import { useConnectionStatus } from "@/features/connection/use-connection-status";
import { formatBytes } from "@/features/container-details/format";
import { useContainers } from "@/features/containers/use-containers";
import { useImageView } from "@/stores/image-view";
import { BuildCacheStrip } from "./build-cache-strip";
import { ConfirmDialog } from "./confirm-dialog";
import { ImageTable } from "./image-table";
import { filterAndSort, imageRows, unusedSize, type UsageFilter } from "./list";
import { useImages } from "./use-images";
import { useRemoveImages } from "./use-remove-images";

const USAGE_FILTERS: { value: UsageFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "unused", label: "Unused" },
  { value: "inUse", label: "In use" },
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

function NameList({ names }: { names: string[] }) {
  return (
    <ul className="mt-2 max-h-48 overflow-y-auto font-mono text-xs">
      {names.map((name, i) => (
        <li key={i}>{name}</li>
      ))}
    </ul>
  );
}

function ImageList({ enabled }: { enabled: boolean }) {
  const images = useImages(enabled);
  const containers = useContainers(enabled);
  const view = useImageView();
  const remove = useRemoveImages();
  const [picked, setPicked] = useState<Set<string>>(new Set());

  const rows = useMemo(
    () => images.data && containers.data && imageRows(images.data, containers.data),
    [images.data, containers.data],
  );
  const shown = useMemo(
    () => rows && filterAndSort(rows, view.text, view.usage, view.sort),
    [rows, view.text, view.usage, view.sort],
  );

  const error = images.error ?? containers.error;
  const isFetching = images.isFetching || containers.isFetching;
  const retry = () => void Promise.all([images.refetch(), containers.refetch()]);
  const unused = rows?.filter((r) => r.usage.kind === "unused") ?? [];
  // Only images that still exist and are unused stay selected.
  const chosen = unused.filter((r) => picked.has(r.id));
  const selected = new Set(chosen.map((r) => r.id));

  const onSelect = (ids: string[], on: boolean) =>
    setPicked(() => {
      const next = new Set(selected);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      return next;
    });

  let body: ReactNode;
  if (error) {
    body = <ErrorPanel error={error} retrying={isFetching} onRetry={retry} />;
  } else if (!rows || !shown) {
    body = <ImageTable />;
  } else if (rows.length === 0) {
    body = <Notice title="No images" />;
  } else if (shown.length === 0) {
    body = (
      <Notice title="No images match">
        <Button variant="outline" size="sm" onClick={view.clear}>
          Clear filters
        </Button>
      </Notice>
    );
  } else {
    body = (
      <ImageTable
        rows={shown}
        selected={selected}
        onSelect={onSelect}
        onRemove={(row) => void remove([row])}
      />
    );
  }

  return (
    <>
      <div className="flex flex-col gap-1">
        {rows && (
          <p className="text-sm text-muted-foreground">
            {`${rows.length} ${rows.length === 1 ? "image" : "images"} · about ${formatBytes(unusedSize(rows))} unused`}
          </p>
        )}
        <BuildCacheStrip enabled={enabled} />
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <Input
          type="search"
          aria-label="Filter images"
          placeholder="Filter by tag or ID"
          value={view.text}
          onChange={(event) => view.setText(event.target.value)}
          className="w-72"
        />
        <ToggleGroup
          aria-label="Usage"
          variant="outline"
          size="sm"
          spacing={0}
          value={[view.usage]}
          onValueChange={([value]) => {
            const next = USAGE_FILTERS.find((f) => f.value === value);
            if (next) view.setUsage(next.value);
          }}
        >
          {USAGE_FILTERS.map(({ value, label }) => (
            <ToggleGroupItem
              key={value}
              value={value}
              className="text-muted-foreground aria-pressed:bg-foreground aria-pressed:text-background"
            >
              {label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <div className="ml-auto flex items-center gap-2">
          {chosen.length > 0 && (
            <ConfirmDialog
              trigger={`Remove ${chosen.length} selected`}
              title={`Remove ${chosen.length} ${chosen.length === 1 ? "image" : "images"}?`}
              confirm="Remove"
              onConfirm={() => void remove(chosen)}
            >
              About {formatBytes(unusedSize(chosen))} will be freed.
              <NameList names={chosen.map((r) => r.name)} />
            </ConfirmDialog>
          )}
          <ConfirmDialog
            trigger="Remove all unused"
            disabled={unused.length === 0}
            title={`Remove ${unused.length} unused ${unused.length === 1 ? "image" : "images"}?`}
            confirm="Remove"
            onConfirm={() => void remove(unused)}
          >
            About {formatBytes(unusedSize(unused))} will be freed. Images with containers are kept.
            <NameList names={unused.map((r) => r.name)} />
          </ConfirmDialog>
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
      </div>
      {body}
    </>
  );
}

export function ImagesPage() {
  const { status } = useConnectionStatus();
  return (
    <main id="images" className="flex min-w-0 flex-1 flex-col gap-6 px-10 py-8">
      <h1 className="text-xl font-semibold tracking-tight">Images</h1>
      {status?.state === "disconnected" ? (
        <ConnectionPanel error={status.error} endpoint={status.endpoint} />
      ) : (
        <ImageList enabled={status?.state === "connected"} />
      )}
    </main>
  );
}
