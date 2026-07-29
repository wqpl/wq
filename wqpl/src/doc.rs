mod builtin_topics;
mod model;
mod registry;
mod render;
mod static_topics;

#[cfg(test)]
mod tests;

pub use builtin_topics::builtin_topic;
pub use model::{DocExample, DocKind, DocRenderTarget, DocTopic, ExampleExpectation};
pub use registry::{all_topics, resolve, topics_by_group};
pub use render::{
    MarkdownRenderOptions, fold_markdown, render_markdown, render_markdown_with_options,
};
