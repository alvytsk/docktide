import { parseDockerTime } from "@/features/container-details/format";

type Scroll = { scrollTop: number; clientHeight: number; scrollHeight: number };

/** Following resumes when the view is scrolled back to the end. */
export function isAtBottom({ scrollTop, clientHeight, scrollHeight }: Scroll, slack = 4) {
  return scrollHeight - scrollTop - clientHeight <= slack;
}

const pad = (n: number, width = 2) => String(n).padStart(width, "0");

/** Local `HH:mm:ss.SSS`. */
export function formatLogTime(timestamp: string): string {
  const date = parseDockerTime(timestamp);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}
