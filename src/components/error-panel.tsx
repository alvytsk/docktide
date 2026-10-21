import { RotateCw } from "lucide-react";
import type { ReactNode } from "react";
import type { IpcError } from "@/bindings/IpcError";
import { Button } from "@/components/ui/button";

type Props = { error: IpcError; retrying: boolean; onRetry: () => void; children?: ReactNode };

/** An error's message and hint, optional details, and a Retry button. */
export function ErrorPanel({ error, retrying, onRetry, children }: Props) {
  return (
    <section aria-live="polite" className="flex max-w-xl flex-col gap-3">
      <h2 className="text-base font-semibold text-foreground">{error.message}</h2>
      {error.hint && <p className="text-sm leading-relaxed text-muted-foreground">{error.hint}</p>}
      {children}
      <div>
        <Button variant="outline" size="sm" disabled={retrying} onClick={onRetry}>
          <RotateCw className={retrying ? "motion-safe:animate-spin" : undefined} />
          Retry
        </Button>
      </div>
    </section>
  );
}
