import type { BuildCacheUsage } from "@/bindings/BuildCacheUsage";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import type { ImageSummary } from "@/bindings/ImageSummary";
import type { IpcError } from "@/bindings/IpcError";
import { formatBytes } from "@/features/container-details/format";
import { ACTIVE } from "@/features/containers/list";

export type Usage =
  | { kind: "inUse"; containers: ContainerSummary[] }
  | { kind: "stopped"; containers: ContainerSummary[] }
  | { kind: "unused" };
export type UsageFilter = "all" | "unused" | "inUse" | "stopped";
export type SortKey = "name" | "uniqueSize" | "size" | "created";
export type ImageSort = { key: SortKey; desc: boolean };
export type ImageRow = ImageSummary & { usage: Usage; uniqueSize: number | null; name: string };

const NONE = "<none>";

/** Joins images to the containers created from them by image id, never by tag. */
export function imageRows(images: ImageSummary[], containers: ContainerSummary[]): ImageRow[] {
  return images.map((image) => {
    const users = containers.filter((c) => c.imageId === image.id);
    const usage: Usage = users.some((c) => ACTIVE.has(c.state))
      ? { kind: "inUse", containers: users }
      : users.length > 0
        ? { kind: "stopped", containers: users }
        : { kind: "unused" };
    return {
      ...image,
      usage,
      uniqueSize: image.sharedSize === null ? null : image.size - image.sharedSize,
      name: image.repoTags[0] ?? NONE,
    };
  });
}

// Missing values (null size, dangling name) sort last in either direction.
function compare(a: ImageRow, b: ImageRow, { key, desc }: ImageSort) {
  const dir = desc ? -1 : 1;
  switch (key) {
    case "name": {
      const dangling = Number(a.repoTags.length === 0) - Number(b.repoTags.length === 0);
      if (dangling !== 0) return dangling;
      return dir * a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    }
    case "uniqueSize": {
      if (a.uniqueSize === null || b.uniqueSize === null) {
        return Number(a.uniqueSize === null) - Number(b.uniqueSize === null);
      }
      return dir * (a.uniqueSize - b.uniqueSize);
    }
    case "size":
      return dir * (a.size - b.size);
    case "created":
      return dir * (a.createdAt - b.createdAt);
  }
}

/** Text matches any tag or an id prefix (with or without `sha256:`). */
export function filterAndSort(
  rows: ImageRow[],
  text: string,
  usage: UsageFilter,
  sort: ImageSort,
): ImageRow[] {
  const query = text.trim().toLowerCase();
  return rows
    .filter((row) => usage === "all" || row.usage.kind === usage)
    .filter(
      (row) =>
        query === "" ||
        row.repoTags.some((tag) => tag.toLowerCase().includes(query)) ||
        row.id.startsWith(query) ||
        row.id.replace(/^sha256:/, "").startsWith(query),
    )
    .sort((a, b) => compare(a, b, sort)); // filter already copied
}

export function unusedSize(rows: ImageRow[]): number {
  return rows.reduce((sum, r) => (r.usage.kind === "unused" ? sum + (r.uniqueSize ?? 0) : sum), 0);
}

/** `Used by a, b, c and 2 more` */
export function usedByText(containers: ContainerSummary[]): string {
  const names = containers.slice(0, 3).map((c) => c.name);
  const rest = containers.length - names.length;
  return `Used by ${names.join(", ")}${rest > 0 ? ` and ${rest} more` : ""}`;
}

export type RemovalResult = { name: string; error: IpcError | null };

/** `freed` is the caller's sum of unique sizes of the successful removals. */
export function removalSummary(
  results: RemovalResult[],
  freed: number,
): { ok: boolean; title: string } {
  const failed = results.find((r) => r.error);
  if (failed?.error) {
    const done = results.filter((r) => !r.error).length;
    return {
      ok: false,
      title: `Removed ${done} of ${results.length}. ${failed.name}: ${failed.error.message}`,
    };
  }
  const n = results.length;
  return {
    ok: true,
    title: `Removed ${n} ${n === 1 ? "image" : "images"}, about ${formatBytes(freed)} freed`,
  };
}

export function buildCacheText({ size, reclaimable }: BuildCacheUsage): {
  text: string;
  canClear: boolean;
} {
  if (size === null) {
    return { text: "Build cache: size not reported by this Engine", canClear: true };
  }
  if (size === 0) return { text: "Build cache: empty", canClear: false };
  const extra = reclaimable === null ? "" : ` (${formatBytes(reclaimable)} reclaimable)`;
  return { text: `Build cache: ${formatBytes(size)}${extra}`, canClear: true };
}
