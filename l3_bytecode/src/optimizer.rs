use std::{cmp::Ordering, collections::VecDeque};

use l3_runtime::{
    Function, Heap, HeapCell, HeapData, Primitive, StackValue,
    heap_data::{add, compare, div, modulo, mul, negative, not_op, pow, sub, to_owned},
};

use crate::{Chunk, Instruction, ProgramBytecode};

/// A whole-program bytecode optimizer. Runs after compilation, before
/// execution.
///
/// Passes (per chunk, iterated to a fixpoint):
/// 1. dead code elimination (unreachable regions after `Jump`/`Return`),
/// 2. constant propagation through local slots,
/// 3. folding of constant-to-constant operation sequences.
#[derive(Debug, Default)]
pub struct Optimizer;

impl Optimizer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn optimize(&self, mut program: ProgramBytecode) -> ProgramBytecode {
        for _ in 0..4 {
            let mut changed = false;
            let arities = chunk_arities(&program);
            for (ci, chunk_ref) in program.chunks.iter_mut().enumerate() {
                let chunk = std::mem::take(chunk_ref);
                let (optimized, pass_changed) =
                    optimize_chunk(chunk, &mut program.constants, arities[ci]);
                *chunk_ref = optimized;
                changed |= pass_changed;
            }
            if !changed {
                break;
            }
        }
        program
    }
}

/// Number of parameters of each chunk. Function chunks store their params in
/// the first `arity` stack slots of the frame, so the abstract stack must
/// reserve that many unknown entries at the bottom.
fn chunk_arities(program: &ProgramBytecode) -> Vec<usize> {
    let mut arities = vec![0; program.chunks.len()];
    for cell in &program.constants {
        if let HeapData::Function(Function::Bytecode(bc)) = &cell.value
            && bc.id < arities.len()
        {
            arities[bc.id] = bc.arity;
        }
    }
    arities
}

fn optimize_chunk(chunk: Chunk, pool: &mut Vec<HeapCell>, arity: usize) -> (Chunk, bool) {
    let mut changed = false;
    let (chunk, c) = remove_dead_code(chunk);
    changed |= c;
    let (chunk, c) = propagate_constants(chunk, arity);
    changed |= c;
    let (chunk, c) = fold_constants(&chunk, pool);
    changed |= c;
    (chunk, changed)
}

// ---------------------------------------------------------------------------
// Dead code elimination
// ---------------------------------------------------------------------------

fn remove_dead_code(chunk: Chunk) -> (Chunk, bool) {
    let len = chunk.code.len();
    let mut reachable = vec![false; len];
    let mut queue = VecDeque::new();
    if len > 0 {
        reachable[0] = true;
        queue.push_back(0);
    }
    while let Some(i) = queue.pop_front() {
        for succ in successors(&chunk.code, i) {
            if succ < len && !reachable[succ] {
                reachable[succ] = true;
                queue.push_back(succ);
            }
        }
    }

    if reachable.iter().all(|&r| r) {
        return (chunk, false);
    }

    let mut new_code = Vec::with_capacity(len);
    let mut new_locations = Vec::with_capacity(len);
    let mut old_to_new = vec![0; len];
    for i in 0..len {
        if reachable[i] {
            old_to_new[i] = new_code.len();
            new_code.push(chunk.code[i].clone());
            new_locations.push(chunk.locations[i].clone());
        } else {
            old_to_new[i] = new_code.len().saturating_sub(1);
        }
    }
    for inst in &mut new_code {
        remap_offsets(inst, &old_to_new);
    }

    (
        Chunk {
            code: new_code,
            locations: new_locations,
        },
        true,
    )
}

fn successors(code: &[Instruction], i: usize) -> Vec<usize> {
    match &code[i] {
        Instruction::Return => Vec::new(),
        Instruction::Jump { offset } => vec![*offset],
        Instruction::JumpIf { offset, .. } => vec![*offset, i + 1],
        Instruction::ForLoop { body_offset, .. } => vec![*body_offset, i + 1],
        _ => vec![i + 1],
    }
}

fn remap_offsets(instruction: &mut Instruction, old_to_new: &[usize]) {
    let remap = |offset: &mut usize| {
        if *offset < old_to_new.len() {
            *offset = old_to_new[*offset];
        }
    };
    match instruction {
        Instruction::Jump { offset } | Instruction::JumpIf { offset, .. } => remap(offset),
        Instruction::ForLoop { body_offset, .. } => remap(body_offset),
        _ => {},
    }
}

// ---------------------------------------------------------------------------
// Constant propagation
// ---------------------------------------------------------------------------

/// Abstract stack value: `Some(pool_index)` when the runtime stack entry is
/// provably the constant at that pool index, `None` otherwise.
type AbstractValue = Option<usize>;
/// Abstract model of the runtime stack (relative to the frame pointer). Local
/// slots live at the bottom of this stack, matching the VM's layout.
type Stack = Vec<AbstractValue>;

