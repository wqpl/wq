use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use colored::{ColoredString, Colorize};

use crate::astnode::{BinaryOperator, UnaryOperator};
use crate::builtins::Builtins;
use crate::interpret::vanilla::VanillaInterpreter;
use crate::interpret::{Interpreter, InterpreterHook, InterpreterKind};
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::vm::inst::Instruction;

const COUNT_BAR_WIDTH: usize = 24;
const HIST_BAR_WIDTH: usize = 10;
const RATIO_BAR_WIDTH: usize = 18;

#[derive(Default)]
struct ProfileStats {
    op_counts: HashMap<String, usize>,
    inst_counts: HashMap<String, usize>,
    sequence_outputs: HashMap<String, SequenceOutputStats>,
    total_ops: usize,
    load_var_cache_hits: usize,
    load_var_const_cache_hits: usize,
    load_var_cache_misses: usize,
    call_user_cache_hits: usize,
    call_user_cache_misses: usize,
    cat_alloc_events: usize,
    cat_alloc_items: usize,
    cat_alloc_lens: BTreeMap<usize, usize>,
    list_alloc_events: usize,
    list_alloc_items: usize,
    list_alloc_lens: BTreeMap<usize, usize>,
    dict_alloc_events: usize,
    dict_alloc_items: usize,
    dict_alloc_lens: BTreeMap<usize, usize>,
    range_alloc_events: usize,
    range_alloc_items: usize,
    range_alloc_lens: BTreeMap<usize, usize>,
    closure_capture_alloc_events: usize,
    closure_capture_cells: usize,
    closure_capture_lens: BTreeMap<usize, usize>,
    max_stack_len: usize,
    max_call_depth: usize,
    final_stack_len: usize,
}

#[derive(Default)]
struct SequenceOutputStats {
    events: usize,
    items: usize,
    lens: BTreeMap<usize, usize>,
}

pub(crate) struct ProfilerInterpreter {
    stats: RefCell<ProfileStats>,
    pub(crate) trace: bool,
}

impl Default for ProfilerInterpreter {
    fn default() -> Self {
        Self {
            stats: RefCell::new(ProfileStats::default()),
            trace: false,
        }
    }
}

impl Drop for ProfilerInterpreter {
    fn drop(&mut self) {
        let stats = self.stats.borrow();
        if stats.total_ops == 0 {
            return;
        }

        eprintln!("{}", "\nPROFILE".bold().underline());
        eprintln!(
            "{}: inst={} | max-stack={} | final-stack={} | call-depth={}",
            "Run".underline(),
            stats.total_ops,
            stats.max_stack_len,
            stats.final_stack_len,
            stats.max_call_depth
        );
        print_count_table("Top opcodes", &stats.op_counts, stats.total_ops, 12);
        print_count_table(
            "Top instruction forms",
            &stats.inst_counts,
            stats.total_ops,
            16,
        );

        let total_var_lookups = stats.load_var_cache_hits
            + stats.load_var_const_cache_hits
            + stats.load_var_cache_misses;
        if total_var_lookups > 0 {
            let total_hits = stats.load_var_cache_hits + stats.load_var_const_cache_hits;
            eprintln!(
                "Global cache: {} hits / {} misses ({:.2}%) {}",
                total_hits,
                stats.load_var_cache_misses,
                pct(total_hits, total_var_lookups),
                format_hit_miss_bar(total_hits, total_var_lookups, RATIO_BAR_WIDTH)
            );
            if stats.load_var_const_cache_hits > 0 {
                eprintln!(
                    "const-value hits: {} | slot hits: {}",
                    stats.load_var_const_cache_hits, stats.load_var_cache_hits,
                );
            }
        }

        let total_user_calls = stats.call_user_cache_hits + stats.call_user_cache_misses;
        if total_user_calls > 0 {
            eprintln!(
                "Call cache: {} hits / {} misses ({:.2}%) {}",
                stats.call_user_cache_hits,
                stats.call_user_cache_misses,
                pct(stats.call_user_cache_hits, total_user_calls),
                format_hit_miss_bar(
                    stats.call_user_cache_hits,
                    total_user_calls,
                    RATIO_BAR_WIDTH
                )
            );
        }

        let total_alloc_events = stats.list_alloc_events
            + stats.dict_alloc_events
            + stats.range_alloc_events
            + stats.closure_capture_alloc_events
            + stats.cat_alloc_events;
        let total_alloc_units = stats.list_alloc_items
            + stats.dict_alloc_items
            + stats.range_alloc_items
            + stats.closure_capture_cells
            + stats.cat_alloc_items;
        if total_alloc_events > 0 {
            eprintln!(
                "{}: {} events, {} aggregate items",
                "Instruction allocations".underline(),
                total_alloc_events,
                total_alloc_units
            );
            print_alloc_line(
                "cat operands",
                stats.cat_alloc_events,
                stats.cat_alloc_items,
                &stats.cat_alloc_lens,
            );
            print_alloc_line(
                "list",
                stats.list_alloc_events,
                stats.list_alloc_items,
                &stats.list_alloc_lens,
            );
            print_alloc_line(
                "dict",
                stats.dict_alloc_events,
                stats.dict_alloc_items,
                &stats.dict_alloc_lens,
            );
            print_alloc_line(
                "range",
                stats.range_alloc_events,
                stats.range_alloc_items,
                &stats.range_alloc_lens,
            );
            print_alloc_line(
                "closure captures",
                stats.closure_capture_alloc_events,
                stats.closure_capture_cells,
                &stats.closure_capture_lens,
            );
        }

        print_sequence_outputs(&stats.sequence_outputs);
    }
}

