import { QueryClient, type QueryKey } from "@tanstack/react-query";
import { describe, expect, test } from "vite-plus/test";
import { connectionQueryKey } from "@/features/connection/use-connection-status";
import { containerQueryKey, containersQueryKey } from "@/features/containers/use-containers";
import { buildCacheQueryKey, imageQueryKey, imagesQueryKey } from "@/features/images/use-images";
import { applyLiveUpdate } from "./apply";

function seeded() {
  const queryClient = new QueryClient();
  for (const key of [
    connectionQueryKey,
    containersQueryKey,
    containerQueryKey("a"),
    containerQueryKey("b"),
    imagesQueryKey,
    imageQueryKey("x"),
    buildCacheQueryKey,
  ]) {
    queryClient.setQueryData(key, {});
  }
  return queryClient;
}

function invalidated(queryClient: QueryClient): QueryKey[] {
  return queryClient
    .getQueryCache()
    .getAll()
    .filter((query) => query.state.isInvalidated)
    .map((query) => query.queryKey);
}

describe("applyLiveUpdate", () => {
  test("connected invalidates everything, since events were missed", async () => {
    const queryClient = seeded();
    await applyLiveUpdate(queryClient, { type: "connected" });
    expect(invalidated(queryClient)).toHaveLength(7);
  });

  test("disconnected invalidates only the connection status", async () => {
    const queryClient = seeded();
    await applyLiveUpdate(queryClient, {
      type: "disconnected",
      error: { kind: "daemonUnavailable", message: "Docker is not running." },
    });
    expect(invalidated(queryClient)).toEqual([connectionQueryKey]);
  });

  test("containersChanged invalidates the list and the named containers", async () => {
    const queryClient = seeded();
    await applyLiveUpdate(queryClient, { type: "containersChanged", ids: ["a"] });
    expect(invalidated(queryClient)).toEqual([containersQueryKey, containerQueryKey("a")]);
  });

  test("imagesChanged invalidates the images list and every image", async () => {
    const queryClient = seeded();
    await applyLiveUpdate(queryClient, { type: "imagesChanged" });
    expect(invalidated(queryClient)).toEqual([imagesQueryKey, imageQueryKey("x")]);
  });
});
