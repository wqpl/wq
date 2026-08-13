use crate::session::dbglog::{DebugLog, DebugLogFlags};
use crate::vm::inst::{ClosurePayload, DebugStmtMark, Instruction};
use crate::wqdb::data::{ChunkId, DebugInfo, LineTable, Span};

pub(crate) fn mark_stmt_heuristic(
    table: &mut LineTable,
    code: &[crate::vm::inst::Instruction],
    debug_log: Option<&DebugLog>,
) {
    use crate::vm::inst::Instruction::*;
    if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB_VERBOSE)) {
        debug_log.emit_line(format!(
            "[wqdb]: mark_stmt_heuristic called with {} instructions",
            code.len()
        ));
    }
    for (pc, op) in code.iter().enumerate() {
        let op = op.call_instruction();
        let is_stmt = matches!(
            op,
            StoreVar(_)
                | StoreVarKeep(_)
                | StoreLocal(_)
                | StoreLocalKeep(_)
                | StoreCapture(_)
                | StoreCaptureKeep(_)
                | CallBuiltinId(_, _)
                | CallBuiltinDiscardId(_, _)
                | CallLocal(_, _)
                | CallUser(_, _)
                | CallAnon(_)
                | Postfix(_)
                | Index
                | IndexMany(_)
                | IndexManyLoadLocal(_, _)
                | IndexManyLoadCapture(_, _)
                | IndexManyLoadVar(_, _)
                | IndexAssignVar(_)
                | IndexAssignLocal(_)
                | IndexAssignCapture(_)
                | IndexManyAssignVar(_, _)
                | IndexManyAssignLocal(_, _)
                | IndexManyAssignCapture(_, _)
                | IndexAssignVarDrop(_)
                | IndexAssignLocalDrop(_)
                | IndexAssignCaptureDrop(_)
                | IndexManyAssignVarDrop(_, _)
                | IndexManyAssignLocalDrop(_, _)
                | IndexManyAssignCaptureDrop(_, _)
                | IndexMutate { .. }
                | JumpIfFalse(_)
                | JumpIfGE(_)
                | JumpIfLEZLocal(_, _)
                | JumpIfNamedProvided(_, _, _)
                | Debug
                | Return
                // Avoid marking plain stack pops as separate statements to reduce duplicates in loops
                | Try(_)
                | BinaryOp(_)
                | UnaryOp(_)
                | LoadVar(_)
                | LoadVarExists(_)
                | LoadConst(_)
                | LoadOwnedConst(_)
                | LoadClosure { .. }
        );
        if is_stmt {
            if let Some(debug_log) =
                debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB_VERBOSE))
            {
                debug_log.emit_line(format!("[wqdb]: marking PC {pc} as statement: {op:?}"));
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
    debug_log: Option<&DebugLog>,
) -> bool {
    // Normalize spans: sort by start ascending and deduplicate
    let mut spans_sorted: Vec<(usize, usize)> = spans.to_vec();
    spans_sorted.sort_by_key(|(s, _)| *s);
    spans_sorted.dedup();
    if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB_VERBOSE)) {
        debug_log.emit_line(format!(
            "[wqdb]: apply_stmt_spans_exact called with {} instructions, {} spans ({} unique, sorted)",
            code.len(),
            spans.len(),
            spans_sorted.len(),
        ));
        debug_log.emit_line(format!("[wqdb]: spans(sorted) = {spans_sorted:?}"));
    }
    if spans_sorted.is_empty() || spans_sorted.len() * 10 < code.len() {
        if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB_VERBOSE)) {
            debug_log.emit_line(format!(
                "[wqdb]: using heuristic fallback (spans.len() * 10 = {} < code.len() = {})",
                spans_sorted.len() * 10,
                code.len()
            ));
        }
        mark_stmt_heuristic(table, code, debug_log);
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
                    start,
                    end,
                };
            }
            return true;
        }
        return false;
    }
    if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB_VERBOSE)) {
        debug_log.emit_line("[wqdb]: proceeding with exact span mapping (overlay mode)");
    }
    mark_stmt_heuristic(table, code, debug_log);
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
                if let Some(debug_log) =
                    debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB_VERBOSE))
                {
                    debug_log.emit_line(format!(
                        "[wqdb]: detected container span; remapped to end: {spans_for_map:?}"
                    ));
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
                let is_call = code.get(pc).is_some_and(|instruction| {
                    matches!(
                        instruction.call_instruction(),
                        CallBuiltinId(_, _)
                            | CallBuiltinDiscardId(_, _)
                            // | CallBuiltin(_, _)
                            | CallLocal(_, _)
                            | CallUser(_, _)
                            | CallAnon(_)
                            | Postfix(_)
                    )
                });
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
                        start,
                        end,
                    };
                }
            } else {
                // Fallback: map all calls to the only span available
                for &i in &call_idx {
                    let pc = cand[i];
                    let (start, end) = spans_for_map[0];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start,
                        end,
                    };
                }
            }
            // Map remaining (non-call) PCs to the container span.
            if let Some((start, end)) = container_span {
                for &i in &other_idx {
                    let pc = cand[i];
                    table.pc_to_stmt_span[pc] = Span {
                        file_id,
                        start,
                        end,
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
                    start,
                    end,
                };
            }
        }
        return true;
    }
    false
}

