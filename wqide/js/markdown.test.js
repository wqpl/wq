import assert from "node:assert/strict";
import test from "node:test";

import { parseMarkdown, parseWqExampleDirective } from "./markdown.js";

test("wq example directives stay hidden and attach to the next fence", () => {
  const html = parseMarkdown(`Before.

<!-- wq-example {"id":"sum","cellGroup":"addition","expect":{"value":"3"}} -->

\`\`\`wq
1+2
\`\`\`
`);

  assert.doesNotMatch(html, /<!--/);
  assert.match(html, /class="language-wq"/);
  assert.match(
    html,
    /data-wq-example="\{&quot;id&quot;:&quot;sum&quot;,&quot;cellGroup&quot;:&quot;addition&quot;,&quot;expect&quot;:\{&quot;value&quot;:&quot;3&quot;\}\}"/,
  );
});

test("a directive does not cross intervening prose", () => {
  const html = parseMarkdown(`<!-- wq-example {"id":"unused"} -->
This explains another example.

\`\`\`wq
1+2
\`\`\`
`);

  assert.doesNotMatch(html, /data-wq-example/);
});

test("invalid wq example JSON identifies the directive", () => {
  assert.throws(
    () => parseWqExampleDirective("<!-- wq-example {not-json} -->"),
    /Invalid wq-example directive/,
  );
});

test("wrapped list items remain inside their list item", () => {
  const html = parseMarkdown(`## Keep

- \`name:value\` binds.
- Continue to **Calls, Indexing, and Postfix** to apply functions and
  containers.
`);

  assert.match(
    html,
    /<li>Continue to <strong>Calls, Indexing, and Postfix<\/strong> to apply functions and containers\.<\/li>/,
  );
  assert.doesNotMatch(html, /<p>containers\.<\/p>/);
});
