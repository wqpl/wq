use crate::builtins::BuiltinEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Builtin,
    Keyword,
    Syntax,
    Guide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocRenderTarget {
    Cli,
    Lsp,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleExpectation {
    Runs,
    ResultContains(&'static str),
    ErrorContains(&'static str),
    StdoutContains(&'static str),
    NoRun(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocExample {
    pub title: &'static str,
    pub code: &'static str,
    pub expectation: ExampleExpectation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocTopic {
    pub id: String,
    pub title: String,
    pub kind: DocKind,
    pub group: String,
    pub aliases: Vec<String>,
    pub summary: String,
    pub details: String,
    pub examples: Vec<DocExample>,
    pub related: Vec<String>,
    pub builtin: Option<BuiltinEnum>,
    pub canonical_builtin: Option<BuiltinEnum>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct StaticDoc {
    pub(super) id: &'static str,
    pub(super) title: &'static str,
    pub(super) kind: DocKind,
    pub(super) group: &'static str,
    pub(super) aliases: &'static [&'static str],
    pub(super) summary: &'static str,
    pub(super) details: &'static str,
    pub(super) examples: &'static [DocExample],
    pub(super) related: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BuiltinDoc {
    pub(super) builtin: BuiltinEnum,
    pub(super) summary: &'static str,
    pub(super) details: &'static str,
    pub(super) examples: &'static [DocExample],
    pub(super) related: &'static [&'static str],
}
