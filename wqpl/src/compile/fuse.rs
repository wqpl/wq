use crate::ast::BinaryOperator;
use crate::compile::Compiler;
use crate::value::Value;
use crate::value::func::FunctionData;
use crate::vm::inst::{DebugStmtMark, Instruction, Operand};

impl Compiler {
    pub(crate) fn fuse(&mut self) {
        let mut stats = Stats::default();
        loop {
            let changed = fuse_once(
                &mut self.instructions,
                Some(&mut self.dbg_pc_spans),
                Some(&mut self.dbg_stmt_marks),
                &mut stats,
            );
            if !changed {
                break;
            }
        }
    }
}

#[derive(Default, Clone)]
struct Stats {
    slk_pop: usize,
    svk_pop: usize,
    idx_local_pop: usize,
    idx_global_pop: usize,
    lt_jifalse: usize,
    ll0_gt_jifalse: usize,
    cmp_jifalse: usize,
}

fn is_branch_comparison(op: BinaryOperator) -> bool {
    use BinaryOperator::*;

    matches!(
        op,
        Equal | EqualDot | NotEqual | NotEqualDot | Lt | Lte | Gt | Gte
    )
}

fn has_fusable_patterns(code: &[Instruction]) -> bool {
    use Instruction::*;

    use crate::value::Value;
    let n = code.len();
    if n == 0 {
        return false;
    }
    let mut i = 0usize;
    while i < n {
        // Early binary patterns (same as in fuse_once)
        if i + 1 < n {
            match (&code[i], &code[i + 1]) {
                (StoreLocalKeep(_), Pop)
                | (StoreVarKeep(_), Pop)
                | (StoreCaptureKeep(_), Pop)
                | (IndexAssignCapture(_), Pop)
                | (IndexAssignLocal(_), Pop)
                | (IndexAssignVar(_), Pop)
                | (IndexManyAssignCapture(_, _), Pop)
                | (IndexManyAssignLocal(_, _), Pop)
                | (IndexManyAssignVar(_, _), Pop)
                | (LoadLocal(_), Index) => return true,
                (LoadConst(_), Pop) => return true,
                _ => {}
            }
        }
        // 2-op fusion: BinaryOp(GT, Local(_), Const(0)); JIFalse
        if i + 1 < n
            && let BinaryOp(data) = &code[i]
            && data.op == BinaryOperator::Gt
            && let Operand::Local(_) = &data.left
            && let JumpIfFalse(_) = &code[i + 1]
            && matches!(&data.right, Operand::Const(box0) if matches!(&**box0, Value::Int(0)))
        {
            return true;
        }
        // LT ; JIFalse (stack-based operands only)
        if i + 1 < n
            && let BinaryOp(data) = &code[i]
            && data.op == BinaryOperator::Lt
            && let Operand::Stack = &data.left
            && let Operand::Stack = &data.right
            && let JumpIfFalse(_) = &code[i + 1]
        {
            return true;
        }
        // Generic comparison ; JIFalse
        if i + 1 < n
            && let BinaryOp(data) = &code[i]
            && is_branch_comparison(data.op)
            && let JumpIfFalse(_) = &code[i + 1]
        {
            return true;
        }
        // JIFalse to next -> Pop
        if let JumpIfFalse(pos) = &code[i]
            && (*pos == i + 1)
        {
            return true;
        }
        // Jump to immediate next -> remove
        if let Jump(pos) = &code[i]
            && (*pos == i + 1)
        {
            return true;
        }
        i += 1;
    }
    false
}

