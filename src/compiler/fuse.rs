use crate::{
    astnode::BinaryOperator, compiler::Compiler, value::Value, vm::instruction::Instruction,
};

impl Compiler {
    pub fn fuse(&mut self) {
        let mut stats = Stats::default();
        loop {
            let changed = fuse_once(&mut self.instructions, &mut stats);
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
    inc1_local: usize,
    inc1_local_keep: usize,
    inc1_var: usize,
    inc1_var_keep: usize,
    inc1_from_local: usize,
    inc1_var_from_var: usize,
}

fn has_fusable_patterns(code: &[Instruction]) -> bool {
    use crate::value::Value;
    use Instruction::*;
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
                | (IndexAssignLocal(_), Pop)
                | (IndexAssign, Pop) => return true,
                _ => {}
            }
        }
        // Inc1Local: LL j; LC 1; Add; SL j
        if i + 3 < n
            && let (
                LoadLocal(a),
                LoadConst(Value::Int(1)),
                BinaryOp(crate::astnode::BinaryOperator::Add),
                StoreLocal(b),
            ) = (&code[i], &code[i + 1], &code[i + 2], &code[i + 3])
            && a == b
        {
            return true;
        }
        // Inc1Var: LV name; LC 1; Add; SV name
        if i + 3 < n
            && let (
                LoadVar(na),
                LoadConst(Value::Int(1)),
                BinaryOp(crate::astnode::BinaryOperator::Add),
                StoreVar(nb),
            ) = (&code[i], &code[i + 1], &code[i + 2], &code[i + 3])
            && na == nb
        {
            return true;
        }
        // Inc1VarKeep: LV name; LC 1; Add; SVK name
        if i + 3 < n
            && let (
                LoadVar(na),
                LoadConst(Value::Int(1)),
                BinaryOp(crate::astnode::BinaryOperator::Add),
                StoreVarKeep(nb),
            ) = (&code[i], &code[i + 1], &code[i + 2], &code[i + 3])
            && na == nb
        {
            return true;
        }
        // Inc1LocalKeep: LL j; LC 1; Add; SLK j
        if i + 3 < n
            && let (
                LoadLocal(a),
                LoadConst(Value::Int(1)),
                BinaryOp(crate::astnode::BinaryOperator::Add),
                StoreLocalKeep(b),
            ) = (&code[i], &code[i + 1], &code[i + 2], &code[i + 3])
            && a == b
        {
            return true;
        }
        // Inc1LocalFromLocal (dedicated N-loop tail): LL src; LC 1; Add; SL dst (src!=dst)
        if i + 3 < n
            && let (
                LoadLocal(src),
                LoadConst(Value::Int(1)),
                BinaryOp(crate::astnode::BinaryOperator::Add),
                StoreLocal(dst),
            ) = (&code[i], &code[i + 1], &code[i + 2], &code[i + 3])
            && src != dst
        {
            return true;
        }
        // Inc1VarFromVar (global N-loop tail): LV src; LC 1; Add; SV dst (src!=dst)
        if i + 3 < n
            && let (
                LoadVar(src),
                LoadConst(Value::Int(1)),
                BinaryOp(crate::astnode::BinaryOperator::Add),
                StoreVar(dst),
            ) = (&code[i], &code[i + 1], &code[i + 2], &code[i + 3])
            && src != dst
        {
            return true;
        }
        // 4-op fusion LL; LC 0; GT; JIFalse
        if i + 3 < n
            && let (
                LoadLocal(_),
                LoadConst(Value::Int(0)),
                BinaryOp(crate::astnode::BinaryOperator::GreaterThan),
                JumpIfFalse(_),
            ) = (&code[i], &code[i + 1], &code[i + 2], &code[i + 3])
        {
            return true;
        }
        // LT ; JIFalse
        if i + 1 < n
            && let (BinaryOp(crate::astnode::BinaryOperator::LessThan), JumpIfFalse(_)) =
                (&code[i], &code[i + 1])
        {
            return true;
        }
        // BinaryOp(Divide); CallBuiltinId(FLOOR, 1) -> FloorDiv
        if i + 1 < n
            && let (BinaryOp(crate::astnode::BinaryOperator::Divide), CallBuiltinId(id, argc)) =
                (&code[i], &code[i + 1])
            && *argc == 1
            && *id == crate::builtins::Builtins::FLOOR
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

fn fuse_once(code: &mut Vec<Instruction>, stats: &mut Stats) -> bool {
    use Instruction::*;
    let mut changed_any = false;
    // Recurse into nested non-capturing functions using copy-on-write:
    // scan for patterns first; only clone and fuse when beneficial.
    for ins in code.iter_mut() {
        if let LoadConst(Value::CompiledFunction { instructions, .. }) = ins
            && has_fusable_patterns(instructions)
        {
            let mut nested = instructions.to_vec();
            if fuse_once(&mut nested, stats) {
                *ins = LoadConst(Value::CompiledFunction {
                    params: match ins {
                        LoadConst(Value::CompiledFunction { params, .. }) => params.clone(),
                        _ => None,
                    },
                    locals: match ins {
                        LoadConst(Value::CompiledFunction { locals, .. }) => *locals,
                        _ => 0,
                    },
                    instructions: std::sync::Arc::<[Instruction]>::from(nested),
                    dbg_chunk: match ins {
                        LoadConst(Value::CompiledFunction { dbg_chunk, .. }) => *dbg_chunk,
                        _ => None,
                    },
                    dbg_stmt_spans: match ins {
                        LoadConst(Value::CompiledFunction { dbg_stmt_spans, .. }) => {
                            dbg_stmt_spans.clone()
                        }
                        _ => None,
                    },
                    dbg_local_names: match ins {
                        LoadConst(Value::CompiledFunction {
                            dbg_local_names, ..
                        }) => dbg_local_names.clone(),
                        _ => None,
                    },
                });
                changed_any = true;
            }
        }
    }
    let old: Vec<Instruction> = code.clone();
    let n = old.len();
    if n == 0 {
        return changed_any;
    }
    let mut keep = vec![true; n];
    let mut out: Vec<Instruction> = Vec::with_capacity(n);
    let mut origin: Vec<usize> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        // Early: eliminate StoreKeep; Pop and IndexAssign*; Pop
        if i + 1 < n {
            match (&old[i], &old[i + 1]) {
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
                (IndexAssign, Pop) => {
                    out.push(IndexAssignDrop);
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
        // BinaryOp(Divide); CallBuiltinId(FLOOR, 1) -> FloorDiv
        if i + 1 < n
            && let (BinaryOp(BinaryOperator::Divide), CallBuiltinId(id, argc)) =
                (&old[i], &old[i + 1])
            && *argc == 1
            && *id == crate::builtins::Builtins::FLOOR
        {
            out.push(Instruction::FloorDiv);
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            changed_any = true;
            i += 2;
            continue;
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
        // 4-op fusion: LL j; LC 0; GreaterThan; JIFalse T -> JumpIfLEZLocal(j, T)
        if i + 3 < n
            && let (
                LoadLocal(slot),
                LoadConst(Value::Int(0)),
                BinaryOp(BinaryOperator::GreaterThan),
                JumpIfFalse(pos),
            ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
        {
            out.push(JumpIfLEZLocal(*slot, *pos));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            keep[i + 2] = false;
            keep[i + 3] = false;
            stats.ll0_gt_jifalse += 1;
            changed_any = true;
            i += 4;
            continue;
        }
        // cmp+branch: LT; JIFalse -> JGE (stack-based)
        if i + 1 < n
            && let (BinaryOp(BinaryOperator::LessThan), JumpIfFalse(pos)) = (&old[i], &old[i + 1])
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
        // Inc1Local: LL j; LC 1; Add; SL j
        if i + 3 < n
            && let (
                LoadLocal(a),
                LoadConst(Value::Int(1)),
                BinaryOp(BinaryOperator::Add),
                StoreLocal(b),
            ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
            && a == b
        {
            out.push(Inc1Local(*a));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            keep[i + 2] = false;
            keep[i + 3] = false;
            stats.inc1_local += 1;
            changed_any = true;
            i += 4;
            continue;
        }
        // Inc1Var: LV name; LC 1; Add; SV name
        if i + 3 < n
            && let (
                LoadVar(na),
                LoadConst(Value::Int(1)),
                BinaryOp(BinaryOperator::Add),
                StoreVar(nb),
            ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
            && na == nb
        {
            out.push(Inc1Var(na.clone()));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            keep[i + 2] = false;
            keep[i + 3] = false;
            stats.inc1_var += 1;
            changed_any = true;
            i += 4;
            continue;
        }
        // Inc1VarKeep: LV name; LC 1; Add; SVK name
        if i + 3 < n
            && let (
                LoadVar(na),
                LoadConst(Value::Int(1)),
                BinaryOp(BinaryOperator::Add),
                StoreVarKeep(nb),
            ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
            && na == nb
        {
            out.push(Inc1VarKeep(na.clone()));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            keep[i + 2] = false;
            keep[i + 3] = false;
            stats.inc1_var_keep += 1;
            changed_any = true;
            i += 4;
            continue;
        }
        // Inc1LocalKeep: LL j; LC 1; Add; SLK j
        if i + 3 < n
            && let (
                LoadLocal(a),
                LoadConst(Value::Int(1)),
                BinaryOp(BinaryOperator::Add),
                StoreLocalKeep(b),
            ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
            && a == b
        {
            out.push(Inc1LocalKeep(*a));
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            keep[i + 2] = false;
            keep[i + 3] = false;
            stats.inc1_local_keep += 1;
            changed_any = true;
            i += 4;
            continue;
        }
        // Dedicated N-loop tail: LL src; LC 1; Add; SL dst (src != dst)
        if i + 3 < n
            && let (
                LoadLocal(src),
                LoadConst(Value::Int(1)),
                BinaryOp(BinaryOperator::Add),
                StoreLocal(dst),
            ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
            && src != dst
        {
            out.push(Inc1LocalFromLocal {
                src: *src,
                dst: *dst,
            });
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            keep[i + 2] = false;
            keep[i + 3] = false;
            stats.inc1_from_local += 1;
            changed_any = true;
            i += 4;
            continue;
        }
        // Dedicated N-loop tail (globals): LV src; LC 1; Add; SV dst (src != dst)
        if i + 3 < n
            && let (
                LoadVar(src),
                LoadConst(Value::Int(1)),
                BinaryOp(BinaryOperator::Add),
                StoreVar(dst),
            ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
            && src != dst
        {
            out.push(Inc1VarFromVar {
                src: src.clone(),
                dst: dst.clone(),
            });
            origin.push(i);
            keep[i] = true;
            keep[i + 1] = false;
            keep[i + 2] = false;
            keep[i + 3] = false;
            stats.inc1_var_from_var += 1;
            changed_any = true;
            i += 4;
            continue;
        }
        out.push(old[i].clone());
        origin.push(i);
        keep[i] = true;
        i += 1;
    }
    if changed_any {
        // Build mapping from old index -> new index of first kept instruction at or after old index
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
                old_to_new[old_idx] = kept_to_new[nk] as usize;
            } else {
                old_to_new[old_idx] = out.len();
            }
        }
        // Remap jump targets to new indices
        for ins in &mut out {
            match ins {
                Jump(pos) | JumpIfFalse(pos) | JumpIfGE(pos) => {
                    *pos = old_to_new[*pos];
                }
                JumpIfLEZLocal(_, pos) => {
                    *pos = old_to_new[*pos];
                }
                _ => {}
            }
        }
        *code = out;
    }
    changed_any
}