#[derive(Clone, Copy)]
struct Block {
    start: usize,
    end: usize,
}

fn propagate_constants(mut chunk: Chunk, arity: usize) -> (Chunk, bool) {
    let blocks = build_blocks(&chunk.code);
    let n = blocks.len();
    let mut index_to_block = vec![0; chunk.code.len()];
    for (bi, block) in blocks.iter().enumerate() {
        index_to_block[block.start..block.end].fill(bi);
    }

    let mut in_stacks: Vec<Option<Stack>> = vec![None; n];
    let mut queued = vec![false; n];
    if n > 0 {
        in_stacks[0] = Some(vec![None; arity]);
        queued[0] = true;
    }
    let mut queue: VecDeque<usize> = (0..n).filter(|&bi| queued[bi]).collect();
    while let Some(bi) = queue.pop_front() {
        queued[bi] = false;
        let Some(in_stack) = &in_stacks[bi] else {
            continue;
        };
        let outs = transfer(&chunk.code, &blocks[bi], &index_to_block, in_stack);
        for (succ, out) in outs {
            let merged = match &mut in_stacks[succ] {
                None => {
                    in_stacks[succ] = Some(out);
                    true
                },
                Some(dst) => merge_stack(dst, &out),
            };
            if merged && !queued[succ] {
                queued[succ] = true;
                queue.push_back(succ);
            }
        }
    }

    let mut changed = false;
    for (bi, block) in blocks.iter().enumerate() {
        let Some(in_stack) = &in_stacks[bi] else {
            continue;
        };
        let mut stack = in_stack.clone();
        for inst in &mut chunk.code[block.start..block.end] {
            if let Instruction::GetLocal { index } = inst {
                let known = stack.get(*index).copied().flatten();
                if let Some(k) = known {
                    changed = true;
                    *inst = Instruction::Constant { index: k };
                }
                stack.push(known);
            } else {
                step(inst, &mut stack);
            }
        }
    }
    (chunk, changed)
}

fn build_blocks(code: &[Instruction]) -> Vec<Block> {
    let len = code.len();
    let mut starts = vec![false; len + 1];
    starts[0] = true;
    for (i, inst) in code.iter().enumerate() {
        match inst {
            Instruction::Jump { offset } | Instruction::JumpIf { offset, .. } if *offset < len => {
                starts[*offset] = true;
            },
            Instruction::ForLoop { body_offset, .. } if *body_offset < len => {
                starts[*body_offset] = true;
            },
            _ => {},
        }
        if matches!(
            inst,
            Instruction::Jump { .. }
                | Instruction::JumpIf { .. }
                | Instruction::ForLoop { .. }
                | Instruction::Return
        ) {
            starts[i + 1] = true;
        }
    }

    let mut blocks = Vec::new();
    let mut start = 0;
    for (end, &is_start) in starts.iter().enumerate() {
        if is_start {
            if end > start {
                blocks.push(Block { start, end });
            }
            start = end;
        }
    }
    blocks
}

/// Simulate a straight-line block, producing the outgoing abstract stack for
/// each successor. Control-flow instructions are always the last in their
/// block.
fn transfer(
    code: &[Instruction],
    block: &Block,
    index_to_block: &[usize],
    in_stack: &Stack,
) -> Vec<(usize, Stack)> {
    let mut stack = in_stack.clone();
    let last = block.end - 1;
    for inst in &code[block.start..last] {
        step(inst, &mut stack);
    }
    match &code[last] {
        Instruction::Return => Vec::new(),
        Instruction::Jump { offset } => vec![(index_to_block[*offset], stack)],
        Instruction::JumpIf {
            offset,
            keep_stay,
            keep_jump,
            ..
        } => {
            let cond = stack.pop();
            let mut taken = stack.clone();
            let mut stay = stack;
            if *keep_jump {
                taken.push(cond.flatten());
            }
            if *keep_stay {
                stay.push(cond.flatten());
            }
            let mut out = vec![(index_to_block[*offset], taken)];
            if block.end < code.len() {
                out.push((index_to_block[block.end], stay));
            }
            out
        },
        Instruction::ForLoop {
            control_index,
            body_offset,
            ..
        } => {
            if *control_index >= stack.len() {
                stack.resize(*control_index + 1, None);
            }
            stack[*control_index] = None;
            let mut out = vec![(index_to_block[*body_offset], stack.clone())];
            if block.end < code.len() {
                out.push((index_to_block[block.end], stack));
            }
            out
        },
        _ => {
            step(&code[last], &mut stack);
            if block.end < code.len() {
                vec![(index_to_block[block.end], stack)]
            } else {
                Vec::new()
            }
        },
    }
}

