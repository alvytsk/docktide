import { expect, it } from "vite-plus/test";
import { chartColumns, growMax, nearestIndex, niceScale, timeTicks } from "./chart-scale";
import type { StatsPoint } from "./stats-store";

it("picks a nice scale", () => {
  expect(niceScale(87, 100, "decimal")).toEqual({ max: 100, step: 25 });
  expect(niceScale(230, 100, "decimal")).toEqual({ max: 250, step: 50 });
  expect(niceScale(3 * 1024 ** 2, 0, "bytes")).toEqual({ max: 4 * 1024 ** 2, step: 1024 ** 2 });
  expect(niceScale(0, 0, "decimal")).toEqual({ max: 1, step: 0.25 });
  expect(niceScale(0, 0, "bytes")).toEqual({ max: 1024, step: 256 });
});

it("grows the maximum only while the data needs it", () => {
  expect(growMax(100, 80)).toBe(100);
  expect(growMax(100, 120)).toBe(120);
  expect(growMax(400, 80)).toBe(80);
});

const start = new Date(2026, 9, 24, 12, 0, 30).getTime();
const end = start + 600_000;
const at = (h: number, m: number) => new Date(2026, 9, 24, h, m).getTime();

it("time ticks every 2 minutes on a wide chart", () => {
  expect(timeTicks(start, end, 900)).toEqual([2, 4, 6, 8, 10].map((m) => at(12, m)));
});

it("time ticks widen when narrow", () => {
  expect(timeTicks(start, end, 360)).toEqual([at(12, 5), at(12, 10)]);
  expect(timeTicks(start, end, 150)).toEqual([at(12, 10)]);
});

it("finds the nearest sample time", () => {
  expect(nearestIndex([0, 1000, 2000], 1400)).toBe(1);
  expect(nearestIndex([0, 1000, 2000], 1600)).toBe(2);
  expect(nearestIndex([0, 1000, 2000], -5)).toBe(0);
  expect(nearestIndex([0, 1000, 2000], 9000)).toBe(2);
  expect(nearestIndex([], 5)).toBe(-1);
});

it("chart columns put a null point at each break", () => {
  const p = (t: number, cpuPercent: number, breakBefore = false) =>
    ({ t, cpuPercent, breakBefore }) as StatsPoint;
  const { times, values } = chartColumns(
    [p(0, 1), p(1000, 2), p(5000, 3, true)],
    [(x) => x.cpuPercent],
  );
  expect(times).toEqual([0, 1000, 3000, 5000]);
  expect(values).toEqual([[1, 2, null, 3]]);
  expect(chartColumns([p(0, 1, true)], [(x) => x.cpuPercent]).times).toEqual([0]);
});
