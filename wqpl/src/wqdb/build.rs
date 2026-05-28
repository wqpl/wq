// mark_stmt_heuristic
// apply_stmt_spans_exact_offs
// apply_stmt_debug_exact_offs
// register_function_chunks

use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::vm::inst::DebugStmtMark;
use crate::wqdb::data::{DebugInfo, LineTable, Span};

pub(crate) fn mark_stmt_heuristic(table: &mut LineTable, code: &[crate::vm::inst::Instruction]) {
    use crate::vm::inst::Instruction::*;
    if get_debug_log_flags().contains(DebugLogFlags::WQDB_VERBOSE) {
        eprintln!(
            "[wqdb]: mark_stmt_heuristic called with {} instructions",
            code.len()
        );
    }
    for (pc, op) in code.iter().enumerate() {
        let is_stmt = matches!(
            op,
            StoreVar(_)
                | StoreVarKeep(_)
                | StoreLocal(_)
                | StoreLocalKeep(_)
                // | StoreCapture(_)
                | StoreCaptureKeep(_)
                | CallBuiltinId(_, _)
                | CallLocal(_, _)
                | CallUser(_, _)
                | CallAnon(_)
                | Postfix(_)
                | PostfixLocal(_, _)
                | PostfixMethodLocal(_, _, _)
                | CallMethodLocal(_, _, _)
                | PostfixCapture(_, _)
                | PostfixMethodCapture(_, _, _)
                | CallMethodCapture(_, _, _)
                | PostfixMethodVar(_, _, _)
                | CallMethodVar(_, _, _)
                | Index
                | IndexAssignVar(_)
                | IndexAssignLocal(_)
                | IndexAssignCapture(_)
                | IndexAssignVarDrop(_)
                | IndexAssignLocalDrop(_)
                | IndexAssignCaptureDrop(_)
                | IndexMutate { .. }
                | JumpIfFalse(_)
                | JumpIfGE(_)
                | JumpIfLEZLocal(_, _)
                | Debug
                | Return
                // Avoid marking plain stack pops as separate statements to reduce duplicates in loops
                | Try(_)
                | BinaryOp(_)
                | UnaryOp(_)
                | LoadVar(_)
                | LoadVarExists(_)
                | LoadConst(_)
                | LoadClosure { .. }
        );
        if is_stmt {
            if get_debug_log_flags().contains(DebugLogFlags::WQDB_VERBOSE) {
                eprintln!("[wqdb]: marking PC {pc} as statement: {op:?}");
            }
            table.mark_stmt(pc, Span::NONE);
        }
    }
}

