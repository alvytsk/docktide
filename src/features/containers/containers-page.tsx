import { ConnectionPanel } from "@/features/connection/connection-panel";
import { useConnectionStatus } from "@/features/connection/use-connection-status";

export function ContainersPage() {
  const { status } = useConnectionStatus();
  return (
    <main id="containers" className="flex min-w-0 flex-1 flex-col gap-6 px-10 py-8">
      <h1 className="text-xl font-semibold tracking-tight">Containers</h1>
      {status?.state === "disconnected" && (
        <ConnectionPanel error={status.error} endpoint={status.endpoint} />
      )}
    </main>
  );
}
