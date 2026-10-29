import { useNavigate } from "@tanstack/react-router";
import { ArrowDown, ArrowUp } from "lucide-react";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatBytes, formatRelative, shortImageId } from "@/features/container-details/format";
import { useImageView } from "@/stores/image-view";
import { usedByText, type ImageRow, type SortKey } from "./list";
import { UsageBadge } from "./usage-badge";

const size = (bytes: number | null) =>
  bytes === null ? (
    "—"
  ) : (
    <span title={`${bytes.toLocaleString()} bytes`}>{formatBytes(bytes)}</span>
  );

function SortHead({ label, k, right }: { label: string; k: SortKey; right?: boolean }) {
  const { sort, toggleSort } = useImageView();
  const active = sort.key === k;
  const Arrow = sort.desc ? ArrowDown : ArrowUp;
  return (
    <TableHead
      className={right ? "text-right" : undefined}
      aria-sort={active ? (sort.desc ? "descending" : "ascending") : "none"}
    >
      <button
        type="button"
        onClick={() => toggleSort(k)}
        className="inline-flex items-center gap-1 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {label}
        {active && <Arrow className="size-3" aria-hidden />}
      </button>
    </TableHead>
  );
}

type Props = {
  rows?: ImageRow[];
  selected?: Set<string>;
  onSelect?: (ids: string[], on: boolean) => void;
  onRemove?: (row: ImageRow) => void;
};

/** `rows` undefined renders skeleton rows for the first fetch. */
export function ImageTable({ rows, selected, onSelect, onRemove }: Props) {
  const navigate = useNavigate();
  const unused = rows?.filter((r) => r.usage.kind === "unused") ?? [];
  const allSelected = unused.length > 0 && unused.every((r) => selected?.has(r.id));
  return (
    <Table aria-busy={!rows}>
      <TableHeader>
        <TableRow>
          <TableHead className="w-8">
            <input
              type="checkbox"
              aria-label="Select all unused"
              disabled={unused.length === 0}
              checked={allSelected}
              onChange={(e) =>
                onSelect?.(
                  unused.map((r) => r.id),
                  e.target.checked,
                )
              }
            />
          </TableHead>
          <SortHead label="Name" k="name" />
          <TableHead>ID</TableHead>
          <SortHead label="Unique size" k="uniqueSize" right />
          <SortHead label="Size" k="size" right />
          <SortHead label="Created" k="created" />
          <TableHead>Usage</TableHead>
          <TableHead className="text-right">Actions</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows
          ? rows.map((row) => {
              const created = new Date(row.createdAt * 1000);
              return (
                <TableRow
                  key={row.id}
                  className="cursor-pointer"
                  onClick={() => void navigate({ to: "/images/$id", params: { id: row.id } })}
                >
                  <TableCell onClick={(e) => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      aria-label={`Select ${row.name}`}
                      disabled={row.usage.kind !== "unused"}
                      checked={selected?.has(row.id) ?? false}
                      onChange={(e) => onSelect?.([row.id], e.target.checked)}
                    />
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {row.repoTags.length === 0 ? (
                      <span className="text-muted-foreground">&lt;none&gt;</span>
                    ) : (
                      row.repoTags.map((tag) => <div key={tag}>{tag}</div>)
                    )}
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {shortImageId(row.id)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{size(row.uniqueSize)}</TableCell>
                  <TableCell className="text-right tabular-nums">{size(row.size)}</TableCell>
                  <TableCell className="text-muted-foreground" title={created.toLocaleString()}>
                    {formatRelative(created)}
                  </TableCell>
                  <TableCell>
                    <UsageBadge usage={row.usage} />
                  </TableCell>
                  <TableCell className="text-right" onClick={(e) => e.stopPropagation()}>
                    {row.usage.kind !== "unused" ? (
                      <Tooltip>
                        <TooltipTrigger render={<span tabIndex={0} className="inline-block" />}>
                          <Button variant="ghost" size="sm" disabled tabIndex={-1}>
                            Remove
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>{usedByText(row.usage.containers)}</TooltipContent>
                      </Tooltip>
                    ) : (
                      <Button variant="ghost" size="sm" onClick={() => onRemove?.(row)}>
                        Remove
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              );
            })
          : [0, 1, 2, 3].map((r) => (
              <TableRow key={r}>
                {[4, 40, 24, 16, 16, 20, 20, 16].map((width, i) => (
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
