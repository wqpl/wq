import test from "node:test";
import assert from "node:assert/strict";
import {
  BUILTINS_GROUP,
  GUIDES_GROUP,
  referenceBuiltinGroupCards,
  referenceRootCards,
  referenceSubfolderCrumb,
  referenceSubfolderTitle,
  referenceTopicCards,
} from "./reference-cards.js";

const topics = [
  {
    id: "builtins",
    kind: "guide",
    group: "Reference",
    title: "Builtins",
    summary: "Built-in functions are values provided by wq.",
    aliases: ["builtins"],
  },
  {
    id: "operators",
    kind: "guide",
    group: "Reference",
    title: "Operators",
    summary: "Operators are also builtin functions.",
    aliases: ["operators"],
  },
  {
    id: "assignment",
    kind: "syntax",
    group: "Syntax",
    title: "Assignment",
    summary: "Bind values.",
    aliases: [":"],
  },
  {
    id: "builtin.len",
    kind: "builtin",
    group: "Intrinsic",
    title: "len builtin",
    summary: "Return the length of a value.",
    usage: "len[xs]",
    aliases: ["len"],
  },
  {
    id: "builtin.fmt",
    kind: "builtin",
    group: "Intrinsic",
    title: "fmt builtin",
    summary: "Build a string from a template and values.",
    usage: "fmt[template;v*]",
    aliases: ["fmt"],
  },
  {
    id: "builtin.map",
    kind: "builtin",
    group: "Higher-Order",
    title: "map builtin",
    summary: "Apply a function to each item.",
    usage: "map[xs;f;d?]",
    aliases: ["map"],
  },
];

test("reference root has a dedicated Builtins branch", () => {
  const cards = referenceRootCards(topics);
  assert.equal(cards[0].title, BUILTINS_GROUP);
  assert.equal(cards[0].href, "subfolder.html?section=Reference&group=Builtins");
  assert.equal(cards[0].code, "len  fmt  map");
});

test("reference guide group is not named Reference Reference", () => {
  const cards = referenceRootCards(topics);
  const guideCard = cards.find((card) => card.title === GUIDES_GROUP);
  assert.ok(guideCard);
  assert.equal(guideCard.href, "subfolder.html?section=Reference&group=Guides");
  assert.equal(
    referenceSubfolderCrumb({
      sectionName: "Reference",
      referenceGroup: GUIDES_GROUP,
      builtinGroup: "",
    }),
    "Reference/Guides",
  );
});

test("builtin group cards show builtin names without help", () => {
  const cards = referenceBuiltinGroupCards(topics);
  const intrinsic = cards.find((card) => card.title === "Intrinsic");
  assert.ok(intrinsic);
  assert.equal(intrinsic.code, "len  fmt");
  assert.equal(
    intrinsic.href,
    "subfolder.html?section=Reference&group=Builtins&builtinGroup=Intrinsic",
  );
});

test("builtin topic cards show signatures instead of help commands", () => {
  const cards = referenceTopicCards(topics, { builtinGroup: "Intrinsic" });
  assert.deepEqual(
    cards.map((card) => card.code),
    ["len[xs]", "fmt[template;v*]"],
  );
});

test("builtin subgroup title is explicit", () => {
  assert.equal(
    referenceSubfolderTitle({
      sectionName: "Reference",
      referenceGroup: BUILTINS_GROUP,
      builtinGroup: "Intrinsic",
    }),
    "Intrinsic Builtins",
  );
});
