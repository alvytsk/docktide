import type { EndpointInfo } from "@/bindings/EndpointInfo";
import type { EndpointSource } from "@/bindings/EndpointSource";

const SOURCES: Record<EndpointSource, (context: string | null) => string> = {
  dockerHostEnv: () => "DOCKER_HOST",
  dockerContextEnv: (context) => `Context ${context} (DOCKER_CONTEXT)`,
  configContext: (context) => `Context ${context} (Docker config)`,
  default: () => "Default socket",
  rootlessFallback: () => "Rootless socket (fallback)",
};

/** Where the endpoint came from, so users can spot the "wrong daemon" case. */
export function describeSource({ source, contextName }: EndpointInfo): string {
  return SOURCES[source](contextName);
}
