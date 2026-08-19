pub mod builtins;

use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::HashMap,
    io::{Read, Write},
    rc::Rc,
    slice::Iter,
};

use foldhash::fast::FixedState;
use l3_ast::Identifier;
use l3_bytecode::*;
use l3_location::Location;
use l3_runtime::{
    conv::as_integer,
    error::RuntimeResult,
    heap_data::{add, compare, div, index, index_mut, modulo, mul, negative, not_op, pow, sub},
    *,
};

pub struct VmStack {
    values: Vec<StackValue>,
}

#[inline]
fn slice_get<T>(slice: &[T], index: usize) -> &T {
    if cfg!(debug_assertions) {
        slice.get(index).expect("Index from the compiler is valid")
    } else {
        // SAFETY: All indices come from the compiler that is considered infallible by
        // the VM
        unsafe { slice.get_unchecked(index) }
    }
}

#[inline]
fn slice_get_mut<T>(slice: &mut [T], index: usize) -> &mut T {
    if cfg!(debug_assertions) {
        slice
            .get_mut(index)
            .expect("Index from the compiler is valid")
    } else {
        // SAFETY: All indices come from the compiler that is considered infallible by
        // the VM
        unsafe { slice.get_unchecked_mut(index) }
    }
}

impl VmStack {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
        }
    }

    fn push(&mut self, val: StackValue) {
        self.values.push(val);
    }

    fn pop(&mut self) -> StackValue {
        self.values
            .pop()
            .expect("compiler invariant: stack non-empty")
    }

    fn top(&self) -> StackValue {
        *self
            .values
            .last()
            .expect("compiler invariant: stack non-empty")
    }

    fn top_mut(&mut self) -> &mut StackValue {
        self.values
            .last_mut()
            .expect("compiler invariant: stack non-empty")
    }

    #[inline]
    fn get_local(&self, fp: StackIndex, index: LocalIndex) -> StackValue {
        *slice_get(&self.values, fp.as_index() + index.as_index())
    }

    #[inline]
    fn set_local(&mut self, fp: StackIndex, index: LocalIndex, val: StackValue) {
        let index = fp.as_index() + index.as_index();
        *slice_get_mut(&mut self.values, index) = val;
    }

    fn truncate(&mut self, len: StackIndex) {
        self.values.truncate(len.as_index());
    }

    const fn len(&self) -> StackIndex {
        StackIndex(self.values.len() as u32)
    }

    fn get(&self, index: StackIndex) -> Option<&StackValue> {
        self.values.get(index.as_index())
    }

    fn drain_from(&mut self, from: StackIndex) -> Vec<StackValue> {
        self.values.drain(from.as_index()..).collect()
    }

    fn get_range(&self, from: StackIndex, count: StackIndex) -> Option<&[StackValue]> {
        let from = from.as_index();
        self.values.get(from..from + count.as_index())
    }

    fn get_mut_from(&mut self, from: StackIndex) -> Option<&mut [StackValue]> {
        self.values.get_mut(from.as_index()..)
    }

    fn extend_from_slice(&mut self, other: &[StackValue]) {
        self.values.extend_from_slice(other);
    }
}

pub struct CallStack {
    frames: Vec<CallFrame>,
}

impl CallStack {
    const fn new() -> Self {
        Self { frames: Vec::new() }
    }

    fn last_mut(&mut self) -> Option<&mut CallFrame> {
        self.frames.last_mut()
    }

    fn push(&mut self, frame: CallFrame) {
        self.frames.push(frame);
    }

    fn pop(&mut self) -> CallFrame {
        self.frames.pop().expect("active frame exists")
    }

    const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    const fn len(&self) -> usize {
        self.frames.len()
    }

    fn iter(&self) -> Iter<'_, CallFrame> {
        self.frames.iter()
    }

    fn last(&self) -> Option<&CallFrame> {
        self.frames.last()
    }
}

