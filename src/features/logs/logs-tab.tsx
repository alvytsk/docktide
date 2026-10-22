import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, Check, Copy, Eraser, LoaderCircle, RotateCw } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useLoadedContainer } from "@/features/container-details/use-container";
import { parseDockerTime } from "@/features/container-details/format";
import { toastError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { colorValue, type AnsiSegment, type AnsiStyle } from "./ansi";
import { formatLogTime, isAtBottom } from "./follow";
import type { LogEntry } from "./log-buffer";
import { TAILS, useLogStream, type LogTail, type StreamStatus } from "./use-log-stream";

// Pressed toolbar toggles look like the selected tail, as in the list's state filter.
const PRESSED = "text-muted-foreground aria-pressed:bg-foreground aria-pressed:text-background";

const TAIL_LABELS = new Map<LogTail, string>([
  [100, "100"],
  [1000, "1,000"],
  [10_000, "10,000"],
  [null, "All"],
]);

function css(style: AnsiStyle): CSSProperties | undefined {
  if (Object.keys(style).length === 0) return undefined;
  let color = style.fg && colorValue(style.fg);
  let background = style.bg && colorValue(style.bg);
  if (style.inverse) {
    [color, background] = [background ?? "var(--background)", color ?? "var(--foreground)"];
  }
  const decorations = [style.underline && "underline", style.strike && "line-through"];
  return {
    color,
    backgroundColor: background,
    fontWeight: style.bold ? 600 : undefined,
    opacity: style.dim ? 0.65 : undefined,
    fontStyle: style.italic ? "italic" : undefined,
    textDecorationLine: decorations.filter(Boolean).join(" ") || undefined,
  };
}

/** React elements only; log text is never rendered as HTML. */
function Segments({ segments }: { segments: AnsiSegment[] }) {
  return segments.map((segment, index) => (
    // Segments never reorder within a line.
    // oxlint-disable-next-line react/no-array-index-key
    <span key={index} style={css(segment.style)}>
      {segment.text}
    </span>
  ));
}

function Row({ entry, timestamps }: { entry: LogEntry; timestamps: boolean }) {
  if (entry.kind === "gap") {
    return (
      <div className="border-l-2 border-transparent py-0.5 pl-3 text-muted-foreground italic">
        {entry.dropped.toLocaleString("en")} lines skipped (output too fast)
      </div>
    );
  }
  const stderr = entry.stream === "stderr";
  return (
    <div
      className={cn(
        "flex gap-3 border-l-2 pl-3",
        stderr ? "border-signal-down" : "border-transparent",
      )}
    >
      {timestamps && (
        <time
          dateTime={entry.timestamp}
          title={parseDockerTime(entry.timestamp).toLocaleString()}
          className="shrink-0 text-muted-foreground select-none"
        >
          {formatLogTime(entry.timestamp)}
        </time>
      )}
      <span className="min-w-0">
        {stderr && <span className="sr-only">stderr: </span>}
        <Segments segments={entry.segments} />
      </span>
    </div>
  );
}

function Footer({ children }: { children: ReactNode }) {
  return (
    <div
      aria-live="polite"
      className="flex items-center gap-3 border-t px-3 py-2 text-sm text-muted-foreground"
    >
      {children}
    </div>
  );
}

function StatusFooter({
  status,
  running,
  onRetry,
}: {
  status: StreamStatus;
  running: boolean;
  onRetry: () => void;
}) {
  const retry = (
    <Button variant="outline" size="sm" onClick={onRetry}>
      <RotateCw />
      Retry
    </Button>
  );
  if (status.kind === "disconnected") {
    return (
      <Footer>
        Docker is not reachable.
        {retry}
      </Footer>
    );
  }
  if (status.kind === "error") {
    return (
      <Footer>
        <span className="text-foreground">{status.error.message}</span>
        {status.error.hint}
        {retry}
      </Footer>
    );
  }
  // A running container reopens as soon as the details show its new run.
  if (status.kind === "ended" && !running) {
    return <Footer>Container stopped. Logs resume when it starts.</Footer>;
  }
  return null;
}

function LiveIndicator({ status }: { status: StreamStatus }) {
  if (status.kind !== "live") return null;
  return (
    <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <span aria-hidden className="size-1.5 rounded-full bg-signal-ok motion-safe:animate-pulse" />
      Live
    </span>
  );
}

function useCopied() {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    const timer = copied ? setTimeout(() => setCopied(false), 2000) : undefined;
    return () => clearTimeout(timer);
  }, [copied]);
  const copy = (text: string) =>
    navigator.clipboard.writeText(text).then(
      () => setCopied(true),
      () => toastError({ kind: "internal", message: "Could not copy to the clipboard." }),
    );
  return [copied, copy] as const;
}

