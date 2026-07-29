import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");
const SYNTAX_COLOR_TOKENS = [
  "syntax-comment",
  "syntax-string",
  "syntax-escape",
  "syntax-invalid",
  "syntax-number",
  "syntax-keyword",
  "syntax-debug",
  "syntax-builtin",
  "syntax-variable",
  "syntax-variable-ref",
  "syntax-parameter",
  "syntax-punctuation",
  "syntax-bracket-1",
  "syntax-bracket-2",
  "syntax-bracket-3",
  "syntax-bracket-4",
  "syntax-bracket-5",
  "syntax-bracket-6",
];

function cssBlock(selector) {
  const selectorIndex = styles.indexOf(selector);
  assert.notEqual(selectorIndex, -1, `missing CSS selector ${selector}`);
  const openIndex = styles.indexOf("{", selectorIndex);
  const closeIndex = styles.indexOf("\n}", openIndex);
  assert.notEqual(closeIndex, -1, `unterminated CSS selector ${selector}`);
  return styles.slice(openIndex + 1, closeIndex);
}

function cssHexToken(block, name) {
  const match = block.match(
    new RegExp(`--${name}:\\s*(#[0-9a-f]{6})\\s*;`, "i"),
  );
  assert.ok(match, `missing CSS token --${name}`);
  return match[1];
}

function relativeLuminance(hex) {
  const channels = hex
    .slice(1)
    .match(/../g)
    .map((channel) => Number.parseInt(channel, 16) / 255)
    .map((channel) =>
      channel <= 0.04045
        ? channel / 12.92
        : ((channel + 0.055) / 1.055) ** 2.4,
    );
  return (
    channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
  );
}

function contrastRatio(foreground, background) {
  const foregroundLuminance = relativeLuminance(foreground);
  const backgroundLuminance = relativeLuminance(background);
  return (
    (Math.max(foregroundLuminance, backgroundLuminance) + 0.05) /
    (Math.min(foregroundLuminance, backgroundLuminance) + 0.05)
  );
}

test("source highlighting uses one shared token map on every surface", () => {
  assert.match(
    styles,
    /\.hl-string-escape\s*\{[^}]*var\(--syntax-escape\)/s,
  );
  assert.match(
    styles,
    /\.hl-character\s*\{[^}]*var\(--syntax-escape\)/s,
  );
  assert.match(
    styles,
    /\.hl-string-invalid\s*\{[^}]*var\(--syntax-invalid\)/s,
  );
  assert.match(
    styles,
    /\.hl-character-invalid\s*\{[^}]*var\(--syntax-invalid\)/s,
  );
  assert.doesNotMatch(styles, /\.repl-flow \.hl-/);
  assert.doesNotMatch(
    styles,
    /:root\[data-theme="midnight"\] \.hl-/,
  );
});

test("light and midnight syntax tokens stay readable on code surfaces", () => {
  const lightPalette = cssBlock(":root {");
  const midnightPalette = cssBlock(':root[data-theme="midnight"] {');

  for (const token of SYNTAX_COLOR_TOKENS) {
    const lightColor = cssHexToken(lightPalette, token);
    const midnightColor = cssHexToken(midnightPalette, token);
    assert.ok(
      contrastRatio(lightColor, "#e8ffe8") >= 4.5,
      `${token} ${lightColor} is not readable on the light code surface`,
    );
    assert.ok(
      contrastRatio(midnightColor, "#11192d") >= 4.5,
      `${token} ${midnightColor} is not readable on the midnight code surface`,
    );
  }
});

test("symbol occurrences share theme tokens for reads and writes", () => {
  assert.match(
    styles,
    /\.wq-symbol-occurrence-read\s*\{[^}]*var\(--syntax-read-bg\)[^}]*var\(--syntax-read-rule\)/s,
  );
  assert.match(
    styles,
    /\.wq-symbol-occurrence-write\s*\{[^}]*var\(--syntax-write-bg\)[^}]*var\(--syntax-write-rule\)/s,
  );
  assert.match(styles, /^\.wq-symbol-occurrence-current\s*\{/m);
});

test("structured result layout reuses semantic ANSI theme colors", () => {
  assert.match(
    styles,
    /\.result-layout-axis-0\s*\{[^}]*var\(--ansi-cyan\)/s,
  );
  assert.match(
    styles,
    /\.result-layout-axis-1\s*\{[^}]*var\(--ansi-yellow\)/s,
  );
  assert.match(
    styles,
    /\.result-layout-axis-2\s*\{[^}]*var\(--ansi-magenta\)/s,
  );
  assert.match(styles, /\.result-layout\s*\{[^}]*opacity:\s*0\.68/s);
});
