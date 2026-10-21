import type { ReactNode } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { errorKindLabel } from "@/lib/errors";
import { ConnectionPanel } from "./connection-panel";
import { describeSource } from "./describe";
import { useConnectionStatus } from "./use-connection-status";

const IMAGE_STORE_LABELS = {
  containerd: "containerd",
  classic: "Classic",
  unknown: "Unknown",
} as const;

function Light({ tone, beacon }: { tone: "ok" | "down" | "idle"; beacon: boolean }) {
  const color = { ok: "bg-signal-ok", down: "bg-signal-down", idle: "bg-sidebar-foreground/40" }[
    tone
  ];
  return (
    <span aria-hidden className="relative flex size-2.5 shrink-0">
      {beacon && (
        <span
          className={cn("absolute inset-0 rounded-full opacity-60 motion-safe:animate-ping", color)}
        />
      )}
      <span className={cn("relative size-2.5 rounded-full", color)} />
    </span>
  );
}

const triggerClass =
  "flex w-full items-center gap-3 rounded-md px-3 py-2 text-left outline-none hover:bg-sidebar-accent focus-visible:ring-2 focus-visible:ring-sidebar-ring";

function Label({ title, detail }: { title: string; detail: ReactNode }) {
  return (
    <span className="flex min-w-0 flex-col">
      <span className="text-sm font-medium text-sidebar-foreground">{title}</span>
      <span className="truncate text-xs text-sidebar-foreground/60">{detail}</span>
    </span>
  );
}

/** Sidebar footer: which daemon Docktide talks to, and whether it answers. */
export function ConnectionIndicator() {
  const { status, isFetching } = useConnectionStatus();

  if (!status) {
    return (
      <div className={triggerClass}>
        <Light tone="idle" beacon />
        <Label title="Connecting" detail="Looking for Docker" />
      </div>
    );
  }

  if (status.state === "disconnected") {
    return (
      <Popover>
        <PopoverTrigger className={triggerClass}>
          <Light tone="down" beacon={isFetching} />
          <Label title="Disconnected" detail={errorKindLabel(status.error.kind)} />
        </PopoverTrigger>
        <PopoverContent side="right" align="end" className="w-96">
          <ConnectionPanel error={status.error} endpoint={status.endpoint} />
        </PopoverContent>
      </Popover>
    );
  }

  const { endpoint, engine } = status;
  return (
    <Tooltip>
      <TooltipTrigger className={triggerClass}>
        <Light tone="ok" beacon={isFetching} />
        <Label title="Connected" detail={`Engine ${engine.serverVersion}`} />
      </TooltipTrigger>
      <TooltipContent side="right" align="end" className="max-w-sm">
        <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 py-1">
          <dt className="opacity-70">Endpoint</dt>
          <dd className="font-mono break-all">{endpoint.uri}</dd>
          <dt className="opacity-70">Source</dt>
          <dd>{describeSource(endpoint)}</dd>
          <dt className="opacity-70">API</dt>
          <dd>
            {engine.apiVersion} (server {engine.serverApiVersion})
          </dd>
          <dt className="opacity-70">Image store</dt>
          <dd>{IMAGE_STORE_LABELS[engine.imageStore]}</dd>
          <dt className="opacity-70">Rootless</dt>
          <dd>{engine.rootless ? "Yes" : "No"}</dd>
        </dl>
      </TooltipContent>
    </Tooltip>
  );
}