impl Interpreter for ProfilerInterpreter {
    fn interpret(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        let mut delegate = VanillaInterpreter;
        let previous_interpreter = vm.interpreter_kind;
        vm.interpreter_kind = InterpreterKind::Vanilla;
        vm.set_hooks(Some(self));
        let result = delegate.interpret(vm, limit);
        self.stats.borrow_mut().final_stack_len = vm.stack.len();
        vm.set_hooks(None);
        vm.interpreter_kind = previous_interpreter;
        result
    }
}

impl InterpreterHook for ProfilerInterpreter {
    fn before_instruction(&self, vm: &Vm, idx: usize, op: &Instruction) {
        let call_depth = vm.locals.len();
        let op_name = instruction_kind(op);
        let inst_key = instruction_profile_key(op);
        let mut stats = self.stats.borrow_mut();
        *stats.op_counts.entry(op_name.to_string()).or_insert(0) += 1;
        *stats.inst_counts.entry(inst_key).or_insert(0) += 1;
        stats.total_ops += 1;
        stats.max_stack_len = stats.max_stack_len.max(vm.stack.len());
        stats.max_call_depth = stats.max_call_depth.max(call_depth);

        fn colorize_number(n: usize, width: usize) -> ColoredString {
            let s = format!("{n:0width$}");
            match n % 6 {
                0 => s.normal(),
                1 => s.red(),
                2 => s.yellow(),
                3 => s.green(),
                4 => s.blue(),
                _ => s.magenta(),
            }
        }

        if self.trace {
            let op_str = crate::vm::inst::InstPrettyDumper::new(true, true).highlight_inst(op);
            eprintln!(
                "pc: {idx:04} | c-depth: {} | stack: {} | inst: {op_str:<25} ",
                colorize_number(call_depth, 2),
                colorize_number(vm.stack.len(), 1),
            );
        }
    }

    fn on_load_var_cache_hit(&self, slot_cached: &dyn Fn() -> bool) {
        let mut stats = self.stats.borrow_mut();
        if slot_cached() {
            stats.load_var_cache_hits += 1;
        } else {
            stats.load_var_const_cache_hits += 1;
        }
    }

    fn on_load_var_cache_miss(&self) {
        self.stats.borrow_mut().load_var_cache_misses += 1;
    }

    fn on_call_user_cache_hit(&self) {
        self.stats.borrow_mut().call_user_cache_hits += 1;
    }

    fn on_call_user_cache_miss(&self) {
        self.stats.borrow_mut().call_user_cache_misses += 1;
    }

    fn on_binary_result(&self, op: &BinaryOperator, result: &Value) {
        let label = format!("binary {op:?}");
        self.stats
            .borrow_mut()
            .record_sequence_output(label, result);
    }

    fn on_unary_result(&self, op: &UnaryOperator, result: &Value) {
        let label = format!("unary {op:?}");
        self.stats
            .borrow_mut()
            .record_sequence_output(label, result);
    }

    fn on_builtin_result(&self, name: &str, argc: usize, result: &Value) {
        let label = format!("builtin {name}/{argc}");
        self.stats
            .borrow_mut()
            .record_sequence_output(label, result);
    }

