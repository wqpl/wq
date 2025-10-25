use crate::{
    value::Value,
    vm::{Vm, instruction::Instruction},
    wqdb::{
        ChunkId, CodeLoc, DebugHost, DebugInfo, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
    },
};

pub type Backtrace = Vec<(CodeLoc, std::sync::Arc<str>)>;

impl Vm {
    pub fn set_bt_mode(&mut self, flag: bool) {
        self.bt_mode = flag;
    }

    /// Prepare debug info for a top-level script run in the REPL.
    /// Creates a new source file and a script chunk and selects it as current.
    pub fn repl_debug_prepare_script(&mut self, virtual_path: &str, source: &str) {
        if !(self.wqdb.enabled || self.bt_mode) {
            return;
        }
        let file_id = self.debug_info.new_file(virtual_path, source);
        let len = self.instructions.len();
        let chunk = self.debug_info.new_chunk("<repl>", file_id, len);
        self.current_chunk = chunk;
    }

    pub fn set_debug_src_offset(&mut self, offs: usize) {
        self.debug_src_offset = offs;
    }

    pub fn debug_src_offset(&self) -> usize {
        self.debug_src_offset
    }

    #[inline]
    pub fn clear_last_bt(&mut self) {
        self.last_backtrace = None;
    }

    #[inline]
    pub fn take_last_bt(&mut self) -> Option<Backtrace> {
        self.last_backtrace.take()
    }

    #[inline]
    pub fn capture_bt_if_empty(&mut self) {
        if self.last_backtrace.is_none() {
            self.last_backtrace = Some(self.bt_frames());
        }
    }

    pub fn func_name_for_chunk(&self, id: ChunkId) -> String {
        self.debug_info
            .chunks
            .get(&id)
            .map(|m| m.name.to_string())
            .unwrap_or_else(|| "<?>".to_string())
    }

    /// Ensures a dbg chunk (with spans if present).
    /// No-op when debugging is off or chunk exists.
    #[inline]
    pub fn ensure_dbg_chunk_with_spans(
        &mut self,
        name: &str,
        dbg_chunk: Option<ChunkId>,
        instructions: &[Instruction],
        dbg_stmt_spans: &Option<std::sync::Arc<[(usize, usize)]>>,
        dbg_local_names: &Option<std::sync::Arc<[String]>>,
        params: &Option<std::sync::Arc<[String]>>,
    ) -> Option<ChunkId> {
        if !(self.wqdb.enabled || self.bt_mode) {
            return dbg_chunk;
        }

        if let Some(id) = dbg_chunk {
            let (file_id, needs_rename, has_real_spans, has_local_names) = {
                let meta = self.debug_info.chunk(id);
                (
                    meta.file_id,
                    meta.name.as_ref() != name,
                    meta.line_table
                        .pc_to_stmt_span
                        .iter()
                        .any(|sp| sp.file_id != u32::MAX),
                    meta.local_names.is_some(),
                )
            };

            if needs_rename {
                self.debug_info.rename_chunk(id, name);
            }

            if let Some(spans) = dbg_stmt_spans.as_ref()
                && !has_real_spans
            {
                let base_offs = self.debug_src_offset();
                let table = &mut self.debug_info.chunk_mut(id).line_table;
                apply_stmt_spans_exact_offs(
                    table,
                    instructions,
                    file_id,
                    spans.as_ref(),
                    base_offs,
                );
            }

            if !has_local_names {
                if let Some(names) = dbg_local_names.as_ref() {
                    self.debug_info.chunk_mut(id).local_names =
                        Some(names.iter().cloned().collect());
                } else if let Some(ps) = params.as_ref() {
                    self.debug_info.chunk_mut(id).local_names = Some(ps.iter().cloned().collect());
                }
            }

            return Some(id);
        }
        let file_id = self.debug_info.chunk(self.current_chunk).file_id;
        let id = self.debug_info.new_chunk(name, file_id, instructions.len());

        let base_offs = self.debug_src_offset();
        let table = &mut self.debug_info.chunk_mut(id).line_table;
        if let Some(spans) = dbg_stmt_spans.as_ref() {
            apply_stmt_spans_exact_offs(table, instructions, file_id, spans.as_ref(), base_offs);
        } else {
            mark_stmt_heuristic(table, instructions);
        }

        if let Some(names) = dbg_local_names.as_ref() {
            self.debug_info.chunk_mut(id).local_names = Some(names.iter().cloned().collect());
        } else if let Some(ps) = params.as_ref() {
            self.debug_info.chunk_mut(id).local_names = Some(ps.iter().cloned().collect());
        }

        Some(id)
    }
}

impl DebugHost for Vm {
    fn loc(&self) -> CodeLoc {
        if let Some(bt) = self.last_backtrace.as_ref()
            && let Some((loc, _)) = bt.first()
        {
            return *loc;
        }
        CodeLoc {
            chunk: self.current_chunk,
            pc: self.pc,
        }
    }

    fn call_depth(&self) -> usize {
        self.call_stack.len()
    }

    fn di(&self) -> &DebugInfo {
        &self.debug_info
    }

    fn dbg_continue(&mut self) {
        self.wqdb.clear_mode();
    }

