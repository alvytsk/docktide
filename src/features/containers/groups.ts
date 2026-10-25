import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { matchesFilter, type ContainerFilter } from "./list";

export type ProjectGroup = {
  project: string;
  /** The whole project, ordered by `compareMembers`. */
  containers: ContainerSummary[];
  /** The members matching the filter, in the same order. */
  shown: ContainerSummary[];
  services: number;
  running: number;
};

export type GroupedList = {
  groups: ProjectGroup[];
  /** Filtered, in input order. */
  standalone: ContainerSummary[];
  /** Containers shown, in groups and standalone together. */
  shownCount: number;
};

const byText = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Service, then replica number (none last), one-off last, ties by name. */
export function compareMembers(a: ContainerSummary, b: ContainerSummary): number {
  const x = a.compose;
  const y = b.compose;
  if (!x || !y) return byText(a.name, b.name);
  return (
    Number(x.oneOff) - Number(y.oneOff) ||
    byText(x.service, y.service) ||
    (x.number ?? Infinity) - (y.number ?? Infinity) ||
    byText(a.name, b.name)
  );
}

export function groupContainers(all: ContainerSummary[], filter: ContainerFilter): GroupedList {
  const projects = new Map<string, ContainerSummary[]>();
  const standalone: ContainerSummary[] = [];
  for (const c of all) {
    if (!c.compose) {
      if (matchesFilter(c, filter)) standalone.push(c);
    } else {
      const members = projects.get(c.compose.project) ?? [];
      members.push(c);
      projects.set(c.compose.project, members);
    }
  }
  const groups: ProjectGroup[] = [];
  for (const [project, members] of projects) {
    // Sorting in place is fine: `members` is local, and the lib target lacks toSorted.
    // oxlint-disable-next-line unicorn/no-array-sort
    const containers = members.sort(compareMembers);
    const shown = containers.filter((c) => matchesFilter(c, filter));
    if (shown.length === 0) continue;
    groups.push({
      project,
      containers,
      shown,
      services: new Set(containers.map((c) => c.compose?.service)).size,
      running: containers.filter((c) => c.state === "running").length,
    });
  }
  groups.sort((a, b) => byText(a.project.toLowerCase(), b.project.toLowerCase()));
  const shownCount = standalone.length + groups.reduce((n, g) => n + g.shown.length, 0);
  return { groups, standalone, shownCount };
}
