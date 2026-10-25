import { afterEach, expect, test, vi } from "vite-plus/test";
import type { ContainerSummary } from "@/bindings/ContainerSummary";
import type { IpcError } from "@/bindings/IpcError";
import type { LogEvent } from "@/bindings/LogEvent";
import { ProjectLogSession, projectSession, type LogsIpc } from "./session";

const member = (service: string, patch: Partial<ContainerSummary> = {}): ContainerSummary => ({
  id: `id-${service}`,
  name: `shop-${service}-1`,
  image: "img",
  state: "running",
  status: "Up",
  ports: [],
  createdAt: 0,
  compose: { project: "shop", service, number: 1, oneOff: false },
  ...patch,
});

const api = member("api");
const worker = member("worker");
const db = member("db");

type Open = {
  id: string;
  tail: number | null;
  after: string | null;
  subscription: number;
  emit: (event: LogEvent) => void;
  resolve: () => Promise<void>;
  reject: (error: IpcError) => Promise<void>;
};

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** Records `open` calls; each subscription id resolves only when the test says so. */
function fakeIpc() {
  const opens: Open[] = [];
  const closed: number[] = [];
  const ipc: LogsIpc = {
    open: (id, tail, after, onEvent) =>
      new Promise((resolve, reject) => {
        const subscription = opens.length + 1;
        opens.push({
          id,
          tail,
          after,
          subscription,
          emit: onEvent,
          resolve: () => (resolve(subscription), flush()),
          reject: (error) => (reject(error), flush()),
        });
      }),
    close: (subscription) => {
      closed.push(subscription);
      return Promise.resolve(null);
    },
  };
  /** The newest `open` call for container `id`. */
  const last = (id: string) => opens.filter((open) => open.id === id).at(-1)!;
  const count = (id: string) => opens.filter((open) => open.id === id).length;
  return { ipc, opens, closed, last, count };
}

const lines = (timestamp: string, text = timestamp): LogEvent => ({
  type: "lines",
  lines: [{ stream: "stdout", timestamp, text }],
});

function setup() {
  const fake = fakeIpc();
  const session = new ProjectLogSession("shop", fake.ipc);
  const hooks = { onEnded: vi.fn(), onConnectionLost: vi.fn() };
  const onChange = vi.fn();
  const stop = session.start(onChange, hooks);
  return { ...fake, session, hooks, onChange, stop };
}

const texts = (session: ProjectLogSession) =>
  session.buffer.entries.map((entry) => (entry.kind === "line" ? entry.plain : "gap"));

afterEach(() => {
  vi.restoreAllMocks();
});

test("opens one stream per member with the tail, then resumes with after", async () => {
  const { session, opens, last, closed, stop, onChange } = setup();
  session.sync([api, worker], 0);
  expect(opens.map(({ id, tail, after }) => ({ id, tail, after }))).toEqual([
    { id: api.id, tail: 1000, after: null },
    { id: worker.id, tail: 1000, after: null },
  ]);
  expect(session.streams.get(api.id)).toEqual({ kind: "opening" });
  await last(api.id).resolve();
  expect(session.streams.get(api.id)).toEqual({ kind: "live" });
  last(api.id).emit(lines("2026-10-25T10:00:01.5Z", "hello"));
  expect(texts(session)).toEqual(["hello"]);
  expect(session.sources.get(api.id)).toEqual({ service: "api", tag: "api" });
  expect(onChange).toHaveBeenCalled();

  stop();
  expect(closed).toEqual([1]);
  session.start(() => {}, { onEnded: () => {}, onConnectionLost: () => {} });
  session.sync([api, worker], 0);
  expect(last(api.id)).toMatchObject({ tail: 1000, after: "2026-10-25T10:00:01.5Z" });
  expect(last(worker.id)).toMatchObject({ tail: 1000, after: null });
  expect(texts(session)).toEqual(["hello"]);
});

test("a member that leaves is closed; its lines and source info stay", async () => {
  const { session, last, closed } = setup();
  session.sync([api, worker], 0);
  await last(worker.id).resolve();
  last(worker.id).emit(lines("2026-10-25T10:00:01Z", "working"));
  session.sync([api], 1);
  expect(closed).toEqual([last(worker.id).subscription]);
  expect(session.streams.has(worker.id)).toBe(false);
  expect(session.sources.get(worker.id)).toEqual({ service: "worker", tag: "worker" });
  expect(texts(session)).toEqual(["working"]);
  // Late events from the closed stream are ignored.
  last(worker.id).emit(lines("2026-10-25T10:00:02Z", "late"));
  expect(texts(session)).toEqual(["working"]);
});

test("a new member opens with the tail", () => {
  const { session, last, count } = setup();
  session.sync([api], 0);
  session.sync([api, worker], 1);
  expect(count(api.id)).toBe(1);
  expect(last(worker.id)).toMatchObject({ tail: 1000, after: null });
});

