import type { ContainerSummary } from "@/bindings/ContainerSummary";

/**
 * A line's tag (spec M3 §5.2): the service, `service-N` while the service has
 * more than one replica, or the container name for a one-off container.
 * Replicas are counted by number: `--force-recreate` briefly leaves an old and
 * a new container with the same number.
 */
export function tagFor(c: ContainerSummary, members: ContainerSummary[]): string {
  const compose = c.compose;
  if (!compose || compose.oneOff) return c.name;
  const replicas = new Set(
    members
      .filter((m) => m.compose?.service === compose.service && !m.compose.oneOff)
      .map((m) => m.compose?.number ?? 1),
  ).size;
  return replicas > 1 ? `${compose.service}-${compose.number ?? 1}` : compose.service;
}

/** Green, yellow, blue, magenta, cyan, and their bright variants: red is for stderr. */
const COLORS = [2, 3, 4, 5, 6, 10, 11, 12, 13, 14];

/** A stable colour per service name, from an FNV-1a hash. */
export function serviceColor(service: string): string {
  let hash = 0x81_1c_9d_c5;
  for (let i = 0; i < service.length; i++) {
    hash = Math.imul(hash ^ service.charCodeAt(i), 0x01_00_01_93) >>> 0;
  }
  return `var(--ansi-${COLORS[hash % COLORS.length]})`;
}
