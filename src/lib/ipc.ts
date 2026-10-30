// The only module that calls invoke(). Rejections are normalized to IpcError.
import { Channel, invoke, type InvokeArgs } from "@tauri-apps/api/core";
import type { BuildCachePruneResult } from "@/bindings/BuildCachePruneResult";
import type { BuildCacheUsage } from "@/bindings/BuildCacheUsage";
import type { ConnectionStatus } from "@/bindings/ConnectionStatus";
import type { ContainerInspect } from "@/bindings/ContainerInspect";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import type { ImageInspect } from "@/bindings/ImageInspect";
import type { ImageSummary } from "@/bindings/ImageSummary";
import type { IpcError } from "@/bindings/IpcError";
import type { LiveUpdate } from "@/bindings/LiveUpdate";
import type { LogEvent } from "@/bindings/LogEvent";
import type { StatsSample } from "@/bindings/StatsSample";
import type { ContainerAction } from "@/features/containers/list";
import { toIpcError } from "./errors";

declare module "@tanstack/react-query" {
  interface Register {
    defaultError: IpcError;
  }
}

async function call<T>(command: string, args?: InvokeArgs): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toIpcError(error);
  }
}

export const getConnectionStatus = () => call<ConnectionStatus>("get_connection_status");
/** Replaces any previous subscription; Rust ends it when the page reloads. */
export const subscribeLiveUpdates = (onUpdate: (update: LiveUpdate) => void) =>
  call<null>("subscribe_live_updates", { onUpdate: new Channel<LiveUpdate>(onUpdate) });
/** Replaces any previous subscription; one batch per second while containers run. */
export const subscribeStats = (onStats: (samples: StatsSample[]) => void) =>
  call<null>("subscribe_stats", { onStats: new Channel<StatsSample[]>(onStats) });
export const listContainers = () => call<ContainerSummary[]>("list_containers");
export const getContainer = (id: string) => call<ContainerInspect>("get_container", { id });
export const runContainerAction = (action: ContainerAction, id: string) =>
  call<null>(`${action}_container`, { id });
export const listImages = () => call<ImageSummary[]>("list_images");
export const getImage = (id: string) => call<ImageInspect>("get_image", { id });
export const removeImage = (id: string) => call<null>("remove_image", { id });
export const getBuildCacheUsage = () => call<BuildCacheUsage>("get_build_cache_usage");
export const pruneBuildCache = () => call<BuildCachePruneResult>("prune_build_cache");
/**
 * Follows a container's logs. `tail` is a line count or `null` for all; `after`
 * resumes after the last shown line's timestamp. Resolves to the id for `closeLogs`.
 */
export const openLogs = (
  id: string,
  tail: number | null,
  after: string | null,
  onEvent: (event: LogEvent) => void,
) => call<number>("open_logs", { id, tail, after, onEvent: new Channel<LogEvent>(onEvent) });
/** Unknown ids are ignored. */
export const closeLogs = (subscription: number) => call<null>("close_logs", { subscription });
