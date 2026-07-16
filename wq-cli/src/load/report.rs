use std::path::PathBuf;

use wqpl::session::EvaluationFailure;
use wqpl::value::Value;

#[derive(Debug, Clone)]
pub struct LoadReport {
    pub label: String,
    pub new_bindings: Vec<String>,
    pub overridden: Vec<String>,
    pub warnings: Vec<String>,
    pub result: Option<Value>,
}

#[derive(Debug)]
pub enum LoadErrorKind {
    Cycle(PathBuf),
    Io(PathBuf, std::io::Error),
    Eval(String, Box<EvaluationFailure>),
    Directive(String),
}

#[derive(Debug)]
pub struct LoadError {
    pub kind: LoadErrorKind,
    pub stack: Vec<String>, // import stack (A -> B -> C)
}

impl LoadError {
    pub fn with_stack(kind: LoadErrorKind, stack: &[String]) -> Self {
        Self {
            kind,
            stack: stack.to_vec(),
        }
    }

    pub fn evaluation_failure(&self) -> Option<&EvaluationFailure> {
        match &self.kind {
            LoadErrorKind::Eval(_, failure) => Some(failure),
            _ => None,
        }
    }
}
