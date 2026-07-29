import assert from "node:assert/strict";
import test from "node:test";

import { appendResultPresentation } from "./result-presentation.js";

class FakeText {
  constructor(ownerDocument, text) {
    this.ownerDocument = ownerDocument;
    this.nodeType = 3;
    this.data = String(text);
  }

  get textContent() {
    return this.data;
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
}

class FakeElement extends FakeNode {
  constructor(ownerDocument, tagName) {
    super(ownerDocument);
    this.nodeType = 1;
    this.tagName = tagName.toUpperCase();
    this.className = "";
    this.textContent = "";
  }

  set textContent(value) {
    this.childNodes = value
      ? [new FakeText(this.ownerDocument, value)]
      : [];
  }

  get textContent() {
    return super.textContent;
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

function createRoot() {
  const documentRef = new FakeDocument();
  return documentRef.createElement("pre");
}

test("result presentation separates source and layout roles", () => {
  const root = createRoot();
  const appended = appendResultPresentation(root, {
    text: "x0 0\n   42",
    highlights: [{ span: [8, 10], kind: "number" }],
    layout: [
      { span: [0, 2], kind: "axis", axis: 0 },
      { span: [3, 4], kind: "index", axis: 0 },
    ],
  });

  assert.equal(appended, true);
  assert.equal(root.textContent, "x0 0\n   42");
  assert.deepEqual(
    root.childNodes
      .filter((node) => node.nodeType === 1)
      .map((node) => [node.className, node.textContent]),
    [
      ["result-layout result-layout-axis result-layout-axis-0", "x0"],
      ["result-layout result-layout-index result-layout-axis-0", "0"],
      ["hl-number", "42"],
    ],
  );
});

test("result presentation remaps spans when continuation lines are indented", () => {
  const root = createRoot();
  appendResultPresentation(
    root,
    {
      text: "1\n🦀",
      highlights: [
        { span: [0, 1], kind: "number" },
        { span: [2, 6], kind: "character" },
      ],
      layout: [],
    },
    { indent: "  ", trailingNewline: true },
  );

  assert.equal(root.textContent, "1\n  🦀\n");
  assert.deepEqual(
    root.childNodes
      .filter((node) => node.nodeType === 1)
      .map((node) => [node.className, node.textContent]),
    [
      ["hl-number", "1"],
      ["hl-character", "🦀"],
    ],
  );
});

test("result presentation supports plain output and legacy fallback", () => {
  const root = createRoot();
  assert.equal(
    appendResultPresentation(root, {
      text: "plain",
      highlights: [],
      layout: [],
    }),
    true,
  );
  assert.equal(root.textContent, "plain");
  assert.equal(root.childNodes.length, 1);
  assert.equal(root.childNodes[0].nodeType, 3);

  assert.equal(appendResultPresentation(root, null), false);
});
