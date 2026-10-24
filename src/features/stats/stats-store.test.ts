import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { StatsSample } from "@/bindings/StatsSample";
import { StatsStore } from "./stats-store";

const T0 = Date.parse("2026-01-01T00:00:00Z");

function sample(id: string, offsetMs: number, extra: Partial<StatsSample> = {}): StatsSample {
  return {
    id,
    time: new Date(T0 + offsetMs).toISOString(),
    streamStart: false,
    cpuPercent: 1,
    memoryUsage: null,
    memoryLimit: null,
    networkRx: null,
    networkTx: null,
    blockRead: null,
    blockWrite: null,
    pids: null,
    ...extra,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(T0);
});
afterEach(() => vi.useRealTimers());

it("keeps samples per container in time order", () => {
  const store = new StatsStore();
  store.push([sample("a", 0), sample("b", 0)], T0);
  store.push([sample("a", 1000)], T0 + 1000);
  expect(store.series("a").map((p) => p.t)).toEqual([T0, T0 + 1000]);
  expect(store.series("b")).toHaveLength(1);
  expect(store.series("c")).toEqual([]);
});

it("drops samples not newer than the last", () => {
  const store = new StatsStore();
  store.push([sample("a", 1000)], T0 + 1000);
  store.push([sample("a", 1000), sample("a", 500)], T0 + 1000);
  expect(store.series("a")).toHaveLength(1);
});

it("marks a break before streamStart even 1 s after the previous sample", () => {
  const store = new StatsStore();
  store.push([sample("a", 0), sample("a", 1000, { streamStart: true })], T0 + 1000);
  expect(store.series("a").map((p) => p.breakBefore)).toEqual([true, true]);
  store.push([sample("a", 2000)], T0 + 2000);
  expect(store.series("a")[2]?.breakBefore).toBe(false);
});

it("marks a break when samples are more than 2.5 s apart", () => {
  const store = new StatsStore();
  store.push([sample("a", 0), sample("a", 2400), sample("a", 5000)], T0 + 5000);
  expect(store.series("a").map((p) => p.breakBefore)).toEqual([true, false, true]);
});

it("trims samples older than 10 minutes on push", () => {
  const store = new StatsStore();
  store.push([sample("a", 0)], T0);
  store.push([sample("a", 600_001)], T0 + 600_001);
  expect(store.series("a").map((p) => p.t)).toEqual([T0 + 600_001]);
});

it("empties the store after 10 minutes without a batch and stops its timer", () => {
  const store = new StatsStore();
  store.push([sample("a", 0)], T0);
  expect(vi.getTimerCount()).toBe(1);
  vi.advanceTimersByTime(601_000);
  expect(store.series("a")).toEqual([]);
  expect(vi.getTimerCount()).toBe(0);
});

it("advances now every second while it holds samples", () => {
  const store = new StatsStore();
  store.push([sample("a", 0)], T0);
  const v = store.version;
  vi.advanceTimersByTime(3000);
  expect(store.version).toBe(v + 3);
  expect(store.now).toBe(T0 + 3000);
});

it("expires the current sample after 3 s", () => {
  const store = new StatsStore();
  store.push([sample("a", 0)], T0);
  store.tick(T0 + 3000);
  expect(store.current("a")).not.toBeNull();
  store.tick(T0 + 3001);
  expect(store.current("a")).toBeNull();
});

it("notifies subscribers once per push or tick", () => {
  const store = new StatsStore();
  const listener = vi.fn();
  store.subscribe(listener);
  store.push([sample("a", 0), sample("b", 0)], T0);
  expect(listener).toHaveBeenCalledTimes(1);
  vi.advanceTimersByTime(1000);
  expect(listener).toHaveBeenCalledTimes(2);
});

// Throws like a browser when a global timer function is called as a method.
const guard = <A extends unknown[], R>(fn: (...args: A) => R) =>
  vi.fn(function (this: unknown, ...args: A) {
    if (this !== undefined && this !== globalThis) throw new TypeError("Illegal invocation");
    return fn(...args);
  });
it("calls the default timers unbound", () => {
  const realSet = globalThis.setInterval;
  const realClear = globalThis.clearInterval;
  const set = guard((f: () => void, ms: number) => realSet(f, ms));
  const clear = guard((id: ReturnType<typeof setInterval>) => realClear(id));
  vi.stubGlobal("setInterval", set);
  vi.stubGlobal("clearInterval", clear);
  try {
    const store = new StatsStore();
    store.push([sample("a", 0)], T0);
    store.tick(T0 + 601_000);
    expect(set).toHaveBeenCalled();
    expect(clear).toHaveBeenCalled();
  } finally {
    vi.unstubAllGlobals();
  }
});
