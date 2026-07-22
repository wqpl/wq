use std::sync::Arc;

use indexmap::IndexMap;

use crate::builtins::{BuiltinEnum, BuiltinFnArgs};
use crate::value::bc::BcError;
use crate::value::seq::ValueSeq;
use crate::value::{Value, WqResult};
use crate::wqerror::{Requirement, WqError, WqErrorType};

pub(crate) enum BuiltinFrameAction {
    Continue,
    Call {
        func: Value,
        args: BuiltinFnArgs,
    },
    HostComplete {
        text: String,
        stderr: bool,
        status: Option<i32>,
    },
    Ready(Value),
}

pub(crate) struct BuiltinFrame {
    pub(crate) id: u16,
    pub(crate) argc: usize,
    pub(crate) discard: bool,
    pub(crate) owner_call_depth: usize,
    state: BuiltinFrameState,
    waiting: CallbackWait,
}

enum BuiltinFrameState {
    Apply(ApplyFrame),
    Map(MapFrame),
    FoldScan(FoldScanFrame),
    Filter(FilterFrame),
    Predicate(PredicateFrame),
    Zip(ZipFrame),
    Split(SplitFrame),
    Find(FindFrame),
    Argparse(crate::builtins::cli::ArgparseFrame),
    Cliargs(crate::builtins::cli::CliargsFrame),
    Asciiplot(Box<crate::builtins::viz::asciiplot::AsciiplotFrame>),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CallbackWait {
    None,
    UserFunction,
    InputResult,
}

impl BuiltinFrame {
    fn new(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        state: BuiltinFrameState,
    ) -> Self {
        Self {
            id,
            argc,
            discard,
            owner_call_depth,
            state,
            waiting: CallbackWait::None,
        }
    }

    pub(crate) fn apply(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
    ) -> Self {
        Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Apply(ApplyFrame::new(args, discard)),
        )
    }

