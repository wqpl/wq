import assert from "node:assert/strict";
import test from "node:test";

import {
  createHighlightedSourceFragment,
  highlightedSourceLineFragments,
  normalizeHighlightSpans,
} from "./syntax-highlight.js";

class FakeText {
  constructor(ownerDocument, text) {
    this.ownerDocument = ownerDocument;
    this.nodeType = 3;
    this.data = String(text);
  }

  get textContent() {
    return this.data;
  }

  set textContent(value) {
    this.data = String(value);
  }
}

class FakeNode {
  constructor(ownerDocument) {
    this.ownerDocument = ownerDocument;
    this.childNodes = [];
  }

  appendChild(child) {
    if (child.nodeType === 11) {
      const children = [...child.childNodes];
      child.childNodes = [];
      for (const grandchild of children) {
        this.appendChild(grandchild);
      }
      return child;
    }
    this.childNodes.push(child);
    return child;
  }

  get textContent() {
    return this.childNodes.map((child) => child.textContent).join("");
  }

  set textContent(value) {
    const text = String(value);
    this.childNodes = text ? [new FakeText(this.ownerDocument, text)] : [];
  }
}

class FakeElement extends FakeNode {
  constructor(ownerDocument, tagName) {
    super(ownerDocument);
    this.nodeType = 1;
    this.tagName = tagName.toUpperCase();
    this.className = "";
  }
}

class FakeDocumentFragment extends FakeNode {
  constructor(ownerDocument) {
    super(ownerDocument);
    this.nodeType = 11;
  }
}

class FakeDocument {
  createTextNode(text) {
    return new FakeText(this, text);
  }

  createElement(tagName) {
    return new FakeElement(this, tagName);
  }

  createDocumentFragment() {
    return new FakeDocumentFragment(this);
  }
}

function frontend(spans) {
  return {
    highlight_spans() {
      return spans;
    },
  };
}

test("structured source highlighting preserves Unicode text and classes", () => {
  const documentRef = new FakeDocument();
  const source = 'x:"🦀";x+1';
  const fragment = createHighlightedSourceFragment(
    documentRef,
    frontend([
      { span: [0, 1], kind: "variable" },
      { span: [2, 8], kind: "character" },
      { span: [9, 10], kind: "variable" },
      { span: [11, 12], kind: "number" },
    ]),
    source,
  );

  assert.equal(fragment.textContent, source);
  assert.deepEqual(
    fragment.childNodes
      .filter((node) => node.nodeType === 1)
      .map((node) => [node.className, node.textContent]),
    [
      ["hl-variable", "x"],
      ["hl-character", '"🦀"'],
      ["hl-variable", "x"],
      ["hl-number", "1"],
    ],
  );
});

test("structured source highlighting discards invalid spans and clips overlaps", () => {
  assert.deepEqual(
    normalizeHighlightSpans("abc", [
      { span: [0, 2], kind: "variable" },
      { span: [1, 3], kind: "number" },
      { span: [0, 1], kind: "bad class" },
      { span: [2, 2], kind: "string" },
    ]),
    [
      { start: 0, end: 2, kind: "variable" },
      { start: 1, end: 3, kind: "number" },
    ],
  );

  const documentRef = new FakeDocument();
  const fragment = createHighlightedSourceFragment(
    documentRef,
    frontend([
      { span: [0, 2], kind: "variable" },
      { span: [1, 3], kind: "number" },
    ]),
    "abc",
  );
  assert.equal(fragment.textContent, "abc");
  assert.deepEqual(
    fragment.childNodes
      .filter((node) => node.nodeType === 1)
      .map((node) => [node.className, node.textContent]),
    [
      ["hl-variable", "ab"],
      ["hl-number", "c"],
    ],
  );
});

test("line fragments retain multiline highlight roles without newlines", () => {
  const documentRef = new FakeDocument();
  const source = "// first\n// second";
  const fragments = highlightedSourceLineFragments(
    documentRef,
    frontend([{ span: [0, source.length], kind: "comment" }]),
    source,
  );

  assert.deepEqual(
    fragments.map((fragment) => fragment.textContent),
    ["// first", "// second"],
  );
  assert.deepEqual(
    fragments.map((fragment) => fragment.childNodes[0].className),
    ["hl-comment", "hl-comment"],
  );
});

test("highlight failures fall back to an exact plain-text fragment", () => {
  const documentRef = new FakeDocument();
  const previousWarn = console.warn;
  console.warn = () => {};
  try {
    const fragment = createHighlightedSourceFragment(
      documentRef,
      {
        highlight_spans() {
          throw new Error("unavailable");
        },
      },
      "<plain>",
    );

    assert.equal(fragment.textContent, "<plain>");
    assert.equal(fragment.childNodes.length, 1);
    assert.equal(fragment.childNodes[0].nodeType, 3);
  } finally {
    console.warn = previousWarn;
  }
});
