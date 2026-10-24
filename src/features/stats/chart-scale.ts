import type { StatsPoint } from "./stats-store";

const EPS = 1e-9;

/** Decimal search: 3 to 5 intervals of a 1, 2, 2.5, 5 × 10^n step; headroom over the data only when it exceeds `minMax`. */
function niceDecimal(dataMax: number, minMax: number): { max: number; step: number } {
  const e = Math.floor(Math.log10(Math.max(dataMax, minMax)));
  let best = { max: Infinity, step: 0 };
  for (let n = e - 2; n <= e + 1; n++) {
    for (const m of [1, 2, 2.5, 5]) {
      const step = m * 10 ** n;
      const count =
        dataMax > minMax ? Math.floor(dataMax / step + EPS) + 1 : Math.ceil(minMax / step - EPS);
      const max = count * step;
      if (count < 3 || count > 5) continue;
      if (max < best.max - EPS || (Math.abs(max - best.max) <= EPS && step > best.step)) {
        best = { max, step };
      }
    }
  }
  return best;
}

/** Gridline step and y maximum for 0 to max; bytes search in the largest 1024^n unit the data reaches. */
export function niceScale(
  dataMax: number,
  minMax: number,
  kind: "decimal" | "bytes",
): { max: number; step: number } {
  const data = Number.isFinite(dataMax) && dataMax > 0 ? dataMax : 0;
  const min = Number.isFinite(minMax) && minMax > 0 ? minMax : 0;
  const target = Math.max(data, min);
  const bytes = kind === "bytes";
  if (target <= 0) return bytes ? { max: 1024, step: 256 } : { max: 1, step: 0.25 };
  const unit = bytes ? 1024 ** Math.max(0, Math.floor(Math.log(target) / Math.log(1024))) : 1;
  const { max, step } = niceDecimal(data / unit, min / unit);
  return { max: max * unit, step: step * unit };
}

/** The y maximum grows at once but shrinks only when the data falls below a quarter of it. */
export function growMax(previous: number, next: number): number {
  return next > previous || next < previous / 4 ? next : previous;
}

/** Local-time minute boundaries in [start, end], 2, 5 or 10 minutes apart so labels stay 80 px apart. */
export function timeTicks(start: number, end: number, width: number): number[] {
  const minutes = (end - start) / 60_000;
  const step = [2, 5, 10].find((s) => (width * s) / minutes >= 80) ?? 10;
  const ticks: number[] = [];
  const d = new Date(start);
  d.setSeconds(0, 0);
  for (; d.getTime() <= end; d.setMinutes(d.getMinutes() + 1)) {
    if (d.getTime() >= start && d.getMinutes() % step === 0) ticks.push(d.getTime());
  }
  return ticks;
}

/** Index of the time closest to `t`, or -1 when there are none. */
export function nearestIndex(times: readonly number[], t: number): number {
  if (times.length === 0) return -1;
  let lo = 0;
  let hi = times.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid]! < t) lo = mid + 1;
    else hi = mid;
  }
  return lo > 0 && t - times[lo - 1]! <= times[lo]! - t ? lo - 1 : lo;
}

/** Shared sample times plus one value column per picker; a null column entry halfway across each break. */
export function chartColumns(
  points: readonly StatsPoint[],
  pick: ((p: StatsPoint) => number | null)[],
): { times: number[]; values: (number | null)[][] } {
  const times: number[] = [];
  const values: (number | null)[][] = pick.map(() => []);
  points.forEach((p, i) => {
    if (i > 0 && p.breakBefore) {
      times.push((points[i - 1]!.t + p.t) / 2);
      for (const col of values) col.push(null);
    }
    times.push(p.t);
    pick.forEach((f, c) => values[c]!.push(f(p)));
  });
  return { times, values };
}

/** Largest finite value at a time inside [start, end] across all columns; 0 when there is none. */
export function visibleMax(
  times: readonly number[],
  columns: readonly (readonly (number | null)[])[],
  start: number,
  end: number,
): number {
  let max = 0;
  times.forEach((t, i) => {
    if (t < start || t > end) return;
    for (const column of columns) {
      const v = column[i] ?? 0;
      if (v > max && Number.isFinite(v)) max = v;
    }
  });
  return max;
}