/** Logs: tail plus follow, filter, and resume (spec M1 §5.4). */
export function LogsTab() {
  const { details } = useLoadedContainer();
  const running = details.state === "running";
  const stream = useLogStream(details.id, details.startedAt, running);
  const { buffer, status } = stream;

  const [query, setQuery] = useState("");
  const [timestamps, setTimestamps] = useState(false);
  const [wrap, setWrap] = useState(true);
  const [following, setFollowing] = useState(true);
  // `buffer.appended` when following paused, to count the lines since.
  const [pausedAt, setPausedAt] = useState(0);
  const [copied, copy] = useCopied();

  const shown = buffer.filter(query);
  const scrollRef = useRef<HTMLDivElement>(null);
  // React Compiler is not used here; the virtualizer re-renders on scroll itself.
  // oxlint-disable-next-line react/incompatible-library
  const virtualizer = useVirtualizer({
    count: shown.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 20,
    getItemKey: (index) => shown[index]?.key ?? index,
    overscan: 30,
  });

  // Stick to the end while following, after every render that may add lines.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (following && element) element.scrollTop = element.scrollHeight;
  });

  const onScroll = () => {
    const element = scrollRef.current;
    if (!element) return;
    const atBottom = isAtBottom(element);
    if (atBottom !== following) {
      setFollowing(atBottom);
      setPausedAt(buffer.appended);
    }
  };

  const jumpToLatest = () => {
    setFollowing(true);
    const element = scrollRef.current;
    if (element) element.scrollTop = element.scrollHeight;
  };

  const lineCount = buffer.lineCount;
  const shownLines =
    query.trim() === "" ? lineCount : shown.filter((e) => e.kind === "line").length;
  const unseen = following ? 0 : buffer.appended - pausedAt;

  let body: ReactNode = null;
  if (buffer.entries.length === 0) {
    body =
      status.kind === "opening" ? (
        <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
          <LoaderCircle className="size-4 motion-safe:animate-spin" aria-hidden />
          Opening logs…
        </div>
      ) : (
        <p className="p-4 text-sm text-muted-foreground">No log output yet</p>
      );
  }

  return (
    <section aria-label="Logs" className="flex min-h-80 flex-1 flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="search"
          aria-label="Filter lines"
          placeholder="Filter lines"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="w-64"
        />
        <span className="text-sm text-muted-foreground tabular-nums">
          {query.trim() === ""
            ? `${lineCount.toLocaleString("en")} lines`
            : `${shownLines.toLocaleString("en")} of ${lineCount.toLocaleString("en")} lines`}
        </span>
        <LiveIndicator status={status} />
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Toggle
            variant="outline"
            size="sm"
            pressed={timestamps}
            onPressedChange={setTimestamps}
            className={PRESSED}
          >
            Timestamps
          </Toggle>
          <Toggle
            variant="outline"
            size="sm"
            pressed={wrap}
            onPressedChange={setWrap}
            className={PRESSED}
          >
            Wrap
          </Toggle>
          <ToggleGroup
            aria-label="Lines to load"
            variant="outline"
            size="sm"
            spacing={0}
            value={[String(stream.tail)]}
            onValueChange={([value]) => {
              const tail = TAILS.find((t) => String(t) === value);
              if (tail !== undefined && tail !== stream.tail) {
                stream.setTail(tail);
                setFollowing(true);
              }
            }}
          >
            {TAILS.map((tail) => (
              <ToggleGroupItem key={String(tail)} value={String(tail)} className={PRESSED}>
                {TAIL_LABELS.get(tail)}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <Button variant="outline" size="sm" onClick={stream.clear}>
            <Eraser />
            Clear
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              void copy(shown.flatMap((e) => (e.kind === "line" ? [e.plain] : [])).join("\n"))
            }
          >
            {copied ? <Check /> : <Copy />}
            {copied ? "Copied" : "Copy"}
          </Button>
        </div>
      </div>

      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border bg-muted/30">
        {buffer.truncated && (
          <p className="border-b px-3 py-1.5 text-xs text-muted-foreground">
            Older lines are no longer shown.
          </p>
        )}
        <div className="relative flex min-h-0 flex-1 flex-col">
          <div
            ref={scrollRef}
            onScroll={onScroll}
            className="min-h-0 flex-1 overflow-auto font-mono text-xs leading-5"
          >
            {body}
            <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
              {virtualizer.getVirtualItems().map((item) => {
                const entry = shown[item.index];
                if (!entry) return null;
                return (
                  <div
                    key={item.key}
                    data-index={item.index}
                    ref={virtualizer.measureElement}
                    className={cn(
                      "absolute top-0 left-0 min-w-full pr-3",
                      wrap ? "w-full break-all whitespace-pre-wrap" : "whitespace-pre",
                    )}
                    style={{ transform: `translateY(${item.start}px)` }}
                  >
                    <Row entry={entry} timestamps={timestamps} />
                  </div>
                );
              })}
            </div>
          </div>
          {!following && (
            <Button
              size="sm"
              className="absolute right-4 bottom-3 shadow-md"
              onClick={jumpToLatest}
            >
              <ArrowDown />
              Jump to latest
              {unseen > 0 && ` (${unseen.toLocaleString("en")} new)`}
            </Button>
          )}
        </div>
        <StatusFooter status={status} running={running} onRetry={stream.retry} />
      </div>
    </section>
  );
}