    pub(crate) fn map(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Map(MapFrame::new(args, discard)?),
        ))
    }

    pub(crate) fn fold_scan(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        builtin: BuiltinEnum,
        args: BuiltinFnArgs,
    ) -> Self {
        Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::FoldScan(FoldScanFrame::new(builtin, args, discard)),
        )
    }

    pub(crate) fn filter(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
    ) -> Self {
        Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Filter(FilterFrame::new(args, discard)),
        )
    }

    pub(crate) fn predicate(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        builtin: BuiltinEnum,
        args: BuiltinFnArgs,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Predicate(PredicateFrame::new(builtin, args)?),
        ))
    }

    pub(crate) fn zip(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Zip(ZipFrame::new(args, discard)?),
        ))
    }

    pub(crate) fn split(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Split(SplitFrame::new(args, discard)?),
        ))
    }

    pub(crate) fn find(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        builtin: BuiltinEnum,
        args: BuiltinFnArgs,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Find(FindFrame::new(builtin, args, discard)?),
        ))
    }

    pub(crate) fn argparse(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Argparse(crate::builtins::cli::ArgparseFrame::new(&args)?),
        ))
    }

    pub(crate) fn cliargs(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
        argv: Vec<String>,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Cliargs(crate::builtins::cli::CliargsFrame::new(&args, argv)?),
        ))
    }

    pub(crate) fn asciiplot(
        id: u16,
        argc: usize,
        discard: bool,
        owner_call_depth: usize,
        args: BuiltinFnArgs,
        terminal_size: Option<(usize, usize)>,
        color_mode: crate::style::ColorMode,
    ) -> WqResult<Self> {
        Ok(Self::new(
            id,
            argc,
            discard,
            owner_call_depth,
            BuiltinFrameState::Asciiplot(Box::new(
                crate::builtins::viz::asciiplot::AsciiplotFrame::new(
                    &args,
                    terminal_size,
                    color_mode,
                )?,
            )),
        ))
    }

    pub(crate) fn is_waiting_for_user_function(&self) -> bool {
        self.waiting == CallbackWait::UserFunction
    }

    pub(crate) fn is_waiting_for_input_result(&self) -> bool {
        self.waiting == CallbackWait::InputResult
    }

    pub(crate) fn captures_callback_errors(&self) -> bool {
        matches!(
            self.state,
            BuiltinFrameState::Argparse(_)
                | BuiltinFrameState::Cliargs(_)
                | BuiltinFrameState::Asciiplot(_)
        )
    }

    pub(crate) fn wait_for_user_function(&mut self) {
        debug_assert!(self.waiting == CallbackWait::None);
        self.waiting = CallbackWait::UserFunction;
    }

    pub(crate) fn wait_for_input_result(&mut self) {
        debug_assert!(self.waiting == CallbackWait::None);
        self.waiting = CallbackWait::InputResult;
    }

    pub(crate) fn accept_callback_result(&mut self, value: Value) {
        debug_assert!(self.waiting != CallbackWait::None);
        self.waiting = CallbackWait::None;
        match &mut self.state {
            BuiltinFrameState::Apply(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Map(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::FoldScan(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Filter(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Predicate(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Zip(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Split(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Find(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Argparse(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Cliargs(frame) => frame.accept_callback_result(value),
            BuiltinFrameState::Asciiplot(frame) => frame.accept_callback_result(value),
        }
    }

    pub(crate) fn accept_callback_error(&mut self, error: WqError) {
        debug_assert!(self.waiting == CallbackWait::UserFunction);
        self.waiting = CallbackWait::None;
        match &mut self.state {
            BuiltinFrameState::Argparse(frame) => frame.accept_callback_error(error),
            BuiltinFrameState::Cliargs(frame) => frame.accept_callback_error(error),
            BuiltinFrameState::Asciiplot(frame) => {
                let _ = error;
                frame.accept_callback_error();
            }
            _ => unreachable!("builtin frame does not capture callback errors"),
        }
    }

    pub(crate) fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        debug_assert!(self.waiting == CallbackWait::None);
        match &mut self.state {
            BuiltinFrameState::Apply(frame) => frame.step(),
            BuiltinFrameState::Map(frame) => frame.step(),
            BuiltinFrameState::FoldScan(frame) => frame.step(),
            BuiltinFrameState::Filter(frame) => frame.step(),
            BuiltinFrameState::Predicate(frame) => frame.step(),
            BuiltinFrameState::Zip(frame) => frame.step(),
            BuiltinFrameState::Split(frame) => frame.step(),
            BuiltinFrameState::Find(frame) => frame.step(),
            BuiltinFrameState::Argparse(frame) => frame.step(),
            BuiltinFrameState::Cliargs(frame) => frame.step(),
            BuiltinFrameState::Asciiplot(frame) => frame.step(),
        }
    }

    pub(crate) fn decorate_callback_error(&self, mut error: WqError) -> WqError {
        let (builtin, path) = match &self.state {
            BuiltinFrameState::Map(frame) => (BuiltinEnum::Map, frame.callback_path.as_deref()),
            BuiltinFrameState::Zip(frame) => (BuiltinEnum::ZipW, frame.callback_path.as_deref()),
            _ => return error,
        };
        if let Some(path) = path
            && !path.is_empty()
        {
            let path = path
                .iter()
                .map(|index| format!("[{index}]"))
                .collect::<String>();
            error = error.attach_note(format!("at {path}"));
        }
        error.src(builtin)
    }
}

struct ApplyFrame {
    funcs: Vec<Value>,
    arg: Value,
    next: usize,
    discard: bool,
    results: Vec<Value>,
    callback_result: Option<Value>,
}

impl ApplyFrame {
    fn new(args: BuiltinFnArgs, discard: bool) -> Self {
        let mut args = args.into_iter();
        let funcs = match args.next().expect("apply arity was validated") {
            Value::List(funcs) => funcs.as_ref().clone(),
            func => vec![func],
        };
        let arg = args.next().expect("apply arity was validated");
        Self {
            funcs,
            arg,
            next: 0,
            discard,
            results: Vec::new(),
            callback_result: None,
        }
    }

    fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(value);
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(value) = self.callback_result.take() {
            if !self.discard {
                self.results.push(value);
            }
            return Ok(BuiltinFrameAction::Continue);
        }
        if let Some(func) = self.funcs.get(self.next).cloned() {
            self.next += 1;
            return Ok(BuiltinFrameAction::Call {
                func,
                args: BuiltinFnArgs::from(self.arg.clone()),
            });
        }
        Ok(BuiltinFrameAction::Ready(if self.discard {
            Value::empty_list()
        } else {
            Value::from_items(std::mem::take(&mut self.results))
        }))
    }
}

enum ItemSource {
    Sequence(Value),
    Dict(Arc<IndexMap<Arc<str>, Value>>),
}

impl ItemSource {
    fn from_value(value: Value) -> Result<Self, Value> {
        if ValueSeq::from_value(&value).is_some() {
            Ok(Self::Sequence(value))
        } else if let Value::Dict(values) = value {
            Ok(Self::Dict(values))
        } else {
            Err(value)
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Sequence(value) => ValueSeq::from_value(value)
                .expect("sequence source should remain a sequence")
                .len(),
            Self::Dict(values) => values.len(),
        }
    }

    fn get(&self, index: usize) -> Option<Value> {
        match self {
            Self::Sequence(value) => ValueSeq::from_value(value)?.get(index),
            Self::Dict(values) => values.get_index(index).map(|(_, value)| value.clone()),
        }
    }

    fn key(&self, index: usize) -> Option<Arc<str>> {
        match self {
            Self::Sequence(_) => None,
            Self::Dict(values) => values.get_index(index).map(|(key, _)| Arc::clone(key)),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FoldScanKind {
    Fold,
    Scan,
    RScan,
}

struct FoldScanFrame {
    kind: FoldScanKind,
    source: Option<ItemSource>,
    func: Value,
    next: Option<usize>,
    acc: Option<Value>,
    results: Vec<Value>,
    callback_result: Option<Value>,
    ready: Option<Value>,
    discard: bool,
}

impl FoldScanFrame {
    fn new(builtin: BuiltinEnum, args: BuiltinFnArgs, discard: bool) -> Self {
        let kind = match builtin {
            BuiltinEnum::Fold => FoldScanKind::Fold,
            BuiltinEnum::Scan => FoldScanKind::Scan,
            BuiltinEnum::RScan => FoldScanKind::RScan,
            _ => unreachable!("fold/scan frame requires a fold or scan builtin"),
        };
        let mut args = args.into_iter();
        let xs = args.next().expect("fold/scan arity was validated");
        let func = args.next().expect("fold/scan arity was validated");
        let initial = args.next();
        let (source, ready) = match ItemSource::from_value(xs) {
            Ok(source) => (Some(source), None),
            Err(atom) => (None, Some(atom)),
        };
        let mut frame = Self {
            kind,
            source,
            func,
            next: None,
            acc: initial,
            results: Vec::new(),
            callback_result: None,
            ready,
            discard,
        };
        if let Some(source) = &frame.source {
            let len = source.len();
            if frame.acc.is_none() && len > 0 {
                let first = if kind == FoldScanKind::RScan {
                    len - 1
                } else {
                    0
                };
                frame.acc = source.get(first);
                if kind != FoldScanKind::Fold {
                    frame
                        .results
                        .push(frame.acc.clone().expect("initial item should exist"));
                }
                frame.next = if kind == FoldScanKind::RScan {
                    first.checked_sub(1)
                } else if first + 1 < len {
                    Some(first + 1)
                } else {
                    None
                };
            } else if frame.acc.is_some() && len > 0 {
                frame.next = Some(if kind == FoldScanKind::RScan {
                    len - 1
                } else {
                    0
                });
            }
        }
        frame
    }

    fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(value);
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(value) = self.callback_result.take() {
            self.acc = Some(value.clone());
            if self.kind != FoldScanKind::Fold && !self.discard {
                self.results.push(value);
            }
            return Ok(BuiltinFrameAction::Continue);
        }
        if let Some(value) = self.ready.take() {
            return Ok(BuiltinFrameAction::Ready(if self.discard {
                Value::empty_list()
            } else {
                value
            }));
        }
        if let Some(index) = self.next {
            let source = self
                .source
                .as_ref()
                .expect("active fold should have a source");
            let item = source.get(index).expect("fold index should be in bounds");
            self.next = if self.kind == FoldScanKind::RScan {
                index.checked_sub(1)
            } else if index + 1 < source.len() {
                Some(index + 1)
            } else {
                None
            };
            let mut args = BuiltinFnArgs::new();
            args.push(
                self.acc
                    .clone()
                    .expect("active fold should have an accumulator"),
            );
            args.push(item);
            return Ok(BuiltinFrameAction::Call {
                func: self.func.clone(),
                args,
            });
        }

        let value = if self.discard {
            Value::empty_list()
        } else if self.kind == FoldScanKind::Fold {
            self.acc.take().unwrap_or_else(Value::empty_list)
        } else {
            if self.kind == FoldScanKind::RScan {
                self.results.reverse();
            }
            Value::from_items(std::mem::take(&mut self.results))
        };
        Ok(BuiltinFrameAction::Ready(value))
    }
}

struct FilterFrame {
    source: Option<ItemSource>,
    func: Value,
    next: usize,
    current: Option<(Option<Arc<str>>, Value)>,
    callback_result: Option<Value>,
    kept: Vec<(Option<Arc<str>>, Value)>,
    ready: Option<Value>,
    discard: bool,
}

impl FilterFrame {
    fn new(args: BuiltinFnArgs, discard: bool) -> Self {
        let mut args = args.into_iter();
        let xs = args.next().expect("filter arity was validated");
        let func = args.next().expect("filter arity was validated");
        let (source, ready) = match ItemSource::from_value(xs) {
            Ok(source) => (Some(source), None),
            Err(atom) => (None, Some(atom)),
        };
        Self {
            source,
            func,
            next: 0,
            current: None,
            callback_result: None,
            kept: Vec::new(),
            ready,
            discard,
        }
    }

    fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(value);
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(predicate) = self.callback_result.take() {
            let keep = predicate_bool(BuiltinEnum::Filter, predicate)?;
            let current = self
                .current
                .take()
                .expect("filter callback should have an item");
            if keep && !self.discard {
                self.kept.push(current);
            }
            return Ok(BuiltinFrameAction::Continue);
        }
        if let Some(value) = self.ready.take() {
            return Ok(BuiltinFrameAction::Ready(if self.discard {
                Value::empty_list()
            } else {
                value
            }));
        }
        let source = self.source.as_ref().expect("filter source should exist");
        if let Some(item) = source.get(self.next) {
            let key = source.key(self.next);
            self.next += 1;
            self.current = Some((key, item.clone()));
            return Ok(BuiltinFrameAction::Call {
                func: self.func.clone(),
                args: BuiltinFnArgs::from(item),
            });
        }
        let value = if self.discard {
            Value::empty_list()
        } else if matches!(source, ItemSource::Dict(_)) {
            Value::Dict(Arc::new(
                std::mem::take(&mut self.kept)
                    .into_iter()
                    .map(|(key, value)| (key.expect("dict item should have a key"), value))
                    .collect(),
            ))
        } else {
            Value::from_items(
                std::mem::take(&mut self.kept)
                    .into_iter()
                    .map(|(_, value)| value)
                    .collect(),
            )
        };
        Ok(BuiltinFrameAction::Ready(value))
    }
}

struct PredicateFrame {
    builtin: BuiltinEnum,
    func: Value,
    max_depth: i64,
    tasks: Vec<PredicateTask>,
    callback_result: Option<Value>,
}

enum PredicateTask {
    Visit {
        value: Value,
        depth: i64,
    },
    Sequence {
        value: Value,
        depth: i64,
        next: usize,
        len: usize,
    },
    Dict {
        values: Arc<IndexMap<Arc<str>, Value>>,
        depth: i64,
        next: usize,
    },
}

impl PredicateFrame {
    fn new(builtin: BuiltinEnum, args: BuiltinFnArgs) -> WqResult<Self> {
        let mut args = args.into_iter();
        let xs = args.next().expect("predicate arity was validated");
        let func = args.next().expect("predicate arity was validated");
        let depth = args.next().unwrap_or(Value::Int(1));
        let max_depth = crate::builtins::ho::predicate_effective_layers(builtin, &xs, &depth)?;
        Ok(Self {
            builtin,
            func,
            max_depth,
            tasks: vec![PredicateTask::Visit {
                value: xs,
                depth: 0,
            }],
            callback_result: None,
        })
    }

    fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(value);
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(predicate) = self.callback_result.take() {
            let value = predicate_bool(self.builtin, predicate)?;
            if (self.builtin == BuiltinEnum::Any && value)
                || (self.builtin == BuiltinEnum::All && !value)
            {
                self.tasks.clear();
                return Ok(BuiltinFrameAction::Ready(Value::Bool(value)));
            }
            return Ok(BuiltinFrameAction::Continue);
        }
        let Some(task) = self.tasks.pop() else {
            return Ok(BuiltinFrameAction::Ready(Value::Bool(
                self.builtin == BuiltinEnum::All,
            )));
        };
        match task {
            PredicateTask::Visit { value, depth } if value.is_atom() || depth >= self.max_depth => {
                Ok(BuiltinFrameAction::Call {
                    func: self.func.clone(),
                    args: BuiltinFnArgs::from(value),
                })
            }
            PredicateTask::Visit { value, depth } => {
                if let Some(seq) = ValueSeq::from_value(&value) {
                    let len = seq.len();
                    self.tasks.push(PredicateTask::Sequence {
                        value,
                        depth,
                        next: 0,
                        len,
                    });
                } else if let Value::Dict(values) = value {
                    self.tasks.push(PredicateTask::Dict {
                        values,
                        depth,
                        next: 0,
                    });
                } else {
                    unreachable!("predicate traversal should stop at non-container values")
                }
                Ok(BuiltinFrameAction::Continue)
            }
            PredicateTask::Sequence {
                value,
                depth,
                next,
                len,
            } if next < len => {
                let item = ValueSeq::from_value(&value)
                    .and_then(|seq| seq.get(next))
                    .expect("predicate sequence index should be in bounds");
                self.tasks.push(PredicateTask::Sequence {
                    value,
                    depth,
                    next: next + 1,
                    len,
                });
                self.tasks.push(PredicateTask::Visit {
                    value: item,
                    depth: depth + 1,
                });
                Ok(BuiltinFrameAction::Continue)
            }
            PredicateTask::Sequence { .. } => Ok(BuiltinFrameAction::Continue),
            PredicateTask::Dict {
                values,
                depth,
                next,
            } if next < values.len() => {
                let item = values
                    .get_index(next)
                    .expect("predicate dict index should be in bounds")
                    .1
                    .clone();
                self.tasks.push(PredicateTask::Dict {
                    values,
                    depth,
                    next: next + 1,
                });
                self.tasks.push(PredicateTask::Visit {
                    value: item,
                    depth: depth + 1,
                });
                Ok(BuiltinFrameAction::Continue)
            }
            PredicateTask::Dict { .. } => Ok(BuiltinFrameAction::Continue),
        }
    }
}

fn predicate_bool(builtin: BuiltinEnum, value: Value) -> WqResult<bool> {
    match value {
        Value::Bool(value) => Ok(value),
        other => Err(WqError::new(WqErrorType::Domain)
            .src(builtin)
            .expected(Requirement::BOOL)
            .got1(&other)),
    }
}

struct ZipFrame {
    func: Value,
    max_depth: i64,
    discard: bool,
    tasks: Vec<ZipTask>,
    results: Vec<Value>,
    callback_result: Option<Value>,
    callback_path: Option<Vec<usize>>,
}

enum ZipTask {
    Visit {
        left: Value,
        right: Value,
        depth: i64,
        path: Vec<usize>,
    },
    Container {
        left: Value,
        right: Value,
        depth: i64,
        path: Vec<usize>,
        next: usize,
        len: usize,
        result_start: usize,
        keys: Option<Vec<Arc<str>>>,
    },
}

impl ZipFrame {
    fn new(args: BuiltinFnArgs, discard: bool) -> WqResult<Self> {
        let mut args = args.into_iter();
        let left = args.next().expect("zipw arity was validated");
        let right = args.next().expect("zipw arity was validated");
        let func = args.next().expect("zipw arity was validated");
        let depth = args.next().unwrap_or(Value::Int(1));
        let max_depth = crate::builtins::ho::zipw_effective_layers(&left, &right, &depth)?;
        Ok(Self {
            func,
            max_depth,
            discard,
            tasks: vec![ZipTask::Visit {
                left,
                right,
                depth: 0,
                path: Vec::new(),
            }],
            results: Vec::new(),
            callback_result: None,
            callback_path: None,
        })
    }

    fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(value);
        self.callback_path = None;
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(value) = self.callback_result.take() {
            if !self.discard {
                self.results.push(value);
            }
            return Ok(BuiltinFrameAction::Continue);
        }
        let Some(task) = self.tasks.pop() else {
            let value = if self.discard {
                Value::empty_list()
            } else {
                self.results
                    .pop()
                    .expect("completed zipw should have one result")
            };
            return Ok(BuiltinFrameAction::Ready(value));
        };
        match task {
            ZipTask::Visit {
                left,
                right,
                depth,
                path,
            } if (left.is_atom() && right.is_atom()) || depth >= self.max_depth => {
                self.callback_path = Some(path);
                let mut args = BuiltinFnArgs::new();
                args.push(left);
                args.push(right);
                Ok(BuiltinFrameAction::Call {
                    func: self.func.clone(),
                    args,
                })
            }
            ZipTask::Visit {
                left,
                right,
                depth,
                path,
            } => {
                let left_len = container_len(&left);
                let right_len = container_len(&right);
                let len = match (left_len, right_len) {
                    (None, Some(len)) | (Some(len), None) => len,
                    (Some(left_len), Some(right_len)) if left_len == right_len => left_len,
                    (Some(left_len), Some(right_len)) => {
                        return Err(BcError::Length {
                            path,
                            left: left_len,
                            right: right_len,
                        }
                        .into_wqerror()
                        .src(BuiltinEnum::ZipW));
                    }
                    (None, None) => unreachable!("zipw atom pair should stop traversal"),
                };
                let keys = match (&left, &right) {
                    (Value::Dict(left), Value::Dict(right)) => {
                        for ((left, _), (right, _)) in left.iter().zip(right.iter()) {
                            if left != right {
                                return Err(BcError::Key {
                                    path,
                                    left: left.to_string(),
                                    right: right.to_string(),
                                }
                                .into_wqerror()
                                .src(BuiltinEnum::ZipW));
                            }
                        }
                        Some(left.keys().cloned().collect())
                    }
                    (Value::Dict(values), _) | (_, Value::Dict(values)) => {
                        Some(values.keys().cloned().collect())
                    }
                    _ => None,
                };
                self.tasks.push(ZipTask::Container {
                    left,
                    right,
                    depth,
                    path,
                    next: 0,
                    len,
                    result_start: self.results.len(),
                    keys,
                });
                Ok(BuiltinFrameAction::Continue)
            }
            ZipTask::Container {
                left,
                right,
                depth,
                path,
                next,
                len,
                result_start,
                keys,
            } if next < len => {
                let left_item = container_get(&left, next).unwrap_or_else(|| left.clone());
                let right_item = container_get(&right, next).unwrap_or_else(|| right.clone());
                let mut child_path = path.clone();
                child_path.push(next);
                self.tasks.push(ZipTask::Container {
                    left,
                    right,
                    depth,
                    path,
                    next: next + 1,
                    len,
                    result_start,
                    keys,
                });
                self.tasks.push(ZipTask::Visit {
                    left: left_item,
                    right: right_item,
                    depth: depth + 1,
                    path: child_path,
                });
                Ok(BuiltinFrameAction::Continue)
            }
            ZipTask::Container {
                result_start, keys, ..
            } => {
                if !self.discard {
                    let values = self.results.drain(result_start..);
                    let value = if let Some(keys) = keys {
                        Value::Dict(Arc::new(keys.into_iter().zip(values).collect()))
                    } else {
                        Value::from_items(values.collect())
                    };
                    self.results.push(value);
                }
                Ok(BuiltinFrameAction::Continue)
            }
        }
    }
}

fn container_len(value: &Value) -> Option<usize> {
    ValueSeq::from_value(value)
        .map(|seq| seq.len())
        .or_else(|| match value {
            Value::Dict(values) => Some(values.len()),
            _ => None,
        })
}

fn container_get(value: &Value, index: usize) -> Option<Value> {
    ValueSeq::from_value(value)
        .and_then(|seq| seq.get(index))
        .or_else(|| match value {
            Value::Dict(values) => values.get_index(index).map(|(_, value)| value.clone()),
            _ => None,
        })
}

#[derive(Clone, Copy)]
enum SplitSourceKind {
    String,
    Packed,
    General,
}

struct SplitFrame {
    source: ItemSource,
    source_kind: SplitSourceKind,
    func: Value,
    limit: usize,
    splits_done: usize,
    next: usize,
    current_item: Option<Value>,
    callback_result: Option<Value>,
    current: Vec<Value>,
    chunks: Vec<Value>,
    discard: bool,
}

impl SplitFrame {
    fn new(args: BuiltinFnArgs, discard: bool) -> WqResult<Self> {
        let maxsplit = crate::builtins::ho::splitw_maxsplit(&args)?;
        let mut args = args.into_iter();
        let value = args.next().expect("splitw arity was validated");
        let func = args.next().expect("splitw arity was validated");
        let source_kind = if value.is_string() {
            SplitSourceKind::String
        } else if matches!(
            value,
            Value::IntList(_) | Value::IntRange(_) | Value::FloatList(_) | Value::BoolList(_)
        ) {
            SplitSourceKind::Packed
        } else {
            SplitSourceKind::General
        };
        let source = ItemSource::from_value(value).map_err(|other| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::SplitW)
                .expected(Requirement::one_of([
                    Requirement::STRING,
                    Requirement::LIST,
                ]))
                .at_arg(0)
                .got1(&other)
        })?;
        Ok(Self {
            source,
            source_kind,
            func,
            limit: maxsplit.unwrap_or(usize::MAX),
            splits_done: 0,
            next: 0,
            current_item: None,
            callback_result: None,
            current: Vec::new(),
            chunks: Vec::new(),
            discard,
        })
    }

    fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(value);
    }

    fn finish_chunk(&mut self) {
        let values = std::mem::take(&mut self.current);
        let chunk = match self.source_kind {
            SplitSourceKind::String => {
                let string = values
                    .into_iter()
                    .map(|value| match value {
                        Value::Char(value) => value,
                        _ => unreachable!("string split should contain chars"),
                    })
                    .collect::<String>();
                crate::value::into_wq_string(string)
            }
            SplitSourceKind::Packed => Value::from_items(values),
            SplitSourceKind::General => Value::List(Arc::new(values)),
        };
        self.chunks.push(chunk);
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(predicate) = self.callback_result.take() {
            let split = predicate_bool(BuiltinEnum::SplitW, predicate)?;
            let item = self
                .current_item
                .take()
                .expect("split callback should have an item");
            if split && self.splits_done < self.limit {
                if !self.discard {
                    self.finish_chunk();
                }
                self.splits_done += 1;
            } else if !self.discard {
                self.current.push(item);
            }
            return Ok(BuiltinFrameAction::Continue);
        }
        if let Some(item) = self.source.get(self.next) {
            self.next += 1;
            self.current_item = Some(item.clone());
            return Ok(BuiltinFrameAction::Call {
                func: self.func.clone(),
                args: BuiltinFnArgs::from(item),
            });
        }
        if self.discard {
            return Ok(BuiltinFrameAction::Ready(Value::empty_list()));
        }
        self.finish_chunk();
        let chunks = std::mem::take(&mut self.chunks);
        Ok(BuiltinFrameAction::Ready(match self.source_kind {
            SplitSourceKind::String => Value::from_items(chunks),
            SplitSourceKind::Packed | SplitSourceKind::General => Value::List(Arc::new(chunks)),
        }))
    }
}

struct FindFrame {
    builtin: BuiltinEnum,
    func: Value,
    threshold: i64,
    max_depth: i64,
    reverse: bool,
    tasks: Vec<FindTask>,
    pending: Option<FindCandidate>,
    callback_result: Option<Value>,
    results: Vec<Value>,
    discard: bool,
}

enum FindTask {
    Explore {
        value: Value,
        depth: i64,
        path: Vec<i64>,
    },
    Candidate(FindCandidate),
}

struct FindCandidate {
    value: Value,
    parent_depth: i64,
    path: Vec<i64>,
}

impl FindFrame {
    fn new(builtin: BuiltinEnum, args: BuiltinFnArgs, discard: bool) -> WqResult<Self> {
        let (threshold, max_depth) = crate::builtins::ho::findw_parameters(&args, builtin)?;
        let mut args = args.into_iter();
        let value = args.next().expect("findw arity was validated");
        let func = args.next().expect("findw arity was validated");
        Ok(Self {
            builtin,
            func,
            threshold,
            max_depth,
            reverse: builtin == BuiltinEnum::RFindW,
            tasks: vec![FindTask::Explore {
                value,
                depth: 0,
                path: Vec::new(),
            }],
            pending: None,
            callback_result: None,
            results: Vec::new(),
            discard,
        })
    }

    fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(value);
    }

    fn threshold_reached(&self) -> bool {
        usize::try_from(self.threshold).is_ok_and(|limit| self.results.len() >= limit)
    }

    fn push_children(&mut self, value: &Value, depth: i64, path: &[i64]) -> bool {
        let len = container_len(value);
        let Some(len) = len else {
            return false;
        };
        let indices: Box<dyn Iterator<Item = usize>> = if self.reverse {
            Box::new(0..len)
        } else {
            Box::new((0..len).rev())
        };
        for index in indices {
            let item = container_get(value, index).expect("findw index should be in bounds");
            let mut item_path = path.to_vec();
            item_path.push(i64::try_from(index).unwrap_or(i64::MAX));
            self.tasks.push(FindTask::Candidate(FindCandidate {
                value: item,
                parent_depth: depth,
                path: item_path,
            }));
        }
        true
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if self.threshold_reached() {
            self.tasks.clear();
        }
        if let Some(predicate) = self.callback_result.take() {
            let matched = predicate_bool(self.builtin, predicate)?;
            let candidate = self
                .pending
                .take()
                .expect("findw callback should have a candidate");
            if matched {
                if !self.discard {
                    self.results.push(Value::IntList(Arc::new(candidate.path)));
                } else {
                    self.results.push(Value::empty_list());
                }
            } else if candidate.parent_depth < self.max_depth {
                self.tasks.push(FindTask::Explore {
                    value: candidate.value,
                    depth: candidate.parent_depth + 1,
                    path: candidate.path,
                });
            }
            return Ok(BuiltinFrameAction::Continue);
        }
        let Some(task) = self.tasks.pop() else {
            return Ok(BuiltinFrameAction::Ready(if self.discard {
                Value::empty_list()
            } else {
                Value::List(Arc::new(std::mem::take(&mut self.results)))
            }));
        };
        match task {
            FindTask::Explore { value, depth, path } => {
                if !self.push_children(&value, depth, &path) {
                    self.tasks.push(FindTask::Candidate(FindCandidate {
                        value,
                        parent_depth: depth,
                        path,
                    }));
                }
                Ok(BuiltinFrameAction::Continue)
            }
            FindTask::Candidate(candidate) => {
                let value = candidate.value.clone();
                self.pending = Some(candidate);
                Ok(BuiltinFrameAction::Call {
                    func: self.func.clone(),
                    args: BuiltinFnArgs::from(value),
                })
            }
        }
    }
}

struct MapFrame {
    func: Value,
    max_depth: i64,
    discard: bool,
    tasks: Vec<MapTask>,
    results: Vec<Value>,
    callback_result: Option<Value>,
    callback_path: Option<Vec<usize>>,
}

enum MapTask {
    Visit {
        value: Value,
        depth: i64,
        path: Vec<usize>,
    },
    Sequence {
        value: Value,
        depth: i64,
        next: usize,
        len: usize,
        result_start: usize,
        path: Vec<usize>,
    },
    Dict {
        values: Arc<IndexMap<Arc<str>, Value>>,
        depth: i64,
        next: usize,
        result_start: usize,
        path: Vec<usize>,
    },
}

impl MapFrame {
    fn new(args: BuiltinFnArgs, discard: bool) -> WqResult<Self> {
        let mut args = args.into_iter();
        let xs = args.next().expect("map arity was validated");
        let func = args.next().expect("map arity was validated");
        let depth = args.next().unwrap_or(Value::Int(1));
        let max_depth = crate::builtins::ho::map_effective_layers(&xs, &depth)?;
        Ok(Self {
            func,
            max_depth,
            discard,
            tasks: vec![MapTask::Visit {
                value: xs,
                depth: 0,
                path: Vec::new(),
            }],
            results: Vec::new(),
            callback_result: None,
            callback_path: None,
        })
    }

    fn accept_callback_result(&mut self, value: Value) {
        debug_assert!(self.callback_result.is_none());
        self.callback_result = Some(value);
        self.callback_path = None;
    }

    fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(value) = self.callback_result.take() {
            if !self.discard {
                self.results.push(value);
            }
            return Ok(BuiltinFrameAction::Continue);
        }

        let Some(task) = self.tasks.pop() else {
            let value = if self.discard {
                Value::empty_list()
            } else {
                self.results
                    .pop()
                    .expect("completed map should have one result")
            };
            return Ok(BuiltinFrameAction::Ready(value));
        };

        match task {
            MapTask::Visit { value, depth, path } if value.is_atom() || depth >= self.max_depth => {
                self.callback_path = Some(path);
                Ok(BuiltinFrameAction::Call {
                    func: self.func.clone(),
                    args: BuiltinFnArgs::from(value),
                })
            }
            MapTask::Visit { value, depth, path } => {
                if let Some(seq) = ValueSeq::from_value(&value) {
                    let len = seq.len();
                    self.tasks.push(MapTask::Sequence {
                        value,
                        depth,
                        next: 0,
                        len,
                        result_start: self.results.len(),
                        path,
                    });
                } else if let Value::Dict(values) = value {
                    self.tasks.push(MapTask::Dict {
                        values,
                        depth,
                        next: 0,
                        result_start: self.results.len(),
                        path,
                    });
                } else {
                    unreachable!("map traversal should stop at non-container values")
                }
                Ok(BuiltinFrameAction::Continue)
            }
            MapTask::Sequence {
                value,
                depth,
                next,
                len,
                result_start,
                path,
            } if next < len => {
                let item = ValueSeq::from_value(&value)
                    .and_then(|seq| seq.get(next))
                    .expect("map sequence index should be in bounds");
                self.tasks.push(MapTask::Sequence {
                    value,
                    depth,
                    next: next + 1,
                    len,
                    result_start,
                    path: path.clone(),
                });
                let mut child_path = path;
                child_path.push(next);
                self.tasks.push(MapTask::Visit {
                    value: item,
                    depth: depth + 1,
                    path: child_path,
                });
                Ok(BuiltinFrameAction::Continue)
            }
            MapTask::Sequence { result_start, .. } => {
                if !self.discard {
                    let children = self.results.drain(result_start..).collect();
                    self.results.push(Value::from_items(children));
                }
                Ok(BuiltinFrameAction::Continue)
            }
            MapTask::Dict {
                values,
                depth,
                next,
                result_start,
                path,
            } if next < values.len() => {
                let item = values
                    .get_index(next)
                    .expect("map dict index should be in bounds")
                    .1
                    .clone();
                self.tasks.push(MapTask::Dict {
                    values,
                    depth,
                    next: next + 1,
                    result_start,
                    path: path.clone(),
                });
                let mut child_path = path;
                child_path.push(next);
                self.tasks.push(MapTask::Visit {
                    value: item,
                    depth: depth + 1,
                    path: child_path,
                });
                Ok(BuiltinFrameAction::Continue)
            }
            MapTask::Dict {
                values,
                result_start,
                ..
            } => {
                if !self.discard {
                    let mapped = self.results.drain(result_start..);
                    let result = values
                        .keys()
                        .cloned()
                        .zip(mapped)
                        .collect::<IndexMap<_, _>>();
                    self.results.push(Value::Dict(Arc::new(result)));
                }
                Ok(BuiltinFrameAction::Continue)
            }
        }
    }
}
