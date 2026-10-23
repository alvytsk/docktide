import type { ContainerState } from "@/bindings/ContainerState";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

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

export function StateBadge({ state }: { state: ContainerState }) {
  return (
    <Badge variant="outline" className="capitalize">
      <span aria-hidden className={cn("size-1.5 rounded-full", STATE_DOTS[state])} />
      {state}
    </Badge>
  );
}
