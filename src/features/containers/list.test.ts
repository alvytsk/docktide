import { describe, expect, test } from "vite-plus/test";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import type { PortMapping } from "@/bindings/PortMapping";
import { formatPort, matchesFilter, type ContainerFilter } from "./list";

const container = (patch: Partial<ContainerSummary>): ContainerSummary => ({
  id: "4f2a9c01d3e5b7a8",
  name: "web-api",
  image: "ghcr.io/acme/API:1.4",
  state: "running",
  status: "Up 3 hours",
  ports: [],
  createdAt: 0,
  ...patch,
});

const filter = (patch: Partial<ContainerFilter>): ContainerFilter => ({
  text: "",
  state: "all",
  ...patch,
});

describe("matchesFilter", () => {
  test("text matches name, image, or id prefix, case-insensitively", () => {
    expect(matchesFilter(container({}), filter({ text: "WEB" }))).toBe(true);
    expect(matchesFilter(container({}), filter({ text: "acme/api" }))).toBe(true);
    expect(matchesFilter(container({}), filter({ text: "4F2A" }))).toBe(true);
    expect(matchesFilter(container({}), filter({ text: "  web  " }))).toBe(true);
    // An id substring that is not a prefix does not match.
    expect(matchesFilter(container({}), filter({ text: "9c01" }))).toBe(false);
    expect(matchesFilter(container({}), filter({ text: "postgres" }))).toBe(false);
  });

  test("running includes paused and restarting; stopped is everything else", () => {
    const states = [
      "running",
      "paused",
      "restarting",
      "created",
      "exited",
      "dead",
      "unknown",
    ] as const;
    const matching = (state: ContainerFilter["state"]) =>
      states.filter((s) => matchesFilter(container({ state: s }), filter({ state })));
    expect(matching("running")).toEqual(["running", "paused", "restarting"]);
    expect(matching("stopped")).toEqual(["created", "exited", "dead", "unknown"]);
    expect(matching("all")).toEqual(states);
  });

  test("text and state filters combine", () => {
    const exited = container({ state: "exited" });
    expect(matchesFilter(exited, filter({ text: "web", state: "running" }))).toBe(false);
    expect(matchesFilter(exited, filter({ text: "web", state: "stopped" }))).toBe(true);
  });
});

const port = (patch: Partial<PortMapping>): PortMapping => ({
  privatePort: 80,
  publicPort: 8080,
  hostIp: null,
  protocol: "tcp",
  ...patch,
});

describe("formatPort", () => {
  test("published on all interfaces", () => {
    expect(formatPort(port({}))).toBe("8080→80/tcp");
  });

  test("published on one address", () => {
    expect(formatPort(port({ hostIp: "127.0.0.1" }))).toBe("127.0.0.1:8080→80/tcp");
    expect(formatPort(port({ hostIp: "::1" }))).toBe("[::1]:8080→80/tcp");
  });

  test("unpublished", () => {
    expect(formatPort(port({ publicPort: null, protocol: "udp" }))).toBe("80/udp");
  });
});