    fn dbg_step_in(&mut self) {
        if crate::repl::get_debug_level() >= 2 {
            eprintln!("[wqdb]: dbg_step_in called at PC {}", self.pc);
        }
        self.wqdb.req_in(self.call_depth());
        if crate::repl::get_debug_level() >= 2 {
            eprintln!("[wqdb]: step-in mode on, will pause at next statement");
        }
    }

    fn dbg_step_over(&mut self) {
        // Step over: pause at the next statement encountered in the
        // current or outer frames (do not step into deeper frames).
        // Heuristic: also place a temporary breakpoint at the first
        // statement inside a forward-branch loop body (e.g. W[...]) so
        // 'next' on a loop header does not jump past the entire loop.
        self.wqdb.req_over(self.call_depth());
        let here = CodeLoc {
            chunk: self.current_chunk,
            pc: self.pc,
        };
        let meta = self.debug_info.chunk(here.chunk);
        // At a Return instruction, set up temp breaks in the caller
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc + 1..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            return;
        }

        // Add a forward-only temp break at the next stmt in this chunk
        // To guarantee progress at the last stmt of a function
        for pc in here.pc + 1..meta.len {
            if meta.line_table.is_stmt(pc) {
                self.wqdb.add_temp_break(CodeLoc {
                    chunk: here.chunk,
                    pc,
                });
                break;
            }
        }
        // If on a loop header (cond -> exit)
        // Pause at the first stmt inside the body
        let code = &self.instructions;
        // Find a nearby conditional jump with a forward target (typical loop header)
        let mut cond_pc_and_exit: Option<(usize, usize)> = None;
        for k in (here.pc.saturating_sub(16))..((here.pc + 32).min(code.len().saturating_sub(1))) {
            use crate::vm::instruction::Instruction::*;
            let hit = match code.get(k) {
                Some(JumpIfFalse(t)) if *t > k + 1 => Some((k, *t)),
                Some(JumpIfGE(t)) if *t > k + 1 => Some((k, *t)),
                Some(JumpIfLEZLocal(_, t)) if *t > k + 1 => Some((k, *t)),
                _ => None,
            };
            if let Some(pair) = hit {
                cond_pc_and_exit = Some(pair);
                break;
            }
        }
        // If at a Return instruction, set up temp breaks in the caller
        // And clear step mode to continue properly
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            self.wqdb.clear_mode();
            return;
        }
        if let Some((cond_pc, exit_pc)) = cond_pc_and_exit {
            // First stmt in [cond_pc+1, exit_pc)
            for pc in (cond_pc + 1)..exit_pc {
                if meta.line_table.is_stmt(pc) {
                    self.wqdb.add_temp_break(crate::wqdb::CodeLoc {
                        chunk: here.chunk,
                        pc,
                    });
                    break;
                }
            }
        }
    }

    fn dbg_step_out(&mut self) {
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            self.wqdb.clear_mode();
            return;
        }
        self.wqdb.req_out(self.call_depth());
        if !self.call_stack.is_empty() {
            let caller_frame = &self.call_stack[self.call_stack.len() - 1];
            let caller_meta = self.debug_info.chunk(caller_frame.chunk);
            // Look for the next statement after the call site
            for pc in caller_frame.pc..caller_meta.len {
                if caller_meta.line_table.is_stmt(pc) {
                    self.wqdb.add_temp_break(CodeLoc {
                        chunk: caller_frame.chunk,
                        pc,
                    });
                    break;
                }
            }
        }
    }

    fn dbg_set_break(&mut self, loc: CodeLoc) {
        self.wqdb.breaks.insert(loc);
    }

    fn dbg_clear_break(&mut self, loc: CodeLoc) {
        self.wqdb.breaks.remove(&loc);
    }

    fn dbg_breakpoints(&self) -> Vec<CodeLoc> {
        self.wqdb.breaks.iter().cloned().collect()
    }

    fn dbg_reset_breaks(&mut self) {
        self.wqdb.breaks.clear();
    }

    fn bt_frames(&self) -> Vec<(CodeLoc, std::sync::Arc<str>)> {
        // Prefer a captured backtrace snapshot if present (e.g., after a crash)
        if let Some(bt) = self.last_backtrace.as_ref() {
            return bt.clone();
        }
        let mut v = Vec::new();
        v.push((
            CodeLoc {
                chunk: self.current_chunk,
                pc: self.pc,
            },
            std::sync::Arc::from(self.func_name_for_chunk(self.current_chunk)),
        ));
        for fr in self.call_stack.iter().rev() {
            v.push((
                CodeLoc {
                    chunk: fr.chunk,
                    pc: fr.pc.saturating_sub(1),
                },
                fr.func_name.clone(),
            ));
        }
        v
    }

    fn dbg_globals(&self) -> Vec<(String, Value)> {
        self.globals
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn dbg_locals(&self) -> Vec<(usize, Value)> {
        if let Some(frame) = self.locals.last() {
            frame
                .iter()
                .enumerate()
                .map(|(idx, slot)| (idx, slot.read()))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn is_at_return(&self) -> bool {
        if self.pc < self.instructions.len() {
            matches!(
                self.instructions[self.pc],
                crate::vm::instruction::Instruction::Return
            )
        } else {
            false
        }
    }

    fn dbg_ins_at(&self, pc: usize) -> Option<String> {
        self.instructions.get(pc).map(|ins| format!("{ins:?}"))
    }
}
