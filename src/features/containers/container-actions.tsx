import { LoaderCircle, Play, RotateCw, Square, type LucideIcon } from "lucide-react";
import type { ContainerState } from "@/bindings/ContainerState";
import { Button } from "@/components/ui/button";
import { availableActions, type ContainerAction } from "./list";
import { useContainerAction } from "./use-container-action";

const ACTION_BUTTONS: Record<ContainerAction, { label: string; icon: LucideIcon }> = {
  start: { label: "Start", icon: Play },
  stop: { label: "Stop", icon: Square },
  restart: { label: "Restart", icon: RotateCw },
};

type Props = {
  container: { id: string; name: string; state: ContainerState };
  /** Icon buttons in table rows, labelled buttons in the details header. */
  variant: "row" | "header";
};

/** Start, stop, and restart (spec M0 §5.3); a pending action disables the others. */
export function ContainerActions({ container, variant }: Props) {
  const { mutate, isPending, variables } = useContainerAction(container.id);
  return (
    <div className="flex justify-end gap-1">
      {availableActions(container.state).map((action) => {
        const { label, icon: Icon } = ACTION_BUTTONS[action];
        const icon =
          isPending && variables === action ? (
            <LoaderCircle className="motion-safe:animate-spin" />
          ) : (
            <Icon />
          );
        return variant === "row" ? (
          <Button
            key={action}
            variant="ghost"
            size="icon-sm"
            aria-label={`${label} ${container.name}`}
            title={label}
            disabled={isPending}
            onClick={() => mutate(action)}
          >
            {icon}
          </Button>
        ) : (
          <Button
            key={action}
            variant="outline"
            size="sm"
            disabled={isPending}
            onClick={() => mutate(action)}
          >
            {icon}
            {label}
          </Button>
        );
      })}
    </div>
  );
}
