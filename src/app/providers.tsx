import {
  focusManager,
  MutationCache,
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import type { ReactNode } from "react";
import type { IpcError } from "@/bindings/IpcError";
import { Toaster } from "@/components/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { connectionQueryKey } from "@/features/connection/use-connection-status";
import { isConnectionLevel } from "@/lib/errors";

// Any request that finds Docker unreachable refreshes the connection status at once,
// instead of waiting for the next poll.
function refreshConnection(error: IpcError) {
  if (isConnectionLevel(error.kind)) {
    void queryClient.invalidateQueries({ queryKey: connectionQueryKey });
  }
}

const queryClient = new QueryClient({
  queryCache: new QueryCache({
    // The connection query reports failures in its value; never let it re-trigger itself.
    onError: (error, query) =>
      query.queryKey[0] !== connectionQueryKey[0] && refreshConnection(error),
  }),
  mutationCache: new MutationCache({ onError: refreshConnection }),
});

// TanStack Query only watches page visibility, which does not change when a
// desktop window loses or regains focus.
focusManager.setEventListener((setFocused) => {
  const onFocus = () => setFocused(true);
  const onBlur = () => setFocused(false);
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);
  return () => {
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", onBlur);
  };
});

export function Providers({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delay={300}>
        <Toaster>{children}</Toaster>
      </TooltipProvider>
    </QueryClientProvider>
  );
}
