import { describe, expect, test } from "vite-plus/test";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { groupContainers } from "./groups";
import type { ContainerFilter } from "./list";

const all: ContainerFilter = { text: "", state: "all" };

const standalone = (name: string, patch: Partial<ContainerSummary> = {}): ContainerSummary => ({
  id: `id-${name}`,
  name,
  image: "img",
  state: "running",
  status: "Up",
  ports: [],
  imageId: "sha256:abc",
  createdAt: 0,
  compose: null,
  ...patch,
});

const member = (
  project: string,
  service: string,
  patch: Partial<ContainerSummary> & { number?: number | null; oneOff?: boolean } = {},
): ContainerSummary => {
  const { number = 1, oneOff = false, ...rest } = patch;
  return standalone(`${project}-${service}-${number ?? "x"}${oneOff ? "-run" : ""}`, {
    compose: { project, service, number, oneOff },
    ...rest,
  });
};

const names = (list: ContainerSummary[] = []) => list.map((c) => c.name);

describe("groupContainers", () => {
  test("projects first, sorted case-insensitively; standalone after in input order", () => {
    const result = groupContainers(
      [
        standalone("zeta"),
        member("shop", "api"),
        member("Blog", "web"),
        standalone("alpha", { state: "exited" }),
      ],
      all,
    );
    expect(result.groups.map((g) => g.project)).toEqual(["Blog", "shop"]);
    expect(names(result.standalone)).toEqual(["zeta", "alpha"]);
  });

  test("members ordered by service, then replica, one-off last", () => {
    const { groups } = groupContainers(
      [
        member("shop", "worker", { number: 2 }),
        member("shop", "api", { number: 1 }),
        member("shop", "worker", { number: 1 }),
        member("shop", "api", { number: null, oneOff: true }),
      ],
      all,
    );
    expect(names(groups[0]?.containers)).toEqual([
      "shop-api-1",
      "shop-worker-1",
      "shop-worker-2",
      "shop-api-x-run",
    ]);
  });

  test("counts cover the whole project under a filter", () => {
    const { groups } = groupContainers(
      [member("shop", "api"), member("shop", "db", { state: "exited" })],
      { text: "", state: "running" },
    );
    expect(names(groups[0]?.shown)).toEqual(["shop-api-1"]);
    expect(groups[0]?.services).toBe(2);
    expect(groups[0]?.running).toBe(1);
  });

  test("running counts only state running", () => {
    const { groups } = groupContainers(
      [
        member("shop", "a"),
        member("shop", "b", { state: "paused" }),
        member("shop", "c", { state: "restarting" }),
      ],
      all,
    );
    expect(groups[0]?.running).toBe(1);
  });

  test("a filter matching the project name shows the whole group", () => {
    const { groups } = groupContainers(
      [member("shop", "api"), member("shop", "db", { state: "exited" })],
      { text: "shop", state: "all" },
    );
    expect(groups[0]?.shown).toHaveLength(2);
  });

  test("a group with no matching members is left out", () => {
    const result = groupContainers([member("shop", "api")], { text: "nomatch", state: "all" });
    expect(result.groups).toEqual([]);
  });

  test("shownCount counts shown containers in groups and standalone", () => {
    const result = groupContainers(
      [member("shop", "api"), member("shop", "db"), standalone("one"), standalone("two")],
      { text: "", state: "all" },
    );
    expect(result.shownCount).toBe(4);
    const filtered = groupContainers(
      [member("shop", "api"), member("shop", "db"), standalone("one")],
      { text: "db", state: "all" },
    );
    expect(filtered.shownCount).toBe(1);
  });
});
