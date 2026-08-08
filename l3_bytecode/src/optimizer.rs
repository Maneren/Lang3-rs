use std::{cmp::Ordering, collections::VecDeque, io, mem};

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
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn optimize(&self, mut program: ProgramBytecode) -> ProgramBytecode {
        for _ in 0..4 {
            let mut changed = false;
            let arities = chunk_arities(&program);
            for (ci, chunk_ref) in program.chunks.iter_mut().enumerate() {
                let chunk = mem::take(chunk_ref);
                let (optimized, pass_changed) = optimize_chunk(
                    chunk,
                    &mut program.constants,
                    *arities
                        .get(ci)
                        .expect("arities is aligned with program chunks"),
                );
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
            && let Some(slot) = arities.get_mut(bc.id)
        {
            *slot = bc.arity;
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
    if let Some(first) = reachable.first_mut() {
        *first = true;
        queue.push_back(0);
    }
    while let Some(i) = queue.pop_front() {
        for succ in successors(&chunk.code, i) {
            if let Some(reached) = reachable.get_mut(succ)
                && !*reached
            {
                *reached = true;
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
    for (i, (reached, (inst, loc))) in reachable
        .iter()
        .zip(chunk.code.iter().zip(&chunk.locations))
        .enumerate()
    {
        let slot = old_to_new.get_mut(i).expect("i is within the remap table");
        if *reached {
            *slot = new_code.len();
            new_code.push(inst.clone());
            new_locations.push(loc.clone());
        } else {
            *slot = new_code.len().saturating_sub(1);
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
    match code.get(i).expect("queue holds valid instruction indices") {
        Instruction::Return => Vec::new(),
        Instruction::Jump { offset } => vec![*offset as usize],
        Instruction::JumpIf { offset, .. } => vec![*offset as usize, i + 1],
        Instruction::ForLoop { body_offset, .. } => vec![*body_offset as usize, i + 1],
        _ => vec![i + 1],
    }
}

fn remap_offsets(instruction: &mut Instruction, old_to_new: &[usize]) {
    let remap = |offset: &mut u32| {
        if let Some(mapped) = old_to_new.get(*offset as usize) {
            *offset = u32::try_from(*mapped).expect("remapped offset fits in u32");
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
        index_to_block
            .get_mut(block.start..block.end)
            .expect("block range is within the code array")
            .fill(bi);
    }

    let mut in_stacks: Vec<Option<Stack>> = vec![None; n];
    let mut queued = vec![false; n];
    if let Some(first) = in_stacks.first_mut() {
        *first = Some(vec![None; arity]);
    }
    if let Some(first) = queued.first_mut() {
        *first = true;
    }
    let mut queue: VecDeque<usize> = queued
        .iter()
        .enumerate()
        .filter_map(|(bi, &q)| q.then_some(bi))
        .collect();
    while let Some(bi) = queue.pop_front() {
        *queued
            .get_mut(bi)
            .expect("queued entries hold valid block indices") = false;
        let Some(in_stack) = in_stacks.get(bi).expect("bi is a valid block index") else {
            continue;
        };
        let outs = transfer(
            &chunk.code,
            blocks.get(bi).expect("bi is a valid block index"),
            &index_to_block,
            in_stack,
        );
        for (succ, out) in outs {
            let merged = if let Some(dst) = in_stacks
                .get_mut(succ)
                .expect("succ is a valid block index")
            {
                merge_stack(dst, &out)
            } else {
                *in_stacks
                    .get_mut(succ)
                    .expect("succ is a valid block index") = Some(out);
                true
            };
            if merged {
                let queued_entry = queued.get_mut(succ).expect("succ is a valid block index");
                if !*queued_entry {
                    *queued_entry = true;
                    queue.push_back(succ);
                }
            }
        }
    }

    let mut changed = false;
    for (bi, block) in blocks.iter().enumerate() {
        let Some(in_stack) = in_stacks.get(bi).expect("bi is a valid block index") else {
            continue;
        };
        let mut stack = in_stack.clone();
        for inst in chunk
            .code
            .get_mut(block.start..block.end)
            .expect("block range is within the code array")
        {
            if let Instruction::GetLocal { index } = inst {
                let known = stack.get(*index as usize).copied().flatten();
                if let Some(k) = known {
                    changed = true;
                    *inst = Instruction::Constant {
                        index: u32::try_from(k).expect("constant pool index fits in u32"),
                    };
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
    *starts
        .first_mut()
        .expect("the starts array always has at least one entry") = true;
    for (i, inst) in code.iter().enumerate() {
        match inst {
            Instruction::Jump { offset } | Instruction::JumpIf { offset, .. } => {
                if let Some(entry) = starts.get_mut(*offset as usize) {
                    *entry = true;
                }
            },
            Instruction::ForLoop { body_offset, .. } => {
                if let Some(entry) = starts.get_mut(*body_offset as usize) {
                    *entry = true;
                }
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
            *starts
                .get_mut(i + 1)
                .expect("i + 1 is within the starts array") = true;
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
    for inst in code
        .get(block.start..last)
        .expect("block start..last is within the code array")
    {
        step(inst, &mut stack);
    }
    match code.get(last).expect("block end is within the code array") {
        Instruction::Return => Vec::new(),
        Instruction::Jump { offset } => {
            vec![(
                *index_to_block
                    .get(*offset as usize)
                    .expect("jump target maps to a block"),
                stack,
            )]
        },
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
            let mut out = vec![(
                *index_to_block
                    .get(*offset as usize)
                    .expect("jump target maps to a block"),
                taken,
            )];
            if let Some(mapped) = index_to_block.get(block.end) {
                out.push((*mapped, stay));
            }
            out
        },
        Instruction::ForLoop {
            control_index,
            body_offset,
            ..
        } => {
            let control_index = *control_index as usize;
            let body_offset = *body_offset as usize;
            if control_index >= stack.len() {
                stack.resize(control_index + 1, None);
            }
            *stack
                .get_mut(control_index)
                .expect("control slot was resized into the stack") = None;
            let mut out = vec![(
                *index_to_block
                    .get(body_offset)
                    .expect("jump target maps to a block"),
                stack.clone(),
            )];
            if let Some(mapped) = index_to_block.get(block.end) {
                out.push((*mapped, stack));
            }
            out
        },
        _ => {
            step(
                code.get(last).expect("block end is within the code array"),
                &mut stack,
            );
            index_to_block
                .get(block.end)
                .map_or_else(Vec::new, |mapped| vec![(*mapped, stack)])
        },
    }
}

/// Intersect a predecessor's abstract stack into the successor's incoming
/// stack. A stack entry is only known when all predecessors agree.
fn merge_stack(dst: &mut Stack, src: &Stack) -> bool {
    let mut changed = false;
    for (dst_slot, src_slot) in dst.iter_mut().zip(src) {
        if *dst_slot != *src_slot {
            *dst_slot = None;
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
        Instruction::Constant { index } => stack.push(Some(*index as usize)),
        Instruction::Pop { count } => {
            for _ in 0..*count {
                stack.pop();
            }
        },
        Instruction::Duplicate { index } => {
            let idx = stack.len().saturating_sub(*index as usize + 1);
            if let Some(&val) = stack.get(idx) {
                stack.push(val);
            }
        },
        Instruction::GetLocal { index } => {
            let value = stack.get(*index as usize).copied().flatten();
            stack.push(value);
        },
        Instruction::SetLocal { index } => {
            let value = stack.pop().flatten();
            let index = *index as usize;
            if index >= stack.len() {
                stack.resize(index + 1, None);
            }
            *stack
                .get_mut(index)
                .expect("local slot was resized into the stack") = value;
        },
        Instruction::ForLoop { control_index, .. } => {
            let control_index = *control_index as usize;
            if control_index >= stack.len() {
                stack.resize(control_index + 1, None);
            }
            *stack
                .get_mut(control_index)
                .expect("control slot was resized into the stack") = None;
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
        Instruction::VectorAppend { count } => {
            for _ in 0..*count {
                stack.pop();
            }
            if let Some(top) = stack.last_mut() {
                *top = None;
            }
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
    let mut sink = io::sink();
    let mut empty = io::empty();
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
            new_code.push(Instruction::Constant {
                index: u32::try_from(idx).expect("constant pool index fits in u32"),
            });
            new_locations.push(
                chunk
                    .locations
                    .get(i)
                    .expect("i is within the code array")
                    .clone(),
            );
            let new_idx = new_code.len() - 1;
            for k in 0..count {
                *old_to_new
                    .get_mut(i + k)
                    .expect("folded window is within the code array") = new_idx;
            }
            i += count;
        } else {
            *old_to_new.get_mut(i).expect("i is within the code array") = new_code.len();
            new_code.push(
                chunk
                    .code
                    .get(i)
                    .expect("i is within the code array")
                    .clone(),
            );
            new_locations.push(
                chunk
                    .locations
                    .get(i)
                    .expect("i is within the code array")
                    .clone(),
            );
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
        && let Some(data) = fold_binary(op, *lhs as usize, *rhs as usize, values, heap)
    {
        return Some((data, 3));
    }
    if let (Some(Instruction::Constant { index }), Some(op)) = (code.get(i), code.get(i + 1))
        && let Some(data) = fold_unary(op, *index as usize, values, heap)
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
    let lhs = values.get(lhs).copied()?;
    let rhs = values.get(rhs).copied()?;
    let result = match op {
        Instruction::Add => add(&lhs, &rhs, heap),
        Instruction::Subtract => sub(&lhs, &rhs, heap),
        Instruction::Multiply => mul(&lhs, &rhs, heap),
        Instruction::Divide => div(&lhs, &rhs, heap),
        Instruction::Modulo => modulo(&lhs, &rhs, heap),
        Instruction::Power => pow(&lhs, &rhs, heap),
        Instruction::Equal { keep_rhs: false } => {
            return Some(compare_result(&lhs, &rhs, heap, |c| {
                c == Some(Ordering::Equal)
            }));
        },
        Instruction::NotEqual { keep_rhs: false } => {
            return Some(compare_result(&lhs, &rhs, heap, |c| {
                c != Some(Ordering::Equal)
            }));
        },
        Instruction::Less { keep_rhs: false } => {
            return Some(compare_result(&lhs, &rhs, heap, |c| {
                c == Some(Ordering::Less)
            }));
        },
        Instruction::LessEqual { keep_rhs: false } => {
            return Some(compare_result(&lhs, &rhs, heap, |c| {
                matches!(c, Some(Ordering::Less | Ordering::Equal))
            }));
        },
        Instruction::Greater { keep_rhs: false } => {
            return Some(compare_result(&lhs, &rhs, heap, |c| {
                c == Some(Ordering::Greater)
            }));
        },
        Instruction::GreaterEqual { keep_rhs: false } => {
            return Some(compare_result(&lhs, &rhs, heap, |c| {
                matches!(c, Some(Ordering::Greater | Ordering::Equal))
            }));
        },
        _ => return None,
    };
    result.as_ref().ok().and_then(|sv| foldable_data(sv, heap))
}

fn compare_result(
    lhs: &StackValue,
    rhs: &StackValue,
    heap: &Heap,
    pred: impl FnOnce(Option<Ordering>) -> bool,
) -> HeapData {
    let ordering = compare(lhs, rhs, heap);
    HeapData::Primitive(Primitive::Bool(pred(ordering)))
}

fn fold_unary(
    op: &Instruction,
    operand: usize,
    values: &[StackValue],
    heap: &mut Heap,
) -> Option<HeapData> {
    let operand = values.get(operand).copied()?;
    let result = match op {
        Instruction::Negate => negative(&operand, heap),
        Instruction::Not => Ok(not_op(&operand, heap)),
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
