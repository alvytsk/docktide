import { formatBytes } from "@/features/container-details/format";

const MISSING = "—";

export const formatPercent = (v: number | null) =>
  v === null ? MISSING : `${Number(v.toFixed(1))} %`;
export const formatMemory = (v: number | null) => (v === null ? MISSING : formatBytes(v));
export const formatRate = (v: number | null) => (v === null ? MISSING : `${formatBytes(v)}/s`);

export function memoryPercent(usage: number | null, limit: number | null): number | null {
  return usage === null || limit === null || limit <= 0 ? null : (usage / limit) * 100;
}
