import { ChevronRight, ScrollText } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { TableCell, TableRow } from "@/components/ui/table";
import { formatMemory, formatPercent } from "@/features/stats/format";
import { useStatsTotals } from "@/features/stats/use-stats";
import { cn } from "@/lib/utils";
import type { ProjectGroup } from "./groups";

type Props = { group: ProjectGroup; collapsed: boolean; onToggle: () => void };

/** The header row of a Compose project, with the same 8 cells as the columns. */
export function ProjectRow({ group, collapsed, onToggle }: Props) {
  const { cpu, memory } = useStatsTotals(group.containers.map((c) => c.id));
  const { running, services, project } = group;
  const total = group.containers.length;
  const dot =
    running === total ? "bg-signal-ok" : running === 0 ? "bg-muted-foreground/50" : "bg-amber-500";
  return (
    <TableRow className="cursor-pointer bg-muted/40 font-medium" onClick={onToggle}>
      <TableCell>
        <button
          type="button"
          aria-expanded={!collapsed}
          // The row's own click toggles too; stop this one so it fires once.
          onClick={(event) => {
            event.stopPropagation();
            onToggle();
          }}
          className="flex items-center gap-2 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronRight
            aria-hidden
            className={cn("size-4 transition-transform", !collapsed && "rotate-90")}
          />
          {project}
          <span className="font-normal text-muted-foreground">
            {services} {services === 1 ? "service" : "services"}
          </span>
        </button>
      </TableCell>
      <TableCell />
      <TableCell>
        <Badge variant="outline">
          <span aria-hidden className={cn("size-1.5 rounded-full", dot)} />
          {running}/{total} running
        </Badge>
      </TableCell>
      <TableCell />
      <TableCell className="text-right tabular-nums">{formatPercent(cpu)}</TableCell>
      <TableCell className="text-right tabular-nums">{formatMemory(memory)}</TableCell>
      <TableCell />
      <TableCell>
        {/* Disabled until the project logs route exists (M3 Task 7). */}
        <Button variant="outline" size="sm" disabled onClick={(event) => event.stopPropagation()}>
          <ScrollText />
          Logs
        </Button>
      </TableCell>
    </TableRow>
  );
}
