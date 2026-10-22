import { useQuery } from "@tanstack/react-query";
import { createContext, use } from "react";
import type { ContainerInspect } from "@/bindings/ContainerInspect";
import { containerQueryKey } from "@/features/containers/use-containers";
import { getContainer } from "@/lib/ipc";

/** Live updates keep this fresh (spec M1 FR-2). Disabled while disconnected. */
export function useContainer(id: string, enabled: boolean) {
  return useQuery({
    queryKey: containerQueryKey(id),
    queryFn: () => getContainer(id),
    enabled,
    // `notFound` is a normal outcome and other errors carry a hint; retrying would only delay them.
    retry: false,
  });
}

/** The loaded container, provided by the details page to its tabs. */
export const ContainerContext = createContext<ContainerInspect | null>(null);

export function useLoadedContainer(): ContainerInspect {
  const container = use(ContainerContext);
  if (!container) throw new Error("useLoadedContainer outside a loaded container page");
  return container;
}
