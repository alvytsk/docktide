import { LoaderCircle, Play, RotateCw, Square, type LucideIcon } from "lucide-react";
import type { ContainerState } from "@/bindings/ContainerState";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { availableActions, formatPort, type ContainerAction } from "./list";
import { useContainerAction } from "./use-container-action";

const STATE_DOTS: Record<ContainerState, string> = {
  running: "bg-signal-ok",
  paused: "bg-amber-500",
  restarting: "bg-amber-500",
  dead: "bg-signal-down",
  created: "bg-muted-foreground/50",
  exited: "bg-muted-foreground/50",
  removing: "bg-muted-foreground/50",
  unknown: "bg-muted-foreground/50",
};

function StateBadge({ state }: { state: ContainerState }) {
  return (
    <Badge variant="outline" className="capitalize">
      <span aria-hidden className={cn("size-1.5 rounded-full", STATE_DOTS[state])} />
      {state}
    </Badge>
  );
}

const ACTION_BUTTONS: Record<ContainerAction, { label: string; icon: LucideIcon }> = {
  start: { label: "Start", icon: Play },
  stop: { label: "Stop", icon: Square },
  restart: { label: "Restart", icon: RotateCw },
};

function RowActions({ container }: { container: ContainerSummary }) {
  const { mutate, isPending, variables } = useContainerAction(container.id);
  return (
    <div className="flex justify-end gap-1">
      {availableActions(container.state).map((action) => {
        const { label, icon: Icon } = ACTION_BUTTONS[action];
        const spinning = isPending && variables === action;
        return (
          <Button
            key={action}
            variant="ghost"
            size="icon-sm"
            aria-label={`${label} ${container.name}`}
            title={label}
            disabled={isPending}
            onClick={() => mutate(action)}
          >
            {spinning ? <LoaderCircle className="motion-safe:animate-spin" /> : <Icon />}
          </Button>
        );
      })}
    </div>
  );
}

function Columns() {
  return (
    <TableHeader>
      <TableRow>
        <TableHead>Name</TableHead>
        <TableHead>Image</TableHead>
        <TableHead>State</TableHead>
        <TableHead>Status</TableHead>
        <TableHead>Ports</TableHead>
        <TableHead className="text-right">Actions</TableHead>
      </TableRow>
    </TableHeader>
  );
}

/** `containers` undefined renders skeleton rows for the first fetch. */
export function ContainerTable({ containers }: { containers?: ContainerSummary[] }) {
  return (
    <Table aria-busy={!containers}>
      <Columns />
      <TableBody>
        {containers
          ? containers.map((c) => (
              <TableRow key={c.id}>
                <TableCell>
                  <div className="font-medium">{c.name}</div>
                  <div className="font-mono text-xs text-muted-foreground">{c.id.slice(0, 12)}</div>
                </TableCell>
                <TableCell className="max-w-64 truncate font-mono text-xs" title={c.image}>
                  {c.image}
                </TableCell>
                <TableCell>
                  <StateBadge state={c.state} />
                </TableCell>
                <TableCell className="text-muted-foreground">{c.status}</TableCell>
                <TableCell className="font-mono text-xs">
                  {c.ports.map((port) => (
                    <div key={formatPort(port)}>{formatPort(port)}</div>
                  ))}
                </TableCell>
                <TableCell>
                  <RowActions container={c} />
                </TableCell>
              </TableRow>
            ))
          : [0, 1, 2, 3].map((row) => (
              <TableRow key={row}>
                {[32, 40, 20, 28, 24, 16].map((width) => (
                  <TableCell key={width}>
                    <Skeleton className="h-4" style={{ width: `${width * 4}px` }} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
      </TableBody>
    </Table>
  );
}
