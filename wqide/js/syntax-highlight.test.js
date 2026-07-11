import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const styles = await readFile(new URL("../styles.css", import.meta.url), "utf8");

test("string escapes have dedicated light and midnight theme colors", () => {
  assert.match(styles, /^\.hl-string-escape\s*\{/m);
  assert.match(
    styles,
    /^:root\[data-theme="midnight"\] \.hl-string-escape\s*\{/m,
  );
});

test("characters and invalid characters have dedicated theme colors", () => {
  assert.match(styles, /^\.hl-character\s*\{/m);
  assert.match(styles, /^\.hl-character-invalid\s*\{/m);
  assert.match(
    styles,
    /^:root\[data-theme="midnight"\] \.hl-character\s*\{/m,
  );
  assert.match(
    styles,
    /^:root\[data-theme="midnight"\] \.hl-character-invalid\s*\{/m,
  );
});