    fn on_cat_alloc(&self, len: &dyn Fn() -> usize) {
        let len = len();
        let mut stats = self.stats.borrow_mut();
        stats.cat_alloc_events += 1;
        stats.cat_alloc_items += len;
        *stats.cat_alloc_lens.entry(len).or_insert(0) += 1;
    }

    fn on_list_alloc(&self, len: &dyn Fn() -> usize) {
        let len = len();
        let mut stats = self.stats.borrow_mut();
        stats.list_alloc_events += 1;
        stats.list_alloc_items += len;
        *stats.list_alloc_lens.entry(len).or_insert(0) += 1;
    }

    fn on_dict_alloc(&self, len: &dyn Fn() -> usize) {
        let len = len();
        let mut stats = self.stats.borrow_mut();
        stats.dict_alloc_events += 1;
        stats.dict_alloc_items += len;
        *stats.dict_alloc_lens.entry(len).or_insert(0) += 1;
    }

    fn on_range_alloc(&self, len: &dyn Fn() -> usize) {
        let len = len();
        let mut stats = self.stats.borrow_mut();
        stats.range_alloc_events += 1;
        stats.range_alloc_items += len;
        *stats.range_alloc_lens.entry(len).or_insert(0) += 1;
    }

    fn on_closure_capture_alloc(&self, len: &dyn Fn() -> usize) {
        let len = len();
        let mut stats = self.stats.borrow_mut();
        stats.closure_capture_alloc_events += 1;
        stats.closure_capture_cells += len;
        *stats.closure_capture_lens.entry(len).or_insert(0) += 1;
    }

    fn on_return(&self, vm: &Vm) {
        self.stats.borrow_mut().final_stack_len = vm.stack.len();
    }
}

impl ProfileStats {
    fn record_sequence_output(&mut self, producer: String, value: &Value) {
        let Some(kind) = sequence_kind(value) else {
            return;
        };
        let len = value.len();
        let key = format!("{producer} -> {kind}");
        let stats = self.sequence_outputs.entry(key).or_default();
        stats.events += 1;
        stats.items += len;
        *stats.lens.entry(len).or_insert(0) += 1;
    }
}

#[inline]
fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        (n as f64 / d as f64) * 100.0
    }
}

fn print_count_table(title: &str, counts: &HashMap<String, usize>, total: usize, limit: usize) {
    if counts.is_empty() {
        return;
    }
    let mut sorted: Vec<_> = counts.iter().collect();
    sorted.sort_by(|(a_name, a_count), (b_name, b_count)| {
        b_count.cmp(a_count).then_with(|| a_name.cmp(b_name))
    });
    eprintln!("{}", title.underline());
    let max_count = sorted.first().map(|(_, count)| **count).unwrap_or(0);
    for (name, count) in sorted.into_iter().take(limit) {
        eprintln!(
            "{:>6} {:>6.2}% {} {}",
            count,
            pct(*count, total),
            format_count_bar(*count, max_count, COUNT_BAR_WIDTH),
            name
        );
    }
}

fn print_alloc_line(label: &str, events: usize, units: usize, lens: &BTreeMap<usize, usize>) {
    if events == 0 {
        return;
    }
    eprintln!(
        "  {:<16} {:>6} events | {:>8} items | avg {:>6.2} | {}",
        label,
        events,
        units,
        units as f64 / events as f64,
        format_len_hist(lens, 5),
    );
}

fn print_sequence_outputs(outputs: &HashMap<String, SequenceOutputStats>) {
    if outputs.is_empty() {
        return;
    }
    let mut sorted: Vec<_> = outputs.iter().collect();
    sorted.sort_by(|(a_name, a), (b_name, b)| {
        b.items
            .cmp(&a.items)
            .then_with(|| b.events.cmp(&a.events))
            .then_with(|| a_name.cmp(b_name))
    });

    eprintln!("{}", "Sequence-producing results".underline());
    for (name, stats) in sorted.into_iter().take(16) {
        eprintln!(
            "{:>6} events | {:>8} items | avg {:>6.2} | {:<34} | {}",
            stats.events,
            stats.items,
            stats.items as f64 / stats.events as f64,
            name,
            format_len_hist(&stats.lens, 4),
        );
    }
}

