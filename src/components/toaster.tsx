import { Toast } from "@base-ui/react/toast";
import { X } from "lucide-react";
import type { ReactNode } from "react";
import { toastManager } from "@/lib/toast";

function Toasts() {
  const { toasts } = Toast.useToastManager();
  return toasts.map((toast) => (
    <Toast.Root
      key={toast.id}
      toast={toast}
      className="relative flex w-full flex-col gap-1 rounded-lg border border-l-4 border-l-signal-down bg-popover py-3 pr-9 pl-4 text-sm text-popover-foreground shadow-lg"
    >
      <Toast.Title className="font-medium" />
      <Toast.Description className="leading-relaxed text-muted-foreground" />
      <Toast.Close
        aria-label="Dismiss"
        className="absolute top-2 right-2 rounded-md p-1 text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
      >
        <X className="size-4" />
      </Toast.Close>
    </Toast.Root>
  ));
}

/** Stacks toasts bottom-right. M0 only shows errors, hence the red edge. */
export function Toaster({ children }: { children: ReactNode }) {
  return (
    <Toast.Provider toastManager={toastManager}>
      {children}
      <Toast.Portal>
        <Toast.Viewport className="fixed right-4 bottom-4 z-50 flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2">
          <Toasts />
        </Toast.Viewport>
      </Toast.Portal>
    </Toast.Provider>
  );
}