/// Same as apply_stmt_spans_exact, but shifts all spans by a base byte offset
pub(crate) fn apply_stmt_spans_exact_offs(
    table: &mut LineTable,
    code: &[crate::vm::inst::Instruction],
    file_id: u32,
    spans: &[(usize, usize)],
    base_offset: usize,
    debug_log: Option<&DebugLog>,
) -> bool {
    if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB)) {
        debug_log.emit_line(format!(
            "[wqdb]: apply_stmt_spans_exact_offs spans={} file_id={} base_offset={} instructions={}",
            spans.len(),
            file_id,
            base_offset,
            code.len(),
        ));
    }
    let shifted: Vec<(usize, usize)> = spans
        .iter()
        .map(|(s, e)| (s.saturating_add(base_offset), e.saturating_add(base_offset)))
        .collect();
    apply_stmt_spans_exact(table, code, file_id, &shifted, debug_log)
}

pub(crate) fn apply_stmt_debug_exact_offs(
    table: &mut LineTable,
    file_id: u32,
    pc_spans: &[Option<(usize, usize)>],
    stmt_marks: &[DebugStmtMark],
    base_offset: usize,
    debug_log: Option<&DebugLog>,
) -> (bool, bool) {
    if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB)) {
        debug_log.emit_line(format!(
            "[wqdb]: apply_stmt_debug_exact_offs pcs={} marks={} file_id={} base_offset={}",
            pc_spans.len(),
            stmt_marks.len(),
            file_id,
            base_offset,
        ));
    }
    let mut has_exact = false;
    table.ensure(pc_spans.len());
    for (pc, span) in pc_spans.iter().enumerate() {
        if let Some((start, end)) = span {
            let span = Span {
                file_id,
                start: start.saturating_add(base_offset),
                end: end.saturating_add(base_offset),
            };
            table.set_exact_span(pc, span);
            has_exact = true;
        }
    }
    let mut has_real = false;
    for mark in stmt_marks {
        let span = Span {
            file_id,
            start: mark.start.saturating_add(base_offset),
            end: mark.end.saturating_add(base_offset),
        };
        table.set_stmt_mark(mark.pc, span);
        has_real = true;
    }
    (has_exact, has_real)
}

fn register_closure_payload_chunk(
    di: &mut DebugInfo,
    file_id: u32,
    payload: &mut ClosurePayload,
    name: Option<std::sync::Arc<str>>,
    base_offset: usize,
    debug_log: Option<&DebugLog>,
) -> ChunkId {
    let chunk = di.new_function_chunk(name.clone(), file_id, payload.instructions.len());
    if let Some(debug_log) = debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB)) {
        debug_log.emit_line(format!(
            "[wqdb]: register_function_chunks new chunk={chunk:?} name={} file_id={} instructions={} base_offset={}",
            di.expect_chunk(chunk).name,
            file_id,
            payload.instructions.len(),
            base_offset,
        ));
    }
    if !payload.dbg_pc_spans.is_empty() && !payload.dbg_stmt_marks.is_empty() {
        let (has_exact, has_real) = {
            let table = &mut di.expect_chunk_mut(chunk).line_table;
            apply_stmt_debug_exact_offs(
                table,
                file_id,
                payload.dbg_pc_spans.as_ref(),
                payload.dbg_stmt_marks.as_ref(),
                base_offset,
                debug_log,
            )
        };
        di.expect_chunk_mut(chunk)
            .note_debug_spans(has_exact, has_real);
    } else if !payload.dbg_stmt_spans.is_empty() {
        let has_real = {
            let table = &mut di.expect_chunk_mut(chunk).line_table;
            apply_stmt_spans_exact_offs(
                table,
                payload.instructions.as_ref(),
                file_id,
                payload.dbg_stmt_spans.as_ref(),
                base_offset,
                debug_log,
            )
        };
        di.expect_chunk_mut(chunk).note_debug_spans(false, has_real);
    } else {
        let table = &mut di.expect_chunk_mut(chunk).line_table;
        mark_stmt_heuristic(table, payload.instructions.as_ref(), debug_log);
    }
    if !payload.dbg_local_names.is_empty() {
        di.expect_chunk_mut(chunk).local_names =
            Some(payload.dbg_local_names.iter().cloned().collect());
    } else if let Some(params) = payload.params.as_ref() {
        di.expect_chunk_mut(chunk).local_names = Some(params.iter().cloned().collect());
    }
    payload.dbg_chunk = Some(chunk);
    chunk
}