fn fuse_once(
    code: &mut Vec<Instruction>,
    dbg_pc_spans: Option<&mut Vec<Option<(usize, usize)>>>,
    dbg_stmt_marks: Option<&mut Vec<DebugStmtMark>>,
    stats: &mut Stats,
) -> bool {
    use Instruction::*;
    let mut changed_any = false;
    // Recurse into nested non-capturing functions using copy-on-write:
    // scan for patterns first; only clone and fuse when beneficial.
    for ins in code.iter_mut() {
        match ins {
            LoadConst(bv)
                if let Value::CompiledFunction(f) = &**bv
                    && has_fusable_patterns(&f.instructions) =>
            {
                let mut nested = f.instructions.to_vec();
                let mut nested_pc_spans = f
                    .dbg_pc_spans
                    .as_deref()
                    .map_or_else(Vec::new, |spans| spans.to_vec());
                let mut nested_stmt_marks = f
                    .dbg_stmt_marks
                    .as_deref()
                    .map_or_else(Vec::new, |marks| marks.to_vec());
                if fuse_once(
                    &mut nested,
                    Some(&mut nested_pc_spans),
                    Some(&mut nested_stmt_marks),
                    stats,
                ) {
                    *ins = Instruction::load_const(Value::CompiledFunction(std::sync::Arc::new(
                        FunctionData {
                            params: f.params.clone(),
                            named_params: f.named_params.clone(),
                            locals: f.locals,
                            isolated_module: f.isolated_module,
                            instructions: std::sync::Arc::<[Instruction]>::from(nested),
                            dbg_chunk: f.dbg_chunk,
                            dbg_stmt_spans: f.dbg_stmt_spans.clone(),
                            dbg_source_base_offset: f.dbg_source_base_offset,
                            dbg_pc_spans: Some(std::sync::Arc::from(nested_pc_spans)),
                            dbg_stmt_marks: Some(std::sync::Arc::from(nested_stmt_marks)),
                            dbg_local_names: f.dbg_local_names.clone(),
                            dbg_provenance: f.dbg_provenance.clone(),
                        },
                    )));
                    changed_any = true;
                }
            }

            LoadClosure(payload) if has_fusable_patterns(&payload.instructions) => {
                let mut nested = payload.instructions.to_vec();
                let mut nested_pc_spans = payload.dbg_pc_spans.to_vec();
                let mut nested_stmt_marks = payload.dbg_stmt_marks.to_vec();
                if fuse_once(
                    &mut nested,
                    Some(&mut nested_pc_spans),
                    Some(&mut nested_stmt_marks),
                    stats,
                ) {
                    payload.instructions = std::sync::Arc::<[Instruction]>::from(nested);
                    payload.dbg_pc_spans = std::sync::Arc::from(nested_pc_spans);
                    payload.dbg_stmt_marks = std::sync::Arc::from(nested_stmt_marks);
                    changed_any = true;
                }
            }

            _ => {}
        }
    }
    let old = std::mem::take(code);
    let n = old.len();
    if n == 0 {
        return changed_any;
    }
    let mut try_boundaries = vec![false; n + 1];
    for (index, instruction) in old.iter().enumerate() {
        if let Try(len) = instruction {
            let end = index.saturating_add(1).saturating_add(*len);
            if end <= n {
                try_boundaries[end] = true;
            }
        }
    }
    let mut keep = vec![true; n];
    let mut out: Vec<Instruction> = Vec::with_capacity(n);
    let mut origin: Vec<usize> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        // Early: eliminate StoreKeep; Pop and IndexAssign*; Pop
        if i + 1 < n && !try_boundaries[i + 1] {
            match (&old[i], &old[i + 1]) {
                // Purge LoadConst(anything); Pop
                (LoadConst(_), Pop) => {
                    keep[i] = false;
                    keep[i + 1] = false;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (StoreLocalKeep(slot), Pop) => {
                    out.push(StoreLocal(*slot));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    stats.slk_pop += 1;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (StoreVarKeep(name), Pop) => {
                    out.push(StoreVar(name.clone()));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    stats.svk_pop += 1;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (StoreCaptureKeep(slot), Pop) => {
                    out.push(StoreCapture(*slot));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (IndexAssignLocal(slot), Pop) => {
                    out.push(IndexAssignLocalDrop(*slot));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    stats.idx_local_pop += 1;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (IndexManyAssignLocal(slot, argc), Pop) => {
                    out.push(IndexManyAssignLocalDrop(*slot, *argc));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    stats.idx_local_pop += 1;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (IndexAssignCapture(slot), Pop) => {
                    out.push(IndexAssignCaptureDrop(*slot));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (IndexManyAssignCapture(slot, argc), Pop) => {
                    out.push(IndexManyAssignCaptureDrop(*slot, *argc));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (IndexAssignVar(name), Pop) => {
                    out.push(IndexAssignVarDrop(name.clone()));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    stats.idx_global_pop += 1;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                (IndexManyAssignVar(name, argc), Pop) => {
                    out.push(IndexManyAssignVarDrop(name.clone(), *argc));
                    origin.push(i);
                    keep[i] = true;
                    keep[i + 1] = false;
                    stats.idx_global_pop += 1;
                    changed_any = true;
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Jump to immediate successor -> remove
        if let Jump(pos) = &old[i]
            && (*pos == i + 1)
        {
            keep[i] = false;
            stats.slk_pop += 0; // no-op; keep stats type usage consistent
            changed_any = true;
            i += 1;
            continue;
        }
        // JIFalse to next -> Pop
        if let JumpIfFalse(pos) = &old[i]
            && (*pos == i + 1)
        {
            out.push(Pop);
            origin.push(i);
            keep[i] = true;
            changed_any = true;
            i += 1;
            continue;
        }
        // 2-op fusion: BinaryOp(GT, Local(slot), Const(0)); JIFalse T ->
        // JumpIfLEZLocal(slot, T)
        if i + 1 < n
            && !try_boundaries[i + 1]
            && let BinaryOp(data) = &old[i]
            && data.op == BinaryOperator::Gt
            && let Operand::Local(slot) = &data.left
            && let JumpIfFalse(pos) = &old[i + 1]
            && matches!(&data.right, Operand::Const(box0) if matches!(&**box0, Value::Int(0)))
        {
            out.push(JumpIfLEZLocal(*slot, *pos));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            stats.ll0_gt_jifalse += 1;
            changed_any = true;
            i += 2;
            continue;
        }
        // cmp+branch: BinaryOp(LT, Stack, Stack); JIFalse -> JGE (stack-based)
        if i + 1 < n
            && !try_boundaries[i + 1]
            && let BinaryOp(data) = &old[i]
            && data.op == BinaryOperator::Lt
            && let Operand::Stack = &data.left
            && let Operand::Stack = &data.right
            && let JumpIfFalse(pos) = &old[i + 1]
        {
            out.push(JumpIfGE(*pos));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            stats.lt_jifalse += 1;
            changed_any = true;
            i += 2;
            continue;
        }
        // cmp+branch: BinaryOp(cmp, lhs, rhs); JIFalse -> JumpIfCmpFalse.
        if i + 1 < n
            && !try_boundaries[i + 1]
            && let BinaryOp(data) = &old[i]
            && is_branch_comparison(data.op)
            && let JumpIfFalse(pos) = &old[i + 1]
        {
            out.push(Instruction::jump_if_cmp_false(
                data.op,
                data.left.clone(),
                data.right.clone(),
                *pos,
            ));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            stats.cmp_jifalse += 1;
            changed_any = true;
            i += 2;
            continue;
        }
        out.push(old[i].clone());
        origin.push(i);
        keep[i] = true;
        i += 1;
    }
    if changed_any {
        // Build mapping from old index -> new index of first kept instruction at or
        // after old index
        let mut old_to_new: Vec<usize> = vec![out.len(); n + 1];
        let mut next_kept = vec![n; n + 1];
        let mut next = n;
        for idx in (0..n).rev() {
            if keep[idx] {
                next = idx;
            }
            next_kept[idx] = next;
        }
        next_kept[n] = n;
        let mut kept_to_new: Vec<isize> = vec![-1; n];
        for (new_idx, &orig) in origin.iter().enumerate() {
            kept_to_new[orig] = new_idx as isize;
        }
        for old_idx in 0..=n {
            let nk = next_kept[old_idx.min(n)];
            if nk < n {
                old_to_new[old_idx] = usize::try_from(kept_to_new[nk])
                    .expect("kept instruction index is non-negative");
            } else {
                old_to_new[old_idx] = out.len();
            }
        }
        // Remap jump targets to new indices
        for (new_idx, ins) in out.iter_mut().enumerate() {
            match ins {
                Jump(pos) | JumpIfFalse(pos) | JumpIfGE(pos) => {
                    *pos = old_to_new[*pos];
                }
                JumpIfLEZLocal(_, pos) => {
                    *pos = old_to_new[*pos];
                }
                JumpIfNamedProvided(_, _, pos) => {
                    *pos = old_to_new[*pos];
                }
                JumpIfCmpFalse(data) => {
                    data.target = old_to_new[data.target];
                }
                NLoopEnter(data) => {
                    data.target = old_to_new[data.target];
                }
                NLoopNext(data) => {
                    data.target = old_to_new[data.target];
                }
                BoolAndLazy(pos) | BoolOrLazy(pos) => {
                    *pos = old_to_new[*pos];
                }
                Try(len) => {
                    let old_idx = origin[new_idx];
                    let old_end = old_idx.saturating_add(1).saturating_add(*len);
                    debug_assert!(
                        old_end <= n,
                        "try body extends beyond its instruction slice"
                    );
                    let new_end = old_to_new[old_end.min(n)];
                    *len = new_end.saturating_sub(new_idx + 1);
                }
                _ => {}
            }
        }
        if let Some(pc_spans) = dbg_pc_spans {
            let old_spans = std::mem::take(pc_spans);
            pc_spans.reserve(out.len());
            for &orig in &origin {
                pc_spans.push(old_spans.get(orig).copied().flatten());
            }
        }
        if let Some(stmt_marks) = dbg_stmt_marks {
            let old_marks = std::mem::take(stmt_marks);
            stmt_marks.reserve(old_marks.len());
            for mark in old_marks {
                let new_pc = old_to_new[mark.pc.min(n)];
                if new_pc < out.len() {
                    stmt_marks.push(DebugStmtMark { pc: new_pc, ..mark });
                }
            }
            stmt_marks.sort_unstable_by_key(|mark| (mark.pc, mark.start, mark.end));
            stmt_marks.dedup_by(|a, b| a.pc == b.pc && a.start == b.start && a.end == b.end);
        }
        *code = out;
    } else {
        *code = old;
    }
    changed_any
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuses_capture_store_pop_and_preserves_debug_origin() {
        let mut code = vec![
            Instruction::StoreCaptureKeep(7),
            Instruction::Pop,
            Instruction::Return,
        ];
        let mut spans = vec![Some((10, 11)), Some((20, 21)), Some((30, 31))];
        let mut marks = vec![
            DebugStmtMark {
                pc: 0,
                start: 10,
                end: 11,
            },
            DebugStmtMark {
                pc: 2,
                start: 30,
                end: 31,
            },
        ];
        let mut stats = Stats::default();

        let changed = fuse_once(&mut code, Some(&mut spans), Some(&mut marks), &mut stats);

        assert!(changed);
        assert_eq!(
            code,
            vec![Instruction::StoreCapture(7), Instruction::Return]
        );
        assert_eq!(spans, vec![Some((10, 11)), Some((30, 31))]);
        assert_eq!(
            marks,
            vec![
                DebugStmtMark {
                    pc: 0,
                    start: 10,
                    end: 11,
                },
                DebugStmtMark {
                    pc: 1,
                    start: 30,
                    end: 31,
                },
            ]
        );
    }

    #[test]
    fn fuses_local_compare_jump_false_and_remaps_targets() {
        let mut code = vec![
            Instruction::binary_op(BinaryOperator::Lt, Operand::Local(0), Operand::Local(1)),
            Instruction::JumpIfFalse(4),
            Instruction::load_const(Value::Int(1)),
            Instruction::Jump(5),
            Instruction::load_const(Value::Int(2)),
            Instruction::Return,
        ];
        let mut stats = Stats::default();

        let changed = fuse_once(&mut code, None, None, &mut stats);

        assert!(changed);
        assert_eq!(
            code,
            vec![
                Instruction::jump_if_cmp_false(
                    BinaryOperator::Lt,
                    Operand::Local(0),
                    Operand::Local(1),
                    3,
                ),
                Instruction::load_const(Value::Int(1)),
                Instruction::Jump(4),
                Instruction::load_const(Value::Int(2)),
                Instruction::Return,
            ]
        );
        assert_eq!(stats.cmp_jifalse, 1);
    }

    #[test]
    fn preserves_local_gt_zero_special_case() {
        let mut code = vec![
            Instruction::binary_op(
                BinaryOperator::Gt,
                Operand::Local(0),
                Operand::const_val(Value::Int(0)),
            ),
            Instruction::JumpIfFalse(3),
            Instruction::load_const(Value::Int(1)),
            Instruction::Return,
        ];
        let mut stats = Stats::default();

        let changed = fuse_once(&mut code, None, None, &mut stats);

        assert!(changed);
        assert_eq!(
            code,
            vec![
                Instruction::JumpIfLEZLocal(0, 2),
                Instruction::load_const(Value::Int(1)),
                Instruction::Return,
            ]
        );
        assert_eq!(stats.ll0_gt_jifalse, 1);
        assert_eq!(stats.cmp_jifalse, 0);
    }

    #[test]
    fn remaps_try_body_length_when_fusion_removes_instructions() {
        let mut code = vec![
            Instruction::Try(5),
            Instruction::binary_op(
                BinaryOperator::Equal,
                Operand::Local(0),
                Operand::const_val(Value::Int(1)),
            ),
            Instruction::JumpIfFalse(5),
            Instruction::load_const(Value::Int(42)),
            Instruction::Jump(6),
            Instruction::load_const(Value::Int(0)),
            Instruction::Return,
        ];
        let mut stats = Stats::default();

        let changed = fuse_once(&mut code, None, None, &mut stats);

        assert!(changed);
        assert_eq!(code.len(), 6);
        assert_eq!(code[0], Instruction::Try(4));
        assert!(matches!(code[1], Instruction::JumpIfCmpFalse(_)));
        assert_eq!(code[3], Instruction::Jump(5));
        assert_eq!(code[5], Instruction::Return);
    }

    #[test]
    fn does_not_fuse_across_try_boundary() {
        let original = vec![
            Instruction::Try(1),
            Instruction::binary_op(
                BinaryOperator::Equal,
                Operand::Local(0),
                Operand::const_val(Value::Int(1)),
            ),
            Instruction::JumpIfFalse(4),
            Instruction::load_const(Value::Int(42)),
            Instruction::Return,
        ];
        let mut code = original.clone();
        let mut stats = Stats::default();

        let changed = fuse_once(&mut code, None, None, &mut stats);

        assert!(!changed);
        assert_eq!(code, original);
    }
}
