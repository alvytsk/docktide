// Display helpers for the container details page (spec M1 §5.3).

const PLAIN_WORD = /^[\w@%+=:,./-]+$/;

/** Joins argv for display the way a POSIX shell would read it back. */
export function shellQuote(args: string[]): string {
  return args
    .map((arg) => (PLAIN_WORD.test(arg) ? arg : `'${arg.replaceAll("'", `'\\''`)}'`))
    .join(" ");
}

/** Docker's restart policy name (`on-failure:5`) as a sentence fragment. */
export function restartPolicyText(policy: string): string {
  const [name = "", retries] = policy.split(":");
  switch (name) {
    case "no":
      return "Never";
    case "always":
      return "Always";
    case "unless-stopped":
      return "Unless stopped";
    case "on-failure":
      if (!retries) return "On failure";
      return `On failure, up to ${retries} ${retries === "1" ? "retry" : "retries"}`;
    default:
      return policy;
  }
}

const BYTE_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"];

export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${Number(value.toFixed(1))} ${BYTE_UNITS[unit]}`;
}

export function formatCpus(cpus: number): string {
  return `${Number(cpus.toFixed(2))} ${cpus === 1 ? "CPU" : "CPUs"}`;
}

/** Docker sends nanoseconds; JavaScript dates only accept up to milliseconds reliably. */
export function parseDockerTime(value: string): Date {
  return new Date(
    value.replace(/\.(\d+)/, (_, fraction: string) => `.${fraction.padEnd(3, "0").slice(0, 3)}`),
  );
}

const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 86400],
  ["month", 30 * 86400],
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
  ["second", 1],
];

const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

export function formatRelative(date: Date, now = new Date()): string {
  const seconds = Math.round((date.getTime() - now.getTime()) / 1000);
  for (const [unit, size] of RELATIVE_UNITS) {
    if (Math.abs(seconds) >= size) return relative.format(Math.trunc(seconds / size), unit);
  }
  return relative.format(0, "second");
}

/** `sha256:0123…` → the first 12 hex characters, as the Docker CLI shows image ids. */
export function shortImageId(id: string): string {
  return id.replace(/^sha256:/, "").slice(0, 12);
}
