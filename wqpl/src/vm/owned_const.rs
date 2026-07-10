use crate::value::Value;
use crate::vm::inst::Instruction;

pub(crate) fn extract_owned_consts(instructions: &mut [Instruction]) -> Vec<Option<Value>> {
    let eligible = at_most_once_pcs(instructions);
    let mut owned = Vec::new();

    for (pc, inst) in instructions.iter_mut().enumerate() {
        if !eligible[pc] {
            continue;
        }
        let Instruction::LoadConst(value) = inst else {
            continue;
        };
        if !has_shared_backing(value) {
            continue;
        }

        let slot = owned.len();
        let old = std::mem::replace(inst, Instruction::LoadOwnedConst(slot));
        let Instruction::LoadConst(value) = old else {
            unreachable!("replaced instruction should be LoadConst");
        };
        owned.push(Some(*value));
    }

    owned
}

fn at_most_once_pcs(instructions: &[Instruction]) -> Vec<bool> {
    let graph = control_flow_graph(instructions);
    let cyclic = cyclic_pcs(&graph);
    cyclic.into_iter().map(|in_cycle| !in_cycle).collect()
}

fn control_flow_graph(instructions: &[Instruction]) -> Vec<Vec<usize>> {
    instructions
        .iter()
        .enumerate()
        .map(|(pc, inst)| successors(pc, inst, instructions.len()))
        .collect()
}

fn successors(pc: usize, inst: &Instruction, len: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(2);
    match inst {
        Instruction::Jump(target) => push_target(&mut out, *target, len),
        Instruction::JumpIfFalse(target)
        | Instruction::JumpIfGE(target)
        | Instruction::BoolAndLazy(target)
        | Instruction::BoolOrLazy(target) => {
            push_fallthrough(&mut out, pc, len);
            push_target(&mut out, *target, len);
        }
        Instruction::JumpIfCmpFalse(data) => {
            push_fallthrough(&mut out, pc, len);
            push_target(&mut out, data.target, len);
        }
        Instruction::JumpIfLEZLocal(_, target) => {
            push_fallthrough(&mut out, pc, len);
            push_target(&mut out, *target, len);
        }
        Instruction::JumpIfNamedProvided(_, _, target) => {
            push_fallthrough(&mut out, pc, len);
            push_target(&mut out, *target, len);
        }
        Instruction::Try(body_len) => {
            if *body_len > 0 {
                push_fallthrough(&mut out, pc, len);
            }
            push_target(&mut out, pc + 1 + body_len, len);
        }
        Instruction::Return => {}
        _ => push_fallthrough(&mut out, pc, len),
    }
    out
}

fn push_fallthrough(out: &mut Vec<usize>, pc: usize, len: usize) {
    push_target(out, pc + 1, len);
}

fn push_target(out: &mut Vec<usize>, target: usize, len: usize) {
    if target < len && !out.contains(&target) {
        out.push(target);
    }
}

fn cyclic_pcs(graph: &[Vec<usize>]) -> Vec<bool> {
    let len = graph.len();
    let mut visited = vec![false; len];
    let mut order = Vec::with_capacity(len);
    for start in 0..len {
        if !visited[start] {
            push_postorder(start, graph, &mut visited, &mut order);
        }
    }

    let reverse = reverse_graph(graph);
    let mut cyclic = vec![false; len];
    let mut assigned = vec![false; len];
    for start in order.into_iter().rev() {
        if assigned[start] {
            continue;
        }
        let mut component = Vec::new();
        collect_component(start, &reverse, &mut assigned, &mut component);
        let is_cycle = component.len() > 1
            || component
                .iter()
                .any(|pc| graph[*pc].iter().any(|target| target == pc));
        if is_cycle {
            for pc in component {
                cyclic[pc] = true;
            }
        }
    }
    cyclic
}

fn push_postorder(
    start: usize,
    graph: &[Vec<usize>],
    visited: &mut [bool],
    order: &mut Vec<usize>,
) {
    visited[start] = true;
    let mut stack = vec![(start, 0usize)];
    while let Some((node, next_child)) = stack.last_mut() {
        if *next_child < graph[*node].len() {
            let child = graph[*node][*next_child];
            *next_child += 1;
            if !visited[child] {
                visited[child] = true;
                stack.push((child, 0));
            }
        } else {
            let (node, _) = stack.pop().expect("postorder stack is not empty");
            order.push(node);
        }
    }
}

fn reverse_graph(graph: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut reverse = vec![Vec::new(); graph.len()];
    for (source, targets) in graph.iter().enumerate() {
        for target in targets {
            reverse[*target].push(source);
        }
    }
    reverse
}

fn collect_component(
    start: usize,
    graph: &[Vec<usize>],
    assigned: &mut [bool],
    component: &mut Vec<usize>,
) {
    assigned[start] = true;
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        component.push(node);
        for child in &graph[node] {
            if !assigned[*child] {
                assigned[*child] = true;
                stack.push(*child);
            }
        }
    }
}

fn has_shared_backing(value: &Value) -> bool {
    match value {
        Value::BigInt(_)
        | Value::Fraction(_)
        | Value::Algebraic(_)
        | Value::Tag(_)
        | Value::IntList(_)
        | Value::IntRange(_)
        | Value::FloatList(_)
        | Value::BoolList(_)
        | Value::List(_)
        | Value::String(_)
        | Value::Cas(_)
        | Value::Dict(_) => true,
        Value::Int(_)
        | Value::Float(_)
        | Value::Complex(_)
        | Value::Char(_)
        | Value::Bool(_)
        | Value::CompiledFunction(_)
        | Value::Closure(_)
        | Value::BuiltinFunction { .. }
        | Value::LiftedCallable(_)
        | Value::Stream(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn extracts_acyclic_shared_constants() {
        let backing = Arc::new(vec![1, 2, 3]);
        let mut instructions = vec![
            Instruction::load_const(Value::IntList(backing.clone())),
            Instruction::StoreVar("a".into()),
        ];

        let owned = extract_owned_consts(&mut instructions);

        assert!(matches!(instructions[0], Instruction::LoadOwnedConst(0)));
        assert_eq!(owned.len(), 1);
        assert!(matches!(&owned[0], Some(Value::IntList(items)) if Arc::ptr_eq(items, &backing)));
    }

    #[test]
    fn keeps_cyclic_constants_in_instructions() {
        let mut instructions = vec![
            Instruction::load_const(Value::IntList(Arc::new(vec![1, 2, 3]))),
            Instruction::Jump(0),
        ];

        let owned = extract_owned_consts(&mut instructions);

        assert!(owned.is_empty());
        assert!(matches!(instructions[0], Instruction::LoadConst(_)));
    }

    #[test]
    fn leaves_immediate_constants_inline() {
        let mut instructions = vec![Instruction::load_const(Value::Int(1))];

        let owned = extract_owned_consts(&mut instructions);

        assert!(owned.is_empty());
        assert!(matches!(instructions[0], Instruction::LoadConst(_)));
    }
}
