import { expect, test } from "vite-plus/test";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import { serviceColor, tagFor } from "./tags";

const member = (service: string, number: number | null, patch: Partial<ContainerSummary> = {}) =>
  ({
    id: `id-${service}-${number}`,
    name: `shop-${service}-${number}`,
    image: "img",
    state: "running",
    status: "Up",
    ports: [],
    imageId: "sha256:abc",
    createdAt: 0,
    compose: { project: "shop", service, number, oneOff: false },
    ...patch,
  }) satisfies ContainerSummary;

test("plain service tag unless the service is scaled", () => {
  const api = member("api", 1);
  const w1 = member("worker", 1);
  const w2 = member("worker", 2);
  const members = [api, w1, w2];
  expect(tagFor(api, members)).toBe("api");
  expect(tagFor(w1, members)).toBe("worker-1");
  expect(tagFor(w2, members)).toBe("worker-2");
});

test("one-off containers are tagged by container name", () => {
  const api = member("api", 1);
  const run = member("api", null, {
    id: "run",
    name: "shop-api-run-3f2a",
    compose: { project: "shop", service: "api", number: null, oneOff: true },
  });
  const members = [api, run];
  expect(tagFor(run, members)).toBe("shop-api-run-3f2a");
  expect(tagFor(api, members)).toBe("api");
});

test("a recreated container keeps the plain service tag", () => {
  const recreated = member("api", 1, { id: "new-id" });
  expect(tagFor(recreated, [recreated])).toBe("api");
  // `--force-recreate` creates the new container before removing the old one.
  const old = member("api", 1);
  expect(tagFor(old, [old, recreated])).toBe("api");
  expect(tagFor(recreated, [old, recreated])).toBe("api");
  const w1 = member("worker", 1);
  const w2 = member("worker", 2);
  expect(tagFor(w2, [w1, w2, member("worker", 2, { id: "new-w2" })])).toBe("worker-2");
});

test("service colour is stable and never red", () => {
  expect(serviceColor("api")).toBe(serviceColor("api"));
  const colors = new Set(Array.from({ length: 200 }, (_, i) => serviceColor(`service-${i}`)));
  expect(colors.has("var(--ansi-1)")).toBe(false);
  expect(colors.has("var(--ansi-9)")).toBe(false);
  expect(colors.size).toBeGreaterThan(5);
  for (const color of colors) expect(color).toMatch(/^var\(--ansi-\d+\)$/);
});
