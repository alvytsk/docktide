import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { subscribeLiveUpdates } from "@/lib/ipc";
import { toastError } from "@/lib/toast";
import { applyLiveUpdate } from "./apply";

/** Keeps the query cache in step with Docker events for the app session. */
export function useLiveUpdates() {
  const queryClient = useQueryClient();
  useEffect(() => {
    // A new subscription replaces the old one in Rust; this only drops hints
    // that were already in flight (StrictMode mounts effects twice in dev).
    let active = true;
    subscribeLiveUpdates((update) => {
      if (active) void applyLiveUpdate(queryClient, update);
    }).catch(toastError);
    return () => {
      active = false;
    };
  }, [queryClient]);
}
