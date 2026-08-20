use std::{cmp::Ordering, collections::VecDeque, mem};

use l3_runtime::{
    Function, HeapData, Primitive, primitive::compare_primitives,
};

use crate::{
    Chunk, Code, CodeOffset, ConstantIndex, ConstantPool, Instruction, LocalIndex, ProgramBytecode,
    idx,
};

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
fn chunk_arities(program: &ProgramBytecode) -> Vec<u32> {
    let mut arities = vec![0; program.chunks.len().as_index()];
    for data in &program.constants {
        if let HeapData::Function(Function::Bytecode(bc)) = data
            && let Some(slot) = arities.get_mut(bc.id as usize)
        {
            *slot = bc.arity;
        }
    }
    arities
}

fn optimize_chunk(chunk: Chunk, pool: &mut ConstantPool, arity: u32) -> (Chunk, bool) {
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
    let len = chunk.code.len().as_index();
    let mut reachable = vec![false; len];
    let mut queue = VecDeque::new();
    if let Some(first) = reachable.first_mut() {
        *first = true;
        queue.push_back(0);
    }
    while let Some(i) = queue.pop_front() {
        for succ in successors(chunk.code.as_slice(), i) {
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
            code: Code::from(new_code),
            locations: new_locations,
        },
        true,
    )
}

fn successors(code: &[Instruction], i: usize) -> Vec<usize> {
    match code.get(i).expect("queue holds valid instruction indices") {
        Instruction::Return => Vec::new(),
        Instruction::Jump { offset } => vec![offset.as_index()],
        Instruction::JumpIf { offset, .. } => vec![offset.as_index(), i + 1],
        Instruction::ForLoop { body_offset, .. } => vec![body_offset.as_index(), i + 1],
        _ => vec![i + 1],
    }
}

