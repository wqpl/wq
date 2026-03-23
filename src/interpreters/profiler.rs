use std::collections::HashMap;

use crate::colored::Colorize;

use crate::{
    interpreters::{
        Interpreter,
        default::{DefaultInterpreter, ProfilerHooks},
    },
    value::{Value, WqResult},
    vm::{Vm, instruction::Instruction},
    wqdb::DebugHost,
};

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

pub struct ProfilerInterpreter {
    stats: ProfileStats,
    pub trace: bool,
}

impl Default for ProfilerInterpreter {
    fn default() -> Self {
        Self {
            stats: ProfileStats::default(),
            trace: true,
        }
    }
}

impl Drop for ProfilerInterpreter {
    fn drop(&mut self) {
        if self.stats.total_ops == 0 {
            return;
        }

        eprintln!("{}", "\nPROFILE".bold().underline());
        let mut sorted: Vec<_> = self.stats.op_counts.iter().collect();
        sorted.sort_by_key(|&(_, count)| std::cmp::Reverse(*count));
        for (op, count) in sorted.into_iter().take(20) {
            let pct = (*count as f64 / self.stats.total_ops as f64) * 100.0;
            eprintln!("{:>5.2}% | {:>6} times | {}", pct, count, op);
        }

        let total_var_lookups = self.stats.load_var_cache_hits
            + self.stats.load_var_const_cache_hits
            + self.stats.load_var_cache_misses;
        if total_var_lookups > 0 {
            let total_hits = self.stats.load_var_cache_hits + self.stats.load_var_const_cache_hits;
            eprintln!(
                "Global cache: {} hits / {} misses ({:.2}%)",
                total_hits,
                self.stats.load_var_cache_misses,
                pct(total_hits, total_var_lookups)
            );
            if self.stats.load_var_const_cache_hits > 0 {
                eprintln!(
                    "const-value hits: {} | slot hits: {}",
                    self.stats.load_var_const_cache_hits, self.stats.load_var_cache_hits,
                );
            }
        }

        let total_user_calls = self.stats.call_user_cache_hits + self.stats.call_user_cache_misses;
        if total_user_calls > 0 {
            eprintln!(
                "Call cache: {} hits / {} misses ({:.2}%)",
                self.stats.call_user_cache_hits,
                self.stats.call_user_cache_misses,
                pct(self.stats.call_user_cache_hits, total_user_calls)
            );
        }

        let total_alloc_events = self.stats.list_alloc_events
            + self.stats.dict_alloc_events
            + self.stats.range_alloc_events
            + self.stats.closure_capture_alloc_events;
        let total_alloc_units = self.stats.list_alloc_items
            + self.stats.dict_alloc_items
            + self.stats.range_alloc_items
            + self.stats.closure_capture_cells;
        if total_alloc_events > 0 {
            eprintln!(
                "Alloc events: {} total, {} aggregate items",
                total_alloc_events, total_alloc_units
            );
            print_alloc_line(
                "list",
                self.stats.list_alloc_events,
                self.stats.list_alloc_items,
            );
            print_alloc_line(
                "dict",
                self.stats.dict_alloc_events,
                self.stats.dict_alloc_items,
            );
            print_alloc_line(
                "range",
                self.stats.range_alloc_events,
                self.stats.range_alloc_items,
            );
            print_alloc_line(
                "closure captures",
                self.stats.closure_capture_alloc_events,
                self.stats.closure_capture_cells,
            );
        }

        eprintln!(
            "{}: stack={} | final-stack={} | call-depth={} | inst={}",
            "Stats".underline(),
            self.stats.max_stack_len,
            self.stats.final_stack_len,
            self.stats.max_call_depth,
            self.stats.total_ops
        );
    }
}

impl Interpreter for ProfilerInterpreter {
    fn execute(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        if limit > vm.instructions.len() {
            return Err(
                crate::wqerror::WqError::new(crate::wqerror::WqErrorType::Vm)
                    .src("profiler interpreter")
                    .msg(format!("limit out of bounds: {limit}")),
            );
        }
        let mut delegate = DefaultInterpreter;
        while vm.pc < limit {
            if !delegate.execute_one_with_hooks(vm, limit, self)? {
                break;
            }
        }
        self.stats.final_stack_len = vm.stack.len();
        Ok(vm.stack.pop().unwrap_or(Value::unit()))
    }
}

impl ProfilerHooks for ProfilerInterpreter {
    fn before_instruction(&mut self, vm: &Vm, idx: usize, op: &Instruction) {
        let op_str = crate::vm::instruction::InstPrettyDumper::new(true, true).highlight_inst(op);
        let unstyled_op = format!("{:?}", op);
        let op_name = unstyled_op
            .split('(')
            .next()
            .unwrap_or(&unstyled_op)
            .split('{')
            .next()
            .unwrap_or(&unstyled_op)
            .trim()
            .to_string();
        *self.stats.op_counts.entry(op_name).or_insert(0) += 1;
        self.stats.total_ops += 1;
        self.stats.max_stack_len = self.stats.max_stack_len.max(vm.stack.len());
        self.stats.max_call_depth = self.stats.max_call_depth.max(vm.call_depth());
        if self.trace {
            eprintln!(
                "pc: {:04} | c-depth: {:02} | stack: {} | inst: {:<25} ",
                idx,
                vm.call_depth(),
                vm.stack.len(),
                op_str
            );
        }
    }

    fn on_load_var_cache_hit(&mut self, slot_cached: bool) {
        if slot_cached {
            self.stats.load_var_cache_hits += 1;
        } else {
            self.stats.load_var_const_cache_hits += 1;
        }
    }

    fn on_load_var_cache_miss(&mut self) {
        self.stats.load_var_cache_misses += 1;
    }

    fn on_call_user_cache_hit(&mut self) {
        self.stats.call_user_cache_hits += 1;
    }

    fn on_call_user_cache_miss(&mut self) {
        self.stats.call_user_cache_misses += 1;
    }

    fn on_list_alloc(&mut self, len: usize) {
        self.stats.list_alloc_events += 1;
        self.stats.list_alloc_items += len;
    }

    fn on_dict_alloc(&mut self, len: usize) {
        self.stats.dict_alloc_events += 1;
        self.stats.dict_alloc_items += len;
    }

    fn on_range_alloc(&mut self, len: usize) {
        self.stats.range_alloc_events += 1;
        self.stats.range_alloc_items += len;
    }

    fn on_closure_capture_alloc(&mut self, len: usize) {
        self.stats.closure_capture_alloc_events += 1;
        self.stats.closure_capture_cells += len;
    }

    fn on_return(&mut self, vm: &Vm) {
        self.stats.final_stack_len = vm.stack.len();
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
