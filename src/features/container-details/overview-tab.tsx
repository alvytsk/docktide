import { Eye, EyeOff } from "lucide-react";
import { useState, type ReactNode } from "react";
import type { ContainerHealth } from "@/bindings/ContainerHealth";
import type { KeyValue } from "@/bindings/KeyValue";
import type { MountType } from "@/bindings/MountType";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatPort } from "@/features/containers/list";
import { cn } from "@/lib/utils";
import {
  formatBytes,
  formatCpus,
  formatRelative,
  parseDockerTime,
  restartPolicyText,
  shellQuote,
  shortImageId,
} from "./format";
import { useLoadedContainer } from "./use-container";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <h2 className="text-sm font-semibold">{title}</h2>
      {children}
    </section>
  );
}

function None() {
  return <p className="text-sm text-muted-foreground">None</p>;
}

/** Label and value rows; rows whose value is `null` are left out. */
function Fields({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[minmax(8rem,max-content)_1fr] gap-x-6 gap-y-1.5 text-sm">
      {rows
        .filter(([, value]) => value !== null)
        .map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="min-w-0 break-words">{value}</dd>
          </div>
        ))}
    </dl>
  );
}

function Mono({ children, title }: { children: ReactNode; title?: string }) {
  return (
    <span className="font-mono text-[0.8rem]" title={title}>
      {children}
    </span>
  );
}

/** Relative, with the absolute local time in a tooltip. */
function Time({ value }: { value: string }) {
  const date = parseDockerTime(value);
  return (
    <time dateTime={value} title={date.toLocaleString()}>
      {formatRelative(date)}
    </time>
  );
}

const HEALTH_TEXT: Record<ContainerHealth["status"], string> = {
  starting: "Starting",
  healthy: "Healthy",
  unhealthy: "Unhealthy",
  unknown: "Unknown",
};

function Health({ health }: { health: ContainerHealth }) {
  return (
    <div className="flex flex-col gap-1">
      <span className={cn(health.status === "unhealthy" && "text-signal-down")}>
        {HEALTH_TEXT[health.status]}
        {health.failingStreak > 0 &&
          `, ${health.failingStreak} failed ${health.failingStreak === 1 ? "check" : "checks"} in a row`}
      </span>
      {health.lastOutput && (
        <pre className="max-h-32 overflow-auto rounded-md bg-muted/40 px-2 py-1 font-mono text-xs whitespace-pre-wrap">
          {health.lastOutput}
        </pre>
      )}
    </div>
  );
}

function StatusSection() {
  const { details: d } = useLoadedContainer();
  const running = d.state === "running" || d.state === "paused" || d.state === "restarting";
  return (
    <Section title="Status">
      <Fields
        rows={[
          ["Created", <Time key="created" value={d.createdAt} />],
          ["Started", running && d.startedAt ? <Time key="started" value={d.startedAt} /> : null],
          [
            "Finished",
            !running && d.finishedAt ? <Time key="finished" value={d.finishedAt} /> : null,
          ],
          [
            "Exit code",
            d.exitCode === null || running ? null : <Mono key="exit">{d.exitCode}</Mono>,
          ],
          ["Out of memory", d.oomKilled ? "Killed by the OOM killer" : null],
          [
            "Error",
            d.error ? (
              <span key="error" className="text-signal-down">
                {d.error}
              </span>
            ) : null,
          ],
          // Docker keeps the last result after the container stops; the CLI hides it then.
          ["Health", d.health && running ? <Health key="health" health={d.health} /> : null],
          ["Restart policy", restartPolicyText(d.restartPolicy)],
          ["Restarts", d.restartCount],
        ]}
      />
    </Section>
  );
}

function ImageSection() {
  const { details: d } = useLoadedContainer();
  return (
    <Section title="Image">
      <Fields
        rows={[
          ["Reference", <Mono key="ref">{d.image}</Mono>],
          [
            "ID",
            <Mono key="id" title={d.imageId}>
              {shortImageId(d.imageId)}
            </Mono>,
          ],
        ]}
      />
    </Section>
  );
}

function argv(args: string[]) {
  return args.length > 0 ? <Mono key="argv">{shellQuote(args)}</Mono> : "None";
}

function ProcessSection() {
  const { details: d } = useLoadedContainer();
  return (
    <Section title="Process">
      <Fields
        rows={[
          ["Entrypoint", argv(d.entrypoint)],
          ["Command", argv(d.command)],
          [
            "Working directory",
            d.workingDir ? <Mono key="dir">{d.workingDir}</Mono> : "Image default",
          ],
          ["User", d.user ? <Mono key="user">{d.user}</Mono> : "Image default"],
          ["TTY", d.tty ? "Yes" : "No"],
        ]}
      />
    </Section>
  );
}

