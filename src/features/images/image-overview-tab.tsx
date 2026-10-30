import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import {
  formatBytes,
  formatRelative,
  parseDockerTime,
  shellQuote,
} from "@/features/container-details/format";
import { useLoadedImage } from "./use-images";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <h2 className="text-sm font-semibold">{title}</h2>
      {children}
    </section>
  );
}

const none = <p className="text-sm text-muted-foreground">None</p>;

function Lines({ items }: { items: string[] }) {
  if (items.length === 0) return none;
  return (
    <ul className="font-mono text-[0.8rem] break-all">
      {items.map((item, i) => (
        <li key={`${i}:${item}`}>{item}</li>
      ))}
    </ul>
  );
}

function Mono({ children, title }: { children: ReactNode; title?: string }) {
  return (
    <span className="font-mono text-[0.8rem]" title={title}>
      {children}
    </span>
  );
}

const argv = (args: string[]) => (args.length > 0 ? <Mono>{shellQuote(args)}</Mono> : "None");

/** The FR-5 sections. Empty sections say "None" rather than disappearing. */
export function ImageOverviewTab() {
  const { details: d, users } = useLoadedImage();
  const created = parseDockerTime(d.createdAt);
  return (
    <div className="flex max-w-5xl flex-col gap-8 pb-8">
      <Section title="Tags">
        <Lines items={d.repoTags} />
      </Section>
      <Section title="Digests">
        <Lines items={d.repoDigests} />
      </Section>
      <Section title="Image">
        <dl className="grid grid-cols-[minmax(8rem,max-content)_1fr] gap-x-6 gap-y-1.5 text-sm">
          <dt className="text-muted-foreground">ID</dt>
          <dd className="min-w-0 break-all">
            <Mono>{d.id}</Mono>
          </dd>
          <dt className="text-muted-foreground">Created</dt>
          <dd>
            <time dateTime={d.createdAt}>{formatRelative(created)}</time>{" "}
            <span className="text-muted-foreground">({created.toLocaleString()})</span>
          </dd>
          <dt className="text-muted-foreground">Size</dt>
          <dd title={`${d.size.toLocaleString()} bytes`}>{formatBytes(d.size)}</dd>
          <dt className="text-muted-foreground">Platform</dt>
          <dd>
            <Mono>{d.platform}</Mono>
          </dd>
          <dt className="text-muted-foreground">Entrypoint</dt>
          <dd>{argv(d.entrypoint)}</dd>
          <dt className="text-muted-foreground">Command</dt>
          <dd>{argv(d.cmd)}</dd>
          <dt className="text-muted-foreground">Working dir</dt>
          <dd>{d.workingDir ? <Mono>{d.workingDir}</Mono> : "Image default"}</dd>
          <dt className="text-muted-foreground">User</dt>
          <dd>{d.user ? <Mono>{d.user}</Mono> : "Image default"}</dd>
        </dl>
      </Section>
      <Section title="Exposed ports">
        <Lines items={d.exposedPorts} />
      </Section>
      <Section title="Environment">
        <Lines items={d.env} />
      </Section>
      <Section title="Labels">
        <Lines items={d.labels.map(({ key, value }) => `${key}=${value}`)} />
      </Section>
      <Section title="Used by">
        {users.length === 0 ? (
          none
        ) : (
          <ul className="text-sm">
            {users.map((c) => (
              <li key={c.id}>
                <Link
                  to="/containers/$id"
                  params={{ id: c.id }}
                  className="underline underline-offset-4"
                >
                  {c.name}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}
