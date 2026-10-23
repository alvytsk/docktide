import {
  Activity,
  Box,
  HardDrive,
  Layers,
  Network,
  SquareStack,
  type LucideIcon,
} from "lucide-react";
import { Link } from "@tanstack/react-router";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ConnectionIndicator } from "@/features/connection/connection-indicator";

const UPCOMING: [string, LucideIcon][] = [
  ["Images", Layers],
  ["Compose", SquareStack],
  ["Volumes", HardDrive],
  ["Networks", Network],
  ["Monitoring", Activity],
];

const itemClass = "flex w-full items-center gap-3 rounded-md px-3 py-1.5 text-sm outline-none";

export function Sidebar() {
  return (
    <aside className="flex w-56 shrink-0 flex-col bg-sidebar text-sidebar-foreground">
      <div className="px-6 pt-6 pb-5 text-[1.05rem] font-semibold tracking-tight">Docktide</div>
      <nav aria-label="Main" className="flex flex-1 flex-col gap-0.5 px-3">
        {/* Active, with aria-current="page", on every /containers route. */}
        <Link
          to="/containers"
          className={`${itemClass} focus-visible:ring-2 focus-visible:ring-sidebar-ring`}
          activeProps={{
            className: "bg-sidebar-primary font-medium text-sidebar-primary-foreground",
          }}
        >
          <Box className="size-4" aria-hidden />
          Containers
        </Link>
        {UPCOMING.map(([label, Icon]) => (
          <Tooltip key={label}>
            <TooltipTrigger
              aria-disabled="true"
              className={`${itemClass} cursor-default text-sidebar-foreground/45 focus-visible:ring-2 focus-visible:ring-sidebar-ring`}
            >
              <Icon className="size-4" aria-hidden />
              {label}
            </TooltipTrigger>
            <TooltipContent side="right">Coming soon</TooltipContent>
          </Tooltip>
        ))}
      </nav>
      <div className="border-t border-sidebar-border p-3">
        <ConnectionIndicator />
      </div>
    </aside>
  );
}
