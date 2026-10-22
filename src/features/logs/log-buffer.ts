import type { LogLine } from "@/bindings/LogLine";
import type { LogStream } from "@/bindings/LogStream";
import { parseAnsi, type AnsiSegment } from "./ansi";

export type LogEntry =
  | {
      kind: "line";
      key: number;
      stream: LogStream;
      timestamp: string;
      /** Without SGR sequences; what filter and copy use. */
      plain: string;
      segments: AnsiSegment[];
      /** `plain`, lowercased once for filtering. */
      search: string;
    }
  | { kind: "gap"; key: number; dropped: number };

/**
 * The lines in the view (spec M1 §5.4): at most `capacity` entries, oldest
 * dropped first. Mutated in place; `version` changes on every mutation.
 */
export class LogBuffer {
  entries: LogEntry[] = [];
  /** Lines have fallen off the front since the last clear. */
  truncated = false;
  /** The newest line's timestamp, kept across `clear` so a reopen resumes after it. */
  lastTimestamp: string | null = null;
  version = 0;
  /** Every line ever appended, for counting lines that arrive while paused. */
  appended = 0;
  private nextKey = 0;

  constructor(readonly capacity = 10_000) {}

  append(lines: LogLine[]) {
    for (const { stream, timestamp, text } of lines) {
      const { plain, segments } = parseAnsi(text);
      this.entries.push({
        kind: "line",
        key: this.nextKey++,
        stream,
        timestamp,
        plain,
        segments,
        search: plain.toLowerCase(),
      });
    }
    this.appended += lines.length;
    const last = lines.at(-1);
    if (last) this.lastTimestamp = last.timestamp;
    this.trim();
  }

  gap(dropped: number) {
    this.entries.push({ kind: "gap", key: this.nextKey++, dropped });
    this.trim();
  }

  clear() {
    this.entries = [];
    this.truncated = false;
    this.version++;
  }

  get lineCount() {
    return this.entries.reduce((count, entry) => count + (entry.kind === "line" ? 1 : 0), 0);
  }

  /** Entries whose text contains `query`, ignoring case. Gap markers always show. */
  filter(query: string): LogEntry[] {
    const needle = query.trim().toLowerCase();
    if (needle === "") return this.entries;
    return this.entries.filter((entry) => entry.kind === "gap" || entry.search.includes(needle));
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
