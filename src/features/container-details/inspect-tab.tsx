import { Check, Copy } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { toastError } from "@/lib/toast";
import { useLoadedContainer } from "./use-container";

/** The raw inspect JSON, unmasked, like `docker inspect` (spec M1 §10, D4). */
export function InspectTab() {
  const { raw } = useLoadedContainer();
  const json = useMemo(() => JSON.stringify(raw, null, 2), [raw]);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const timer = copied ? setTimeout(() => setCopied(false), 2000) : undefined;
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = () =>
    navigator.clipboard.writeText(json).then(
      () => setCopied(true),
      () => toastError({ kind: "internal", message: "Could not copy to the clipboard." }),
    );

  return (
    <section aria-label="Inspect" className="flex shrink-0 flex-col gap-3">
      <div>
        <Button variant="outline" size="sm" onClick={() => void copy()}>
          {copied ? <Check /> : <Copy />}
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <pre className="overflow-x-auto rounded-lg border bg-muted/40 p-4 font-mono text-xs leading-relaxed select-text">
        {json}
      </pre>
    </section>
  );
}
