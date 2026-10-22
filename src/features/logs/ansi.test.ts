import { describe, expect, test } from "vite-plus/test";
import { parseAnsi, paletteColor, type AnsiStyle } from "./ansi";

const ESC = "\u001b";
const plain: AnsiStyle = {};

function styles(text: string) {
  return parseAnsi(text).segments.map(({ text: t, style }) => [t, style]);
}

describe("parseAnsi", () => {
  test("text without escapes is one plain segment", () => {
    expect(parseAnsi("hello")).toEqual({
      plain: "hello",
      segments: [{ text: "hello", style: plain }],
    });
  });

  test("plain text is the input without SGR sequences", () => {
    expect(parseAnsi(`${ESC}[1;31mERROR${ESC}[0m: disk ${ESC}[4mfull${ESC}[m`).plain).toBe(
      "ERROR: disk full",
    );
  });

  test("standard and bright foreground and background colours", () => {
    expect(styles(`${ESC}[31mr${ESC}[42mg${ESC}[39;49mn${ESC}[91;104mb`)).toEqual([
      ["r", { fg: { palette: 1 } }],
      ["g", { fg: { palette: 1 }, bg: { palette: 2 } }],
      ["n", {}],
      ["b", { fg: { palette: 9 }, bg: { palette: 12 } }],
    ]);
  });

  test("256-colour and 24-bit colours, with semicolons or colons", () => {
    expect(styles(`${ESC}[38;5;208ma${ESC}[48;2;10;20;30mb`)).toEqual([
      ["a", { fg: { palette: 208 } }],
      ["b", { fg: { palette: 208 }, bg: { rgb: "#0a141e" } }],
    ]);
    expect(styles(`${ESC}[38:2::255:0:0mc${ESC}[38:5:21md`)).toEqual([
      ["c", { fg: { rgb: "#ff0000" } }],
      ["d", { fg: { palette: 21 } }],
    ]);
  });

  test("attributes turn on and off", () => {
    expect(styles(`${ESC}[1;2;3;4;7;9mall${ESC}[22;23;24;27;29mnone`)).toEqual([
      [
        "all",
        { bold: true, dim: true, italic: true, underline: true, inverse: true, strike: true },
      ],
      ["none", {}],
    ]);
  });

  test("reset clears everything; an empty parameter list is a reset", () => {
    expect(styles(`${ESC}[1;31mx${ESC}[0my${ESC}[1mz${ESC}[mw`)).toEqual([
      ["x", { bold: true, fg: { palette: 1 } }],
      ["y", {}],
      ["z", { bold: true }],
      ["w", {}],
    ]);
  });

  test("unknown codes are ignored", () => {
    expect(styles(`${ESC}[5;53;31mx`)).toEqual([["x", { fg: { palette: 1 } }]]);
  });

  test("truncated extended colours are ignored", () => {
    expect(styles(`${ESC}[38;5mx${ESC}[38;2;1;2my`)).toEqual([
      ["x", {}],
      ["y", {}],
    ]);
  });

  test("malformed sequences are shown as text", () => {
    expect(parseAnsi(`a${ESC}[31xb`).plain).toBe(`a${ESC}[31xb`);
  });

  test("empty segments are dropped", () => {
    expect(styles(`${ESC}[31m${ESC}[1mx${ESC}[0m`)).toEqual([
      ["x", { bold: true, fg: { palette: 1 } }],
    ]);
  });
});

describe("paletteColor", () => {
  test("the 16 standard colours are theme tokens", () => {
    expect(paletteColor(1)).toBe("var(--ansi-1)");
    expect(paletteColor(15)).toBe("var(--ansi-15)");
  });

  test("the colour cube and grey ramp follow xterm", () => {
    expect(paletteColor(16)).toBe("#000000");
    expect(paletteColor(21)).toBe("#0000ff");
    expect(paletteColor(208)).toBe("#ff8700");
    expect(paletteColor(232)).toBe("#080808");
    expect(paletteColor(255)).toBe("#eeeeee");
  });
});