/// Intersect a predecessor's abstract stack into the successor's incoming
/// stack. A stack entry is only known when all predecessors agree.
fn merge_stack(dst: &mut Stack, src: &Stack) -> bool {
    let mut changed = false;
    for i in 0..dst.len().min(src.len()) {
        if dst[i] != src[i] {
            dst[i] = None;
            changed = true;
        }
    }
    if dst.len() > src.len() {
        dst.truncate(src.len());
        changed = true;
    }
    changed
}

/// Update the abstract stack for one instruction. Mirrors the VM's handlers
/// exactly so the abstract stack stays aligned with the runtime stack.
fn step(inst: &Instruction, stack: &mut Stack) {
    match inst {
        Instruction::Constant { index } => stack.push(Some(*index)),
        Instruction::Pop { count } => {
            for _ in 0..*count {
                stack.pop();
            }
        },
        Instruction::Duplicate { index } => {
            let idx = stack.len().saturating_sub(*index + 1);
            if idx < stack.len() {
                stack.push(stack[idx]);
            }
        },
        Instruction::GetLocal { index } => {
            let value = stack.get(*index).copied().flatten();
            stack.push(value);
        },
        Instruction::SetLocal { index } => {
            let value = stack.pop().flatten();
            if *index >= stack.len() {
                stack.resize(*index + 1, None);
            }
            stack[*index] = value;
        },
        Instruction::ForLoop { control_index, .. } => {
            if *control_index >= stack.len() {
                stack.resize(*control_index + 1, None);
            }
            stack[*control_index] = None;
        },
        Instruction::Call {
            arg_count,
            keep_return_value,
        } => {
            for _ in 0..=*arg_count {
                stack.pop();
            }
            if *keep_return_value {
                stack.push(None);
            }
        },
        Instruction::MakeArray { count } => {
            for _ in 0..*count {
                stack.pop();
            }
            stack.push(None);
        },
        Instruction::GetIndex => {
            stack.pop();
            if let Some(top) = stack.last_mut() {
                *top = None;
            }
        },
        Instruction::SetIndex => {
            stack.pop();
            stack.pop();
        },
        Instruction::GetGlobal { .. }
        | Instruction::GetUpvalue { .. }
        | Instruction::Closure { .. } => {
            stack.push(None);
        },
        Instruction::SetGlobal { .. } | Instruction::SetUpvalue { .. } => {
            stack.pop();
        },
        Instruction::Add
        | Instruction::Subtract
        | Instruction::Multiply
        | Instruction::Divide
        | Instruction::Modulo
        | Instruction::Power
        | Instruction::Equal { keep_rhs: false }
        | Instruction::NotEqual { keep_rhs: false }
        | Instruction::Greater { keep_rhs: false }
        | Instruction::GreaterEqual { keep_rhs: false }
        | Instruction::Less { keep_rhs: false }
        | Instruction::LessEqual { keep_rhs: false } => {
            stack.pop();
            stack.pop();
            stack.push(None);
        },
        Instruction::Equal { keep_rhs: true }
        | Instruction::NotEqual { keep_rhs: true }
        | Instruction::Greater { keep_rhs: true }
        | Instruction::GreaterEqual { keep_rhs: true }
        | Instruction::Less { keep_rhs: true }
        | Instruction::LessEqual { keep_rhs: true } => {
            stack.pop();
            stack.pop();
            stack.push(None);
            stack.push(None);
        },
        Instruction::Negate | Instruction::Not => {
            stack.pop();
            stack.push(None);
        },
        Instruction::Jump { .. } | Instruction::JumpIf { .. } | Instruction::Return => {},
    }
}

// ---------------------------------------------------------------------------
// Constant folding over adjacent Constant + op sequences
// ---------------------------------------------------------------------------

fn fold_constants(chunk: &Chunk, pool: &mut Vec<HeapCell>) -> (Chunk, bool) {
    let mut sink = std::io::sink();
    let mut empty = std::io::empty();
    let mut heap = Heap::new(&mut sink, &mut empty);
    let mut values: Vec<StackValue> = pool
        .iter()
        .map(|cell| heap.alloc(cell.value.clone()))
        .collect();

    let len = chunk.code.len();
    let mut new_code = Vec::with_capacity(len);
    let mut new_locations = Vec::with_capacity(len);
    let mut old_to_new = vec![0; len];
    let mut changed = false;

    let mut i = 0;
    while i < len {
        let fold = fold_window(&chunk.code, i, &values, &mut heap);
        if let Some((data, count)) = fold {
            changed = true;
            let idx = add_constant(pool, &mut values, &mut heap, data);
            new_code.push(Instruction::Constant { index: idx });
            new_locations.push(chunk.locations[i].clone());
            let new_idx = new_code.len() - 1;
            for k in 0..count {
                old_to_new[i + k] = new_idx;
            }
            i += count;
        } else {
            old_to_new[i] = new_code.len();
            new_code.push(chunk.code[i].clone());
            new_locations.push(chunk.locations[i].clone());
            i += 1;
        }
    }

    for inst in &mut new_code {
        remap_offsets(inst, &old_to_new);
    }

    (
        Chunk {
            code: new_code,
            locations: new_locations,
        },
        changed,
    )
}

