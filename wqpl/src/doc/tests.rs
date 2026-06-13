use crate::builtins::BuiltinEnum;
use crate::doc::{DocExample, DocRenderTarget, DocTopic, ExampleExpectation};
use crate::session::Session;

use super::{all_topics, builtin_topic, render_markdown, resolve};
use crate::builtins::Builtins;

#[test]
fn every_builtin_has_a_doc_topic() {
    for builtin in Builtins::ENUMS {
        let topic = builtin_topic(*builtin);
        assert_eq!(topic.builtin, Some(*builtin));
        assert!(
            render_markdown(&topic, DocRenderTarget::Cli).contains(builtin.usage()),
            "rendered doc for {} should use builtin usage metadata",
            builtin.name()
        );
        assert!(
            render_markdown(&topic, DocRenderTarget::Cli).contains(builtin.arity()),
            "rendered doc for {} should use builtin arity metadata",
            builtin.name()
        );
    }
}

#[test]
fn resolves_keywords_and_depth_modifiers() {
    assert_eq!(resolve("@r").expect("@r doc").id, "at-return".to_string());
    assert_eq!(resolve("@12").expect("@12 doc").id, "at-depth".to_string());
    assert_eq!(
        resolve("words").expect("words doc").builtin,
        Some(BuiltinEnum::Words)
    );
}

#[test]
fn executable_examples_stay_in_sync() {
    for topic in all_topics() {
        for example in &topic.examples {
            check_example(&topic, example);
        }
    }
}

fn check_example(topic: &DocTopic, example: &DocExample) {
    match example.expectation {
        ExampleExpectation::NoRun(_) => {}
        ExampleExpectation::Runs => {
            let mut session = Session::new();
            session
                .eval_string(example.code)
                .unwrap_or_else(|err| panic!("{} example failed: {err}", topic.id));
        }
        ExampleExpectation::ResultContains(expected) => {
            let mut session = Session::new();
            let value = session
                .eval_string(example.code)
                .unwrap_or_else(|err| panic!("{} example failed: {err}", topic.id));
            let actual = value.to_string();
            assert!(
                actual.contains(expected),
                "{} example result mismatch: expected {expected:?} in {actual:?}",
                topic.id
            );
        }
        ExampleExpectation::ErrorContains(expected) => {
            let mut session = Session::new();
            let err = session
                .eval_string(example.code)
                .expect_err("example should fail");
            let actual = err.to_string();
            assert!(
                actual.contains(expected),
                "{} example error mismatch: expected {expected:?} in {actual:?}",
                topic.id
            );
        }
        ExampleExpectation::StdoutContains(_) => {
            panic!("stdout expectations need an explicit capture harness")
        }
    }
}
