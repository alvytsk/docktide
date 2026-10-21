import type { ContainerState } from "@/bindings/ContainerState";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import type { PortMapping } from "@/bindings/PortMapping";

export type StateFilter = "all" | "running" | "stopped";
export type ContainerFilter = { text: string; state: StateFilter };

// The "Running" filter. "Stopped" is everything else.
const ACTIVE = new Set<ContainerState>(["running", "paused", "restarting"]);

/** Case-insensitive substring over name and image, or an id prefix. */
export function matchesFilter(container: ContainerSummary, { text, state }: ContainerFilter) {
  if (state !== "all" && ACTIVE.has(container.state) !== (state === "running")) return false;
  const query = text.trim().toLowerCase();
  return (
    query === "" ||
    container.name.toLowerCase().includes(query) ||
    container.image.toLowerCase().includes(query) ||
    container.id.startsWith(query)
  );
}

/** `8080→80/tcp`, `127.0.0.1:8080→80/tcp`, or `80/tcp` when unpublished. */
export function formatPort({ privatePort, publicPort, hostIp, protocol }: PortMapping) {
  const target = `${privatePort}/${protocol}`;
  if (publicPort === null) return target;
  if (hostIp === null) return `${publicPort}→${target}`;
  const host = hostIp.includes(":") ? `[${hostIp}]` : hostIp;
  return `${host}:${publicPort}→${target}`;
}

export type ContainerAction = "start" | "stop" | "restart";

const ACTIONS: Record<ContainerState, ContainerAction[]> = {
  created: ["start"],
  exited: ["start"],
  running: ["stop", "restart"],
  paused: ["stop"],
  restarting: ["stop"],
  removing: [],
  dead: [],
  unknown: [],
};

/** The lifecycle actions offered for a container in `state` (spec M0 §5.3). */
export function availableActions(state: ContainerState): ContainerAction[] {
  return ACTIONS[state];
}
