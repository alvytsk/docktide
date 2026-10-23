import { describe, expect, test } from "vite-plus/test";
import { formatLogTime, isAtBottom } from "./follow";

describe("isAtBottom", () => {
  test("within a few pixels of the end counts as the bottom", () => {
    expect(isAtBottom({ scrollTop: 600, clientHeight: 400, scrollHeight: 1000 })).toBe(true);
    expect(isAtBottom({ scrollTop: 597, clientHeight: 400, scrollHeight: 1000 })).toBe(true);
    expect(isAtBottom({ scrollTop: 500, clientHeight: 400, scrollHeight: 1000 })).toBe(false);
  });

  test("content shorter than the view is at the bottom", () => {
    expect(isAtBottom({ scrollTop: 0, clientHeight: 400, scrollHeight: 200 })).toBe(true);
  });
});

const pad = (n: number) => String(n).padStart(2, "0");

test("log times are local HH:mm:ss.SSS", () => {
  const timestamp = "2026-10-22T04:38:47.825201379Z";
  const date = new Date("2026-10-22T04:38:47.825Z");
  expect(formatLogTime(timestamp)).toBe(
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.825`,
  );
});
