import { useEffect, useSyncExternalStore } from "react";
import { subscribeStats } from "@/lib/ipc";
import { toastError } from "@/lib/toast";
import { statsStore } from "./stats-store";

/** Feeds Docker stats into `statsStore` for the app session. */
export function useStatsSubscription() {
  useEffect(() => {
    // A new subscription replaces the old one in Rust (StrictMode mounts effects twice in dev).
    let active = true;
    subscribeStats((samples) => {
      if (active) statsStore.push(samples);
    }).catch(toastError);
    return () => {
      active = false;
    };
  }, []);
}

/** Re-renders once per batch or tick; reads happen after the version, so they are consistent. */
export function useStatsSeries(id: string) {
  useSyncExternalStore(statsStore.subscribe, () => statsStore.version);
  return { points: statsStore.series(id), current: statsStore.current(id), now: statsStore.now };
}
