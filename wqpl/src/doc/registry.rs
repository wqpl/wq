use std::collections::BTreeMap;

use super::builtin_topics::builtin_topic;
use super::model::DocTopic;
use super::static_topics;
use crate::builtins::Builtins;

pub fn resolve(query: &str) -> Option<DocTopic> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    if let Some(topic) = resolve_builtin(query) {
        return Some(topic);
    }

    if is_depth_query(query) {
        return static_topics::topic_by_id("at-depth");
    }

    resolve_static(query)
}

pub fn all_topics() -> Vec<DocTopic> {
    let mut topics: Vec<DocTopic> = static_topics::topics().collect();
    topics.extend(Builtins::ENUMS.iter().copied().map(builtin_topic));
    topics
}

pub fn topics_by_group() -> Vec<(String, Vec<DocTopic>)> {
    let mut groups: BTreeMap<String, Vec<DocTopic>> = BTreeMap::new();
    for topic in all_topics() {
        groups.entry(topic.group.clone()).or_default().push(topic);
    }
    groups.into_iter().collect()
}

fn resolve_builtin(query: &str) -> Option<DocTopic> {
    Builtins::new().doc_for_name(query)
}

fn resolve_static(query: &str) -> Option<DocTopic> {
    let query_lower = query.to_ascii_lowercase();
    static_topics::STATIC_DOCS
        .iter()
        .find(|doc| {
            doc.id == query
                || doc.id.eq_ignore_ascii_case(query)
                || doc.title.eq_ignore_ascii_case(query)
                || doc
                    .aliases
                    .iter()
                    .any(|alias| *alias == query || alias.to_ascii_lowercase() == query_lower)
        })
        .map(static_topics::topic)
}

fn is_depth_query(query: &str) -> bool {
    query
        .strip_prefix('@')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}