function PortsSection() {
  const { ports } = useLoadedContainer().details;
  return (
    <Section title="Ports">
      {ports.length === 0 ? (
        <None />
      ) : (
        <ul className="font-mono text-[0.8rem]">
          {ports.map((port) => (
            <li key={formatPort(port)}>{formatPort(port)}</li>
          ))}
        </ul>
      )}
    </Section>
  );
}

const MOUNT_TYPES: Record<MountType, string> = {
  bind: "Bind",
  volume: "Volume",
  tmpfs: "tmpfs",
  npipe: "Named pipe",
  cluster: "Cluster",
  image: "Image",
  unknown: "Unknown",
};

function MountsSection() {
  const { mounts } = useLoadedContainer().details;
  return (
    <Section title="Mounts">
      {mounts.length === 0 ? (
        <None />
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Type</TableHead>
              <TableHead>Source</TableHead>
              <TableHead>Destination</TableHead>
              <TableHead>Mode</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {mounts.map((m) => (
              <TableRow key={m.destination}>
                <TableCell>{MOUNT_TYPES[m.type]}</TableCell>
                <TableCell className="max-w-80 truncate font-mono text-xs" title={m.source}>
                  {m.type === "volume" && m.name ? m.name : m.source || "—"}
                </TableCell>
                <TableCell className="font-mono text-xs">{m.destination}</TableCell>
                <TableCell>{m.readOnly ? "Read-only" : "Read-write"}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Section>
  );
}

function NetworksSection() {
  const { networks } = useLoadedContainer().details;
  return (
    <Section title="Networks">
      {networks.length === 0 ? (
        <None />
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>IPv4</TableHead>
              <TableHead>IPv6</TableHead>
              <TableHead>Gateway</TableHead>
              <TableHead>MAC</TableHead>
              <TableHead>Aliases</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody className="font-mono text-xs">
            {networks.map((n) => (
              <TableRow key={n.name}>
                <TableCell className="font-sans text-sm">{n.name}</TableCell>
                <TableCell>{n.ipAddress ?? "—"}</TableCell>
                <TableCell>{n.ipv6Address ?? "—"}</TableCell>
                <TableCell>{n.gateway ?? "—"}</TableCell>
                <TableCell>{n.macAddress ?? "—"}</TableCell>
                <TableCell>{n.aliases.join(", ") || "—"}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Section>
  );
}

function KeyValueTable({ entries, masked }: { entries: KeyValue[]; masked?: boolean }) {
  return (
    <Table className="table-fixed">
      <TableBody className="font-mono text-xs">
        {entries.map(({ key, value }, index) => (
          // Env may repeat a name; Docker keeps the order, so the index is stable.
          <TableRow key={`${index}:${key}`}>
            <TableCell className="w-2/5 truncate" title={key}>
              {key}
            </TableCell>
            <TableCell className="truncate" title={masked ? undefined : value}>
              {masked ? <span className="text-muted-foreground">••••••</span> : value}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function EnvironmentSection() {
  const { env } = useLoadedContainer().details;
  // Not remembered: values are hidden again on every visit (spec M1 §10, D4).
  const [shown, setShown] = useState(false);
  return (
    <Section title="Environment">
      {env.length === 0 ? (
        <None />
      ) : (
        <>
          <div>
            <Button variant="outline" size="sm" onClick={() => setShown(!shown)}>
              {shown ? <EyeOff /> : <Eye />}
              {shown ? "Hide values" : "Show values"}
            </Button>
          </div>
          <KeyValueTable entries={env} masked={!shown} />
        </>
      )}
    </Section>
  );
}

function LabelsSection() {
  const { labels } = useLoadedContainer().details;
  return (
    <Section title="Labels">
      {labels.length === 0 ? <None /> : <KeyValueTable entries={labels} />}
    </Section>
  );
}

function ResourcesSection() {
  const { details: d } = useLoadedContainer();
  return (
    <Section title="Resources">
      <Fields
        rows={[
          ["Memory limit", d.memoryLimit === null ? "Unlimited" : formatBytes(d.memoryLimit)],
          ["CPU limit", d.cpuLimit === null ? "Unlimited" : formatCpus(d.cpuLimit)],
          ["Log driver", <Mono key="log">{d.logDriver || "—"}</Mono>],
        ]}
      />
    </Section>
  );
}

/** The sections of spec M1 §5.3. Empty sections say "None" rather than disappearing. */
export function OverviewTab() {
  return (
    <div className="flex max-w-5xl flex-col gap-8 pb-8">
      <StatusSection />
      <ImageSection />
      <ProcessSection />
      <PortsSection />
      <MountsSection />
      <NetworksSection />
      <EnvironmentSection />
      <LabelsSection />
      <ResourcesSection />
    </div>
  );
}
