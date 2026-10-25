import { describe, expect, test } from "vite-plus/test";
import type { LogLine } from "@/bindings/LogLine";
import { LogBuffer, orderKey } from "./log-buffer";

function line(text: string, stream: LogLine["stream"] = "stdout"): LogLine {
  return { stream, timestamp: `2026-10-22T04:38:47.${text.padStart(9, "0")}Z`, text };
}

function at(ts: string, text = ts): LogLine {
  return { stream: "stdout", timestamp: ts === "" ? "" : `2026-10-22T04:38:${ts}Z`, text };
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
    expect(buffer.resumePoint()).toBeNull();
    buffer.append([line("1"), line("2")]);
    buffer.gap(1);
    expect(buffer.resumePoint()).toBe(line("2").timestamp);
    buffer.clear();
    expect(buffer.entries).toEqual([]);
    expect(buffer.truncated).toBe(false);
    expect(buffer.resumePoint()).toBe(line("2").timestamp);
  });

  test("ANSI is parsed once on append; plain text drops it", () => {
    const buffer = new LogBuffer(10);
    buffer.append([line("\u001b[31mred\u001b[0m", "stderr")]);
    const [entry] = buffer.entries;
    expect(entry).toMatchObject({ kind: "line", plain: "red", stream: "stderr" });
  });

  test("filter is a case-insensitive substring on plain text, gaps always shown", () => {
    const buffer = new LogBuffer(10);
    buffer.append([
      at("01", "GET /health"),
      at("02", "POST /login"),
      at("03", "\u001b[1mget\u001b[0m /x"),
    ]);
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

  test("orderKey pads the fraction to 9 digits", () => {
    expect(orderKey("2026-10-22T04:38:47Z")).toBe("2026-10-22T04:38:47.000000000Z");
    expect(orderKey("2026-10-22T04:38:47.5Z")).toBe("2026-10-22T04:38:47.500000000Z");
    expect(orderKey("")).toBe("");
  });

  test("interleaves sources by instant despite trimmed fractions", () => {
    const buffer = new LogBuffer(10);
    buffer.append([at("47.5", "a5")], "A");
    buffer.append([at("47.25", "b25")], "B");
    buffer.append([at("47.75", "a75")], "A");
    expect(texts(buffer)).toEqual(["b25", "a5", "a75"]);
  });

  test("orders lines within one source by timestamp", () => {
    const buffer = new LogBuffer(10);
    buffer.append([at("03", "3"), at("01", "1")], "A");
    buffer.append([at("02", "2")], "B");
    expect(texts(buffer)).toEqual(["1", "2", "3"]);
  });

  test("a late tail merges into place", () => {
    const buffer = new LogBuffer(10);
    buffer.append([at("10"), at("20"), at("30")], "A");
    buffer.append([at("05"), at("15"), at("25")], "B");
    expect(texts(buffer)).toEqual(["05", "10", "15", "20", "25", "30"]);
  });

  test("equal timestamps keep arrival order", () => {
    const buffer = new LogBuffer(10);
    buffer.append([at("10", "a1"), at("10", "a2")], "A");
    buffer.append([at("10", "b1")], "B");
    buffer.append([at("05", "b0"), at("10", "b2")], "B");
    expect(texts(buffer)).toEqual(["b0", "a1", "a2", "b1", "b2"]);
  });

  test("a line without a timestamp stays after its container's previous line", () => {
    const buffer = new LogBuffer(10);
    buffer.append([at("01", "1"), at("", "x")], "A");
    buffer.append([at("02", "2")], "B");
    expect(texts(buffer)).toEqual(["1", "x", "2"]);
  });

  test("capacity trims the oldest across all sources", () => {
    const buffer = new LogBuffer(3);
    buffer.append([at("02"), at("04")], "A");
    buffer.append([at("01"), at("03")], "B");
    expect(texts(buffer)).toEqual(["02", "03", "04"]);
    expect(buffer.truncated).toBe(true);
  });

  test("resume point is the newest timestamp per source, not the last arrived, and survives clear", () => {
    const buffer = new LogBuffer(10);
    buffer.append([at("03"), at("01")], "A");
    expect(buffer.resumePoint("A")).toBe(at("03").timestamp);
    buffer.clear();
    expect(buffer.resumePoint("A")).toBe(at("03").timestamp);
    expect(buffer.resumePoint("B")).toBeNull();
  });

  test("hidden sources are filtered, with their gaps", () => {
    const buffer = new LogBuffer(10);
    buffer.append([at("01", "a")], "A");
    buffer.gap(1, "A");
    buffer.append([at("02", "b")], "B");
    buffer.gap(2, "B");
    const shown = buffer.filter("", (source) => source === "B");
    expect(shown.map((e) => (e.kind === "line" ? e.plain : `gap:${e.dropped}`))).toEqual([
      "a",
      "gap:1",
    ]);
  });
});