/// Replace statement markers with exact mapping from provided spans.
/// Uses heuristics only to pick candidate PCs, then clears all markers and
/// marks exactly one PC per span in order. Falls back to heuristic marking
/// when spans are insufficient for complex control structures.
fn apply_stmt_spans_exact(
    table: &mut LineTable,
    code: &[crate::vm::inst::Instruction],
    file_id: u32,
    spans: &[(usize, usize)],
) {
    // Normalize spans: sort by start ascending and deduplicate
    let mut spans_sorted: Vec<(usize, usize)> = spans.to_vec();
    spans_sorted.sort_by_key(|(s, _)| *s);
    spans_sorted.dedup();
    if get_debug_log_flags().contains(DebugLogFlags::WQDB_VERBOSE) {
        eprintln!(
            "[wqdb]: apply_stmt_spans_exact called with {} instructions, {} spans ({} unique, sorted)",
            code.len(),
            spans.len(),
            spans_sorted.len(),
        );
        eprintln!("[wqdb]: spans(sorted) = {spans_sorted:?}");
    }
    if spans_sorted.is_empty() || spans_sorted.len() * 10 < code.len() {
        if get_debug_log_flags().contains(DebugLogFlags::WQDB_VERBOSE) {
            eprintln!(
                "[wqdb]: using heuristic fallback (spans.len() * 10 = {} < code.len() = {})",
                spans_sorted.len() * 10,
                code.len()
            );
        }
        mark_stmt_heuristic(table, code);
        let mut cand: Vec<usize> = Vec::new();
        for pc in 0..code.len() {
            if table.is_stmt(pc) {
                cand.push(pc);
            }
        }
        if !spans_sorted.is_empty() && !cand.is_empty() {
            for (i, &pc) in cand.iter().enumerate() {
                let span_idx = (i * spans_sorted.len()) / cand.len();
                let span_idx = span_idx.min(spans_sorted.len() - 1);
                let (start, end) = spans_sorted[span_idx];
                table.pc_to_stmt_span[pc] = Span {
                    file_id,
                    start: start as u32,
                    end: end as u32,
                };
            }
        }
        return;
    }
    if get_debug_log_flags().contains(DebugLogFlags::WQDB_VERBOSE) {
        eprintln!("[wqdb]: proceeding with exact span mapping (overlay mode)");
    }
    mark_stmt_heuristic(table, code);
    let len = code.len();
    let mut cand: Vec<usize> = Vec::new();
    for pc in 0..len {
        if table.is_stmt(pc) {
            cand.push(pc);
        }
    }
    table.ensure(len);
    if !spans_sorted.is_empty() && !cand.is_empty() {
        // Heuristic: detect container span (first span fully covering all others)
        let mut spans_for_map: Vec<(usize, usize)> = spans_sorted.clone();
        let mut has_container = false;
        if spans_for_map.len() >= 2 {
            let (s0, e0) = spans_for_map[0];
            let (_sn, en) = spans_for_map[spans_for_map.len() - 1];
            let contains_rest = s0 <= spans_for_map[1].0 && e0 >= en;
            if contains_rest {
                let container = spans_for_map.remove(0);
                spans_for_map.push(container);
                has_container = true;
                if get_debug_log_flags().contains(DebugLogFlags::WQDB_VERBOSE) {
                    eprintln!(
                        "[wqdb]: detected container span; remapped to end: {spans_for_map:?}"
                    );
                }
            }
        }
        if has_container {
            // Split into body spans and container span
            let container_span = Some(spans_for_map[spans_for_map.len() - 1]);
            let body_spans: Vec<(usize, usize)> = spans_for_map[..spans_for_map.len() - 1].to_vec();
            // Classify candidate PCs as call vs other to improve loop/body alignment.
            use crate::vm::inst::Instruction::*;
            let mut call_idx: Vec<usize> = Vec::new();
            let mut other_idx: Vec<usize> = Vec::new();
            for (i, &pc) in cand.iter().enumerate() {
                let is_call = matches!(
                    code.get(pc),
                    Some(CallBuiltinId(_, _))
                        // | Some(CallBuiltin(_, _))
                        | Some(CallLocal(_, _))
                        | Some(CallUser(_, _))
                        | Some(CallAnon(_))
                        | Some(Postfix(_))
                        | Some(PostfixLocal(_, _))
                        | Some(PostfixMethodLocal(_, _, _))
                        | Some(CallMethodLocal(_, _, _))
                        | Some(PostfixCapture(_, _))
                        | Some(PostfixMethodCapture(_, _, _))
                        | Some(CallMethodCapture(_, _, _))
                        | Some(PostfixVar(_, _))
                        | Some(PostfixMethodVar(_, _, _))
                        | Some(CallMethodVar(_, _, _))
                );
                if is_call {
                    call_idx.push(i);
                } else {
                    other_idx.push(i);
                }
            }
            // Map calls round-robin across body spans to create a cyclic feel inside loops.
            if !body_spans.is_empty() {
                for (j, &i) in call_idx.iter().enumerate() {
                    let pc = cand[i];
                    let (start, end) = body_spans[j % body_spans.len()];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start: start as u32,
                        end: end as u32,
                    };
                }
            } else {
                // Fallback: map all calls to the only span available
                for &i in &call_idx {
                    let pc = cand[i];
                    let (start, end) = spans_for_map[0];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start: start as u32,
                        end: end as u32,
                    };
                }
            }
            // Map remaining (non-call) PCs to the container span.
            if let Some((start, end)) = container_span {
                for &i in &other_idx {
                    let pc = cand[i];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start: start as u32,
                        end: end as u32,
                    };
                }
            }
        } else {
            // No container scenario: distribute spans evenly across candidates (overlay)
            let nsp = spans_for_map.len();
            for (i, &pc) in cand.iter().enumerate() {
                let si = (i * nsp) / cand.len();
                let si = si.min(nsp - 1);
                let (start, end) = spans_for_map[si];
                table.pc_to_stmt_span[pc] = Span {
                    file_id,
                    start: start as u32,
                    end: end as u32,
                };
            }
        }
    }
}

/// Same as apply_stmt_spans_exact, but shifts all spans by a base byte offset
pub(crate) fn apply_stmt_spans_exact_offs(
    table: &mut LineTable,
    code: &[crate::vm::inst::Instruction],
    file_id: u32,
    spans: &[(usize, usize)],
    base_offset: usize,
) {
    if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
        eprintln!(
            "[wqdb]: apply_stmt_spans_exact_offs spans={} file_id={} base_offset={} instructions={}",
            spans.len(),
            file_id,
            base_offset,
            code.len(),
        );
    }
    let shifted: Vec<(usize, usize)> = spans
        .iter()
        .map(|(s, e)| (s.saturating_add(base_offset), e.saturating_add(base_offset)))
        .collect();
    apply_stmt_spans_exact(table, code, file_id, &shifted);
}

