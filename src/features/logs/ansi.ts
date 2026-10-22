// SGR colours and text attributes in log lines (spec M1 §5.4, D3). Rust has
// already removed every other escape sequence.

export type AnsiColor = { palette: number } | { rgb: string };

/** Only attributes that are on are present, so a plain style is `{}`. */
export type AnsiStyle = {
  bold?: true;
  dim?: true;
  italic?: true;
  underline?: true;
  inverse?: true;
  strike?: true;
  fg?: AnsiColor;
  bg?: AnsiColor;
};

export type AnsiSegment = { text: string; style: AnsiStyle };

// oxlint-disable-next-line no-control-regex -- matching ESC is the point
const SGR = /\u001b\[([0-9;:]*)m/g;

const byte = (value: string | undefined) => {
  if (value === undefined || value === "") return undefined;
  const n = Number(value);
  return Number.isInteger(n) && n >= 0 && n <= 255 ? n : undefined;
};

const hex = (channels: (number | undefined)[]) =>
  channels.length === 3 && channels.every((c) => c !== undefined)
    ? `#${channels.map((c) => c.toString(16).padStart(2, "0")).join("")}`
    : undefined;

/** `5;n` or `2;r;g;b` after a 38 or 48; returns the colour and how many parameters it used. */
function extended(params: string[]): [AnsiColor | undefined, number] {
  if (params[0] === "5") {
    const index = byte(params[1]);
    return [index === undefined ? undefined : { palette: index }, 2];
  }
  if (params[0] === "2") {
    const rgb = hex(params.slice(1, 4).map(byte));
    return [rgb === undefined ? undefined : { rgb }, 4];
  }
  return [undefined, 1];
}

/** `38:5:n`, `38:2:r:g:b`, or `38:2:<colour space>:r:g:b`. */
function extendedColon(sub: string[]): AnsiColor | undefined {
  if (sub[0] === "2" && sub.length >= 5) {
    const rgb = hex(sub.slice(2, 5).map(byte));
    return rgb === undefined ? undefined : { rgb };
  }
  return extended(sub)[0];
}

const ATTRIBUTES_ON: Record<number, keyof AnsiStyle> = {
  1: "bold",
  2: "dim",
  3: "italic",
  4: "underline",
  7: "inverse",
  9: "strike",
};

const ATTRIBUTES_OFF: Record<number, (keyof AnsiStyle)[]> = {
  22: ["bold", "dim"],
  23: ["italic"],
  24: ["underline"],
  27: ["inverse"],
  29: ["strike"],
  39: ["fg"],
  49: ["bg"],
};

function apply(current: AnsiStyle, params: string): AnsiStyle {
  let style: AnsiStyle = { ...current };
  const tokens = params === "" ? ["0"] : params.split(";");
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i] ?? "";
    if (token.includes(":")) {
      const [code, ...sub] = token.split(":");
      const color = extendedColon(sub);
      if (color && code === "38") style.fg = color;
      if (color && code === "48") style.bg = color;
      continue;
    }
    const code = token === "" ? 0 : Number(token);
    const on = ATTRIBUTES_ON[code];
    const off = ATTRIBUTES_OFF[code];
    if (code === 0) {
      style = {};
    } else if (on) {
      style = { ...style, [on]: true };
    } else if (off) {
      for (const key of off) delete style[key];
    } else if (code >= 30 && code <= 37) {
      style.fg = { palette: code - 30 };
    } else if (code >= 40 && code <= 47) {
      style.bg = { palette: code - 40 };
    } else if (code >= 90 && code <= 97) {
      style.fg = { palette: code - 90 + 8 };
    } else if (code >= 100 && code <= 107) {
      style.bg = { palette: code - 100 + 8 };
    } else if (code === 38 || code === 48) {
      const [color, used] = extended(tokens.slice(i + 1));
      i += used;
      if (color && code === 38) style.fg = color;
      if (color && code === 48) style.bg = color;
    }
    // Anything else (blink, overline, fonts, …) is ignored.
  }
  return style;
}

/** Styled segments and the text without SGR sequences. Style starts plain on every line. */
export function parseAnsi(text: string): { plain: string; segments: AnsiSegment[] } {
  if (!text.includes("\u001b")) return { plain: text, segments: [{ text, style: {} }] };
  const segments: AnsiSegment[] = [];
  let style: AnsiStyle = {};
  let last = 0;
  for (const match of text.matchAll(SGR)) {
    if (match.index > last) segments.push({ text: text.slice(last, match.index), style });
    style = apply(style, match[1] ?? "");
    last = match.index + match[0].length;
  }
  if (last < text.length) segments.push({ text: text.slice(last), style });
  return { plain: segments.map((s) => s.text).join(""), segments };
}

const CUBE = [0, 95, 135, 175, 215, 255];

/** The 16 standard colours are theme tokens; the rest follow xterm's palette. */
export function paletteColor(index: number): string {
  if (index < 16) return `var(--ansi-${index})`;
  if (index < 232) {
    const n = index - 16;
    return hex([CUBE[Math.floor(n / 36)], CUBE[Math.floor(n / 6) % 6], CUBE[n % 6]]) ?? "";
  }
  const grey = 8 + (index - 232) * 10;
  return hex([grey, grey, grey]) ?? "";
}

export function colorValue(color: AnsiColor): string {
  return "rgb" in color ? color.rgb : paletteColor(color.palette);
}
