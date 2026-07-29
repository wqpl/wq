import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const styles = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
const rendererSource = readFileSync(new URL("./ansi.js", import.meta.url), "utf8");
const ANSI_COLOR_NAMES = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "bright-black",
  "bright-red",
  "bright-green",
  "bright-yellow",
  "bright-blue",
  "bright-magenta",
  "bright-cyan",
  "bright-white",
];
const ANSI_SYNTAX_ROLES = {
  red: "syntax-invalid",
  green: "syntax-string",
  yellow: "syntax-number",
  blue: "syntax-builtin",
  magenta: "syntax-keyword",
  cyan: "syntax-escape",
  white: "syntax-punctuation",
};

function cssBlock(selector) {
  const selectorIndex = styles.indexOf(selector);
  assert.notEqual(selectorIndex, -1, `missing CSS selector ${selector}`);
  const openIndex = styles.indexOf("{", selectorIndex);
  const closeIndex = styles.indexOf("\n}", openIndex);
  assert.notEqual(closeIndex, -1, `unterminated CSS selector ${selector}`);
  return styles.slice(openIndex + 1, closeIndex);
}

function cssToken(block, name) {
  const match = block.match(
    new RegExp(`--${name}:\\s*([^;]+)\\s*;`, "i"),
  );
  assert.ok(match, `missing CSS token --${name}`);
  return match[1].trim();
}

function resolveCssColor(blocks, name, seen = new Set()) {
  assert.ok(!seen.has(name), `circular CSS token --${name}`);
  seen.add(name);
  const block = blocks.find((candidate) =>
    new RegExp(`--${name}:\\s*`, "i").test(candidate),
  );
  assert.ok(block, `missing CSS token --${name}`);
  const value = cssToken(block, name);
  const variable = /^var\(--([^)]+)\)$/.exec(value);
  if (variable) {
    return resolveCssColor(blocks, variable[1], seen);
  }
  assert.match(value, /^#[0-9a-f]{6}$/i, `--${name} is not a hex color`);
  return value;
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

test("ansi renderer delegates the standard color map to css", () => {
  assert.doesNotMatch(rendererSource, /#[0-9a-f]{3,8}\b/i);

  for (const name of ANSI_COLOR_NAMES) {
    assert.match(styles, new RegExp(`\\.ansi-fg-${name}\\s*\\{`));
    assert.match(styles, new RegExp(`\\.ansi-bg-${name}\\s*\\{`));
  }
});

test("standard ansi hues share the source syntax color roles", () => {
  const palette = cssBlock(":root {");

  for (const [ansiName, syntaxToken] of Object.entries(ANSI_SYNTAX_ROLES)) {
    assert.equal(cssToken(palette, `ansi-${ansiName}`), `var(--${syntaxToken})`);
  }
});

test("light ansi colors remain readable on the lightest output surfaces", () => {
  const palette = cssBlock(":root {");
  const outputBackground = "#e8ffe8";

  for (const name of ANSI_COLOR_NAMES) {
    const color = resolveCssColor([palette], `ansi-${name}`);
    assert.ok(
      contrastRatio(color, outputBackground) >= 4.5,
      `${name} ${color} is not readable on ${outputBackground}`,
    );
  }
});

test("dark ansi colors remain readable on the lightest dark output surface", () => {
  const palette = cssBlock(".viz-output {");
  const outputBackground = "#11191f";

  for (const name of ANSI_COLOR_NAMES) {
    const color = resolveCssColor([palette], `ansi-${name}`);
    assert.ok(
      contrastRatio(color, outputBackground) >= 4.5,
      `${name} ${color} is not readable on ${outputBackground}`,
    );
  }
});
