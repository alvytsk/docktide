import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { Usage } from "./list";

export function UsageBadge({ usage }: { usage: Usage }) {
  const [label, dot] =
    usage.kind === "inUse"
      ? ["In use", "bg-signal-ok"]
      : usage.kind === "stopped"
        ? [`Stopped (${usage.containers.length})`, "bg-muted-foreground/50"]
        : ["Unused", "bg-amber-500"];
  return (
    <Badge variant="outline">
      <span aria-hidden className={cn("size-1.5 rounded-full", dot)} />
      {label}
    </Badge>
  );
}