test("an ended container reopens once per newer running result", async () => {
  const { session, last, count, hooks } = setup();
  const now = vi.spyOn(Date, "now").mockReturnValue(100);
  session.sync([api], 0);
  await last(api.id).resolve();
  last(api.id).emit(lines("2026-10-25T10:00:01Z"));
  last(api.id).emit({ type: "ended" });
  expect(session.streams.get(api.id)).toEqual({ kind: "ended" });
  expect(hooks.onEnded).toHaveBeenCalledOnce();

  session.sync([api], 90);
  expect(count(api.id)).toBe(1);
  session.sync([api], 110);
  expect(count(api.id)).toBe(2);
  expect(last(api.id)).toMatchObject({ after: "2026-10-25T10:00:01Z" });

  // It ends again before a newer result arrives: the same result does not reopen it.
  now.mockReturnValue(105);
  last(api.id).emit({ type: "ended" });
  session.sync([api], 110);
  expect(count(api.id)).toBe(2);
  session.sync([api], 120);
  expect(count(api.id)).toBe(3);
});

test("an exited result never reopens", () => {
  const { session, last, count } = setup();
  vi.spyOn(Date, "now").mockReturnValue(100);
  session.sync([api], 0);
  last(api.id).emit({ type: "ended" });
  session.sync([{ ...api, state: "exited" }], 110);
  session.sync([{ ...api, state: "exited" }], 120);
  expect(count(api.id)).toBe(1);
});

test("ended calls onEnded; a connection-level error calls onConnectionLost and marks that stream disconnected", async () => {
  const { session, last, hooks } = setup();
  session.sync([api, worker, db], 0);
  await last(api.id).resolve();
  last(api.id).emit({ type: "ended" });
  expect(hooks.onEnded).toHaveBeenCalledOnce();

  last(worker.id).emit({ type: "error", error: { kind: "daemonUnavailable", message: "down" } });
  expect(hooks.onConnectionLost).toHaveBeenCalledOnce();
  expect(session.streams.get(worker.id)).toEqual({ kind: "disconnected" });
  expect(session.streams.get(api.id)).toEqual({ kind: "ended" });

  // `notFound` is treated like `ended`.
  await last(db.id).reject({ kind: "notFound", message: "gone" });
  expect(session.streams.get(db.id)).toEqual({ kind: "ended" });
  expect(hooks.onEnded).toHaveBeenCalledTimes(2);
});

test("another error marks only that stream", async () => {
  const { session, last, hooks } = setup();
  session.sync([api, worker], 0);
  await last(api.id).resolve();
  await last(worker.id).resolve();
  const error: IpcError = { kind: "internal", message: "boom" };
  last(worker.id).emit({ type: "error", error });
  expect(session.streams.get(worker.id)).toEqual({ kind: "error", error });
  expect(session.streams.get(api.id)).toEqual({ kind: "live" });
  expect(hooks.onConnectionLost).not.toHaveBeenCalled();

  session.retry(worker.id);
  expect(session.streams.get(worker.id)).toEqual({ kind: "opening" });
  expect(last(worker.id).subscription).toBe(3);
});

test("a stream that resolves after close is closed at once", async () => {
  const { session, last, closed, stop } = setup();
  session.sync([api], 0);
  stop();
  expect(closed).toEqual([]);
  await last(api.id).resolve();
  expect(closed).toEqual([last(api.id).subscription]);
  last(api.id).emit(lines("2026-10-25T10:00:01Z"));
  expect(texts(session)).toEqual([]);
});

test("changing the tail drops late lines from the old streams", async () => {
  const { session, last, closed } = setup();
  session.sync([api], 0);
  const old = last(api.id);
  await old.resolve();
  old.emit(lines("2026-10-25T10:00:01Z", "old"));
  session.setTail(100);
  expect(session.tail).toBe(100);
  expect(closed).toEqual([old.subscription]);
  expect(last(api.id)).toMatchObject({ tail: 100, after: null });
  old.emit(lines("2026-10-25T10:00:02Z", "late"));
  last(api.id).emit(lines("2026-10-25T10:00:03Z", "new"));
  expect(texts(session)).toEqual(["new"]);
});

test("services lists members and services with buffered lines, sorted", () => {
  const { session, last } = setup();
  session.sync([worker, db, api], 0);
  expect(session.services()).toEqual(["api", "db", "worker"]);
  last(worker.id).emit(lines("2026-10-25T10:00:01Z"));
  session.sync([api], 1);
  expect(session.services()).toEqual(["api", "worker"]);
  session.sync([], 2);
  expect(session.services()).toEqual(["worker"]);
});

test("toggleService hides and shows without reopening", () => {
  const { session, opens, onChange } = setup();
  session.sync([api], 0);
  const version = session.version;
  const changes = onChange.mock.calls.length;
  session.toggleService("api");
  expect(session.hidden.has("api")).toBe(true);
  session.toggleService("api");
  expect(session.hidden.has("api")).toBe(false);
  expect(opens).toHaveLength(1);
  expect(session.version).toBe(version + 2);
  expect(onChange).toHaveBeenCalledTimes(changes + 2);
});

test("projectSession keeps one session for the most recent project", () => {
  const shop = projectSession("shop");
  expect(projectSession("shop")).toBe(shop);
  const blog = projectSession("blog");
  expect(blog).not.toBe(shop);
  expect(blog.project).toBe("blog");
  expect(projectSession("shop")).not.toBe(shop);
});
