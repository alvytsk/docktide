import { Link } from "@tanstack/react-router";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ContainerActions } from "./container-actions";
import { formatPort } from "./list";
import { StateBadge } from "./state-badge";

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
                  <Link
                    to="/containers/$id"
                    params={{ id: c.id }}
                    className="rounded-sm font-medium underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {c.name}
                  </Link>
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
                  <ContainerActions container={c} variant="row" />
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
