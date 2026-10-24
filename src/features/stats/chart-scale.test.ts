import { expect, it } from "vite-plus/test";
import { chartColumns, growMax, nearestIndex, niceScale, timeTicks } from "./chart-scale";
import type { StatsPoint } from "./stats-store";

it("picks a nice scale", () => {
  expect(niceScale(87, 100, "decimal")).toEqual({ max: 100, step: 25 });
  expect(niceScale(230, 100, "decimal")).toEqual({ max: 250, step: 50 });
  expect(niceScale(3 * 1024 ** 2, 0, "bytes")).toEqual({ max: 4 * 1024 ** 2, step: 1024 ** 2 });
  expect(niceScale(0, 0, "decimal")).toEqual({ max: 1, step: 0.25 });
  expect(niceScale(0, 0, "bytes")).toEqual({ max: 1024, step: 256 });
  expect(niceScale(100, 100, "decimal")).toEqual({ max: 100, step: 25 });
  expect(niceScale(100 * 1024 ** 2, 0, "bytes")).toEqual({
    max: 125 * 1024 ** 2,
    step: 25 * 1024 ** 2,
  });
});

it("ignores non-finite data", () => {
  expect(niceScale(Infinity, 0, "decimal")).toEqual({ max: 1, step: 0.25 });
  expect(niceScale(NaN, 100, "decimal")).toEqual({ max: 100, step: 25 });
});

it.each([
  ["bytes", 1024, 8 * 1024 ** 3],
  ["decimal", 0.01, 10_000],
] as const)("always finds a scale across the %s range", (kind, from, to) => {
  for (let v = from; v <= to; v *= 1.3) {
    const { max, step } = niceScale(v, 0, kind);
    expect(Number.isFinite(max)).toBe(true);
    expect(step).toBeGreaterThan(0);
    expect(max / step).toBeGreaterThanOrEqual(3 - 1e-9);
    expect(max / step).toBeLessThanOrEqual(5 + 1e-9);
    expect(max).toBeGreaterThanOrEqual(v);
  }
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

const cpu = (x: StatsPoint) => x.cpuPercent;
const point = (t: number, cpuPercent: number, breakBefore = false): StatsPoint => ({
  id: "c",
  time: "",
  streamStart: false,
  cpuPercent,
  memoryUsage: null,
  memoryLimit: null,
  networkRx: null,
  networkTx: null,
  blockRead: null,
  blockWrite: null,
  pids: null,
  t,
  breakBefore,
});

it("chart columns put a null point at each break", () => {
  const { times, values } = chartColumns(
    [point(0, 1), point(1000, 2), point(5000, 3, true)],
    [cpu],
  );
  expect(times).toEqual([0, 1000, 3000, 5000]);
  expect(values).toEqual([[1, 2, null, 3]]);
  expect(chartColumns([point(0, 1, true)], [cpu]).times).toEqual([0]);
});
