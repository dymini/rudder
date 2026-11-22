import assert from "node:assert/strict";
import { test } from "node:test";
import { parseAnsi, readableColor } from "../src/ansi.ts";

const text = (input) => parseAnsi(input).map((run) => run.text).join("");

test("empty, plain, Unicode and HTML-like output stay literal", () => {
  assert.deepEqual(parseAnsi(""), []);
  const plain = '<script>alert("hello")</script> Tiếng Việt 😀\n\tline';
  assert.deepEqual(parseAnsi(plain), [{ text: plain, style: {} }]);
});

test("Dockside tracing output has styles instead of escape garbage", () => {
  const runs = parseAnsi("\x1b[2m2026-11-20T10:02:28Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mdockside_server\x1b[0m");
  assert.equal(runs.map((run) => run.text).join(""), "2026-11-20T10:02:28Z  INFO dockside_server");
  assert.equal(runs[0].style.dim, true);
  assert.deepEqual(runs[2].style.foreground, [0, 170, 0]);
  assert.deepEqual(runs[3].style, {});
});

test("combined styles persist across lines and reset independently", () => {
  const runs = parseAnsi("\x1b[1;2;3;4;31;44mstyled\nline\x1b[22;23;24;39;49mplain\x1b[1mbold\x1b[mreset");
  assert.deepEqual(runs[0].style, { bold: true, dim: true, italic: true, underline: true, foreground: [170, 0, 0], background: [0, 0, 170] });
  assert.deepEqual(runs[1], { text: "plain", style: {} });
  assert.deepEqual(runs[2].style, { bold: true });
  assert.deepEqual(runs[3].style, {});
});

test("bright, indexed, grayscale, and truecolor SGR", () => {
  const runs = parseAnsi("\x1b[91;104ma\x1b[38;5;196mb\x1b[48;5;232mc\x1b[38;2;12;34;56md\x1b[38:2::45:67:89me");
  assert.deepEqual(runs[0].style.foreground, [255, 85, 85]);
  assert.deepEqual(runs[0].style.background, [85, 85, 255]);
  assert.deepEqual(runs[1].style.foreground, [255, 0, 0]);
  assert.deepEqual(runs[2].style.background, [8, 8, 8]);
  assert.deepEqual(runs[3].style.foreground, [12, 34, 56]);
  assert.deepEqual(runs[4].style.foreground, [45, 67, 89]);
  assert.deepEqual(parseAnsi("\x1b[38:2:0:45:67:89mx")[0].style.foreground, [45, 67, 89]);
  const underline = parseAnsi("\x1b[31;4:3mx\x1b[4:0my");
  assert.deepEqual(underline[0].style, { foreground: [170, 0, 0], underline: true });
  assert.deepEqual(underline[1].style, { foreground: [170, 0, 0] });
});

test("cursor, OSC hyperlinks/titles/clipboard, DCS and charset controls are removed", () => {
  assert.equal(text("a\x1b[2J\x1b[?25l\x1b[2;3Hb\x1b]0;title\x07c\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\d"), "abclinkd");
  assert.equal(text("a\x1b]52;c;c2VjcmV0\x1b\\b\x1bPpayload\x1b\\c\x1bXhidden\x1b\\d\x1b^hidden\x1b\\e\x1b_hidden\x1b\\f\x1b(Bg"), "abcdefg");
  assert.equal(text("a\u009b31mb\u009d0;title\u009cc\u0090payload\u009cd"), "abcd");
  assert.equal(text("a\x00\x07\r\x08\x7fb"), "ab");
});

test("malformed and partial controls are safe, including during live appends", () => {
  for (const suffix of ["\x1b", "\x1b[", "\x1b[38;2;", "\x1b]title", "\x1bPpayload"]) {
    assert.equal(text(`before${suffix}`), "before");
  }
  assert.equal(text("\x1b[38;5;999mvisible\x1b[38;2;1;2mtext"), "visibletext");
  assert.equal(text("\x1b[?31mvisible\x1b[999mtext"), "visibletext");
  assert.deepEqual(parseAnsi("\x1b[31mred")[0].style.foreground, [170, 0, 0]);
  assert.deepEqual(parseAnsi("\x1b[31mred\x1b[0m plain")[1].style, {});
});

test("colors keep at least 4.5:1 contrast on light, dark and explicit backgrounds", () => {
  const luminance = (rgb) => rgb.map((n) => n / 255).map((n) => n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4)
    .reduce((sum, n, i) => sum + n * [0.2126, 0.7152, 0.0722][i], 0);
  for (const background of [[255, 253, 250], [18, 17, 15], [128, 128, 128], [255, 0, 0]]) {
    for (const foreground of [[0, 0, 0], [255, 255, 255], [0, 0, 170], [255, 255, 85], [12, 34, 56]]) {
      const rgb = readableColor(foreground, background).match(/\d+/g).map(Number);
      const fg = luminance(rgb), bg = luminance(background);
      assert.ok((Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05) >= 4.5);
    }
  }
});
