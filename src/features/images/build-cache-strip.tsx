import { useMutation, useQueryClient } from "@tanstack/react-query";
import { formatBytes } from "@/features/container-details/format";
import { pruneBuildCache } from "@/lib/ipc";
import { toastError, toastSuccess } from "@/lib/toast";
import { ConfirmDialog } from "./confirm-dialog";
import { buildCacheText } from "./list";
import { buildCacheQueryKey, useBuildCache } from "./use-images";

export function BuildCacheStrip({ enabled }: { enabled: boolean }) {
  const { data } = useBuildCache(enabled);
  const queryClient = useQueryClient();
  const prune = useMutation({
    mutationFn: pruneBuildCache,
    onSuccess: ({ spaceReclaimed }) =>
      toastSuccess(`Cleared ${formatBytes(spaceReclaimed)} of build cache`),
    onError: toastError,
    onSettled: () => queryClient.invalidateQueries({ queryKey: buildCacheQueryKey }),
  });
  if (!data) return null;
  const { text, canClear } = buildCacheText(data);
  const reclaimable =
    data.reclaimable === null ? "all unused build cache" : formatBytes(data.reclaimable);
  return (
    <div className="flex items-center gap-3 text-sm">
      <span className="text-muted-foreground">{text}</span>
      {canClear && (
        <ConfirmDialog
          trigger="Clear build cache"
          disabled={prune.isPending}
          title={() => "Clear build cache?"}
          confirm="Clear build cache"
          source={reclaimable}
          onConfirm={() => prune.mutate()}
        >
          {(size) => `This frees about ${size}. Later builds will start from scratch.`}
        </ConfirmDialog>
      )}
    </div>
  );
}