fn fold_window(
    code: &[Instruction],
    i: usize,
    values: &[StackValue],
    heap: &mut Heap,
) -> Option<(HeapData, usize)> {
    if let (
        Some(Instruction::Constant { index: lhs }),
        Some(Instruction::Constant { index: rhs }),
        Some(op),
    ) = (code.get(i), code.get(i + 1), code.get(i + 2))
        && let Some(data) = fold_binary(op, *lhs, *rhs, values, heap)
    {
        return Some((data, 3));
    }
    if let (Some(Instruction::Constant { index }), Some(op)) = (code.get(i), code.get(i + 1))
        && let Some(data) = fold_unary(op, *index, values, heap)
    {
        return Some((data, 2));
    }
    None
}

fn fold_binary(
    op: &Instruction,
    lhs: usize,
    rhs: usize,
    values: &[StackValue],
    heap: &mut Heap,
) -> Option<HeapData> {
    let result = match op {
        Instruction::Add => add(&values[lhs], &values[rhs], heap),
        Instruction::Subtract => sub(&values[lhs], &values[rhs], heap),
        Instruction::Multiply => mul(&values[lhs], &values[rhs], heap),
        Instruction::Divide => div(&values[lhs], &values[rhs], heap),
        Instruction::Modulo => modulo(&values[lhs], &values[rhs], heap),
        Instruction::Power => pow(&values[lhs], &values[rhs], heap),
        Instruction::Equal { keep_rhs: false } => {
            return Some(compare_result(lhs, rhs, values, heap, |c| {
                c == Some(Ordering::Equal)
            }));
        },
        Instruction::NotEqual { keep_rhs: false } => {
            return Some(compare_result(lhs, rhs, values, heap, |c| {
                c != Some(Ordering::Equal)
            }));
        },
        Instruction::Less { keep_rhs: false } => {
            return Some(compare_result(lhs, rhs, values, heap, |c| {
                c == Some(Ordering::Less)
            }));
        },
        Instruction::LessEqual { keep_rhs: false } => {
            return Some(compare_result(lhs, rhs, values, heap, |c| {
                matches!(c, Some(Ordering::Less | Ordering::Equal))
            }));
        },
        Instruction::Greater { keep_rhs: false } => {
            return Some(compare_result(lhs, rhs, values, heap, |c| {
                c == Some(Ordering::Greater)
            }));
        },
        Instruction::GreaterEqual { keep_rhs: false } => {
            return Some(compare_result(lhs, rhs, values, heap, |c| {
                matches!(c, Some(Ordering::Greater | Ordering::Equal))
            }));
        },
        _ => return None,
    };
    result.as_ref().ok().and_then(|sv| foldable_data(sv, heap))
}

fn compare_result(
    lhs: usize,
    rhs: usize,
    values: &[StackValue],
    heap: &Heap,
    pred: impl FnOnce(Option<Ordering>) -> bool,
) -> HeapData {
    let ordering = compare(&values[lhs], &values[rhs], heap);
    HeapData::Primitive(Primitive::Bool(pred(ordering)))
}

fn fold_unary(
    op: &Instruction,
    operand: usize,
    values: &[StackValue],
    heap: &mut Heap,
) -> Option<HeapData> {
    let result = match op {
        Instruction::Negate => negative(&values[operand], heap),
        Instruction::Not => Ok(not_op(&values[operand], heap)),
        _ => return None,
    };
    result.as_ref().ok().and_then(|sv| foldable_data(sv, heap))
}

/// Foldable results are only values that do not reference optimizer-internal
/// heap cells (vectors and functions embed cell keys and are therefore
/// skipped).
fn foldable_data(result: &StackValue, heap: &Heap) -> Option<HeapData> {
    let data = to_owned(result, heap);
    match data {
        HeapData::Nil | HeapData::Primitive(_) | HeapData::String(_) => Some(data),
        _ => None,
    }
}

fn add_constant(
    pool: &mut Vec<HeapCell>,
    values: &mut Vec<StackValue>,
    heap: &mut Heap,
    data: HeapData,
) -> usize {
    if let Some(idx) = pool.iter().position(|cell| cell.value == data) {
        return idx;
    }
    let idx = pool.len();
    let stack_value = heap.alloc(data.clone());
    pool.push(HeapCell::new(data));
    values.push(stack_value);
    idx
}
