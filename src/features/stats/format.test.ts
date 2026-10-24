import { expect, it } from "vite-plus/test";
import { formatMemory, formatPercent, formatRate, memoryPercent } from "./format";

it("formats values and missing values", () => {
  expect(formatPercent(12.345)).toBe("12.3 %");
  expect(formatPercent(0)).toBe("0 %");
  expect(formatPercent(null)).toBe("—");
  expect(formatMemory(50_544_640)).toBe("48.2 MiB");
  expect(formatMemory(null)).toBe("—");
  expect(formatRate(2048)).toBe("2 KiB/s");
  expect(formatRate(null)).toBe("—");
});

it("computes memory percent only from usage and a positive limit", () => {
  expect(memoryPercent(512, 1024)).toBe(50);
  expect(memoryPercent(512, null)).toBeNull();
  expect(memoryPercent(null, 1024)).toBeNull();
  expect(memoryPercent(512, 0)).toBeNull();
});
