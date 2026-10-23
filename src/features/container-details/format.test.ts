import { describe, expect, test } from "vite-plus/test";
import {
  formatBytes,
  formatCpus,
  formatRelative,
  parseDockerTime,
  restartPolicyText,
  shellQuote,
  shortImageId,
} from "./format";

describe("shellQuote", () => {
  test("leaves plain words alone", () => {
    expect(shellQuote(["nginx", "-g", "--port=80", "/etc/a.conf"])).toBe(
      "nginx -g --port=80 /etc/a.conf",
    );
  });

  test("single-quotes words with spaces or shell characters", () => {
    expect(shellQuote(["sh", "-c", "echo $HOME; ls"])).toBe("sh -c 'echo $HOME; ls'");
    expect(shellQuote(["daemon off;"])).toBe("'daemon off;'");
  });

  test("escapes single quotes and shows empty words", () => {
    expect(shellQuote(["echo", "it's", ""])).toBe(`echo 'it'\\''s' ''`);
  });

  test("nothing is an empty string", () => {
    expect(shellQuote([])).toBe("");
  });
});

describe("restartPolicyText", () => {
  test.each([
    ["no", "Never"],
    ["always", "Always"],
    ["unless-stopped", "Unless stopped"],
    ["on-failure", "On failure"],
    ["on-failure:5", "On failure, up to 5 retries"],
    ["on-failure:1", "On failure, up to 1 retry"],
    ["something-new", "something-new"],
  ])("%s", (policy, text) => {
    expect(restartPolicyText(policy)).toBe(text);
  });
});

describe("limits", () => {
  test("bytes use binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(64 * 1024 * 1024)).toBe("64 MiB");
    expect(formatBytes(1.5 * 1024 ** 3)).toBe("1.5 GiB");
  });

  test("CPUs", () => {
    expect(formatCpus(1)).toBe("1 CPU");
    expect(formatCpus(0.5)).toBe("0.5 CPUs");
    expect(formatCpus(2)).toBe("2 CPUs");
  });
});

describe("times", () => {
  test("Docker's nanosecond times parse with millisecond precision", () => {
    expect(parseDockerTime("2026-10-21T10:00:00.123456789Z").toISOString()).toBe(
      "2026-10-21T10:00:00.123Z",
    );
    expect(parseDockerTime("2026-10-21T10:00:00Z").toISOString()).toBe("2026-10-21T10:00:00.000Z");
    expect(parseDockerTime("2026-10-21T12:00:00.5+02:00").toISOString()).toBe(
      "2026-10-21T10:00:00.500Z",
    );
  });

  test("relative times pick a sensible unit", () => {
    const now = new Date("2026-10-21T12:00:00Z");
    const ago = (seconds: number) => new Date(now.getTime() - seconds * 1000);
    expect(formatRelative(ago(5), now)).toBe("5 seconds ago");
    expect(formatRelative(ago(3 * 60), now)).toBe("3 minutes ago");
    expect(formatRelative(ago(2 * 3600), now)).toBe("2 hours ago");
    expect(formatRelative(ago(3 * 86400), now)).toBe("3 days ago");
    expect(formatRelative(ago(0), now)).toBe("now");
  });
});

test("shortImageId drops the algorithm and keeps 12 characters", () => {
  expect(shortImageId("sha256:0123456789abcdef0123")).toBe("0123456789ab");
  expect(shortImageId("0123456789abcdef")).toBe("0123456789ab");
});
