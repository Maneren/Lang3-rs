pub mod builtins;

use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::HashMap,
    io::{Read, Write},
    rc::Rc,
};

use foldhash::fast::FixedState;
use l3_ast::Identifier;
use l3_bytecode::*;
use l3_location::Location;
use l3_runtime::{
    error::RuntimeResult,
    heap_data::{add, compare, div, index, index_mut, modulo, mul, negative, not_op, pow, sub},
    *,
};

pub struct BytecodeVM<'a> {
    pub heap: Heap<'a>,
    pub stack: Vec<StackValue>,
    pub global_symbols: HashMap<String, StackValue, FixedState>,
    constant_keys: Vec<slotmap::DefaultKey>,
    frames: Vec<CallFrame>,
    debug: bool,
}

struct CallFrame {
    chunk_id: usize,
    ip: usize,
    frame_pointer: usize,
    closure_info: Option<(BytecodeFunction, StackValue)>,
    call_location: Option<Location>,
    upvalues: Box<[Rc<RefCell<UpvalueCell>>]>,
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
            stack: Vec::with_capacity(1024),
            global_symbols: HashMap::with_hasher(FixedState::default()),
            constant_keys: Vec::new(),
            frames: Vec::new(),
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

        vm
    }

    pub fn execute(&mut self, program: &ProgramBytecode) -> RuntimeResult<()> {
        let debug = self.debug;
        let chunks = &program.chunks;
        let constants = &program.constants;

        // Pre-insert all program constants into the heap so the Constant
        // dispatch can use a cached slotmap key instead of a linear scan.
        self.constant_keys = program
            .constants
            .iter()
            .map(|hc| self.heap.cells.insert(hc.clone()))
            .collect();

        let frame = CallFrame {
            chunk_id: 0,
            ip: 0,
            frame_pointer: 0,
            closure_info: None,
            call_location: None,
            upvalues: Box::new([]),
            captured_locals: HashMap::with_hasher(FixedState::default()),
            discard_return: false,
        };
        self.frames.push(frame);

        let result = self.execute_loop(chunks, constants, debug);

        if let Err(mut err) = result {
            if err.location().is_none() {
                let loc = self
                    .frames
                    .last()
                    .map_or_default(|frame| self.instruction_location(chunks, frame.ip));
                err = err.with_location(loc);
            }
            err.set_stacktrace(self.build_stacktrace());
            return Err(err);
        }

        self.constant_keys.clear();
        Ok(())
    }

    /// The instruction being executed is the one at `ip`, which `execute_loop`
    /// keeps synced with `frame.ip`.
    fn instruction_location(&self, chunks: &[Chunk], ip: usize) -> Location {
        let Some(frame) = self.frames.last() else {
            return Location::default();
        };
        chunks
            .get(frame.chunk_id)
            .and_then(|chunk| chunk.locations.get(ip))
            .cloned()
            .unwrap_or_default()
    }

    fn build_stacktrace(&self) -> Vec<StacktraceFrame> {
        self.frames
            .iter()
            .filter_map(|frame| {
                let call_location = frame.call_location.as_ref()?;
                let function_name = frame
                    .closure_info
                    .as_ref()
                    .map_or_else(|| "<toplevel>".to_string(), |(bc, _)| bc.name.clone());
                Some(StacktraceFrame {
                    function_name,
                    call_location: call_location.clone(),
                })
            })
            .collect()
    }

    fn execute_loop(
        &mut self,
        chunks: &[Chunk],
        constants: &[l3_runtime::HeapCell],
        debug: bool,
    ) -> RuntimeResult<()> {
        while let Some(frame) = self.frames.last() {
            let (mut ip, chunk_id, fp) = (frame.ip, frame.chunk_id, frame.frame_pointer);
            let code = &chunks
                .get(chunk_id)
                .expect("frames only reference chunks known to the program")
                .code;

            while let Some(instruction) = code.get(ip) {
                debug_println!(debug, "  IP={} {:?}", ip, instruction);
                ip += 1;

                // The frame's `ip` is only synced here (on error) and in the
                // Call/Return handlers, so the hot dispatch loop avoids a
                // per-instruction write back into `self.frames`.
                let result: RuntimeResult<()> = match instruction {
                    Instruction::Return => {
                        let CallFrame {
                            frame_pointer,
                            discard_return,
                            ..
                        } = self
                            .frames
                            .pop()
                            .expect("Return is only emitted inside a function");

                        if !self.frames.is_empty() {
                            // if the function returns a value, pop it off the stack
                            let return_value = *self.stack.last().expect(
                                "the compiler pushes a return value before every Return inside a \
                                 function",
                            );
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
                        let val = &constants
                            .get(*index as usize)
                            .expect("constant index emitted by the compiler is valid");
                        let sv = match &val.value {
                            HeapData::Nil => StackValue::Nil,
                            HeapData::Primitive(p) => StackValue::Primitive(*p),
                            _ => StackValue::Heap(
                                *self
                                    .constant_keys
                                    .get(*index as usize)
                                    .expect("constant was pre-inserted into the heap"),
                            ),
                        };
                        debug_println!(debug, "    CONSTANT({}) -> {:?}", index, sv);
                        self.stack.push(sv);
                        Ok(())
                    },
                    Instruction::Pop { count } => {
                        debug_println!(debug, "    POP {}", count);
                        match self.stack.len().checked_sub(*count as usize) {
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
                            .checked_sub(*index as usize)
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
                        let a = Self::pop_value(&mut self.stack);
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
                        let a = Self::pop_value(&mut self.stack);
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
                        let name = &constants
                            .get(*name_index as usize)
                            .expect("global name constant emitted by the compiler is valid");
                        let name_str = name
                            .value
                            .as_string()
                            .expect("global name constants emitted by the compiler are strings");
                        debug_println!(debug, "    GET_GLOBAL {}", name_str);
                        if let Some(&val) = self.global_symbols.get(name_str) {
                            self.stack.push(val);
                            Ok(())
                        } else {
                            Err(RuntimeError::undefined(name_str))
                        }
                    },
                    Instruction::SetGlobal { name_index } => {
                        let name = &constants
                            .get(*name_index as usize)
                            .expect("global name constant emitted by the compiler is valid");
                        let name_str = name
                            .value
                            .as_string()
                            .expect("global name constants emitted by the compiler are strings")
                            .to_string();
                        let val = Self::pop_value(&mut self.stack);
                        debug_println!(debug, "    SET_GLOBAL {} = {:?}", name_str, val);
                        self.global_symbols.insert(name_str, val);
                        Ok(())
                    },
                    Instruction::GetLocal { index } => {
                        debug_println!(debug, "    GET_LOCAL {} fp={}", index, fp);
                        if let Some(&cell) = self.stack.get(fp + *index as usize) {
                            self.stack.push(cell);
                        }
                        Ok(())
                    },
                    Instruction::SetLocal { index } => {
                        let val = Self::pop_value(&mut self.stack);
                        debug_println!(debug, "    SET_LOCAL {} fp={} val={:?}", index, fp, val);
                        *self
                            .stack
                            .get_mut(fp + *index as usize)
                            .expect("local slot is within the current frame") = val;

                        if let Some(cell) = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .captured_locals
                            .get(&(*index as usize))
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
                        let current = self
                            .stack
                            .get(fp + *control_index as usize)
                            .expect("for-loop control slot is within the current frame")
                            .as_primitive()
                            .and_then(|p| match p {
                                Primitive::Integer(i) => Some(i),
                                _ => None,
                            });
                        let limit_val = self
                            .stack
                            .get(fp + *limit_index as usize)
                            .expect("for-loop limit slot is within the current frame")
                            .as_primitive()
                            .and_then(|p| match p {
                                Primitive::Integer(i) => Some(i),
                                _ => None,
                            });

                        match (current, limit_val) {
                            (Some(current), Some(limit_val)) => {
                                let step = step_index
                                    .and_then(|si| {
                                        self.stack
                                            .get(fp + si as usize)
                                            .expect(
                                                "for-loop step slot is within the current frame",
                                            )
                                            .as_primitive()
                                    })
                                    .and_then(|p| match p {
                                        Primitive::Integer(i) => Some(i),
                                        _ => None,
                                    })
                                    .unwrap_or(1);

                                let next = current + step;
                                *self
                                    .stack
                                    .get_mut(fp + *control_index as usize)
                                    .expect("for-loop control slot is within the current frame") =
                                    StackValue::Primitive(Primitive::Integer(next));

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
                                    ip = *body_offset as usize;
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
                        ip = *offset as usize;
                        Ok(())
                    },
                    Instruction::JumpIf {
                        offset,
                        expected,
                        keep_stay,
                        keep_jump,
                    } => {
                        let cond = Self::top_value(&self.stack);
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
                            Self::pop_value(&mut self.stack);
                        }
                        if should_jump {
                            ip = *offset as usize;
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
                        match self
                            .stack
                            .len()
                            .checked_sub(*arg_count as usize)
                            .and_then(|n| n.checked_sub(1))
                        {
                            None => Err(RuntimeError::generic("stack underflow")),
                            Some(base) => {
                                let func_sv = *self
                                    .stack
                                    .get(base)
                                    .expect("call base is a valid stack index");
                                let frame_count = self.frames.len();
                                let call_location = self.instruction_location(chunks, ip - 1);
                                self.frames
                                    .last_mut()
                                    .expect("execution continues only while a frame exists")
                                    .ip = ip;
                                match self.call_function(
                                    func_sv,
                                    base,
                                    *arg_count as usize,
                                    *keep_return_value,
                                    &call_location,
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
                            },
                        }
                    },
                    Instruction::MakeArray { count } => {
                        debug_println!(debug, "    MAKE_ARRAY {}", count);
                        match self.stack.len().checked_sub(*count as usize) {
                            Some(start) => {
                                let elements: Vec<StackValue> = self.stack.drain(start..).collect();
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
                        let Some(start) = self.stack.len().checked_sub(*count as usize + 1) else {
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
                        vec.extend(self.stack.drain(start + 1..));
                        Ok(())
                    },
                    Instruction::GetIndex => {
                        debug_println!(debug, "    GET_INDEX");
                        let idx = Self::pop_value(&mut self.stack);
                        let container = Self::top_value_mut(&mut self.stack);
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
                        let val = Self::pop_value(&mut self.stack);
                        let idx = Self::pop_value(&mut self.stack);
                        let container = Self::top_value_mut(&mut self.stack);
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
                            .get(*function_index as usize)
                            .expect("closure constant was pre-inserted into the heap");
                        let bc = match self.heap.cells.get(constant_key) {
                            Some(cell) => match &cell.value {
                                HeapData::Function(Function::Bytecode(bc)) => Ok(bc.clone()),
                                _ => Err(RuntimeError::type_error(
                                    "closure constant is not a function",
                                )),
                            },
                            None => Err(RuntimeError::type_error(
                                "invalid closure function constant",
                            )),
                        };
                        match bc {
                            Err(e) => Err(e),
                            Ok(mut bc) => {
                                bc.captured_upvalues = upvalues
                                    .iter()
                                    .map(|&UpvalueDesc { is_local, index }| {
                                        let current_frame = self.frames.last_mut().expect(
                                            "execution continues only while a frame exists",
                                        );

                                        if is_local {
                                            current_frame
                                                .captured_locals
                                                .entry(index)
                                                .or_insert_with(|| {
                                                    let captured =
                                                        *self.stack.get(fp + index).expect(
                                                            "captured local slot is within the \
                                                             current frame",
                                                        );
                                                    Rc::new(RefCell::new(UpvalueCell::new(
                                                        captured,
                                                    )))
                                                })
                                                .clone()
                                        } else {
                                            current_frame
                                                .upvalues
                                                .get(index)
                                                .expect(
                                                    "upvalue index is within the captured upvalues",
                                                )
                                                .clone()
                                        }
                                    })
                                    .collect();
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
                            .get(*index as usize)
                            .expect("upvalue index is within the captured upvalues")
                            .try_borrow()
                        {
                            self.stack.push(cell.value);
                        }
                        Ok(())
                    },
                    Instruction::SetUpvalue { index } => {
                        let val = Self::pop_value(&mut self.stack);
                        debug_println!(debug, "    SET_UPVALUE {} val={:?}", index, val);
                        if let Ok(mut cell) = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .upvalues
                            .get(*index as usize)
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
                        .ip = ip - 1;
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

    #[expect(
        clippy::inline_always,
        reason = "It inlines just the check and helps tremendously"
    )]
    #[inline(always)]
    fn maybe_gc(&mut self) {
        if self.heap.added_since_last_sweep >= self.heap.next_gc_threshold {
            self.run_gc();
        }
    }

    fn gc_mark_roots(&self) {
        for sv in &self.stack {
            mark_stack_value(sv, &self.heap.cells);
        }
        for frame in &self.frames {
            if let Some((_, ref sv)) = frame.closure_info {
                mark_stack_value(sv, &self.heap.cells);
            }
            for uv in &frame.upvalues {
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
        func: StackValue,
        base: usize,
        arg_count: usize,
        keep_return_value: bool,
        call_location: &Location,
    ) -> RuntimeResult<StackValue> {
        let func_key = match &func {
            StackValue::Heap(key) => *key,
            _ => return Err(RuntimeError::type_error("cannot call non-function")),
        };

        let args_base = base + 1;
        let Some(args) = self.stack.get(args_base..(args_base + arg_count)) else {
            return Err(RuntimeError::generic("stack underflow"));
        };

        let is_builtin = self
            .heap
            .cells
            .get(func_key)
            .is_some_and(|c| matches!(&c.value, HeapData::Function(Function::Builtin(_))));

        if is_builtin {
            let Some(body) = self.heap.cells.get(func_key).and_then(|c| match &c.value {
                HeapData::Function(Function::Builtin(b)) => Some(b.body.clone()),
                _ => None,
            }) else {
                return Err(RuntimeError::type_error("invalid builtin function"));
            };
            let result = { body(args, &mut self.heap) };
            self.stack.truncate(base);
            self.maybe_gc();
            return result.map_err(|e| RuntimeError::type_error(format!("builtin error: {e}")));
        }

        let Some(cell) = self.heap.cells.get(func_key) else {
            return Err(RuntimeError::type_error("invalid function reference"));
        };
        let HeapData::Function(Function::Bytecode(bc)) = &cell.value else {
            return Err(RuntimeError::type_error("cannot call non-function"));
        };

        let total_args = bc.curried_args.len() + arg_count;
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
            if let Some(remainder) = self.stack.get_mut(frame_pointer..) {
                remainder.rotate_right(bc.curried_args.len());
            }
        }

        let new_frame = CallFrame {
            chunk_id: bc.id,
            ip: 0,
            frame_pointer,
            closure_info: Some((*bc.clone(), func)),
            call_location: Some(call_location.clone()),
            upvalues: bc.captured_upvalues.clone().into(),
            captured_locals: HashMap::with_hasher(FixedState::default()),
            discard_return: !keep_return_value,
        };
        self.frames.push(new_frame);

        Ok(StackValue::Nil)
    }

    fn pop_value(stack: &mut Vec<StackValue>) -> StackValue {
        stack
            .pop()
            .expect("Valid program should not cause stack underflow.")
    }

    const fn top_value(stack: &[StackValue]) -> StackValue {
        *stack
            .last()
            .expect("Valid program should not cause stack underflow")
    }

    const fn top_value_mut(stack: &mut [StackValue]) -> &mut StackValue {
        stack
            .last_mut()
            .expect("Valid program should not cause stack underflow")
    }

    fn binary_op<F>(&mut self, f: F) -> RuntimeResult<()>
    where
        F: Fn(&StackValue, &StackValue, &mut Heap) -> RuntimeResult<StackValue>,
    {
        let b = Self::pop_value(&mut self.stack);
        let a = Self::top_value_mut(&mut self.stack);
        let result = f(a, &b, &mut self.heap)?;
        *a = result;
        Ok(())
    }

    fn compare_op<F>(&mut self, pred: F, keep_rhs: bool)
    where
        F: Fn(Option<Ordering>) -> bool,
    {
        let b = Self::pop_value(&mut self.stack);
        if keep_rhs {
            let a = Self::pop_value(&mut self.stack);
            let result = StackValue::Primitive(Primitive::Bool(pred(compare(&a, &b, &self.heap))));
            self.stack.push(b);
            self.stack.push(result);
        } else {
            let a = Self::top_value(&self.stack);
            let result = StackValue::Primitive(Primitive::Bool(pred(compare(&a, &b, &self.heap))));
            let top = Self::top_value_mut(&mut self.stack);
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
