import { useEffect, useSyncExternalStore } from "react";
import { subscribeStats } from "@/lib/ipc";
import { toastError } from "@/lib/toast";
import { statsStore } from "./stats-store";

/** Feeds Docker stats into `statsStore` for the app session. */
export function useStatsSubscription() {
  useEffect(() => {
    // No active gate: under StrictMode either invoke may win in Rust, and push ignores stale samples.
    subscribeStats((samples) => statsStore.push(samples)).catch(toastError);
  }, []);
}

/** Re-renders once per batch or tick; reads happen after the version, so they are consistent. */
export function useStatsSeries(id: string) {
  useSyncExternalStore(statsStore.subscribe, () => statsStore.version);
  return { points: statsStore.series(id), current: statsStore.current(id), now: statsStore.now };
}

/** Sums over several containers; re-renders and reads like `useStatsSeries`. */
export function useStatsTotals(ids: string[]) {
  useSyncExternalStore(statsStore.subscribe, () => statsStore.version);
  return statsStore.totals(ids);
}
