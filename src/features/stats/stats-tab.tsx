import type { ReactNode } from "react";
import { formatBytes } from "@/features/container-details/format";
import { useLoadedContainer } from "@/features/container-details/use-container";
import { chartColumns } from "./chart-scale";
import { formatMemory, formatPercent, formatRate, memoryPercent } from "./format";
import { HISTORY_MS } from "./stats-store";
import { TimeSeriesChart } from "./time-series-chart";
import { useStatsSeries } from "./use-stats";

function Value({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-sm tabular-nums">{children}</dd>
    </div>
  );
}

const Note = ({ children }: { children: ReactNode }) => (
  <p className="text-sm text-muted-foreground">{children}</p>
);

/** Current values and live charts for the container (spec M2 §5.2). */
export function StatsTab() {
  const { details } = useLoadedContainer();
  const { points, current, now } = useStatsSeries(details.id);
  const running = details.state === "running" || details.state === "paused";
  const limit = details.memoryLimit;

  if (!running && points.length === 0) {
    return <Note>Container is not running. Stats appear when it starts.</Note>;
  }

  const { times, values } = chartColumns(points, [
    (p) => p.cpuPercent,
    (p) => p.memoryUsage,
    (p) => p.networkRx,
    (p) => p.networkTx,
    (p) => p.blockRead,
    (p) => p.blockWrite,
  ]);
  const [cpu = [], memory = [], rx = [], tx = [], read = [], write = []] = values;
  const window = { times, start: now - HISTORY_MS, end: now };

  let note: string | null = null;
  if (!running) note = "Container is not running.";
  else if (points.length === 0) note = "Waiting for the first sample…";
  else if (!current) note = "Waiting for stats…";

  return (
    <div className="flex flex-col gap-6">
      <dl className="flex flex-wrap gap-x-8 gap-y-3">
        <Value label="CPU">{formatPercent(current?.cpuPercent ?? null)}</Value>
        <Value label="Memory">
          {formatMemory(current?.memoryUsage ?? null)} /{" "}
          {limit === null ? "Unlimited" : formatBytes(limit)} (
          {formatPercent(memoryPercent(current?.memoryUsage ?? null, limit))})
        </Value>
        <Value label="Network">
          {formatRate(current?.networkRx ?? null)} in / {formatRate(current?.networkTx ?? null)} out
        </Value>
        <Value label="Block I/O">
          {formatRate(current?.blockRead ?? null)} read / {formatRate(current?.blockWrite ?? null)}{" "}
          write
        </Value>
        <Value label="PIDs">{current?.pids ?? "—"}</Value>
      </dl>
      {note && <Note>{note}</Note>}
      {/* Keyed by container so a chart's remembered y maximum does not carry over. */}
      <div key={details.id} className="grid gap-6 xl:grid-cols-2">
        <TimeSeriesChart
          title="CPU"
          {...window}
          kind="decimal"
          minMax={100}
          format={formatPercent}
          series={[{ label: "CPU", color: "--chart-1", values: cpu }]}
        />
        <TimeSeriesChart
          title="Memory"
          {...window}
          kind="bytes"
          minMax={0}
          format={formatMemory}
          series={[
            { label: "Usage", color: "--chart-1", values: memory },
            ...(limit === null
              ? []
              : [
                  {
                    label: "Limit",
                    color: "--chart-3",
                    values: times.map(() => limit),
                    dashed: true,
                  },
                ]),
          ]}
        />
        <TimeSeriesChart
          title="Network"
          {...window}
          kind="bytes"
          minMax={0}
          format={formatRate}
          series={[
            { label: "Received", color: "--chart-1", values: rx },
            { label: "Sent", color: "--chart-2", values: tx },
          ]}
        />
        <TimeSeriesChart
          title="Block I/O"
          {...window}
          kind="bytes"
          minMax={0}
          format={formatRate}
          series={[
            { label: "Read", color: "--chart-1", values: read },
            { label: "Write", color: "--chart-2", values: write },
          ]}
        />
      </div>
    </div>
  );
}