fn remap_offsets(instruction: &mut Instruction, old_to_new: &[usize]) {
    let remap = |offset: &mut CodeOffset| {
        if let Some(mapped) = old_to_new.get(offset.as_index()) {
            *offset = idx(*mapped);
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
type AbstractValue = Option<ConstantIndex>;

/// Abstract model of the runtime stack (relative to the frame pointer). Local
/// slots live at the bottom of this stack, matching the VM's layout.
#[derive(Clone, Default)]
struct AbstractStack(Vec<AbstractValue>);

impl AbstractStack {
    fn with_unknowns(len: u32) -> Self {
        Self(vec![None; len as usize])
    }

    /// Value in the local slot at `index` (relative to the frame pointer),
    /// flattened: `Some(pool_index)` when known, `None` when unknown.
    fn get_local(&self, index: LocalIndex) -> Option<ConstantIndex> {
        self.0.get(index.as_index()).copied().flatten()
    }

    fn set_local(&mut self, index: LocalIndex, value: AbstractValue) {
        if let Some(slot) = self.0.get_mut(index.as_index()) {
            *slot = value;
        }
    }

    fn push(&mut self, value: AbstractValue) {
        self.0.push(value);
    }

    fn pop(&mut self) -> AbstractValue {
        self.0.pop().unwrap_or(None)
    }

    fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }

    const fn len(&self) -> usize {
        self.0.len()
    }

    fn last_mut(&mut self) -> Option<&mut AbstractValue> {
        self.0.last_mut()
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut AbstractValue> {
        self.0.iter_mut()
    }

    fn as_slice(&self) -> &[AbstractValue] {
        &self.0
    }
}

#[derive(Clone, Copy)]
struct Block {
    start: usize,
    end: usize,
}

fn propagate_constants(mut chunk: Chunk, arity: u32) -> (Chunk, bool) {
    let blocks = build_blocks(chunk.code.as_slice());
    let n = blocks.len();
    let mut index_to_block = vec![0; chunk.code.len().as_index()];
    for (bi, block) in blocks.iter().enumerate() {
        index_to_block
            .get_mut(block.start..block.end)
            .expect("block range is within the code array")
            .fill(bi);
    }

    let mut in_stacks: Vec<Option<AbstractStack>> = vec![None; n];
    let mut queued = vec![false; n];
    if let Some(first) = in_stacks.first_mut() {
        *first = Some(AbstractStack::with_unknowns(arity));
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
            chunk.code.as_slice(),
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
            .as_mut_slice()
            .get_mut(block.start..block.end)
            .expect("block range is within the code array")
        {
            if let Instruction::GetLocal { index } = inst {
                let known = stack.get_local(*index);
                if let Some(k) = known {
                    changed = true;
                    *inst = Instruction::Constant { index: k };
                }
                stack.push(known);
            } else {
                transfer_instruction(inst, &mut stack);
            }
        }
    }

    (chunk, changed)
}

fn build_blocks(code: &[Instruction]) -> Vec<Block> {
    let len = code.len();
    if len == 0 {
        return Vec::new();
    }
    let mut starts = vec![false; len];
    if let Some(first) = starts.first_mut() {
        *first = true;
    }
    for (i, inst) in code.iter().enumerate() {
        match inst {
            Instruction::Jump { offset } => {
                if let Some(s) = starts.get_mut(offset.as_index()) {
                    *s = true;
                }
            },
            Instruction::JumpIf { offset, .. } => {
                if let Some(s) = starts.get_mut(offset.as_index()) {
                    *s = true;
                }
                if let Some(s) = starts.get_mut(i + 1) {
                    *s = true;
                }
            },
            Instruction::ForLoop { body_offset, .. } => {
                if let Some(s) = starts.get_mut(body_offset.as_index()) {
                    *s = true;
                }
                if let Some(s) = starts.get_mut(i + 1) {
                    *s = true;
                }
            },
            Instruction::Return => {
                if let Some(s) = starts.get_mut(i + 1) {
                    *s = true;
                }
            },
            _ => {},
        }
    }

    let mut blocks = Vec::new();
    let mut start = 0;
    for i in 1..len {
        if *starts.get(i).expect("i is within the starts array") {
            blocks.push(Block { start, end: i });
            start = i;
        }
    }
    blocks.push(Block { start, end: len });
    blocks
}

fn merge_stack(dst: &mut AbstractStack, src: &AbstractStack) -> bool {
    let mut changed = false;
    let n = dst.len().min(src.len());
    dst.truncate(n);
    for (d, s) in dst.iter_mut().zip(src.as_slice()) {
        if *d != *s && d.is_some() {
            *d = None;
            changed = true;
        }
    }
    changed
}

fn transfer(
    code: &[Instruction],
    block: &Block,
    index_to_block: &[usize],
    in_stack: &AbstractStack,
) -> Vec<(usize, AbstractStack)> {
    let mut stack = in_stack.clone();
    for inst in code
        .get(block.start..block.end)
        .expect("block range is within the code array")
    {
        transfer_instruction(inst, &mut stack);
    }

    let last = block.end.saturating_sub(1);
    let mut outs = Vec::new();
    match code.get(last).expect("block end is within the code array") {
        Instruction::Return => {},
        Instruction::Jump { offset } => {
            if let Some(&succ) = index_to_block.get(offset.as_index()) {
                outs.push((succ, stack));
            }
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
                taken.push(cond);
            }
            if *keep_stay {
                stay.push(cond);
            }
            if let Some(&succ) = index_to_block.get(offset.as_index()) {
                outs.push((succ, taken));
            }
            if let Some(&succ) = index_to_block.get(last + 1) {
                outs.push((succ, stay));
            }
        },
        Instruction::ForLoop {
            control_index,
            body_offset,
            ..
        } => {
            let mut loop_stack = stack.clone();
            loop_stack.set_local(*control_index, None);
            if let Some(&succ) = index_to_block.get(body_offset.as_index()) {
                outs.push((succ, loop_stack));
            }
            if let Some(&succ) = index_to_block.get(last + 1) {
                outs.push((succ, stack));
            }
        },
        _ => {
            if let Some(&succ) = index_to_block.get(last + 1) {
                outs.push((succ, stack));
            }
        },
    }
    outs
}

fn transfer_instruction(inst: &Instruction, stack: &mut AbstractStack) {
    match inst {
        Instruction::Constant { index } => stack.push(Some(*index)),
        Instruction::Pop { count } => {
            let new_len = stack.len().saturating_sub(*count as usize);
            stack.truncate(new_len);
        },
        Instruction::Duplicate { index } => {
            let known = stack
                .len()
                .checked_sub(*index as usize)
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| stack.as_slice().get(n).copied())
                .flatten();
            stack.push(known);
        },
        Instruction::GetLocal { index } => {
            let known = stack.get_local(*index);
            stack.push(known);
        },
        Instruction::SetLocal { index } => {
            let value = stack.pop();
            stack.set_local(*index, value);
        },
        Instruction::ForLoop { control_index, .. } => {
            stack.set_local(*control_index, None);
        },
        Instruction::Call {
            arg_count,
            keep_return_value,
        } => {
            let pop_len = arg_count + 1;
            let new_len = stack.len().saturating_sub(pop_len as usize);
            stack.truncate(new_len);
            // A call may mutate captured locals through upvalues, so no local
            // value is provably constant afterwards.
            for slot in stack.iter_mut() {
                *slot = None;
            }
            if *keep_return_value {
                stack.push(None);
            }
        },
        Instruction::MakeArray { count } => {
            let new_len = stack.len().saturating_sub(*count as usize);
            stack.truncate(new_len);
            stack.push(None);
        },
        Instruction::VectorAppend { count } => {
            let new_len = stack.len().saturating_sub(*count as usize);
            stack.truncate(new_len);
            if let Some(top) = stack.last_mut() {
                *top = None;
            }
        },
        Instruction::GetIndex
        | Instruction::Add
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
        Instruction::SetIndex => {
            stack.pop();
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
//
// Folds directly on constant-pool `HeapData` values, so no runtime heap is
// needed. Only results that can be represented standalone (nil, primitives,
// strings) are folded; vector/function results are left to the interpreter.

pub fn fold_constants(chunk: &Chunk, pool: &mut ConstantPool) -> (Chunk, bool) {
    let len = chunk.code.len().as_index();
    let mut new_code = Vec::with_capacity(len);
    let mut new_locations = Vec::with_capacity(len);
    let mut old_to_new = vec![0; len];
    let mut changed = false;

    let mut i = 0;
    while i < len {
        let fold = fold_window(chunk.code.as_slice(), i, pool);
        if let Some((data, count)) = fold {
            changed = true;
            let idx = add_constant(pool, data);
            new_code.push(Instruction::Constant { index: idx });
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
                    .as_slice()
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
            code: Code::from(new_code),
            locations: new_locations,
        },
        changed,
    )
}

fn fold_window(
    code: &[Instruction],
    i: usize,
    pool: &ConstantPool,
) -> Option<(HeapData, usize)> {
    if let (
        Some(Instruction::Constant { index: lhs }),
        Some(Instruction::Constant { index: rhs }),
        Some(op),
    ) = (code.get(i), code.get(i + 1), code.get(i + 2))
        && let Some(data) = fold_binary(op, *lhs, *rhs, pool)
    {
        return Some((data, 3));
    }
    if let (Some(Instruction::Constant { index }), Some(op)) = (code.get(i), code.get(i + 1))
        && let Some(data) = fold_unary(op, *index, pool)
    {
        return Some((data, 2));
    }
    None
}

fn fold_binary(
    op: &Instruction,
    lhs: ConstantIndex,
    rhs: ConstantIndex,
    pool: &ConstantPool,
) -> Option<HeapData> {
    let lhs = pool.get(lhs)?;
    let rhs = pool.get(rhs)?;
    match op {
        Instruction::Add => match (lhs, rhs) {
            (HeapData::Primitive(a), HeapData::Primitive(b)) => {
                (*a + *b).ok().map(HeapData::Primitive)
            },
            (HeapData::String(a), HeapData::String(b)) => Some(HeapData::String(format!("{a}{b}"))),
            _ => None,
        },
        Instruction::Subtract => fold_numeric(lhs, rhs, |a, b| (a - b).ok()),
        Instruction::Multiply => fold_numeric(lhs, rhs, |a, b| (a * b).ok()),
        Instruction::Divide => fold_numeric(lhs, rhs, |a, b| (a / b).ok()),
        Instruction::Modulo => fold_numeric(lhs, rhs, |a, b| (a % b).ok()),
        Instruction::Power => fold_numeric(lhs, rhs, |a, b| a.pow(b).ok()),
        Instruction::Equal { keep_rhs: false } => Some(fold_compare(lhs, rhs, |c| {
            c == Some(Ordering::Equal)
        })),
        Instruction::NotEqual { keep_rhs: false } => Some(fold_compare(lhs, rhs, |c| {
            c != Some(Ordering::Equal)
        })),
        Instruction::Less { keep_rhs: false } => Some(fold_compare(lhs, rhs, |c| {
            c == Some(Ordering::Less)
        })),
        Instruction::LessEqual { keep_rhs: false } => Some(fold_compare(lhs, rhs, |c| {
            matches!(c, Some(Ordering::Less | Ordering::Equal))
        })),
        Instruction::Greater { keep_rhs: false } => Some(fold_compare(lhs, rhs, |c| {
            c == Some(Ordering::Greater)
        })),
        Instruction::GreaterEqual { keep_rhs: false } => Some(fold_compare(lhs, rhs, |c| {
            matches!(c, Some(Ordering::Greater | Ordering::Equal))
        })),
        _ => None,
    }
}

fn fold_numeric(
    lhs: &HeapData,
    rhs: &HeapData,
    f: impl FnOnce(Primitive, Primitive) -> Option<Primitive>,
) -> Option<HeapData> {
    match (lhs, rhs) {
        (HeapData::Primitive(a), HeapData::Primitive(b)) => f(*a, *b).map(HeapData::Primitive),
        _ => None,
    }
}

fn fold_compare(
    lhs: &HeapData,
    rhs: &HeapData,
    pred: impl FnOnce(Option<Ordering>) -> bool,
) -> HeapData {
    HeapData::Primitive(Primitive::Bool(pred(compare_data(lhs, rhs))))
}

/// Value comparison over foldable data: primitives, strings and nil only.
fn compare_data(a: &HeapData, b: &HeapData) -> Option<Ordering> {
    match (a, b) {
        (HeapData::Primitive(x), HeapData::Primitive(y)) => compare_primitives(*x, *y),
        (HeapData::String(x), HeapData::String(y)) => Some(x.cmp(y)),
        (HeapData::Nil, HeapData::Nil) => Some(Ordering::Equal),
        _ => None,
    }
}

fn fold_unary(op: &Instruction, operand: ConstantIndex, pool: &ConstantPool) -> Option<HeapData> {
    let operand = pool.get(operand)?;
    match op {
        Instruction::Negate => match operand {
            HeapData::Primitive(p) => Some(HeapData::Primitive(-*p)),
            _ => None,
        },
        Instruction::Not => Some(HeapData::Primitive(Primitive::Bool(!is_truthy_data(operand)))),
        _ => None,
    }
}

/// Truthiness over foldable data, mirroring `HeapData::is_truthy`.
fn is_truthy_data(data: &HeapData) -> bool {
    match data {
        HeapData::Nil => false,
        HeapData::Primitive(p) => p.is_truthy(),
        HeapData::Function(_) => true,
        HeapData::Vector(v) => !v.is_empty(),
        HeapData::String(s) => !s.is_empty(),
    }
}

/// Append `data` to the pool if absent; reuse the existing slot otherwise.
fn add_constant(pool: &mut ConstantPool, data: HeapData) -> ConstantIndex {
    if let Some(i) = pool.iter().position(|existing| *existing == data) {
        return idx(i);
    }
    pool.push(data)
}
