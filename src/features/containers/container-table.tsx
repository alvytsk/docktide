import { Fragment } from "react";
import { Link } from "@tanstack/react-router";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { formatBytes } from "@/features/container-details/format";
import { formatMemory, formatPercent, memoryPercent } from "@/features/stats/format";
import { useStatsSeries } from "@/features/stats/use-stats";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useCollapsedProjects } from "@/stores/collapsed-projects";
import { ContainerActions } from "./container-actions";
import type { GroupedList } from "./groups";
import { formatPort } from "./list";
import { ProjectRow } from "./project-row";
import { StateBadge } from "./state-badge";

/** Own component so a stats batch re-renders only these cells, not the whole table. */
function StatsCells({ id }: { id: string }) {
  const { current } = useStatsSeries(id);
  const usage = current?.memoryUsage ?? null;
  const limit = current?.memoryLimit ?? null;
  const percent = memoryPercent(usage, limit);
  const memory = formatMemory(usage);
  return (
    <>
      <TableCell className="text-right tabular-nums">
        {formatPercent(current?.cpuPercent ?? null)}
      </TableCell>
      <TableCell className="text-right tabular-nums">
        {percent === null ? (
          memory
        ) : (
          <Tooltip>
            <TooltipTrigger render={<span tabIndex={0} />}>{memory}</TooltipTrigger>
            <TooltipContent>
              {`${memory} of ${formatBytes(limit ?? 0)} (${formatPercent(percent)})`}
            </TooltipContent>
          </Tooltip>
        )}
      </TableCell>
    </>
  );
}

function Columns() {
  return (
    <TableHeader>
      <TableRow>
        <TableHead>Name</TableHead>
        <TableHead className="w-[25%]">Image</TableHead>
        <TableHead>State</TableHead>
        <TableHead>Status</TableHead>
        <TableHead className="text-right">CPU</TableHead>
        <TableHead className="text-right">Memory</TableHead>
        <TableHead>Ports</TableHead>
        <TableHead className="text-right">Actions</TableHead>
      </TableRow>
    </TableHeader>
  );
}

function ContainerRow({ c, indent }: { c: ContainerSummary; indent?: boolean }) {
  return (
    <TableRow>
      <TableCell className={cn(indent && "pl-8")}>
        <Link
          to="/containers/$id"
          params={{ id: c.id }}
          className="rounded-sm font-medium underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
        >
          {c.name}
        </Link>
        <div className="font-mono text-xs text-muted-foreground">{c.id.slice(0, 12)}</div>
      </TableCell>
      <TableCell className="w-[25%] max-w-0 truncate font-mono text-xs" title={c.image}>
        {c.image}
      </TableCell>
      <TableCell>
        <StateBadge state={c.state} />
      </TableCell>
      <TableCell className="whitespace-normal text-muted-foreground">{c.status}</TableCell>
      <StatsCells id={c.id} />
      <TableCell className="font-mono text-xs">
        {c.ports.map((port) => (
          <div key={formatPort(port)}>{formatPort(port)}</div>
        ))}
      </TableCell>
      <TableCell>
        <ContainerActions container={c} variant="row" />
      </TableCell>
    </TableRow>
  );
}

/** `list` undefined renders skeleton rows for the first fetch. */
export function ContainerTable({ list }: { list?: GroupedList }) {
  const { collapsed, toggle } = useCollapsedProjects();
  return (
    <Table aria-busy={!list}>
      <Columns />
      <TableBody>
        {list ? (
          <>
            {list.groups.map((g) => (
              <Fragment key={g.project}>
                <ProjectRow
                  group={g}
                  collapsed={!!collapsed[g.project]}
                  onToggle={() => toggle(g.project)}
                />
                {!collapsed[g.project] &&
                  g.shown.map((c) => <ContainerRow key={c.id} c={c} indent />)}
              </Fragment>
            ))}
            {list.standalone.map((c) => (
              <ContainerRow key={c.id} c={c} />
            ))}
          </>
        ) : (
          [0, 1, 2, 3].map((row) => (
            <TableRow key={row}>
              {[32, 40, 20, 28, 12, 12, 24, 16].map((width, i) => (
                <TableCell key={i}>
                  <Skeleton className="h-4" style={{ width: `${width * 4}px` }} />
                </TableCell>
              ))}
            </TableRow>
          ))
        )}
      </TableBody>
    </Table>
  );
}