impl<'a> IntoIterator for &'a CallStack {
    type Item = &'a CallFrame;
    type IntoIter = Iter<'a, CallFrame>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct BytecodeVM<'a> {
    pub heap: Heap<'a>,
    pub stack: VmStack,
    pub global_symbols: HashMap<String, StackValue, FixedState>,
    constant_keys: Vec<slotmap::DefaultKey>,
    constant_values: Vec<StackValue>,
    frames: CallStack,
    trigger_gc_key: Option<slotmap::DefaultKey>,
    debug: bool,
}

pub struct CallFrame {
    chunk_id: ChunkId,
    ip: CodeOffset,
    frame_pointer: StackIndex,
    closure_info: Option<(Rc<str>, StackValue)>,
    call_site: Option<(ChunkId, CodeOffset)>,
    upvalues: Rc<Vec<Rc<RefCell<UpvalueCell>>>>,
    captured_locals: HashMap<usize, Rc<RefCell<UpvalueCell>>, FixedState>,
    discard_return: bool,
}

macro_rules! debug_println {
    ($debug:expr, $($arg:tt)*) => {
        if $debug {
            // eprintln!($($arg)*);
        }
    };
}

impl<'a> BytecodeVM<'a> {
    #[must_use]
    pub fn new(writer: &'a mut impl Write, reader: &'a mut impl Read, debug: bool) -> Self {
        let mut vm = Self {
            heap: Heap::new(writer, reader),
            stack: VmStack::with_capacity(1024),
            global_symbols: HashMap::with_hasher(FixedState::default()),
            constant_keys: Vec::new(),
            constant_values: Vec::new(),
            frames: CallStack::new(),
            trigger_gc_key: None,
            debug,
        };

        // Register builtins
        for (name, body) in builtins::builtins() {
            let func = vm
                .heap
                .alloc_function(Function::Builtin(BuiltinFunction::new(
                    Identifier::new(name.to_string(), Location::default()),
                    body,
                )));
            vm.global_symbols.insert(name.to_string(), func);
        }

        // __trigger_gc runs a full mark-and-sweep over VM roots (stack, frames,
        // upvalues, globals, constants), which a builtin body cannot reach, so the
        // VM registers it here and intercepts calls to it in `call_function`.
        let trigger_key = vm
            .heap
            .alloc_function(Function::Builtin(BuiltinFunction::new(
                Identifier::new("__trigger_gc".to_string(), Location::default()),
                Rc::new(|_: &[StackValue], _: &mut Heap| {
                    Err(RuntimeError::type_error(
                        "__trigger_gc is handled by the VM",
                    ))
                }),
            )))
            .get_heap_key()
            .expect("allocated function lives on the heap");
        vm.global_symbols
            .insert("__trigger_gc".to_string(), StackValue::Heap(trigger_key));
        vm.trigger_gc_key = Some(trigger_key);

        vm
    }

    pub fn execute(&mut self, program: &ProgramBytecode) -> RuntimeResult<()> {
        let debug = self.debug;

        // Pre-insert all program constants into the heap so the Constant
        // dispatch can use a cached slotmap key instead of a linear scan.
        self.constant_keys = program
            .constants
            .iter()
            .map(|hc| self.heap.cells.insert(hc.clone()))
            .collect();
        self.constant_values = program
            .constants
            .iter()
            .zip(&self.constant_keys)
            .map(|(hc, key)| match &hc.value {
                HeapData::Nil => StackValue::Nil,
                HeapData::Primitive(p) => StackValue::Primitive(*p),
                _ => StackValue::Heap(*key),
            })
            .collect();

        let frame = CallFrame {
            chunk_id: ChunkId(0),
            ip: CodeOffset(0),
            frame_pointer: StackIndex(0),
            closure_info: None,
            call_site: None,
            upvalues: Rc::new(Vec::new()),
            captured_locals: HashMap::with_hasher(FixedState::default()),
            discard_return: false,
        };
        self.frames.push(frame);

        let result = self.execute_loop(program, debug);

        if let Err(mut err) = result {
            if err.location().is_none() {
                let loc = self
                    .frames
                    .last()
                    .map_or_default(|frame| self.instruction_location(&program.chunks, frame.ip));
                err = err.with_location(loc);
            }
            err.set_stacktrace(self.build_stacktrace(&program.chunks));
            return Err(err);
        }

        self.constant_keys.clear();
        self.constant_values.clear();
        Ok(())
    }

    /// The instruction being executed is the one at `ip`, which `execute_loop`
    /// keeps synced with `frame.ip`.
    fn instruction_location(&self, chunks: &Chunks, ip: CodeOffset) -> Location {
        let Some(frame) = self.frames.last() else {
            return Location::default();
        };
        chunks
            .get(frame.chunk_id)
            .and_then(|chunk| chunk.locations.get(ip.as_index()))
            .cloned()
            .unwrap_or_default()
    }

