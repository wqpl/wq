export const REFERENCE_SECTION = "Reference";
export const BUILTINS_GROUP = "Builtins";
export const GUIDES_GROUP = "Guides";

function topicCountLabel(count) {
  return count === 1 ? "topic" : "topics";
}

function builtinCountLabel(count) {
  return count === 1 ? "builtin" : "builtins";
}

export function displayReferenceGroup(group) {
  return group === REFERENCE_SECTION ? GUIDES_GROUP : group || GUIDES_GROUP;
}

function codeSample(topics, codeForTopic) {
  return topics.slice(0, 3).map(codeForTopic).filter(Boolean).join("  ");
}

function groupTopics(topics, groupForTopic) {
  const groups = new Map();
  for (const topic of topics) {
    const group = groupForTopic(topic);
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group).push(topic);
  }
  return groups;
}

export function docQueryForTopic(topic) {
  if (topic?.kind === "builtin") {
    return topic.aliases?.[0] || topic.title?.replace(/ builtin$/, "") || "";
  }
  return topic?.id || topic?.title || "";
}

export function docCardCode(topic) {
  if (topic?.kind === "builtin") {
    return topic.usage || docQueryForTopic(topic);
  }
  return docQueryForTopic(topic);
}

export function referenceRootCards(topics) {
  const builtins = topics.filter((topic) => topic.kind === "builtin");
  const nonBuiltins = topics.filter((topic) => topic.kind !== "builtin");
  const builtinGroups = groupTopics(
    builtins,
    (topic) => topic.group || BUILTINS_GROUP,
  );
  const cards = [];
  if (builtins.length) {
    const groupNames = Array.from(builtinGroups.keys());
    cards.push({
      type: "reference-group",
      meta: "Reference group",
      title: BUILTINS_GROUP,
      description: `${builtins.length} ${builtinCountLabel(
        builtins.length,
      )} grouped by ${groupNames.slice(0, 3).join(", ")}${
        groupNames.length > 3 ? ", ..." : ""
      }`,
      code: codeSample(builtins, docQueryForTopic),
      href: `subfolder.html?section=${REFERENCE_SECTION}&group=${BUILTINS_GROUP}`,
      label: "Open builtin reference groups",
    });
  }
  for (const [group, items] of groupTopics(
    nonBuiltins,
    (topic) => displayReferenceGroup(topic.group),
  )) {
    cards.push({
      type: "reference-group",
      meta: "Reference group",
      title: group,
      description: `${items.length} ${topicCountLabel(
        items.length,
      )} covering ${items
        .slice(0, 2)
        .map((topic) => topic.title)
        .join(", ")}${items.length > 2 ? ", ..." : ""}`,
      code: codeSample(items, docQueryForTopic),
      href: `subfolder.html?section=${REFERENCE_SECTION}&group=${encodeURIComponent(
        group,
      )}`,
      label: `Open ${group} reference topics`,
    });
  }
  return cards;
}

export function referenceBuiltinGroupCards(topics) {
  const groups = groupTopics(
    topics.filter((topic) => topic.kind === "builtin"),
    (topic) => topic.group || BUILTINS_GROUP,
  );
  return Array.from(groups, ([group, items]) => ({
    type: "reference-group",
    meta: "Builtin group",
    title: group,
    description: `${items.length} ${builtinCountLabel(items.length)} covering ${items
      .slice(0, 2)
      .map((topic) => topic.title)
      .join(", ")}${items.length > 2 ? ", ..." : ""}`,
    code: codeSample(items, docQueryForTopic),
    href: `subfolder.html?section=${REFERENCE_SECTION}&group=${BUILTINS_GROUP}&builtinGroup=${encodeURIComponent(
      group,
    )}`,
    label: `Open ${group} builtins`,
  }));
}

export function referenceTopicCards(topics, { group, builtinGroup } = {}) {
  return topics
    .filter((topic) => {
      if (builtinGroup) {
        return topic.kind === "builtin" && topic.group === builtinGroup;
      }
      return (
        topic.kind !== "builtin" && displayReferenceGroup(topic.group) === group
      );
    })
    .map((topic) => {
      const query = docQueryForTopic(topic);
      return {
        type: "reference",
        meta: topic.group ? `${displayReferenceGroup(topic.group)} reference` : "Reference",
        title: topic.title,
        description: topic.summary ? `${topic.kind}: ${topic.summary}` : topic.kind,
        code: docCardCode(topic),
        href: `article.html?slug=ref:${encodeURIComponent(query)}`,
        label: `${topic.title} reference`,
      };
    });
}

export function referenceSubfolderTitle({ sectionName, referenceGroup, builtinGroup }) {
  if (builtinGroup) return `${builtinGroup} Builtins`;
  if (referenceGroup) return referenceGroup;
  return sectionName;
}

export function referenceSubfolderCrumb({
  sectionName,
  referenceGroup,
  builtinGroup,
}) {
  if (builtinGroup) {
    return `${sectionName}/${BUILTINS_GROUP}/${builtinGroup}`;
  }
  if (referenceGroup) return `${sectionName}/${referenceGroup}`;
  return sectionName;
}

export function referenceParentHref({ referenceGroup, builtinGroup }) {
  if (builtinGroup) {
    return `subfolder.html?section=${REFERENCE_SECTION}&group=${BUILTINS_GROUP}`;
  }
  if (referenceGroup) return `subfolder.html?section=${REFERENCE_SECTION}`;
  return "index.html";
}
