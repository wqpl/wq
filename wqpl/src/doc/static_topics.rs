mod guides;
mod keywords;
mod syntax;

use super::model::{DocTopic, StaticDoc};

pub(super) const STATIC_DOCS: &[StaticDoc] = &[
    guides::BUILTINS,
    guides::OPERATORS,
    guides::WQDB,
    keywords::AT_ASSERT,
    keywords::AT_BREAK,
    keywords::AT_CONTINUE,
    keywords::AT_RETURN,
    keywords::AT_DEBUG,
    keywords::AT_PAUSE,
    keywords::AT_TRY,
    keywords::AT_SYMBOLIC,
    keywords::AT_FSTRING,
    keywords::AT_RAW_STRING,
    keywords::AT_DEPTH,
    syntax::ASSIGNMENT,
    syntax::EQUALITY,
    syntax::LISTS,
    syntax::DICTS,
    syntax::COMMENTS,
    syntax::CALLS,
    syntax::RANGES,
    syntax::INDEX_MUTATION,
    syntax::POSTFIX,
    syntax::FUNCTIONS,
    syntax::NAMED_ARGUMENTS,
    syntax::PIPES,
    syntax::PRECEDENCE,
    syntax::CONDITIONALS,
    syntax::N_LOOP,
    syntax::W_LOOP,
    syntax::BLOCK,
];

pub(super) fn topics() -> impl Iterator<Item = DocTopic> {
    STATIC_DOCS.iter().map(topic)
}

pub(super) fn topic_by_id(id: &str) -> Option<DocTopic> {
    STATIC_DOCS.iter().find(|doc| doc.id == id).map(topic)
}

pub(super) fn topic(doc: &StaticDoc) -> DocTopic {
    DocTopic {
        id: doc.id.to_string(),
        title: doc.title.to_string(),
        kind: doc.kind,
        group: doc.group.to_string(),
        aliases: doc
            .aliases
            .iter()
            .map(|alias| (*alias).to_string())
            .collect(),
        summary: doc.summary.to_string(),
        details: doc.details.to_string(),
        examples: doc.examples.to_vec(),
        related: doc.related.iter().map(|item| (*item).to_string()).collect(),
        builtin: None,
        canonical_builtin: None,
    }
}