fn format_len_hist(lens: &BTreeMap<usize, usize>, limit: usize) -> String {
    if lens.is_empty() {
        return "sizes -".to_string();
    }
    let mut sorted: Vec<_> = lens.iter().collect();
    sorted.sort_by(|(a_len, a_count), (b_len, b_count)| {
        b_count.cmp(a_count).then_with(|| a_len.cmp(b_len))
    });
    let max_count = sorted.first().map(|(_, count)| **count).unwrap_or(0);
    let parts = sorted
        .into_iter()
        .take(limit)
        .map(|(len, count)| {
            format!(
                "{len}:{count} {}",
                format_count_bar(*count, max_count, HIST_BAR_WIDTH)
            )
        })
        .collect::<Vec<_>>();
    format!("sizes {}", parts.join("  "))
}

fn format_count_bar(value: usize, max: usize, width: usize) -> String {
    let filled = scaled_len(value, max, width);
    let empty = width.saturating_sub(filled);
    let filled_bar = "+".repeat(filled);
    format!(
        "[{}{}]",
        colorize_heat(&filled_bar, value, max),
        " ".repeat(empty).dimmed()
    )
}

fn format_hit_miss_bar(hits: usize, total: usize, width: usize) -> String {
    let hit_width = scaled_len(hits, total, width);
    let miss_width = width.saturating_sub(hit_width);
    format!(
        "[{}{}]",
        "+".repeat(hit_width).green().bold(),
        "-".repeat(miss_width).red()
    )
}

fn scaled_len(value: usize, max: usize, width: usize) -> usize {
    if value == 0 || max == 0 || width == 0 {
        return 0;
    }
    value.saturating_mul(width).div_ceil(max).clamp(1, width)
}

fn colorize_heat(text: &str, value: usize, max: usize) -> ColoredString {
    if max == 0 || value == 0 {
        return text.normal();
    }
    let share = pct(value, max);
    if share >= 66.0 {
        text.red().bold()
    } else if share >= 33.0 {
        text.yellow()
    } else {
        text.green()
    }
}

fn sequence_kind(value: &Value) -> Option<&'static str> {
    match value {
        Value::IntList(_) => Some("intlist"),
        Value::List(_) => Some("list"),
        Value::String(_) => Some("string"),
        Value::Dict(_) => Some("dict"),

        _ => None,
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Int(_) => "int",
        Value::BigInt(_) => "bigint",
        Value::Float(_) => "float",
        Value::Complex(_) => "complex",
        Value::Fraction(_) => "fraction",
        Value::Algebraic(_) => "algebraic",
        Value::Char(_) => "char",
        Value::Tag(_) => "tag",
        Value::Bool(_) => "bool",
        Value::IntList(_) => "intlist",
        Value::List(_) => "list",
        Value::String(_) => "string",
        Value::Cas(_) => "cas",
        Value::Dict(_) => "dict",
        Value::CompiledFunction(_) => "fn",
        Value::Closure(_) => "closure",
        Value::BuiltinFunction(_) => "bfn",
        Value::FunctionComposition(_) => "fn",
        Value::Stream(_) => "stream",
    }
}