pub fn apply_stmt_debug_exact_offs(
    table: &mut LineTable,
    file_id: u32,
    pc_spans: &[Option<(usize, usize)>],
    stmt_marks: &[DebugStmtMark],
    base_offset: usize,
) {
    if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
        eprintln!(
            "[wqdb]: apply_stmt_debug_exact_offs pcs={} marks={} file_id={} base_offset={}",
            pc_spans.len(),
            stmt_marks.len(),
            file_id,
            base_offset,
        );
    }
    table.ensure(pc_spans.len());
    for (pc, span) in pc_spans.iter().enumerate() {
        if let Some((start, end)) = span {
            let span = Span {
                file_id,
                start: start.saturating_add(base_offset) as u32,
                end: end.saturating_add(base_offset) as u32,
            };
            table.set_exact_span(pc, span);
        }
    }
    for mark in stmt_marks {
        let span = Span {
            file_id,
            start: mark.start.saturating_add(base_offset) as u32,
            end: mark.end.saturating_add(base_offset) as u32,
        };
        table.set_stmt_mark(mark.pc, span);
    }
}

/// Register chunks for nested non-capturing functions and mark their statement
/// PCs. Recursively descends into both `LoadConst` compiled functions and
/// `LoadClosure` payloads so that deeply-nested functions get their debug
/// chunks (and the correct `file_id`) assigned eagerly at compile time.
pub(crate) fn register_function_chunks(
    di: &mut DebugInfo,
    file_id: u32,
    code: &mut [crate::vm::inst::Instruction],
    base_offset: usize,
) {
    use crate::value::Value;
    use crate::vm::inst::Instruction::*;
    let mut i = 0usize;
    while i < code.len() {
        // Peek at the next instruction to detect `StoreVar` / `StoreVarKeep`
        // so we can name the chunk after the variable being assigned.
        let next_name = if let Some(StoreVar(name) | StoreVarKeep(name)) = code.get(i + 1) {
            Some(std::sync::Arc::<str>::from(&**name))
        } else {
            None
        };

        if let Some(ins) = code.get_mut(i) {
            match ins {
                LoadConst(box_val) => {
                    let val_mut = box_val.as_mut();
                    if let Value::CompiledFunction(f) = val_mut
                        && f.dbg_chunk.is_none()
                    {
                        let f_mut = std::sync::Arc::make_mut(f);
                        let chunk = di.new_chunk("<fn>", file_id, f_mut.instructions.len());
                        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
                            eprintln!(
                                "[wqdb]: register_function_chunks new chunk={chunk:?} name={} file_id={} instructions={} base_offset={}",
                                di.chunk(chunk).name,
                                file_id,
                                f_mut.instructions.len(),
                                base_offset,
                            );
                        }
                        let table = &mut di.chunk_mut(chunk).line_table;
                        if let (Some(pc_spans), Some(stmt_marks)) =
                            (&f_mut.dbg_pc_spans, &f_mut.dbg_stmt_marks)
                        {
                            apply_stmt_debug_exact_offs(
                                table,
                                file_id,
                                pc_spans.as_ref(),
                                stmt_marks.as_ref(),
                                base_offset,
                            );
                        } else if let Some(spans) = &f_mut.dbg_stmt_spans {
                            apply_stmt_spans_exact_offs(
                                table,
                                &f_mut.instructions,
                                file_id,
                                spans.as_ref(),
                                base_offset,
                            );
                        } else {
                            mark_stmt_heuristic(table, &f_mut.instructions);
                        }
                        if let Some(names) = &f_mut.dbg_local_names {
                            di.chunk_mut(chunk).local_names = Some(names.iter().cloned().collect());
                        }
                        if let Some(name) = next_name {
                            di.chunk_mut(chunk).name = name.clone();
                            di.by_name.insert(name, chunk);
                        }
                        f_mut.dbg_chunk = Some(chunk);
                        // Recurse into nested functions
                        let nested = std::sync::Arc::make_mut(&mut f_mut.instructions);
                        register_function_chunks(di, file_id, nested, base_offset);
                    }
                }
                LoadClosure(payload) => {
                    let nested = std::sync::Arc::make_mut(&mut payload.instructions);
                    register_function_chunks(di, file_id, nested, base_offset);
                }
                _ => {}
            }
        }

        i += 1;
    }
}
