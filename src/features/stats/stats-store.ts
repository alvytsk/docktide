import type { StatsSample } from "@/bindings/StatsSample";
import { parseDockerTime } from "@/features/container-details/format";

export const HISTORY_MS = 600_000;
export const BREAK_MS = 2_500;
export const CURRENT_MS = 3_000;

export type StatsPoint = StatsSample & { t: number; breakBefore: boolean };

/**
 * Stats history per container, outside React (like the logs session). Components
 * subscribe and re-render on `version`; `series` arrays are replaced, never mutated,
 * so they are safe as memo keys.
 */
export class StatsStore {
  now = Date.now();
  version = 0;
  private data = new Map<string, readonly StatsPoint[]>();
  private listeners = new Set<() => void>();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(private timers = { setInterval, clearInterval }) {}

  push(samples: StatsSample[], now = Date.now()) {
    if (samples.length === 0) return;
    for (const s of samples) {
      const points = this.data.get(s.id) ?? [];
      const last = points.at(-1);
      const t = parseDockerTime(s.time).getTime();
      if (last && t <= last.t) continue;
      const breakBefore = s.streamStart || !last || t - last.t > BREAK_MS;
      this.data.set(s.id, [...points, { ...s, t, breakBefore }]);
    }
    this.update(now);
  }

  tick(now = Date.now()) {
    this.update(now);
  }

  series(id: string): readonly StatsPoint[] {
    return this.data.get(id) ?? [];
  }

  current(id: string): StatsPoint | null {
    const last = this.data.get(id)?.at(-1);
    return last && this.now - last.t <= CURRENT_MS ? last : null;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private update(now: number) {
    this.now = now;
    const cutoff = now - HISTORY_MS;
    for (const [id, points] of this.data) {
      if (points[0] && points[0].t >= cutoff) continue;
      const kept = points.filter((p) => p.t >= cutoff);
      if (kept.length === 0) this.data.delete(id);
      else this.data.set(id, kept);
    }
    if (this.data.size > 0 && this.timer === null) {
      this.timer = this.timers.setInterval(() => this.tick(), 1000);
    } else if (this.data.size === 0 && this.timer !== null) {
      this.timers.clearInterval(this.timer);
      this.timer = null;
    }
    this.version += 1;
    for (const listener of this.listeners) listener();
  }
}

export const statsStore = new StatsStore();
