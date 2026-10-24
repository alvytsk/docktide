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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ContainerActions } from "./container-actions";
import { formatPort } from "./list";
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
            <TooltipTrigger render={<span />}>{memory}</TooltipTrigger>
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
        <TableHead className="w-full">Image</TableHead>
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
                  <Link
                    to="/containers/$id"
                    params={{ id: c.id }}
                    className="rounded-sm font-medium underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {c.name}
                  </Link>
                  <div className="font-mono text-xs text-muted-foreground">{c.id.slice(0, 12)}</div>
                </TableCell>
                <TableCell className="w-full max-w-0 truncate font-mono text-xs" title={c.image}>
                  {c.image}
                </TableCell>
                <TableCell>
                  <StateBadge state={c.state} />
                </TableCell>
                <TableCell className="whitespace-normal text-muted-foreground">
                  {c.status}
                </TableCell>
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
            ))
          : [0, 1, 2, 3].map((row) => (
              <TableRow key={row}>
                {[32, 40, 20, 28, 12, 12, 24, 16].map((width, i) => (
                  <TableCell key={i}>
                    <Skeleton className="h-4" style={{ width: `${width * 4}px` }} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
      </TableBody>
    </Table>
  );
}