/// Register chunks for nested functions and mark their statement PCs.
/// Recursively descends into both `LoadConst` compiled functions and
/// `LoadClosure` payloads so that deeply-nested functions get their debug
/// chunks (and the correct `file_id`) assigned eagerly at compile time.
pub(crate) fn register_function_chunks(
    di: &mut DebugInfo,
    file_id: u32,
    code: &mut [Instruction],
    base_offset: usize,
    debug_log: Option<&DebugLog>,
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
                        let chunk = di.new_function_chunk(
                            next_name.clone(),
                            file_id,
                            f_mut.instructions.len(),
                        );
                        if let Some(debug_log) =
                            debug_log.filter(|log| log.enabled(DebugLogFlags::WQDB))
                        {
                            debug_log.emit_line(format!(
                                "[wqdb]: register_function_chunks new chunk={chunk:?} name={} file_id={} instructions={} base_offset={}",
                                di.expect_chunk(chunk).name,
                                file_id,
                                f_mut.instructions.len(),
                                base_offset,
                            ));
                        }
                        if let (Some(pc_spans), Some(stmt_marks)) =
                            (&f_mut.dbg_pc_spans, &f_mut.dbg_stmt_marks)
                        {
                            let (has_exact, has_real) = {
                                let table = &mut di.expect_chunk_mut(chunk).line_table;
                                apply_stmt_debug_exact_offs(
                                    table,
                                    file_id,
                                    pc_spans.as_ref(),
                                    stmt_marks.as_ref(),
                                    base_offset,
                                    debug_log,
                                )
                            };
                            di.expect_chunk_mut(chunk)
                                .note_debug_spans(has_exact, has_real);
                        } else if let Some(spans) = &f_mut.dbg_stmt_spans {
                            let has_real = {
                                let table = &mut di.expect_chunk_mut(chunk).line_table;
                                apply_stmt_spans_exact_offs(
                                    table,
                                    &f_mut.instructions,
                                    file_id,
                                    spans.as_ref(),
                                    base_offset,
                                    debug_log,
                                )
                            };
                            di.expect_chunk_mut(chunk).note_debug_spans(false, has_real);
                        } else {
                            let table = &mut di.expect_chunk_mut(chunk).line_table;
                            mark_stmt_heuristic(table, &f_mut.instructions, debug_log);
                        }
                        if let Some(names) = &f_mut.dbg_local_names {
                            di.expect_chunk_mut(chunk).local_names =
                                Some(names.iter().cloned().collect());
                        }
                        f_mut.dbg_chunk = Some(chunk);
                        // Recurse into nested functions
                        let nested = std::sync::Arc::make_mut(&mut f_mut.instructions);
                        register_function_chunks(di, file_id, nested, base_offset, debug_log);
                    }
                }
                LoadClosure(payload) => {
                    if payload.dbg_chunk.is_none() {
                        register_closure_payload_chunk(
                            di,
                            file_id,
                            payload,
                            next_name,
                            base_offset,
                            debug_log,
                        );
                    }
                    let nested = std::sync::Arc::make_mut(&mut payload.instructions);
                    register_function_chunks(di, file_id, nested, base_offset, debug_log);
                }
                _ => {}
            }
        }

        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wqdb::data::ChunkId;

    fn empty_payload() -> ClosurePayload {
        ClosurePayload {
            params: None,
            named_params: None,
            locals: 0,
            isolated_module: false,
            captures: Vec::new(),
            instructions: vec![Instruction::Return].into(),
            dbg_chunk: None,
            dbg_stmt_spans: Vec::<(usize, usize)>::new().into(),
            dbg_pc_spans: Vec::<Option<(usize, usize)>>::new().into(),
            dbg_stmt_marks: Vec::new().into(),
            dbg_local_names: Vec::<String>::new().into(),
        }
    }

    #[test]
    fn register_function_chunks_assigns_stable_closure_chunk() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("<test>", "");
        let mut code = vec![Instruction::LoadClosure(Box::new(empty_payload()))];

        register_function_chunks(&mut di, file_id, &mut code, 0, None);

        let Instruction::LoadClosure(payload) = &code[0] else {
            panic!("expected closure payload");
        };
        assert_eq!(payload.dbg_chunk, Some(ChunkId(0)));
        assert_eq!(di.expect_chunk(ChunkId(0)).len, 1);

        register_function_chunks(&mut di, file_id, &mut code, 0, None);

        let Instruction::LoadClosure(payload) = &code[0] else {
            panic!("expected closure payload");
        };
        assert_eq!(payload.dbg_chunk, Some(ChunkId(0)));
        assert!(
            di.get_chunk(ChunkId(1)).is_none(),
            "registering the same closure payload twice must not allocate a second chunk"
        );
    }

    #[test]
    fn named_call_remains_a_statement_boundary() {
        let instruction = Instruction::CallAnon(1).with_named_args(Some(std::sync::Arc::new(
            crate::vm::inst::NamedArgMeta {
                pos_count: 0,
                named: vec![(0, std::sync::Arc::from("value"))].into_boxed_slice(),
            },
        )));
        let mut table = LineTable::default();

        mark_stmt_heuristic(&mut table, &[instruction], None);

        assert!(table.is_stmt(0));
    }
}
