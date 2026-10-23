import { describe, expect, test } from "vite-plus/test";
import type { LogLine } from "@/bindings/LogLine";
import { LogBuffer } from "./log-buffer";

function line(text: string, stream: LogLine["stream"] = "stdout"): LogLine {
  return { stream, timestamp: `2026-10-22T04:38:47.${text.padStart(9, "0")}Z`, text };
}

function texts(buffer: LogBuffer) {
  return buffer.entries.map((entry) =>
    entry.kind === "line" ? entry.plain : `gap:${entry.dropped}`,
  );
}

describe("LogBuffer", () => {
  test("keeps lines in order with stable, increasing keys", () => {
    const buffer = new LogBuffer(10);
    buffer.append([line("1"), line("2")]);
    buffer.append([line("3")]);
    expect(texts(buffer)).toEqual(["1", "2", "3"]);
    const keys = buffer.entries.map((entry) => entry.key);
    expect(keys.every((key, i) => i === 0 || key > (keys[i - 1] ?? key))).toBe(true);
    expect(new Set(keys).size).toBe(3);
  });

  test("drops the oldest beyond capacity and remembers that it did", () => {
    const buffer = new LogBuffer(3);
    buffer.append([line("1"), line("2")]);
    expect(buffer.truncated).toBe(false);
    buffer.append([line("3"), line("4"), line("5")]);
    expect(texts(buffer)).toEqual(["3", "4", "5"]);
    expect(buffer.truncated).toBe(true);
  });

  test("gap markers take a place in the buffer", () => {
    const buffer = new LogBuffer(10);
    buffer.append([line("1")]);
    buffer.gap(42);
    buffer.append([line("2")]);
    expect(texts(buffer)).toEqual(["1", "gap:42", "2"]);
  });

  test("the last timestamp is that of the newest line, kept across clear", () => {
    const buffer = new LogBuffer(10);
    expect(buffer.lastTimestamp).toBeNull();
    buffer.append([line("1"), line("2")]);
    buffer.gap(1);
    expect(buffer.lastTimestamp).toBe(line("2").timestamp);
    buffer.clear();
    expect(buffer.entries).toEqual([]);
    expect(buffer.truncated).toBe(false);
    expect(buffer.lastTimestamp).toBe(line("2").timestamp);
  });

  test("ANSI is parsed once on append; plain text drops it", () => {
    const buffer = new LogBuffer(10);
    buffer.append([line("\u001b[31mred\u001b[0m", "stderr")]);
    const [entry] = buffer.entries;
    expect(entry).toMatchObject({ kind: "line", plain: "red", stream: "stderr" });
  });

  test("filter is a case-insensitive substring on plain text, gaps always shown", () => {
    const buffer = new LogBuffer(10);
    buffer.append([line("GET /health"), line("POST /login"), line("\u001b[1mget\u001b[0m /x")]);
    buffer.gap(3);
    const shown = buffer.filter("get");
    expect(shown.map((entry) => (entry.kind === "line" ? entry.plain : "gap"))).toEqual([
      "GET /health",
      "get /x",
      "gap",
    ]);
    expect(buffer.filter("  ")).toEqual(buffer.entries);
    expect(buffer.lineCount).toBe(3);
  });

  test("version changes on every mutation", () => {
    const buffer = new LogBuffer(10);
    const versions = [buffer.version];
    buffer.append([line("1")]);
    versions.push(buffer.version);
    buffer.gap(1);
    versions.push(buffer.version);
    buffer.clear();
    versions.push(buffer.version);
    expect(new Set(versions).size).toBe(4);
  });

  test("appended counts every line ever added, for the new-line counter", () => {
    const buffer = new LogBuffer(2);
    buffer.append([line("1"), line("2"), line("3")]);
    buffer.clear();
    buffer.append([line("4")]);
    expect(buffer.appended).toBe(4);
  });
});
