import { describe, expect, test } from "vite-plus/test";
import { errorKindLabel, toIpcError } from "./errors";

describe("toIpcError", () => {
  test("passes an IpcError through", () => {
    const error = { kind: "timeout", message: "Docker did not respond in time." } as const;
    expect(toIpcError(error)).toBe(error);
  });

  test("wraps a string rejection from invoke as internal", () => {
    expect(toIpcError("command get_connection_status not allowed")).toEqual({
      kind: "internal",
      message: "command get_connection_status not allowed",
    });
  });

  test("wraps an Error as internal with its message", () => {
    expect(toIpcError(new Error("boom"))).toEqual({ kind: "internal", message: "boom" });
  });

  test("rejects objects with an unknown kind", () => {
    expect(toIpcError({ kind: "nope", message: "x" }).kind).toBe("internal");
  });
});

describe("errorKindLabel", () => {
  test("names the daemon being down in plain words", () => {
    expect(errorKindLabel("daemonUnavailable")).toBe("Docker not running");
  });
});
