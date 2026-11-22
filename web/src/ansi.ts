export type Rgb = readonly [number, number, number];

export interface AnsiStyle {
  foreground?: Rgb;
  background?: Rgb;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
}

export interface AnsiRun {
  text: string;
  style: AnsiStyle;
}

const palette: readonly Rgb[] = [
  [0, 0, 0], [170, 0, 0], [0, 170, 0], [170, 85, 0],
  [0, 0, 170], [170, 0, 170], [0, 170, 170], [170, 170, 170],
  [85, 85, 85], [255, 85, 85], [85, 255, 85], [255, 255, 85],
  [85, 85, 255], [255, 85, 255], [85, 255, 255], [255, 255, 255],
];

function indexedColor(index: number): Rgb | undefined {
  if (!Number.isInteger(index) || index < 0 || index > 255) return undefined;
  if (index < 16) return palette[index];
  if (index >= 232) {
    const gray = 8 + (index - 232) * 10;
    return [gray, gray, gray];
  }
  const n = index - 16;
  const channel = (value: number) => value === 0 ? 0 : 55 + value * 40;
  return [channel(Math.floor(n / 36)), channel(Math.floor(n / 6) % 6), channel(n % 6)];
}

function sgr(style: AnsiStyle, parameters: string): AnsiStyle {
  const next = { ...style };
  const codes = parameters.split(";").flatMap((parameter) => {
    const values = parameter.split(":").map(Number);
    if (values.length === 1) return values;
    if (values[0] === 38 || values[0] === 48) {
      if (values[1] === 2 && values.length === 6) values.splice(2, 1);
      return values;
    }
    if (values[0] === 4) return [values[1] === 0 ? 24 : 4];
    return [];
  });
  for (let i = 0; i < codes.length; i++) {
    const code = codes[i]!;
    if (code === 0) {
      for (const key of Object.keys(next) as (keyof AnsiStyle)[]) delete next[key];
    } else if (code === 1) next.bold = true;
    else if (code === 2) next.dim = true;
    else if (code === 3) next.italic = true;
    else if (code === 4 || code === 21) next.underline = true;
    else if (code === 22) { delete next.bold; delete next.dim; }
    else if (code === 23) delete next.italic;
    else if (code === 24) delete next.underline;
    else if (code === 39) delete next.foreground;
    else if (code === 49) delete next.background;
    else if (code >= 30 && code <= 37) next.foreground = palette[code - 30];
    else if (code >= 90 && code <= 97) next.foreground = palette[code - 90 + 8];
    else if (code >= 40 && code <= 47) next.background = palette[code - 40];
    else if (code >= 100 && code <= 107) next.background = palette[code - 100 + 8];
    else if (code === 38 || code === 48) {
      const mode = codes[++i];
      let color: Rgb | undefined;
      if (mode === 5) color = indexedColor(codes[++i]!);
      else if (mode === 2) {
        const rgb = codes.slice(i + 1, i + 4);
        i += 3;
        if (rgb.length === 3 && rgb.every((n) => Number.isInteger(n) && n >= 0 && n <= 255)) {
          color = [rgb[0]!, rgb[1]!, rgb[2]!];
        }
      } else break;
      if (color) next[code === 38 ? "foreground" : "background"] = color;
    }
  }
  return next;
}

/** Log text only: SGR styles survive; terminal commands and their payloads do not. */
export function parseAnsi(text: string): AnsiRun[] {
  const runs: AnsiRun[] = [];
  let style: AnsiStyle = {};
  let plain = "";
  const flush = () => {
    if (plain) runs.push({ text: plain, style });
    plain = "";
  };
  for (let i = 0; i < text.length;) {
    const code = text.charCodeAt(i);
    if (code === 27 || (code >= 0x80 && code <= 0x9f)) {
      flush();
      const command = code === 27 ? text[++i] : String.fromCharCode(code - 0x40);
      i++;
      if (command === "[") {
        const start = i;
        while (i < text.length && !(text.charCodeAt(i) >= 0x40 && text.charCodeAt(i) <= 0x7e)) i++;
        const parameters = text.slice(start, i);
        if (text[i] === "m" && /^[\d;:]*$/.test(parameters)) style = sgr(style, parameters);
        if (i < text.length) i++;
      } else if (command && "]PX^_".includes(command)) {
        while (i < text.length) {
          if (text.charCodeAt(i) === 0x9c || (command === "]" && text.charCodeAt(i) === 7)) { i++; break; }
          if (text[i] === "\x1b" && text[i + 1] === "\\") { i += 2; break; }
          i++;
        }
      } else if (code === 27) {
        // ESC intermediates (e.g. charset selection) end at the next final byte.
        while (i <= text.length && text.charCodeAt(i - 1) >= 0x20 && text.charCodeAt(i - 1) <= 0x2f) i++;
      }
    } else {
      if (code === 9 || code === 10 || (code >= 32 && code !== 127)) plain += text[i];
      i++;
    }
  }
  flush();
  return runs;
}

function luminance(rgb: Rgb): number {
  const [r, g, b] = rgb.map((n) => {
    const v = n / 255;
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  return r! * 0.2126 + g! * 0.7152 + b! * 0.0722;
}

/** Keep even black/white and truecolor output legible on the viewer's background. */
export function readableColor(foreground: Rgb, background: Rgb): string {
  const bg = luminance(background);
  const contrast = (rgb: Rgb) => {
    const fg = luminance(rgb);
    return (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05);
  };
  const target = bg > 0.179 ? 0 : 255;
  let color = foreground;
  for (let step = 0; step <= 20; step++) {
    const channel = (n: number) => Math.round(n + (target - n) * step / 20);
    color = [channel(foreground[0]), channel(foreground[1]), channel(foreground[2])];
    if (contrast(color) >= 4.5) break;
  }
  return `rgb(${color.join(", ")})`;
}
