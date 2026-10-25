import type { LogLine } from "@/bindings/LogLine";
import type { LogStream } from "@/bindings/LogStream";
import { parseAnsi, type AnsiSegment } from "./ansi";

export type LogEntry =
  | {
      kind: "line";
      key: number;
      /** Which container's stream; `""` for the single-container Logs tab. */
      source: string;
      /** Sort key: `orderKey(timestamp)`, or a neighbour's when the line has none. */
      order: string;
      stream: LogStream;
      timestamp: string;
      /** Without SGR sequences; what filter and copy use. */
      plain: string;
      segments: AnsiSegment[];
      /** `plain`, lowercased once for filtering. */
      search: string;
    }
  | { kind: "gap"; key: number; source: string; order: string; dropped: number };

/** RFC 3339 UTC with the fraction padded to 9 digits, so keys compare as strings. Twin of Rust's `normalize_timestamp`. */
export function orderKey(timestamp: string): string {
  const match = /^(.*T\d\d:\d\d:\d\d)(?:\.(\d+))?Z$/.exec(timestamp);
  return match ? `${match[1]}.${(match[2] ?? "").padEnd(9, "0")}Z` : timestamp;
}

/**
 * The lines in the view (spec M1 §5.4, M3 FR-2): at most `capacity` entries
 * across all sources, ordered by timestamp, oldest dropped first. Mutated in place; `version` changes on every mutation.
 */
export class LogBuffer {
  entries: LogEntry[] = [];
  /** Lines have fallen off the front since the last clear. */
  truncated = false;
  version = 0;
  /** Every line ever appended, for counting lines that arrive while paused. */
  appended = 0;
  private nextKey = 0;
  /** Newest timestamp per source, kept across `clear` so a reopen resumes after it. */
  private newest = new Map<string, { order: string; timestamp: string }>();

  constructor(readonly capacity = 10_000) {}

  append(lines: LogLine[], source = "") {
    const batch: Extract<LogEntry, { kind: "line" }>[] = [];
    for (const { stream, timestamp, text } of lines) {
      const { plain, segments } = parseAnsi(text);
      let order = orderKey(timestamp);
      const seen = this.newest.get(source);
      if (order === "") {
        order = seen?.order ?? this.entries.at(-1)?.order ?? "";
      } else if (!seen || order > seen.order) {
        this.newest.set(source, { order, timestamp });
      }
      batch.push({
        kind: "line",
        key: this.nextKey++,
        source,
        order,
        stream,
        timestamp,
        plain,
        segments,
        search: plain.toLowerCase(),
      });
    }
    this.appended += lines.length;
    batch.sort((a, b) => (a.order < b.order ? -1 : a.order > b.order ? 1 : 0));
    const lastOrder = this.entries.at(-1)?.order;
    if (batch.length > 0 && lastOrder !== undefined && batch[0]!.order < lastOrder) {
      // ponytail: O(n) per out-of-order batch; a gap-buffer if AC-12 shows jank
      const merged: LogEntry[] = [];
      let i = 0;
      let j = 0;
      while (i < this.entries.length && j < batch.length) {
        merged.push(this.entries[i]!.order <= batch[j]!.order ? this.entries[i++]! : batch[j++]!);
      }
      this.entries = merged.concat(this.entries.slice(i), batch.slice(j));
    } else {
      this.entries.push(...batch);
    }
    this.trim();
  }

  gap(dropped: number, source = "") {
    const order = this.entries.at(-1)?.order ?? "";
    this.entries.push({ kind: "gap", key: this.nextKey++, source, order, dropped });
    this.trim();
  }

  /** The newest timestamp seen from `source`, as Docker sent it. */
  resumePoint(source = ""): string | null {
    return this.newest.get(source)?.timestamp ?? null;
  }

  clear() {
    this.entries = [];
    this.truncated = false;
    this.version++;
  }

  get lineCount() {
    return this.entries.reduce((count, entry) => count + (entry.kind === "line" ? 1 : 0), 0);
  }

  /** Entries whose text contains `query`, ignoring case. Hidden sources drop out; gaps of shown sources always show. */
  filter(query: string, hidden?: (source: string) => boolean): LogEntry[] {
    const needle = query.trim().toLowerCase();
    if (needle === "" && !hidden) return this.entries;
    return this.entries.filter(
      (entry) =>
        !hidden?.(entry.source) &&
        (entry.kind === "gap" || needle === "" || entry.search.includes(needle)),
    );
  }

  private trim() {
    const excess = this.entries.length - this.capacity;
    if (excess > 0) {
      this.entries.splice(0, excess);
      this.truncated = true;
    }
    this.version++;
  }
}
