use std::cell::RefCell;
use std::collections::HashMap;

use colored::{ColoredString, Colorize};

use crate::interpret::vanilla::VanillaInterpreter;
use crate::interpret::{Interpreter, InterpreterHook};
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::vm::inst::Instruction;

#[derive(Default)]
struct ProfileStats {
    op_counts: HashMap<String, usize>,
    total_ops: usize,
    load_var_cache_hits: usize,
    load_var_const_cache_hits: usize,
    load_var_cache_misses: usize,
    call_user_cache_hits: usize,
    call_user_cache_misses: usize,
    list_alloc_events: usize,
    list_alloc_items: usize,
    dict_alloc_events: usize,
    dict_alloc_items: usize,
    range_alloc_events: usize,
    range_alloc_items: usize,
    closure_capture_alloc_events: usize,
    closure_capture_cells: usize,
    max_stack_len: usize,
    max_call_depth: usize,
    final_stack_len: usize,
}

pub(crate) struct ProfilerInterpreter {
    stats: RefCell<ProfileStats>,
    pub(crate) trace: bool,
}

impl Default for ProfilerInterpreter {
    fn default() -> Self {
        Self {
            stats: RefCell::new(ProfileStats::default()),
            trace: true,
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
        let mut sorted: Vec<_> = stats.op_counts.iter().collect();
        sorted.sort_by_key(|&(_, count)| std::cmp::Reverse(*count));
        for (op, count) in sorted.into_iter().take(20) {
            let pct = (*count as f64 / stats.total_ops as f64) * 100.0;
            eprintln!("{:>5.2}% | {:>6} times | {}", pct, count, op);
        }

        let total_var_lookups = stats.load_var_cache_hits
            + stats.load_var_const_cache_hits
            + stats.load_var_cache_misses;
        if total_var_lookups > 0 {
            let total_hits = stats.load_var_cache_hits + stats.load_var_const_cache_hits;
            eprintln!(
                "Global cache: {} hits / {} misses ({:.2}%)",
                total_hits,
                stats.load_var_cache_misses,
                pct(total_hits, total_var_lookups)
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
                "Call cache: {} hits / {} misses ({:.2}%)",
                stats.call_user_cache_hits,
                stats.call_user_cache_misses,
                pct(stats.call_user_cache_hits, total_user_calls)
            );
        }

        let total_alloc_events = stats.list_alloc_events
            + stats.dict_alloc_events
            + stats.range_alloc_events
            + stats.closure_capture_alloc_events;
        let total_alloc_units = stats.list_alloc_items
            + stats.dict_alloc_items
            + stats.range_alloc_items
            + stats.closure_capture_cells;
        if total_alloc_events > 0 {
            eprintln!(
                "Alloc events: {} total, {} aggregate items",
                total_alloc_events, total_alloc_units
            );
            print_alloc_line("list", stats.list_alloc_events, stats.list_alloc_items);
            print_alloc_line("dict", stats.dict_alloc_events, stats.dict_alloc_items);
            print_alloc_line("range", stats.range_alloc_events, stats.range_alloc_items);
            print_alloc_line(
                "closure captures",
                stats.closure_capture_alloc_events,
                stats.closure_capture_cells,
            );
        }

        eprintln!(
            "{}: stack={} | final-stack={} | call-depth={} | inst={}",
            "Stats".underline(),
            stats.max_stack_len,
            stats.final_stack_len,
            stats.max_call_depth,
            stats.total_ops
        );
    }
}

impl Interpreter for ProfilerInterpreter {
    fn interpret(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        let mut delegate = VanillaInterpreter;
        vm.set_hooks(Some(self));
        let result = delegate.interpret(vm, limit);
        self.stats.borrow_mut().final_stack_len = vm.stack.len();
        vm.set_hooks(None);
        result
    }
}

impl InterpreterHook for ProfilerInterpreter {
    fn before_instruction(&self, vm: &Vm, idx: usize, op: &Instruction) {
        let call_depth = vm.locals.len();
        let op_str = crate::vm::inst::InstPrettyDumper::new(true, true).highlight_inst(op);
        let unstyled_op = format!("{op:?}");
        let op_name = unstyled_op
            .split('(')
            .next()
            .unwrap_or(&unstyled_op)
            .split('{')
            .next()
            .unwrap_or(&unstyled_op)
            .trim()
            .to_string();
        let mut stats = self.stats.borrow_mut();
        *stats.op_counts.entry(op_name).or_insert(0) += 1;
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

    fn on_list_alloc(&self, len: &dyn Fn() -> usize) {
        let mut stats = self.stats.borrow_mut();
        stats.list_alloc_events += 1;
        stats.list_alloc_items += len();
    }

    fn on_dict_alloc(&self, len: &dyn Fn() -> usize) {
        let mut stats = self.stats.borrow_mut();
        stats.dict_alloc_events += 1;
        stats.dict_alloc_items += len();
    }

    fn on_range_alloc(&self, len: &dyn Fn() -> usize) {
        let mut stats = self.stats.borrow_mut();
        stats.range_alloc_events += 1;
        stats.range_alloc_items += len();
    }

    fn on_closure_capture_alloc(&self, len: &dyn Fn() -> usize) {
        let mut stats = self.stats.borrow_mut();
        stats.closure_capture_alloc_events += 1;
        stats.closure_capture_cells += len();
    }

    fn on_return(&self, vm: &Vm) {
        self.stats.borrow_mut().final_stack_len = vm.stack.len();
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

fn print_alloc_line(label: &str, events: usize, units: usize) {
    if events == 0 {
        return;
    }
    eprintln!(
        "  {:<16} {:>5} events | {:>6} items | avg {:>6.2}",
        label,
        events,
        units,
        units as f64 / events as f64,
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::resolve::Resolver;
    use crate::parser::{Parser, fold};
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
}
