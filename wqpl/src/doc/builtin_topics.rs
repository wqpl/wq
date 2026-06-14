mod aliases;
mod docs;

use super::model::{DocKind, DocTopic};
use crate::builtins::{BUILTIN_GROUPS, BuiltinEnum, BuiltinGroup};

pub fn builtin_topic(builtin: BuiltinEnum) -> DocTopic {
    let canonical = canonical_builtin(builtin);
    let builtin_doc = builtin_doc(canonical);
    let group = builtin_group(builtin)
        .map(BuiltinGroup::name)
        .unwrap_or("Builtin")
        .to_string();
    let alias_summary;
    let summary = if let Some(doc) = builtin_doc {
        if canonical == builtin {
            doc.summary.to_string()
        } else {
            alias_summary = format!("Alias of `{}`. {}", canonical.name(), doc.summary);
            alias_summary
        }
    } else if canonical == builtin {
        format!("Builtin in the {group} group.")
    } else {
        format!("Alias of `{}`.", canonical.name())
    };
    let details = builtin_doc
        .map(|doc| doc.details.to_string())
        .unwrap_or_else(|| "This page is generated from builtin metadata; add a hand-written doc entry when the behavior needs more explanation.".to_string());
    let examples = builtin_doc
        .map(|doc| doc.examples.to_vec())
        .unwrap_or_default();
    let mut related: Vec<String> = builtin_doc
        .map(|doc| doc.related.iter().map(|item| (*item).to_string()).collect())
        .unwrap_or_default();
    if canonical != builtin {
        related.push(canonical.name().to_string());
    }
    DocTopic {
        id: format!("builtin.{}", builtin.name()),
        title: format!("{} builtin", builtin.name()),
        kind: DocKind::Builtin,
        group,
        aliases: vec![builtin.name().to_string()],
        summary,
        details,
        examples,
        related,
        builtin: Some(builtin),
        canonical_builtin: Some(canonical),
    }
}

fn builtin_doc(builtin: BuiltinEnum) -> Option<&'static super::model::BuiltinDoc> {
    docs::BUILTIN_DOCS.iter().find(|doc| doc.builtin == builtin)
}

fn canonical_builtin(builtin: BuiltinEnum) -> BuiltinEnum {
    aliases::BUILTIN_ALIASES
        .iter()
        .find_map(|(alias, canonical)| (*alias == builtin).then_some(*canonical))
        .unwrap_or(builtin)
}

fn builtin_group(builtin: BuiltinEnum) -> Option<BuiltinGroup> {
    BUILTIN_GROUPS.get(builtin.id() as usize).copied()
}