fn instruction_kind(inst: &Instruction) -> &'static str {
    use Instruction as I;
    match inst {
        I::LoadConst(_) => "LoadConst",
        I::LoadClosure(_) => "LoadClosure",
        I::LoadVar(_) => "LoadVar",
        I::LoadVarExists(_) => "LoadVarExists",
        I::LoadCapture(_) => "LoadCapture",
        I::LoadSelf => "LoadSelf",
        I::StoreVar(_) => "StoreVar",
        I::StoreVarKeep(_) => "StoreVarKeep",
        I::LoadLocal(_) => "LoadLocal",
        I::StoreLocal(_) => "StoreLocal",
        I::StoreLocalKeep(_) => "StoreLocalKeep",
        I::StoreCaptureKeep(_) => "StoreCaptureKeep",
        I::BinaryOp(_) => "BinaryOp",
        I::CmpChain(_) => "CmpChain",
        I::Cat(_) => "Cat",
        I::UnaryOp(_) => "UnaryOp",
        I::BoolAndLazy(_) => "BoolAndLazy",
        I::BoolOrLazy(_) => "BoolOrLazy",
        I::CallBuiltinId(_, _) => "CallBuiltinId",
        I::CallLocal(_, _) => "CallLocal",
        I::CallUser(_, _) => "CallUser",
        I::TailCallLocal(_, _) => "TailCallLocal",
        I::TailCallUser(_, _) => "TailCallUser",
        I::CallAnon(_) => "CallAnon",
        I::TailCallAnon(_) => "TailCallAnon",
        I::Postfix(_) => "Postfix",
        I::TailPostfix(_) => "TailPostfix",
        I::PostfixLocal(_, _) => "PostfixLocal",
        I::TailPostfixLocal(_, _) => "TailPostfixLocal",
        I::PostfixCapture(_, _) => "PostfixCapture",
        I::TailPostfixCapture(_, _) => "TailPostfixCapture",
        I::PostfixVar(_, _) => "PostfixVar",
        I::TailPostfixVar(_, _) => "TailPostfixVar",
        I::MakeList(_) => "MakeList",
        I::MakeDict(_) => "MakeDict",

        I::MakeRange { .. } => "MakeRange",
        I::Index => "Index",
        I::IndexLoadLocal(_) => "IndexLoadLocal",
        I::IndexLoadCapture(_) => "IndexLoadCapture",
        I::IndexLoadVar(_) => "IndexLoadVar",
        I::IndexAssignVar(_) => "IndexAssignVar",
        I::IndexAssignLocal(_) => "IndexAssignLocal",
        I::IndexAssignCapture(_) => "IndexAssignCapture",
        I::IndexAssignVarDrop(_) => "IndexAssignVarDrop",
        I::IndexAssignLocalDrop(_) => "IndexAssignLocalDrop",
        I::IndexAssignCaptureDrop(_) => "IndexAssignCaptureDrop",
        I::Jump(_) => "Jump",
        I::JumpIfFalse(_) => "JumpIfFalse",
        I::JumpIfGE(_) => "JumpIfGE",
        I::JumpIfLEZLocal(_, _) => "JumpIfLEZLocal",
        I::IndexMutate { .. } => "IndexMutate",
        I::Pop => "Pop",
        I::Return => "Return",
        I::Assert => "Assert",
        I::TraceBegin => "TraceBegin",
        I::Debug => "Debug",
        I::Pause => "Pause",
        I::Try(_) => "Try",
        I::PrepareNamedArgs(_) => "PrepareNamedArgs",
        I::LoadNamedArgsProvided(_) => "LoadNamedArgsProvided",
    }
}

