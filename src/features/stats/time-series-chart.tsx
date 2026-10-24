import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent } from "react";
import { useChartCursor } from "./chart-cursor";
import { growMax, nearestIndex, niceScale, timeTicks, visibleMax } from "./chart-scale";

export type ChartSeries = {
  label: string;
  /** CSS custom property name, e.g. "--chart-1". */
  color: string;
  values: readonly (number | null)[];
  dashed?: boolean;
};

type Props = {
  title: string;
  times: readonly number[];
  series: ChartSeries[];
  start: number;
  end: number;
  kind: "decimal" | "bytes";
  minMax: number;
  format: (v: number | null) => string;
  height?: number;
};

const HHMM = new Intl.DateTimeFormat("en", {
  hour: "2-digit",
  minute: "2-digit",
  hourCycle: "h23",
});

// Plot insets in CSS pixels: half a label above the top grid line, a row of time labels below.
const TOP = 6;
const BOTTOM = 18;
const RIGHT = 4;
const LABEL_GAP = 6;

function latest(values: readonly (number | null)[]): number | null {
  for (let i = values.length - 1; i >= 0; i--) {
    const v = values[i];
    if (v !== null && v !== undefined) return v;
  }
  return null;
}

/** One live time series chart on a canvas; values are read in the DOM legend above it. */
export function TimeSeriesChart({
  title,
  times,
  series,
  start,
  end,
  kind,
  minMax,
  format,
  height = 160,
}: Props) {
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  // The y scale only grows while the data needs it, so it does not jitter between ticks.
  const scale = useRef({ max: 0, step: 0 });
  // Where the last draw put the plot, for mapping the pointer back to a time.
  const plot = useRef({ left: 0, width: 0 });
  const [width, setWidth] = useState(0);
  const time = useChartCursor((s) => s.time);
  const setTime = useChartCursor((s) => s.setTime);
  const index = time !== null && time >= start && time <= end ? nearestIndex(times, time) : -1;

  // Clear a stale crosshair when leaving the route.
  useEffect(() => () => setTime(null), [setTime]);

  useLayoutEffect(() => {
    const observer = new ResizeObserver(([entry]) => setWidth(entry!.contentRect.width));
    observer.observe(box.current!);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    const el = canvas.current;
    const ctx = el?.getContext("2d");
    if (!el || !ctx || width <= 0) return;
    // ponytail: devicePixelRatio is read per draw; a DPR change without a resize waits for the next tick.
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(width * dpr);
    const h = Math.round(height * dpr);
    if (el.width !== w) el.width = w;
    if (el.height !== h) el.height = h;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const style = getComputedStyle(el);
    const token = (name: string) => style.getPropertyValue(name).trim();
    ctx.font = `11px ${style.fontFamily}`;
    const nice = niceScale(
      visibleMax(
        times,
        series.map((s) => s.values),
        start,
        end,
      ),
      minMax,
      kind,
    );
    if (growMax(scale.current.max, nice.max) === nice.max) scale.current = nice;
    const { max, step } = scale.current;
    const labels = Array.from({ length: Math.round(max / step) + 1 }, (_, i) => format(i * step));
    const left = Math.ceil(Math.max(...labels.map((l) => ctx.measureText(l).width))) + LABEL_GAP;
    const bottom = height - BOTTOM;
    const plotWidth = width - left - RIGHT;
    plot.current = { left, width: plotWidth };
    if (plotWidth <= 0 || end <= start) return;

    const x = (t: number) => left + ((t - start) / (end - start)) * plotWidth;
    const y = (v: number) => bottom - (v / max) * (bottom - TOP);
    // Hairlines centred on a device pixel stay one pixel wide at every scale.
    const crisp = (v: number) => (Math.round(v * dpr) + 0.5) / dpr;
    const text = token("--chart-text");

    ctx.lineWidth = 1 / dpr;
    ctx.strokeStyle = token("--chart-grid");
    ctx.fillStyle = text;
    ctx.textAlign = "right";
    ctx.textBaseline = "middle";
    ctx.beginPath();
    labels.forEach((label, i) => {
      const gy = crisp(y(i * step));
      ctx.moveTo(left, gy);
      ctx.lineTo(left + plotWidth, gy);
      ctx.fillText(label, left - LABEL_GAP, gy);
    });
    ctx.stroke();

    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    for (const t of timeTicks(start, end, plotWidth)) {
      const label = HHMM.format(t);
      const half = ctx.measureText(label).width / 2;
      ctx.fillText(label, Math.min(Math.max(x(t), half), width - half), bottom + 5);
    }

    // One subpath per run of non-null values; `area` closes each run down to the baseline.
    const trace = (values: readonly (number | null)[], area: boolean) => {
      ctx.beginPath();
      let first = -1;
      const close = (last: number) => {
        if (area && first >= 0) {
          ctx.lineTo(x(times[last]!), bottom);
          ctx.lineTo(x(times[first]!), bottom);
          ctx.closePath();
        }
        first = -1;
      };
      times.forEach((t, i) => {
        const v = values[i] ?? null;
        if (v === null) close(i - 1);
        else if (first < 0) {
          first = i;
          ctx.moveTo(x(t), y(v));
        } else ctx.lineTo(x(t), y(v));
      });
      close(times.length - 1);
    };

    ctx.save();
    ctx.beginPath();
    ctx.rect(left, 0, plotWidth, height);
    ctx.clip();
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "round";
    series.forEach((s, n) => {
      const color = token(s.color);
      if (n === 0) {
        trace(s.values, true);
        ctx.globalAlpha = 0.12;
        ctx.fillStyle = color;
        ctx.fill();
        ctx.globalAlpha = 1;
      }
      trace(s.values, false);
      ctx.setLineDash(s.dashed ? [4, 4] : []);
      ctx.strokeStyle = color;
      ctx.stroke();
    });
    ctx.restore();

    if (index < 0) return;
    const cx = x(times[index]!);
    ctx.lineWidth = 1 / dpr;
    ctx.strokeStyle = text;
    ctx.beginPath();
    ctx.moveTo(crisp(cx), TOP);
    ctx.lineTo(crisp(cx), bottom);
    ctx.stroke();
    for (const s of series) {
      const v = s.values[index] ?? null;
      if (v === null) continue;
      ctx.fillStyle = token(s.color);
      ctx.beginPath();
      ctx.arc(cx, y(v), 3, 0, 2 * Math.PI);
      ctx.fill();
    }
  }, [times, series, start, end, kind, minMax, format, height, width, index]);

  const onPointerMove = (e: PointerEvent<HTMLCanvasElement>) => {
    const { left, width: plotWidth } = plot.current;
    if (plotWidth <= 0) return;
    const t = start + ((e.nativeEvent.offsetX - left) / plotWidth) * (end - start);
    // Snapping to a sample time leaves the store unchanged, and the charts unrendered, within one sample.
    const i = nearestIndex(times, t);
    setTime(i < 0 ? null : times[i]!);
  };

  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1 text-sm">
        <h2 className="font-semibold">{title}</h2>
        {series.map((s) => (
          <span key={s.label} className="flex items-center gap-1.5 text-muted-foreground">
            <span
              aria-hidden
              className="w-3 border-t-2"
              style={{ borderColor: `var(${s.color})`, borderStyle: s.dashed ? "dashed" : "solid" }}
            />
            {s.label}
            <span className="text-foreground tabular-nums">
              {format(index >= 0 ? (s.values[index] ?? null) : latest(s.values))}
            </span>
          </span>
        ))}
      </div>
      <div ref={box} className="relative" style={{ height }}>
        <canvas
          ref={canvas}
          role="img"
          aria-label={title}
          className="absolute inset-0 size-full"
          onPointerMove={onPointerMove}
          onPointerLeave={() => setTime(null)}
        />
      </div>
    </section>
  );
}