    fn build_stacktrace(&self, chunks: &Chunks) -> Vec<StacktraceFrame> {
        self.frames
            .iter()
            .filter_map(|frame| {
                let (call_chunk, call_ip) = frame.call_site?;
                let call_location = chunks
                    .get(call_chunk)
                    .and_then(|chunk| chunk.locations.get(call_ip.as_index()))
                    .cloned()
                    .unwrap_or_default();
                let function_name = frame
                    .closure_info
                    .as_ref()
                    .map_or_else(|| "<toplevel>".to_string(), |(name, _)| name.to_string());
                Some(StacktraceFrame {
                    function_name,
                    call_location,
                })
            })
            .collect()
    }

    fn execute_loop(&mut self, program: &ProgramBytecode, debug: bool) -> RuntimeResult<()> {
        let constants = &program.constants;

        while let Some(frame) = self.frames.last() {
            let (mut ip, chunk_id, fp) = (frame.ip, frame.chunk_id, frame.frame_pointer);
            let code = program.chunks[chunk_id].code.as_slice();

            loop {
                let instruction = slice_get(code, ip.as_index());
                // Advance first so that jumps set `ip` directly to their target.
                debug_println!(debug, "  IP={} {:?}", ip, instruction);
                ip.0 += 1;

                // The frame's `ip` is only synced here (on error) and in the
                // Call/Return handlers, so the hot dispatch loop avoids a
                // per-instruction write back into `self.frames`.
                let result: RuntimeResult<()> = match instruction {
                    Instruction::Return => {
                        let CallFrame {
                            frame_pointer,
                            discard_return,
                            ..
                        } = self.frames.pop();

                        if !self.frames.is_empty() {
                            let return_value = self.stack.top();
                            debug_println!(debug, "    RETURN value={:?}", return_value);
                            self.stack.truncate(frame_pointer - 1);
                            if !discard_return {
                                self.stack.push(return_value);
                            }
                        }
                        self.maybe_gc();
                        break;
                    },
                    Instruction::Constant { index } => {
                        let val = *slice_get(&self.constant_values, index.as_index());
                        debug_println!(debug, "    CONSTANT({}) -> {:?}", index, val);
                        self.stack.push(val);
                        Ok(())
                    },
                    Instruction::Pop { count } => {
                        debug_println!(debug, "    POP {}", count);
                        match self.stack.len().checked_sub(*count) {
                            Some(remaining) => {
                                self.stack.truncate(remaining);
                                Ok(())
                            },
                            None => Err(RuntimeError::generic("stack underflow")),
                        }
                    },
                    Instruction::Duplicate { index } => {
                        debug_println!(debug, "    DUPLICATE({})", index);
                        match self
                            .stack
                            .len()
                            .checked_sub(*index)
                            .and_then(|n| n.checked_sub(1))
                            .and_then(|n| self.stack.get(n))
                        {
                            Some(&sv) => {
                                self.stack.push(sv);
                                Ok(())
                            },
                            None => Err(RuntimeError::generic("stack underflow")),
                        }
                    },
                    Instruction::Add => {
                        debug_println!(debug, "    ADD");
                        self.binary_op(add)
                    },
                    Instruction::Subtract => {
                        debug_println!(debug, "    SUB");
                        self.binary_op(sub)
                    },
                    Instruction::Multiply => {
                        debug_println!(debug, "    MUL");
                        self.binary_op(mul)
                    },
                    Instruction::Divide => {
                        debug_println!(debug, "    DIV");
                        self.binary_op(div)
                    },
                    Instruction::Modulo => {
                        debug_println!(debug, "    MOD");
                        self.binary_op(modulo)
                    },
                    Instruction::Power => {
                        debug_println!(debug, "    POW");
                        self.binary_op(pow)
                    },
                    Instruction::Negate => {
                        debug_println!(debug, "    NEGATE");
                        let a = self.stack.pop();
                        match negative(&a, &self.heap) {
                            Ok(result) => {
                                self.stack.push(result);
                                Ok(())
                            },
                            Err(e) => Err(e),
                        }
                    },
                    Instruction::Not => {
                        debug_println!(debug, "    NOT");
                        let a = self.stack.pop();
                        self.stack.push(not_op(&a, &self.heap));
                        Ok(())
                    },
                    Instruction::Equal { keep_rhs } => {
                        debug_println!(debug, "    EQ keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c == Some(Ordering::Equal), *keep_rhs);
                        Ok(())
                    },
                    Instruction::NotEqual { keep_rhs } => {
                        debug_println!(debug, "    NE keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c != Some(Ordering::Equal), *keep_rhs);
                        Ok(())
                    },
                    Instruction::Less { keep_rhs } => {
                        debug_println!(debug, "    LT keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c == Some(Ordering::Less), *keep_rhs);
                        Ok(())
                    },
                    Instruction::LessEqual { keep_rhs } => {
                        debug_println!(debug, "    LE keep_rhs={}", keep_rhs);
                        self.compare_op(
                            |c| matches!(c, Some(Ordering::Less | Ordering::Equal)),
                            *keep_rhs,
                        );
                        Ok(())
                    },
                    Instruction::Greater { keep_rhs } => {
                        debug_println!(debug, "    GT keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c == Some(Ordering::Greater), *keep_rhs);
                        Ok(())
                    },
                    Instruction::GreaterEqual { keep_rhs } => {
                        debug_println!(debug, "    GE keep_rhs={}", keep_rhs);
                        self.compare_op(
                            |c| matches!(c, Some(Ordering::Greater | Ordering::Equal)),
                            *keep_rhs,
                        );
                        Ok(())
                    },
                    Instruction::GetGlobal { name_index } => {
                        let name_str = constants.string(*name_index);
                        debug_println!(debug, "    GET_GLOBAL {}", name_str);
                        if let Some(&val) = self.global_symbols.get(name_str) {
                            self.stack.push(val);
                            Ok(())
                        } else {
                            Err(RuntimeError::undefined(name_str))
                        }
                    },
                    Instruction::SetGlobal { name_index } => {
                        let name_str = constants.string(*name_index).to_string();
                        let val = self.stack.pop();
                        debug_println!(debug, "    SET_GLOBAL {} = {:?}", name_str, val);
                        self.global_symbols.insert(name_str, val);
                        Ok(())
                    },
                    Instruction::GetLocal { index } => {
                        debug_println!(debug, "    GET_LOCAL {} fp={}", index, fp);
                        // A captured local's cell is the authoritative value: a
                        // closure may have updated it via `SetUpvalue` without
                        // the owner's stack slot being refreshed.
                        let frame = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists");
                        let val = if frame.captured_locals.is_empty() {
                            self.stack.get_local(fp, *index)
                        } else {
                            frame
                                .captured_locals
                                .get(&index.as_index())
                                .and_then(|cell| cell.try_borrow().ok())
                                .map_or_else(|| self.stack.get_local(fp, *index), |cell| cell.value)
                        };
                        self.stack.push(val);
                        Ok(())
                    },
                    Instruction::SetLocal { index } => {
                        let val = self.stack.pop();
                        debug_println!(debug, "    SET_LOCAL {} fp={} val={:?}", index, fp, val);
                        self.stack.set_local(fp, *index, val);

                        let frame = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists");
                        if !frame.captured_locals.is_empty()
                            && let Some(cell) = frame.captured_locals.get(&index.as_index())
                        {
                            cell.borrow_mut().value = val;
                        }
                        Ok(())
                    },
                    Instruction::ForLoop {
                        control_index,
                        limit_index,
                        body_offset,
                        inclusive,
                        step_index,
                    } => {
                        debug_println!(debug, "    FOR_LOOP");
                        let current = as_integer(&self.stack.get_local(fp, *control_index));
                        let limit_val = as_integer(&self.stack.get_local(fp, *limit_index));

                        match (current, limit_val) {
                            (Some(current), Some(limit_val)) => {
                                let step = step_index
                                    .and_then(|si| as_integer(&self.stack.get_local(fp, si)))
                                    .unwrap_or(1);

                                let next = current + step;
                                self.stack.set_local(
                                    fp,
                                    *control_index,
                                    StackValue::Primitive(Primitive::Integer(next)),
                                );

                                let keep_going = if *inclusive {
                                    next <= limit_val
                                } else {
                                    next < limit_val
                                };
                                debug_println!(
                                    debug,
                                    "    FOR_LOOP ctrl={} limit={} step={} next={} keep_going={}",
                                    current,
                                    limit_val,
                                    step,
                                    next,
                                    keep_going
                                );
                                if keep_going {
                                    ip = *body_offset;
                                }
                                Ok(())
                            },
                            (None, _) => Err(RuntimeError::type_error(
                                "for loop requires integer control",
                            )),
                            (_, None) => {
                                Err(RuntimeError::type_error("for loop requires integer limit"))
                            },
                        }
                    },
                    Instruction::Jump { offset } => {
                        debug_println!(debug, "    JUMP -> {}", offset);
                        ip = *offset;
                        Ok(())
                    },
                    Instruction::JumpIf {
                        offset,
                        expected,
                        keep_stay,
                        keep_jump,
                    } => {
                        let cond = self.stack.top();
                        let truthy = cond.is_truthy(&self.heap);
                        let should_jump = truthy == *expected;
                        debug_println!(
                            debug,
                            "    JUMP_IF expected={} truthy={} should_jump={} -> {}",
                            expected,
                            truthy,
                            should_jump,
                            offset
                        );
                        let should_pop = if should_jump { !keep_jump } else { !keep_stay };
                        if should_pop {
                            self.stack.pop();
                        }
                        if should_jump {
                            ip = *offset;
                        }
                        Ok(())
                    },
                    Instruction::Call {
                        arg_count,
                        keep_return_value,
                    } => {
                        debug_println!(
                            debug,
                            "    CALL argc={} keep_ret={}",
                            arg_count,
                            keep_return_value
                        );
                        // stack = …… base arg1 arg2 arg3 end
                        if let Some(base) = self.stack.len().checked_sub(*arg_count + 1) {
                            let frame_count = self.frames.len();
                            let call_site = (chunk_id, CodeOffset(ip.0 - 1));
                            self.frames
                                .last_mut()
                                .expect("execution continues only while a frame exists")
                                .ip = ip;

                            let func_sv = *self.stack.get(base).expect("stack underflow");

                            match self.call_function(
                                func_sv,
                                base,
                                *arg_count,
                                *keep_return_value,
                                call_site,
                            ) {
                                Err(e) => Err(e),
                                Ok(result) => {
                                    if *keep_return_value && self.frames.len() <= frame_count {
                                        self.stack.push(result);
                                    }
                                    if self.frames.len() > frame_count {
                                        self.maybe_gc();
                                        break;
                                    }
                                    Ok(())
                                },
                            }
                        } else {
                            Err(RuntimeError::generic("stack underflow"))
                        }
                    },
                    Instruction::MakeArray { count } => {
                        debug_println!(debug, "    MAKE_ARRAY {}", count);
                        match self.stack.len().checked_sub(*count) {
                            Some(start) => {
                                let elements = self.stack.drain_from(start);
                                let sv = self.heap.alloc_vector(elements);
                                self.stack.push(sv);
                                self.maybe_gc();
                                Ok(())
                            },
                            None => Err(RuntimeError::generic("stack underflow")),
                        }
                    },
                    Instruction::VectorAppend { count } => {
                        debug_println!(debug, "    VECTOR_APPEND {}", count);
                        let Some(start) = self.stack.len().checked_sub(*count + 1) else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        let Some(&container) = self.stack.get(start) else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        let StackValue::Heap(key) = container else {
                            return Err(RuntimeError::type_error(
                                "unsupported operand types for +",
                            ));
                        };
                        let Some(cell) = self.heap.cells.get_mut(key) else {
                            return Err(RuntimeError::type_error("invalid heap reference"));
                        };
                        let Some(vec) = cell.value.as_mut_vector() else {
                            return Err(RuntimeError::type_error(
                                "unsupported operand types for +",
                            ));
                        };
                        vec.extend(self.stack.drain_from(start + 1));
                        Ok(())
                    },
                    Instruction::GetIndex => {
                        debug_println!(debug, "    GET_INDEX");
                        let idx = self.stack.pop();
                        let container = self.stack.top_mut();
                        match index(container, &idx, &mut self.heap) {
                            Ok(result) => {
                                *container = result;
                                Ok(())
                            },
                            Err(e) => Err(e),
                        }
                    },
                    Instruction::SetIndex => {
                        debug_println!(debug, "    SET_INDEX");
                        let val = self.stack.pop();
                        let idx = self.stack.pop();
                        let container = self.stack.top_mut();
                        match index_mut(container, &idx, &mut self.heap) {
                            Ok(target) => {
                                *target = val;
                                Ok(())
                            },
                            Err(e) => Err(e),
                        }
                    },
                    Instruction::Closure {
                        function_index,
                        upvalues,
                    } => {
                        debug_println!(
                            debug,
                            "    CLOSURE func={} upvalues={}",
                            function_index,
                            upvalues.len()
                        );
                        let constant_key = *self
                            .constant_keys
                            .get(function_index.as_index())
                            .expect("closure constant was pre-inserted into the heap");
                        let bc = self.heap.cells.get(constant_key).map_or_else(
                            || {
                                Err(RuntimeError::type_error(
                                    "invalid closure function constant",
                                ))
                            },
                            |cell| match &cell.value {
                                HeapData::Function(Function::Bytecode(bc)) => Ok(bc.clone()),
                                _ => Err(RuntimeError::type_error(
                                    "closure constant is not a function",
                                )),
                            },
                        );
                        match bc {
                            Err(e) => Err(e),
                            Ok(mut bc) => {
                                bc.captured_upvalues = Rc::new(
                                    upvalues
                                        .iter()
                                        .map(|&UpvalueDesc { is_local, index }| {
                                            let current_frame = self.frames.last_mut().expect(
                                                "execution continues only while a frame exists",
                                            );

                                            if is_local {
                                                current_frame
                                                    .captured_locals
                                                    .entry(index as usize)
                                                    .or_insert_with(|| {
                                                        let captured =
                                                            self.stack.get_local(fp, LocalIndex(index));
                                                        Rc::new(RefCell::new(UpvalueCell::new(
                                                            captured,
                                                        )))
                                                    })
                                                    .clone()
                                            } else {
                                                current_frame
                                                    .upvalues
                                                    .get(index as usize)
                                                    .expect(
                                                        "upvalue index is within the captured upvalues",
                                                    )
                                                    .clone()
                                            }
                                        })
                                        .collect(),
                                );
                                let sv = self.heap.alloc_function(Function::Bytecode(bc));
                                debug_println!(debug, "      -> allocated function {:?}", sv);
                                self.stack.push(sv);
                                self.maybe_gc();
                                Ok(())
                            },
                        }
                    },
                    Instruction::GetUpvalue { index } => {
                        debug_println!(debug, "    GET_UPVALUE {}", index);
                        if let Ok(cell) = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .upvalues
                            .get(index.as_index())
                            .expect("upvalue index is within the captured upvalues")
                            .try_borrow()
                        {
                            self.stack.push(cell.value);
                        }
                        Ok(())
                    },
                    Instruction::SetUpvalue { index } => {
                        let val = self.stack.pop();
                        debug_println!(debug, "    SET_UPVALUE {} val={:?}", index, val);
                        if let Ok(mut cell) = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .upvalues
                            .get(index.as_index())
                            .expect("upvalue index is within the captured upvalues")
                            .try_borrow_mut()
                        {
                            cell.value = val;
                        }
                        Ok(())
                    },
                };

                if let Err(e) = result {
                    self.frames
                        .last_mut()
                        .expect("execution continues only while a frame exists")
                        .ip = CodeOffset(ip.0 - 1);
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    fn run_gc(&mut self) {
        self.gc_mark_roots();
        self.heap.sweep();
    }

    #[inline]
    fn maybe_gc(&mut self) {
        if self.heap.added_since_last_sweep >= self.heap.next_gc_threshold {
            self.run_gc();
        }
    }

    fn gc_mark_roots(&self) {
        for sv in &self.stack.values {
            mark_stack_value(sv, &self.heap.cells);
        }
        for frame in &self.frames {
            if let Some((_, ref sv)) = frame.closure_info {
                mark_stack_value(sv, &self.heap.cells);
            }
            for uv in frame.upvalues.iter() {
                if let Ok(uv) = uv.try_borrow() {
                    mark_stack_value(&uv.value, &self.heap.cells);
                }
            }
            for cell in frame.captured_locals.values() {
                if let Ok(cell) = cell.try_borrow() {
                    mark_stack_value(&cell.value, &self.heap.cells);
                }
            }
        }
        for sv in self.global_symbols.values() {
            mark_stack_value(sv, &self.heap.cells);
        }
        for key in &self.constant_keys {
            if let Some(cell) = self.heap.cells.get(*key) {
                cell.mark(&self.heap.cells);
            }
        }
    }

    fn call_function(
        &mut self,
        func_sv: StackValue,
        base: StackIndex,
        arg_count: u32,
        keep_return_value: bool,
        call_site: (ChunkId, CodeOffset),
    ) -> RuntimeResult<StackValue> {
        let func_key = match &func_sv {
            StackValue::Heap(key) => *key,
            _ => return Err(RuntimeError::type_error("cannot call non-function")),
        };

        let args_base = base + 1;
        let Some(args) = self.stack.get_range(args_base, StackIndex(arg_count)) else {
            return Err(RuntimeError::generic("stack underflow"));
        };

        let Some(cell) = self.heap.cells.get(func_key) else {
            return Err(RuntimeError::type_error("invalid heap reference"));
        };

        let function = match cell.value {
            HeapData::Function(ref f) => Some(f),
            _ => None,
        }
        .ok_or_else(|| RuntimeError::type_error("invalid function reference"))?;

        if let Function::Builtin(builtin_function) = function {
            if Some(func_key) == self.trigger_gc_key {
                let before = self.heap.cells.len();
                self.run_gc();
                let erased = before - self.heap.cells.len();
                self.stack.truncate(base);
                return Ok(self.heap.alloc_string(format!("GC swept {erased} cells")));
            }
            let body = Rc::clone(&builtin_function.body);
            let result = body(args, &mut self.heap);
            self.stack.truncate(base);
            self.maybe_gc();
            return result.map_err(|e| RuntimeError::type_error(format!("builtin error: {e}")));
        }

        let Function::Bytecode(bc) = &function else {
            return Err(RuntimeError::type_error("cannot call non-function"));
        };

        let total_args = bc.curried_args.len() as u32 + arg_count;
        if total_args > bc.arity {
            return Err(RuntimeError::type_error("too many arguments"));
        }
        if total_args < bc.arity {
            let mut new_bc = bc.clone();
            new_bc.curried_args.extend_from_slice(args);
            self.stack.truncate(base);
            let sv = self.heap.alloc_function(Function::Bytecode(new_bc));
            self.maybe_gc();
            return Ok(sv);
        }

        let frame_pointer = base + 1;

        if !bc.curried_args.is_empty() {
            self.stack.extend_from_slice(&bc.curried_args);
            if let Some(remainder) = self.stack.get_mut_from(frame_pointer) {
                remainder.rotate_right(bc.curried_args.len());
            }
        }

        let new_frame = CallFrame {
            chunk_id: ChunkId(bc.id),
            ip: CodeOffset(0),
            frame_pointer,
            closure_info: Some((bc.name.clone(), func_sv)),
            call_site: Some(call_site),
            upvalues: Rc::clone(&bc.captured_upvalues),
            captured_locals: HashMap::with_hasher(FixedState::default()),
            discard_return: !keep_return_value,
        };
        self.frames.push(new_frame);

        Ok(StackValue::Nil)
    }

    #[inline]
    fn binary_op<F>(&mut self, f: F) -> RuntimeResult<()>
    where
        F: Fn(&StackValue, &StackValue, &mut Heap) -> RuntimeResult<StackValue>,
    {
        let b = self.stack.pop();
        let a = self.stack.top_mut();
        let result = f(a, &b, &mut self.heap)?;
        *a = result;
        Ok(())
    }

    #[inline]
    fn compare_op<F>(&mut self, pred: F, keep_rhs: bool)
    where
        F: Fn(Option<Ordering>) -> bool,
    {
        let b = self.stack.pop();
        if keep_rhs {
            let a = self.stack.pop();
            let result = StackValue::Primitive(Primitive::Bool(pred(compare(&a, &b, &self.heap))));
            self.stack.push(b);
            self.stack.push(result);
        } else {
            let a = self.stack.top();
            let result = StackValue::Primitive(Primitive::Bool(pred(compare(&a, &b, &self.heap))));
            let top = self.stack.top_mut();
            *top = result;
        }
    }
}

fn mark_stack_value(
    sv: &StackValue,
    cells: &slotmap::SlotMap<slotmap::DefaultKey, l3_runtime::HeapCell>,
) {
    if let StackValue::Heap(key) = sv
        && let Some(cell) = cells.get(*key)
    {
        cell.mark(cells);
    }
}
