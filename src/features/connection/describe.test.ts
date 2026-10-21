import { describe, expect, test } from "vite-plus/test";
import type { EndpointInfo } from "@/bindings/EndpointInfo";
import { describeSource } from "./describe";

const endpoint = (patch: Partial<EndpointInfo>): EndpointInfo => ({
  uri: "unix:///var/run/docker.sock",
  source: "default",
  contextName: null,
  ...patch,
});

describe("describeSource", () => {
  test("default socket", () => {
    expect(describeSource(endpoint({}))).toBe("Default socket");
  });

  test("DOCKER_HOST", () => {
    expect(describeSource(endpoint({ source: "dockerHostEnv" }))).toBe("DOCKER_HOST");
  });

  test("context from DOCKER_CONTEXT names the context", () => {
    expect(describeSource(endpoint({ source: "dockerContextEnv", contextName: "rootless" }))).toBe(
      "Context rootless (DOCKER_CONTEXT)",
    );
  });

  test("context from the Docker config names the context", () => {
    expect(describeSource(endpoint({ source: "configContext", contextName: "work" }))).toBe(
      "Context work (Docker config)",
    );
  });

  test("rootless fallback", () => {
    expect(describeSource(endpoint({ source: "rootlessFallback" }))).toBe(
      "Rootless socket (fallback)",
    );
  });
});
