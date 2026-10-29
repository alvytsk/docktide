import { useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { removeImage } from "@/lib/ipc";
import { toastError, toastSuccess } from "@/lib/toast";
import { toIpcError } from "@/lib/errors";
import { removalSummary, type ImageRow, type RemovalResult } from "./list";
import { imagesQueryKey } from "./use-images";

/** Removes one image at a time; failures do not stop the rest. One toast summarises. */
export function useRemoveImages() {
  const queryClient = useQueryClient();
  const running = useRef(false);
  const [busy, setBusy] = useState(false);
  const remove = async (rows: ImageRow[]) => {
    // The ref blocks a second click before the re-render that disables the buttons.
    if (running.current) return;
    running.current = true;
    setBusy(true);
    try {
      const results: RemovalResult[] = [];
      let freed = 0;
      for (const row of rows) {
        try {
          await removeImage(row.id);
          freed += row.uniqueSize ?? 0;
          results.push({ name: row.name, error: null });
        } catch (error) {
          results.push({ name: row.name, error: toIpcError(error) });
        }
      }
      const { ok, title } = removalSummary(results, freed);
      const failed = results.find((r) => r.error)?.error;
      if (ok || !failed) toastSuccess(title);
      else toastError({ ...failed, message: title });
      await queryClient.invalidateQueries({ queryKey: imagesQueryKey });
    } finally {
      running.current = false;
      setBusy(false);
    }
  };
  return { remove, busy };
}
