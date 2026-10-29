import { describe, expect, test } from "vite-plus/test";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import type { ImageSummary } from "@/bindings/ImageSummary";
import {
  buildCacheText,
  filterAndSort,
  imageRows,
  removalSummary,
  unusedSize,
  usedByText,
  type ImageSort,
} from "./list";

const image = (patch: Partial<ImageSummary>): ImageSummary => ({
  id: "sha256:i",
  repoTags: ["app:a"],
  createdAt: 0,
  size: 100,
  sharedSize: 0,
  ...patch,
});

const container = (patch: Partial<ContainerSummary>): ContainerSummary => ({
  id: "c1",
  name: "web",
  image: "app:a",
  state: "running",
  status: "Up",
  ports: [],
  imageId: "sha256:i",
  createdAt: 0,
  compose: null,
  ...patch,
});

const bySize: ImageSort = { key: "uniqueSize", desc: true };
const names = (rows: { name: string }[]) => rows.map((r) => r.name);

describe("imageRows", () => {
  test("usage matches by image id, not by the tag the container names", () => {
    const [row] = imageRows([image({ repoTags: ["app:a", "app:b"] })], [container({})]);
    expect(row?.usage.kind).toBe("inUse");
    expect(row?.name).toBe("app:a");
  });

  test("paused and restarting count as in use; only exited containers → stopped with the count", () => {
    const usage = (cs: ContainerSummary[]) => imageRows([image({})], cs)[0]?.usage;
    expect(usage([container({ state: "paused" })])?.kind).toBe("inUse");
    expect(usage([container({ state: "restarting" })])?.kind).toBe("inUse");
    const stopped = usage([
      container({ id: "1", state: "exited" }),
      container({ id: "2", state: "exited" }),
    ]);
    expect(stopped?.kind).toBe("stopped");
    expect(stopped && "containers" in stopped && stopped.containers).toHaveLength(2);
  });

  test("no container → unused", () => {
    expect(imageRows([image({})], [container({ imageId: "sha256:other" })])[0]?.usage).toEqual({
      kind: "unused",
    });
  });

  test("unique size is size minus shared size, null when shared size is null", () => {
    const rows = imageRows([image({ size: 100, sharedSize: 30 }), image({ sharedSize: null })], []);
    expect(rows.map((r) => r.uniqueSize)).toEqual([70, null]);
  });

  test("dangling images are named <none>", () => {
    expect(imageRows([image({ repoTags: [] })], [])[0]?.name).toBe("<none>");
  });
});

describe("filterAndSort", () => {
  const rows = imageRows(
    [
      image({ id: "sha256:aaa", repoTags: ["Zed:1"], size: 50, sharedSize: 0, createdAt: 1 }),
      image({ id: "sha256:bbb", repoTags: [], size: 10, sharedSize: 0, createdAt: 3 }),
      image({
        id: "sha256:ccc",
        repoTags: ["alpha:1", "beta:2"],
        size: 90,
        sharedSize: 0,
        createdAt: 2,
      }),
      image({ id: "sha256:ddd", repoTags: ["nil:1"], size: 80, sharedSize: null, createdAt: 4 }),
    ],
    [container({ imageId: "sha256:aaa", state: "exited" }), container({ imageId: "sha256:ccc" })],
  );

  test("default sort puts the largest unique size first; null unique sizes last", () => {
    expect(names(filterAndSort(rows, "", "all", bySize))).toEqual([
      "alpha:1",
      "Zed:1",
      "<none>",
      "nil:1",
    ]);
    expect(names(filterAndSort(rows, "", "all", { key: "uniqueSize", desc: false }))).toEqual([
      "<none>",
      "Zed:1",
      "alpha:1",
      "nil:1",
    ]);
  });

  test("name sort is case-insensitive with dangling images last", () => {
    for (const desc of [false, true]) {
      const sorted = names(filterAndSort(rows, "", "all", { key: "name", desc }));
      expect(sorted.at(-1)).toBe("<none>");
    }
    expect(names(filterAndSort(rows, "", "all", { key: "name", desc: false }))).toEqual([
      "alpha:1",
      "nil:1",
      "Zed:1",
      "<none>",
    ]);
  });

  test("created sort", () => {
    expect(names(filterAndSort(rows, "", "all", { key: "created", desc: true }))[0]).toBe("nil:1");
  });

  test("text filter matches any tag or an id prefix with or without sha256:", () => {
    expect(names(filterAndSort(rows, "BETA", "all", bySize))).toEqual(["alpha:1"]);
    expect(names(filterAndSort(rows, "bb", "all", bySize))).toEqual(["<none>"]);
    expect(names(filterAndSort(rows, "sha256:bb", "all", bySize))).toEqual(["<none>"]);
    expect(names(filterAndSort(rows, "zzz", "all", bySize))).toEqual([]);
  });

  test("usage filter", () => {
    expect(names(filterAndSort(rows, "", "inUse", bySize))).toEqual(["alpha:1"]);
    expect(names(filterAndSort(rows, "", "stopped", bySize))).toEqual(["Zed:1"]);
    expect(names(filterAndSort(rows, "", "unused", bySize))).toEqual(["<none>", "nil:1"]);
  });

  test("unusedSize sums unique sizes of unused rows only", () => {
    expect(unusedSize(rows)).toBe(10);
  });
});

test("usedByText names up to three containers", () => {
  const cs = (n: number) =>
    Array.from({ length: n }, (_, i) => container({ name: "abcde"[i] ?? "x" }));
  expect(usedByText(cs(1))).toBe("Used by a");
  expect(usedByText(cs(3))).toBe("Used by a, b, c");
  expect(usedByText(cs(5))).toBe("Used by a, b, c and 2 more");
});

test("removalSummary", () => {
  const ok = (name: string) => ({ name, error: null });
  expect(removalSummary([ok("a"), ok("b"), ok("c")], 1048576)).toEqual({
    ok: true,
    title: "Removed 3 images, about 1 MiB freed",
  });
  expect(removalSummary([ok("a")], 0).title).toBe("Removed 1 image, about 0 B freed");
  expect(
    removalSummary(
      [
        ok("a"),
        { name: "app:b", error: { kind: "conflict", message: "in use" } },
        ok("c"),
        ok("d"),
      ],
      5,
    ),
  ).toEqual({ ok: false, title: "Removed 3 of 4. app:b: in use" });
});

test("build cache strip state", () => {
  expect(buildCacheText({ size: 0, reclaimable: 0 })).toEqual({
    text: "Build cache: empty",
    canClear: false,
  });
  expect(buildCacheText({ size: null, reclaimable: null })).toEqual({
    text: "Build cache: size not reported by this Engine",
    canClear: true,
  });
  expect(buildCacheText({ size: 6402404974, reclaimable: 5713938493 })).toEqual({
    text: "Build cache: 6 GiB (5.3 GiB reclaimable)",
    canClear: true,
  });
});