fn instruction_profile_key(inst: &Instruction) -> String {
    use Instruction as I;
    match inst {
        I::LoadConst(value) => format!("LoadConst({})", value_kind(value)),
        I::LoadClosure(payload) => format!(
            "LoadClosure(locals={}, captures={}, inst={})",
            payload.locals,
            payload.captures.len(),
            payload.instructions.len()
        ),
        I::LoadVar(name) => format!("LoadVar({name})"),
        I::LoadVarExists(name) => format!("LoadVarExists({name})"),
        I::LoadCapture(slot) => format!("LoadCapture({slot})"),
        I::LoadSelf => "LoadSelf".to_string(),
        I::StoreVar(name) => format!("StoreVar({name})"),
        I::StoreVarKeep(name) => format!("StoreVarKeep({name})"),
        I::LoadLocal(slot) => format!("LoadLocal({slot})"),
        I::StoreLocal(slot) => format!("StoreLocal({slot})"),
        I::StoreLocalKeep(slot) => format!("StoreLocalKeep({slot})"),
        I::StoreCaptureKeep(slot) => format!("StoreCaptureKeep({slot})"),
        I::BinaryOp(data) => format!("BinaryOp({:?})", data.op),
        I::CmpChain(ops) => format!("CmpChain({})", ops.len()),
        I::Cat(count) => format!("Cat({count})"),
        I::UnaryOp(data) => format!("UnaryOp({:?})", data.op),
        I::BoolAndLazy(_) => "BoolAndLazy".to_string(),
        I::BoolOrLazy(_) => "BoolOrLazy".to_string(),
        I::CallBuiltinId(id, argc) => {
            let name = Builtins::name_from_id(*id).unwrap_or("<invalid>");
            format!("CallBuiltin({name}/{argc})")
        }
        I::CallLocal(slot, argc) => format!("CallLocal({slot}/{argc})"),
        I::CallUser(name, argc) => format!("CallUser({name}/{argc})"),
        I::TailCallLocal(slot, argc) => format!("TailCallLocal({slot}/{argc})"),
        I::TailCallUser(name, argc) => format!("TailCallUser({name}/{argc})"),
        I::CallAnon(argc) => format!("CallAnon({argc})"),
        I::TailCallAnon(argc) => format!("TailCallAnon({argc})"),
        I::Postfix(argc) => format!("Postfix({argc})"),
        I::TailPostfix(argc) => format!("TailPostfix({argc})"),
        I::PostfixLocal(slot, argc) => format!("PostfixLocal({slot}/{argc})"),
        I::TailPostfixLocal(slot, argc) => format!("TailPostfixLocal({slot}/{argc})"),
        I::PostfixCapture(slot, argc) => format!("PostfixCapture({slot}/{argc})"),
        I::TailPostfixCapture(slot, argc) => format!("TailPostfixCapture({slot}/{argc})"),
        I::PostfixVar(name, argc) => format!("PostfixVar({name}/{argc})"),
        I::TailPostfixVar(name, argc) => format!("TailPostfixVar({name}/{argc})"),
        I::MakeList(count) => format!("MakeList({count})"),
        I::MakeDict(count) => format!("MakeDict({count})"),

        I::MakeRange {
            inclusive,
            has_step,
        } => format!("MakeRange(inclusive={inclusive}, step={has_step})"),
        I::Index => "Index".to_string(),
        I::IndexLoadLocal(slot) => format!("IndexLoadLocal({slot})"),
        I::IndexLoadCapture(slot) => format!("IndexLoadCapture({slot})"),
        I::IndexLoadVar(name) => format!("IndexLoadVar({name})"),
        I::IndexAssignVar(name) => format!("IndexAssignVar({name})"),
        I::IndexAssignLocal(slot) => format!("IndexAssignLocal({slot})"),
        I::IndexAssignCapture(slot) => format!("IndexAssignCapture({slot})"),
        I::IndexAssignVarDrop(name) => format!("IndexAssignVarDrop({name})"),
        I::IndexAssignLocalDrop(slot) => format!("IndexAssignLocalDrop({slot})"),
        I::IndexAssignCaptureDrop(slot) => format!("IndexAssignCaptureDrop({slot})"),
        I::Jump(_) => "Jump".to_string(),
        I::JumpIfFalse(_) => "JumpIfFalse".to_string(),
        I::JumpIfGE(_) => "JumpIfGE".to_string(),
        I::JumpIfLEZLocal(slot, _) => format!("JumpIfLEZLocal({slot})"),
        I::IndexMutate { target, op } => format!("IndexMutate({target:?}, {op:?})"),
        I::Pop => "Pop".to_string(),
        I::Return => "Return".to_string(),
        I::Assert => "Assert".to_string(),
        I::TraceBegin => "TraceBegin".to_string(),
        I::Debug => "Debug".to_string(),
        I::Pause => "Pause".to_string(),
        I::Try(_) => "Try".to_string(),
        I::PrepareNamedArgs(meta) => {
            format!(
                "PrepareNamedArgs(pos={}, named={})",
                meta.pos_count,
                meta.named.len()
            )
        }
        I::LoadNamedArgsProvided(bit) => format!("LoadNamedArgsProvided({bit})"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::compile::Compiler;
    use crate::lex::Lexer;
    use crate::parse::resolve::Resolver;
    use crate::parse::{Parser, fold};
    use crate::vm::inst::Instruction;

    fn run_profiled(src: &str) -> (Value, ProfilerInterpreter) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens, src.to_string());
        let ast = parser.parse().expect("parse");
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);
        let ast = fold::fold(ast);

        let mut compiler = Compiler::new();
        compiler.compile(&ast).expect("compile");
        compiler.propagate_constants();
        compiler.rewrite_tail_calls();
        compiler.fuse();
        compiler.instructions.push(Instruction::Return);
        let mut vm = Vm::new(std::mem::take(&mut compiler.instructions));
        let mut profiler = ProfilerInterpreter::default();
        profiler.trace = false;
        let limit = vm.instructions.len();
        let value = profiler.interpret(&mut vm, limit).expect("execute");
        (value, profiler)
    }

    fn run_profiled_with_kind(
        src: &str,
        nested_kind: InterpreterKind,
    ) -> (Value, ProfilerInterpreter) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens, src.to_string());
        let ast = parser.parse().expect("parse");
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);
        let ast = fold::fold(ast);

        let mut compiler = Compiler::new();
        compiler.compile(&ast).expect("compile");
        compiler.propagate_constants();
        compiler.rewrite_tail_calls();
        compiler.fuse();
        compiler.instructions.push(Instruction::Return);
        let mut vm = Vm::new(std::mem::take(&mut compiler.instructions));
        vm.interpreter_kind = nested_kind;
        let mut profiler = ProfilerInterpreter::default();
        let limit = vm.instructions.len();
        let value = profiler.interpret(&mut vm, limit).expect("execute");
        (value, profiler)
    }

    #[test]
    fn profiler_defaults_to_summary_mode() {
        let profiler = ProfilerInterpreter::default();
        assert!(!profiler.trace);
    }

    #[test]
    fn profile_count_bar_scales_to_width() {
        colored::control::set_override(false);
        assert_eq!(format_count_bar(5, 10, 10), "[+++++     ]");
        colored::control::unset_override();
    }

    #[test]
    fn profile_hit_miss_bar_splits_hits_and_misses() {
        colored::control::set_override(false);
        assert_eq!(format_hit_miss_bar(3, 4, 8), "[++++++--]");
        colored::control::unset_override();
    }

    #[test]
    fn profile_len_hist_includes_size_bars() {
        let mut lens = std::collections::BTreeMap::new();
        lens.insert(2, 1);
        lens.insert(4, 3);

        colored::control::set_override(false);
        assert_eq!(
            format_len_hist(&lens, 2),
            "sizes 4:3 [++++++++++]  2:1 [++++      ]"
        );
        colored::control::unset_override();
    }

    #[test]
    fn profiles_instructions_inside_user_functions() {
        let (value, profiler) = run_profiled("f:{x+1};f[41]");

        assert_eq!(value, Value::Int(42));
        assert!(
            profiler
                .stats
                .borrow()
                .op_counts
                .get("BinaryOp")
                .copied()
                .unwrap_or(0)
                >= 1,
            "profile was missing function body ops: {:?}",
            profiler.stats.borrow().op_counts
        );

        profiler.stats.borrow_mut().total_ops = 0;
    }

    #[test]
    fn profiles_instructions_inside_closures() {
        let (value, profiler) = run_profiled("f:{a:4;g:{'a};g};f[][]");

        assert_eq!(value, Value::Int(4));
        assert!(
            profiler
                .stats
                .borrow()
                .op_counts
                .get("LoadCapture")
                .copied()
                .unwrap_or(0)
                >= 1,
            "profile was missing closure capture loads: {:?}",
            profiler.stats.borrow().op_counts
        );
        profiler.stats.borrow_mut().total_ops = 0;
    }

    #[test]
    fn profiler_kind_reuses_outer_collector_for_nested_calls() {
        let (value, profiler) = run_profiled_with_kind("f:{x+1};f[41]", InterpreterKind::Profiler);

        assert_eq!(value, Value::Int(42));
        assert!(
            profiler
                .stats
                .borrow()
                .op_counts
                .get("BinaryOp")
                .copied()
                .unwrap_or(0)
                >= 1,
            "profile was missing function body ops: {:?}",
            profiler.stats.borrow().op_counts
        );
        profiler.stats.borrow_mut().total_ops = 0;
    }

    #[test]
    fn profiles_closures_invoked_from_builtins() {
        let (value, profiler) = run_profiled("map[1..4;{x+1}]");

        assert_eq!(value, Value::IntList(Arc::new(vec![2, 3, 4])));
        assert!(
            profiler
                .stats
                .borrow()
                .op_counts
                .get("BinaryOp")
                .copied()
                .unwrap_or(0)
                >= 3,
            "profile was missing builtin callback body ops: {:?}",
            profiler.stats.borrow().op_counts
        );
        profiler.stats.borrow_mut().total_ops = 0;
    }

    #[test]
    fn profiles_list_allocation_lengths() {
        let (value, profiler) = run_profiled("f:{(x;2;3)};f[1]");

        assert_eq!(value, Value::IntList(Arc::new(vec![1, 2, 3])));
        assert_eq!(
            profiler.stats.borrow().list_alloc_lens.get(&3).copied(),
            Some(1)
        );
        assert_eq!(
            profiler
                .stats
                .borrow()
                .inst_counts
                .get("MakeList(3)")
                .copied(),
            Some(1)
        );
        profiler.stats.borrow_mut().total_ops = 0;
    }
}
