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
  assert.equal(ansiRoot.childNodes[0].className, "ansi-fg-red");
  assert.equal(ansiRoot.childNodes[0].style.color, undefined);
});

test("ansi output preserves dim axis styling", () => {
  const root = createRoot();
  const renderer = createOutputRenderer(root);

  renderer.appendOutput("\u001b[2;36mx0\u001b[0m");

  assert.equal(root.textContent, "x0");
  assert.equal(root.childNodes[0].className, "ansi-fg-cyan");
  assert.equal(root.childNodes[0].style.opacity, "0.68");
});

test("standard ansi colors render as themeable foreground and background classes", () => {
  const root = createRoot();
  const renderer = createOutputRenderer(root);
  const names = [
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
  const foregroundCodes = [
    30, 31, 32, 33, 34, 35, 36, 37, 90, 91, 92, 93, 94, 95, 96, 97,
  ];
  const backgroundCodes = [
    40, 41, 42, 43, 44, 45, 46, 47, 100, 101, 102, 103, 104, 105, 106, 107,
  ];

  for (let index = 0; index < names.length; index++) {
    renderer.appendOutput(
      `\u001b[${foregroundCodes[index]};${backgroundCodes[index]}m${index}\u001b[0m`,
    );
  }

  assert.equal(root.childNodes.length, names.length);
  for (let index = 0; index < names.length; index++) {
    assert.equal(
      root.childNodes[index].className,
      `ansi-fg-${names[index]} ansi-bg-${names[index]}`,
    );
  }
});

test("indexed standard colors use css classes while extended colors stay inline", () => {
  const root = createRoot();
  const renderer = createOutputRenderer(root);

  renderer.appendOutput("\u001b[38;5;9mred\u001b[0m");
  renderer.appendOutput("\u001b[38;5;16mblack\u001b[0m");
  renderer.appendOutput("\u001b[38;2;12;34;56mrgb\u001b[0m");

  assert.equal(root.childNodes[0].className, "ansi-fg-bright-red");
  assert.equal(root.childNodes[0].style.color, undefined);
  assert.equal(root.childNodes[1].className, "");
  assert.equal(root.childNodes[1].style.color, "rgb(0, 0, 0)");
  assert.equal(root.childNodes[2].className, "");
  assert.equal(root.childNodes[2].style.color, "rgb(12, 34, 56)");
});

test("inverse ansi colors swap their semantic css roles", () => {
  const root = createRoot();
  const renderer = createOutputRenderer(root);

  renderer.appendOutput("\u001b[31;44;7minverse\u001b[0m");

  assert.equal(
    root.childNodes[0].className,
    "ansi-inverse ansi-bg-red ansi-fg-blue",
  );
  assert.equal(root.childNodes[0].style.color, undefined);
  assert.equal(root.childNodes[0].style.backgroundColor, undefined);
});

test("streamed backend output styles plain errors", () => {
  const root = createRoot();
  const renderer = createOutputRenderer(root);

  renderer.appendStreamOutput("boom", "error");

  assert.equal(root.textContent, "boom");
  assert.equal(root.childNodes[0].className, "output-text-error");
});

test("streamed backend output handles split ansi escapes", () => {
  const root = createRoot();
  const renderer = createOutputRenderer(root);

  renderer.appendStreamOutput("\u001b[1;4");
  renderer.appendStreamOutput("mAST\u001b[0m");

  assert.equal(root.textContent, "AST");
  assert.equal(root.childNodes.length, 1);
  assert.equal(root.childNodes[0].style.fontWeight, "700");
  assert.equal(root.childNodes[0].style.textDecoration, "underline");
});

test("deprecated ansi renderer remains as a compatibility alias", () => {
  const root = createRoot();
  const renderer = createAnsiRenderer(root);

  renderer.append("\u001b[31mboom\u001b[0m");

  assert.equal(root.textContent, "boom");
  assert.equal(root.childNodes[0].className, "ansi-fg-red");
});
