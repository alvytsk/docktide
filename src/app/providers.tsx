import { focusManager, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { Toaster } from "@/components/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";

const queryClient = new QueryClient();

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
