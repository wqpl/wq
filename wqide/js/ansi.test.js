import test from "node:test";
import assert from "node:assert/strict";

import { createAnsiRenderer, createOutputRenderer } from "./ansi.js";

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
    this.style = {};
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

test("styled output text does not interpret ansi escapes", () => {
  const root = createRoot();
  const renderer = createOutputRenderer(root);

  renderer.appendStyledText("\u001b[31mboom\u001b[0m", "error");

  assert.equal(root.textContent, "\u001b[31mboom\u001b[0m");
  assert.equal(root.childNodes.length, 1);
  assert.equal(root.childNodes[0].className, "output-text-error");
  assert.equal(root.childNodes[0].style.color, undefined);
});

test("complete backend output only uses ansi fallback when escapes are present", () => {
  const plainRoot = createRoot();
  const plainRenderer = createOutputRenderer(plainRoot);

  plainRenderer.appendOutput("boom", "error");

  assert.equal(plainRoot.textContent, "boom");
  assert.equal(plainRoot.childNodes[0].className, "output-text-error");

  const ansiRoot = createRoot();
  const ansiRenderer = createOutputRenderer(ansiRoot);

  ansiRenderer.appendOutput("\u001b[31mboom\u001b[0m", "error");

  assert.equal(ansiRoot.textContent, "boom");
  assert.equal(ansiRoot.childNodes[0].className, "");
  assert.equal(ansiRoot.childNodes[0].style.color, "#b03030");
});

test("deprecated ansi renderer remains as a compatibility alias", () => {
  const root = createRoot();
  const renderer = createAnsiRenderer(root);

  renderer.append("\u001b[31mboom\u001b[0m");

  assert.equal(root.textContent, "boom");
  assert.equal(root.childNodes[0].style.color, "#b03030");
});
