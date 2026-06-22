import {
  displayReferenceGroup,
  docCardCode,
  docQueryForTopic,
  referenceBuiltinGroupCards,
  referenceRootCards,
} from "./reference-cards.js";

export const FEATURED_SECTION_CARDS = [
  {
    id: "wqpl",
    type: "section",
    meta: "Section",
    title: "The wq Programming Language",
    description: "A short journey through the fundamentals of wq.",
    code: "f:{(f_:{$[x=0;y;f_[x-1;z;y+z]]})[x;0;1]}",
    href: "subfolder.html?section=wqpl",
    label: "Open wqpl folder",
    terms: "tutorials book basics fundamentals arithmetic binding lists dicts functions pipes control flow cas errors primes",
  },
  {
    id: "reference",
    type: "section",
    meta: "Section",
    title: "Reference Docs",
    description: "Generated docs for builtins, syntax, keywords, and guides.",
    code: "map",
    href: "subfolder.html?section=Reference",
    label: "Open Reference docs folder",
    terms: "builtins syntax keywords guides docs help reference",
  },
  {
    id: "misc",
    type: "section",
    meta: "Section",
    title: "Misc",
    description: "Installation, CLI usage, etc.",
    code: "!wqdb",
    href: "subfolder.html?section=Misc",
    label: "Open Misc folder",
    terms: "install installation cli command line wqdb setup",
  },
  {
    id: "wip",
    type: "section",
    meta: "Section",
    title: "WIP",
    description: "Tests and WIP articles.",
    code: "//todo",
    href: "subfolder.html?section=WIP",
    label: "Open WIP folder",
    terms: "tests draft todo markdown test",
  },
];

const RESULT_LIMIT = 24;

function asText(value) {
  if (Array.isArray(value)) return value.map(asText).join(" ");
  if (value == null) return "";
  return String(value);
}

export function normalizeSearchText(value) {
  return asText(value)
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9_?]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function compactRawText(value) {
  return asText(value).toLowerCase().replace(/\s+/g, " ").trim();
}

function searchNeedles(query) {
  const raw = compactRawText(query);
  if (!raw) return { rawNeedles: [], wordNeedles: [] };
  const wordNeedles = normalizeSearchText(raw)
    .split(" ")
    .filter(Boolean);
  const rawNeedles = /\s/.test(raw)
    ? (raw.match(/[@$`\\|+\-*/%^=<>!~.,;:()[\]{}]+/g) || [])
    : /[^a-z0-9_?\s-]/.test(raw)
      ? [raw]
      : [];
  return {
    rawNeedles: Array.from(new Set(rawNeedles)),
    wordNeedles: Array.from(new Set(wordNeedles)),
  };
}

function referenceGroupEntries(docs) {
  return [...referenceRootCards(docs), ...referenceBuiltinGroupCards(docs)].map(
    (entry) => ({
      ...entry,
      id: `reference-group:${entry.href}`,
      terms: [entry.title, entry.description, entry.code, "reference", "docs"],
    }),
  );
}

function tutorialEntries(tutorials) {
  return tutorials.map((tutorial) => ({
    id: `tutorial:${tutorial.slug}`,
    type: "tutorial",
    meta: tutorial.section ? `${tutorial.section} article` : "Article",
    title: tutorial.title,
    description: tutorial.description,
    code: tutorial.code,
    href: `article.html?slug=${encodeURIComponent(tutorial.slug)}`,
    label: `${tutorial.title} lesson`,
    terms: [
      tutorial.slug,
      tutorial.section,
      tutorial.file,
      tutorial.title,
      tutorial.description,
      tutorial.code,
    ],
  }));
}

function referenceTopicEntries(docs) {
  return docs.map((topic) => {
    const query = docQueryForTopic(topic);
    return {
      id: `reference:${query}`,
      type: "reference",
      meta: topic.group
        ? `${displayReferenceGroup(topic.group)} reference`
        : "Reference",
      title: topic.title,
      description: topic.summary ? `${topic.kind}: ${topic.summary}` : topic.kind,
      code: docCardCode(topic),
      href: `article.html?slug=ref:${encodeURIComponent(query)}`,
      label: `${topic.title} reference`,
      terms: [
        topic.id,
        topic.kind,
        topic.group,
        topic.summary,
        topic.aliases,
        query,
        topic.usage,
        topic.kind === "builtin" ? `help ${query}` : "",
      ],
    };
  });
}

function prepareEntry(entry, order) {
  const title = asText(entry.title);
  const code = asText(entry.code);
  const description = asText(entry.description);
  const meta = asText(entry.meta);
  const terms = asText(entry.terms);
  const raw = compactRawText([title, code, description, meta, terms]);
  return {
    ...entry,
    order,
    searchRaw: raw,
    searchWords: normalizeSearchText(raw),
    titleWords: normalizeSearchText(title),
    codeWords: normalizeSearchText(code),
    metaWords: normalizeSearchText(meta),
  };
}

export function buildFeaturedSearchIndex({
  sections = FEATURED_SECTION_CARDS,
  tutorials = [],
  docs = [],
} = {}) {
  return [
    ...sections,
    ...tutorialEntries(tutorials),
    ...referenceGroupEntries(docs),
    ...referenceTopicEntries(docs),
  ].map(prepareEntry);
}

function scoreWord(entry, needle) {
  let score = 0;
  if (entry.titleWords === needle) score += 90;
  else if (entry.titleWords.split(" ").includes(needle)) score += 70;
  else if (entry.titleWords.includes(needle)) score += 48;
  if (entry.codeWords === needle) score += 64;
  else if (entry.codeWords.split(" ").includes(needle)) score += 48;
  else if (entry.codeWords.includes(needle)) score += 32;
  if (entry.metaWords.split(" ").includes(needle)) score += 24;
  if (entry.searchWords.includes(needle)) score += 12;
  return score;
}

function scoreRaw(entry, needle) {
  if (!entry.searchRaw.includes(needle)) return 0;
  if (entry.searchRaw.startsWith(needle)) return 54;
  return 30;
}

export function searchFeaturedItems(index, query, { limit = RESULT_LIMIT } = {}) {
  const { rawNeedles, wordNeedles } = searchNeedles(query);
  if (!rawNeedles.length && !wordNeedles.length) return [];
  const matches = [];
  for (const entry of index) {
    let score = 0;
    let matched = true;
    for (const needle of rawNeedles) {
      const next = scoreRaw(entry, needle);
      if (!next) {
        matched = false;
        break;
      }
      score += next;
    }
    if (!matched) continue;
    for (const needle of wordNeedles) {
      const next = scoreWord(entry, needle);
      if (!next) {
        matched = false;
        break;
      }
      score += next;
    }
    if (!matched) continue;
    matches.push({ ...entry, score });
  }
  matches.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    if (a.type !== b.type) return a.type.localeCompare(b.type);
    if (a.order !== b.order) return a.order - b.order;
    return a.title.localeCompare(b.title);
  });
  return matches.slice(0, limit);
}
