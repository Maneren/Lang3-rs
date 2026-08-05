pub mod builtins;

use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::HashMap,
    io::{Read, Write},
    mem,
    rc::Rc,
};

use foldhash::fast::FixedState;
use l3_ast::Identifier;
use l3_bytecode::*;
use l3_location::Location;
use l3_runtime::{
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
    upvalues: Vec<Rc<RefCell<UpvalueCell>>>,
    captured_locals: HashMap<usize, Rc<RefCell<UpvalueCell>>>,
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
            stack: Vec::new(),
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

    pub fn execute(&mut self, program: &ProgramBytecode) -> Result<(), RuntimeError> {
        let debug = self.debug;
        let chunks = &program.chunks;

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
            upvalues: Vec::new(),
            captured_locals: HashMap::new(),
            discard_return: false,
        };
        self.frames.push(frame);

        let result = self.execute_loop(chunks, constants, debug);

        if let Err(mut err) = result {
            if err.location().is_none() {
                let loc = self.current_instruction_location(chunks);
                err = err.with_location(loc);
            }
            err.set_stacktrace(self.build_stacktrace());
            return Err(err);
        }

        self.constant_keys.clear();
        Ok(())
    }

    /// The instruction currently being executed is the one at `frame.ip`, which
    /// `execute_loop` keeps synced with the dispatch index.
    fn current_instruction_location(&self, chunks: &[Chunk]) -> Location {
        let Some(frame) = self.frames.last() else {
            return Location::default();
        };
        chunks
            .get(frame.chunk_id)
            .and_then(|chunk| chunk.locations.get(frame.ip))
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
    ) -> Result<(), RuntimeError> {
        while let Some(frame) = self.frames.last() {
            let (mut ip, chunk_id) = (frame.ip, frame.chunk_id);
            let code = &chunks
                .get(chunk_id)
                .expect("frames only reference chunks known to the program")
                .code;

            while let Some(instruction) = code.get(ip) {
                debug_println!(debug, "  IP={} {:?}", ip, instruction);
                ip += 1;

                match instruction {
                    Instruction::Return => {
                        let return_value = self.stack.pop().unwrap_or(StackValue::Nil);
                        debug_println!(debug, "    RETURN value={:?}", return_value);
                        let fp = self.frames.last().map_or(0, |f| f.frame_pointer);
                        let discard = self.frames.last().is_some_and(|f| f.discard_return);
                        self.frames.pop();
                        if !self.frames.is_empty() {
                            self.stack.truncate(fp - 1);
                        }
                        if !discard {
                            self.stack.push(return_value);
                        }
                        self.maybe_gc();
                        break;
                    },
                    Instruction::Constant { index } => {
                        let val = &program
                            .constants
                            .get(*index)
                            .expect("constant index emitted by the compiler is valid");
                        let sv = match &val.value {
                            HeapData::Nil => StackValue::Nil,
                            HeapData::Primitive(p) => StackValue::Primitive(*p),
                            _ => StackValue::Heap(
                                *self
                                    .constant_keys
                                    .get(*index)
                                    .expect("constant was pre-inserted into the heap"),
                            ),
                        };
                        debug_println!(debug, "    CONSTANT({}) -> {:?}", index, sv);
                        self.stack.push(sv);
                    },
                    Instruction::Pop { count } => {
                        debug_println!(debug, "    POP {}", count);
                        for _ in 0..*count {
                            self.stack.pop();
                        }
                    },
                    Instruction::Duplicate { index } => {
                        let idx = self.stack.len() - 1 - index;
                        debug_println!(debug, "    DUPLICATE({}) stack[len-1-{}]", index, index);
                        if let Some(sv) = self.stack.get(idx) {
                            self.stack.push(*sv);
                        }
                    },
                    Instruction::Add => {
                        debug_println!(debug, "    ADD");
                        self.binary_op(add)?;
                    },
                    Instruction::Subtract => {
                        debug_println!(debug, "    SUB");
                        self.binary_op(sub)?;
                    },
                    Instruction::Multiply => {
                        debug_println!(debug, "    MUL");
                        self.binary_op(mul)?;
                    },
                    Instruction::Divide => {
                        debug_println!(debug, "    DIV");
                        self.binary_op(div)?;
                    },
                    Instruction::Modulo => {
                        debug_println!(debug, "    MOD");
                        self.binary_op(modulo)?;
                    },
                    Instruction::Power => {
                        debug_println!(debug, "    POW");
                        self.binary_op(pow)?;
                    },
                    Instruction::Negate => {
                        debug_println!(debug, "    NEGATE");
                        let Some(a) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        let result = negative(&a, &self.heap)?;
                        self.stack.push(result);
                    },
                    Instruction::Not => {
                        debug_println!(debug, "    NOT");
                        let Some(a) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        self.stack.push(not_op(&a, &self.heap));
                    },
                    Instruction::Equal { keep_rhs } => {
                        debug_println!(debug, "    EQ keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c == Some(Ordering::Equal), *keep_rhs);
                    },
                    Instruction::NotEqual { keep_rhs } => {
                        debug_println!(debug, "    NE keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c != Some(Ordering::Equal), *keep_rhs);
                    },
                    Instruction::Less { keep_rhs } => {
                        debug_println!(debug, "    LT keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c == Some(Ordering::Less), *keep_rhs);
                    },
                    Instruction::LessEqual { keep_rhs } => {
                        debug_println!(debug, "    LE keep_rhs={}", keep_rhs);
                        self.compare_op(
                            |c| matches!(c, Some(Ordering::Less | Ordering::Equal)),
                            *keep_rhs,
                        );
                    },
                    Instruction::Greater { keep_rhs } => {
                        debug_println!(debug, "    GT keep_rhs={}", keep_rhs);
                        self.compare_op(|c| c == Some(Ordering::Greater), *keep_rhs);
                    },
                    Instruction::GreaterEqual { keep_rhs } => {
                        debug_println!(debug, "    GE keep_rhs={}", keep_rhs);
                        self.compare_op(
                            |c| matches!(c, Some(Ordering::Greater | Ordering::Equal)),
                            *keep_rhs,
                        );
                    },
                    Instruction::GetGlobal { name_index } => {
                        let name = &program
                            .constants
                            .get(*name_index)
                            .expect("global name constant emitted by the compiler is valid");
                        if let HeapData::String(s) = &name.value {
                            debug_println!(debug, "    GET_GLOBAL {}", s);
                            if let Some(&val) = self.global_symbols.get(s) {
                                self.stack.push(val);
                            } else {
                                return Err(RuntimeError::undefined(s));
                            }
                        }
                    },
                    Instruction::SetGlobal { name_index } => {
                        let name = &program
                            .constants
                            .get(*name_index)
                            .expect("global name constant emitted by the compiler is valid");
                        let name_str = name.value.as_string().unwrap_or("__unknown__").to_string();
                        let Some(val) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        debug_println!(debug, "    SET_GLOBAL {} = {:?}", name_str, val);
                        self.global_symbols.insert(name_str, val);
                    },
                    Instruction::GetLocal { index } => {
                        let fp = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .frame_pointer;
                        debug_println!(debug, "    GET_LOCAL {} fp={}", index, fp);
                        if let Some(&cell) = self.stack.get(fp + index) {
                            self.stack.push(cell);
                        }
                    },
                    Instruction::SetLocal { index } => {
                        let fp = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .frame_pointer;
                        let Some(val) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        debug_println!(debug, "    SET_LOCAL {} fp={} val={:?}", index, fp, val);
                        let _old = mem::replace(
                            self.stack
                                .get_mut(fp + index)
                                .expect("local slot is within the current frame"),
                            val,
                        );

                        if let Some(cell) = self
                            .frames
                            .last_mut()
                            .expect("execution continues only while a frame exists")
                            .captured_locals
                            .get(index)
                        {
                            cell.borrow_mut().value = *self
                                .stack
                                .get(fp + index)
                                .expect("local slot is within the current frame");
                        }
                    },
                    Instruction::ForLoop {
                        control_index,
                        limit_index,
                        body_offset,
                        inclusive,
                        step_index,
                    } => {
                        let fp = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .frame_pointer;
                        let control = self
                            .stack
                            .get(fp + control_index)
                            .expect("for-loop control slot is within the current frame");
                        let limit = self
                            .stack
                            .get(fp + limit_index)
                            .expect("for-loop limit slot is within the current frame");

                        let Some(Primitive::Integer(current)) = control.as_primitive() else {
                            return Err(RuntimeError::type_error(
                                "for loop requires integer control",
                            ));
                        };
                        let Some(Primitive::Integer(limit_val)) = limit.as_primitive() else {
                            return Err(RuntimeError::type_error(
                                "for loop requires integer limit",
                            ));
                        };

                        let step = if let Some(si) = step_index {
                            match self
                                .stack
                                .get(fp + si)
                                .expect("for-loop step slot is within the current frame")
                                .as_primitive()
                            {
                                Some(Primitive::Integer(i)) => i,
                                _ => 1,
                            }
                        } else {
                            1
                        };

                        let next = current + step;
                        *self
                            .stack
                            .get_mut(fp + control_index)
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
                            ip = *body_offset;
                        }
                    },
                    Instruction::Jump { offset } => {
                        debug_println!(debug, "    JUMP -> {}", offset);
                        ip = *offset;
                    },
                    Instruction::JumpIf {
                        offset,
                        expected,
                        keep_stay,
                        keep_jump,
                    } => {
                        let Some(cond) = self.stack.last() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
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
                    },
                    Instruction::Call {
                        arg_count,
                        keep_return_value,
                    } => {
                        let base = self.stack.len() - 1 - arg_count;
                        let func_sv = *self.stack.get(base).expect("call base is within the stack");
                        debug_println!(
                            debug,
                            "    CALL argc={} keep_ret={} func={:?}",
                            arg_count,
                            keep_return_value,
                            func_sv
                        );
                        let frame_count = self.frames.len();
                        self.frames
                            .last_mut()
                            .expect("execution continues only while a frame exists")
                            .ip = ip;
                        let result =
                            self.call_function(func_sv, base, *arg_count, *keep_return_value)?;
                        if *keep_return_value && self.frames.len() <= frame_count {
                            self.stack.push(result);
                        }
                        if self.frames.len() > frame_count {
                            self.maybe_gc();
                            break;
                        }
                    },
                    Instruction::MakeArray { count } => {
                        debug_println!(debug, "    MAKE_ARRAY {}", count);
                        let start = self.stack.len() - count;
                        let elements: Vec<StackValue> = self.stack.drain(start..).collect();
                        let sv = self.heap.alloc_vector(elements);
                        self.stack.push(sv);
                    },
                    Instruction::GetIndex => {
                        debug_println!(debug, "    GET_INDEX");
                        let Some(idx) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        let Some(container) = self.stack.last_mut() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        *container = index(container, &idx, &mut self.heap)?;
                    },
                    Instruction::SetIndex => {
                        debug_println!(debug, "    SET_INDEX");
                        let Some(val) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        let Some(idx) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        let Some(container) = self.stack.last_mut() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        *index_mut(container, &idx, &mut self.heap)? = val;
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
                            .get(*function_index)
                            .expect("closure constant was pre-inserted into the heap");
                        let mut bc = match self.heap.cells.get(constant_key) {
                            Some(cell) => match &cell.value {
                                HeapData::Function(Function::Bytecode(bc)) => bc.clone(),
                                _ => {
                                    return Err(RuntimeError::type_error(
                                        "closure constant is not a function",
                                    ));
                                },
                            },
                            None => {
                                return Err(RuntimeError::type_error(
                                    "invalid closure function constant",
                                ));
                            },
                        };
                        for uv_desc in upvalues {
                            if uv_desc.is_local {
                                let fp = self
                                    .frames
                                    .last()
                                    .expect("execution continues only while a frame exists")
                                    .frame_pointer;
                                let idx = uv_desc.index;
                                let captured = *self
                                    .stack
                                    .get(fp + idx)
                                    .expect("captured local slot is within the current frame");
                                let cell = Rc::new(RefCell::new(UpvalueCell::new(captured)));
                                self.frames
                                    .last_mut()
                                    .expect("execution continues only while a frame exists")
                                    .captured_locals
                                    .insert(idx, cell.clone());
                                bc.captured_upvalues.push(cell);
                            } else {
                                let uv = self
                                    .frames
                                    .last()
                                    .expect("execution continues only while a frame exists")
                                    .upvalues
                                    .get(uv_desc.index)
                                    .expect("upvalue index is within the captured upvalues")
                                    .clone();
                                bc.captured_upvalues.push(uv);
                            }
                        }
                        let sv = self.heap.alloc_function(Function::Bytecode(bc));
                        debug_println!(debug, "      -> allocated function {:?}", sv);
                        self.stack.push(sv);
                    },
                    Instruction::GetUpvalue { index } => {
                        debug_println!(debug, "    GET_UPVALUE {}", index);
                        if let Ok(cell) = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .upvalues
                            .get(*index)
                            .expect("upvalue index is within the captured upvalues")
                            .try_borrow()
                        {
                            self.stack.push(cell.value);
                        }
                    },
                    Instruction::SetUpvalue { index } => {
                        let Some(val) = self.stack.pop() else {
                            return Err(RuntimeError::generic("stack underflow"));
                        };
                        debug_println!(debug, "    SET_UPVALUE {} val={:?}", index, val);
                        if let Ok(mut cell) = self
                            .frames
                            .last()
                            .expect("execution continues only while a frame exists")
                            .upvalues
                            .get(*index)
                            .expect("upvalue index is within the captured upvalues")
                            .try_borrow_mut()
                        {
                            cell.value = val;
                        }
                    },
                }

                self.maybe_gc();
            }
        }

        self.constant_keys.clear();
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
    ) -> Result<StackValue, RuntimeError> {
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
            return Ok(self.heap.alloc_function(Function::Bytecode(new_bc)));
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
            upvalues: bc.captured_upvalues.clone(),
            captured_locals: HashMap::new(),
            discard_return: !keep_return_value,
        };
        self.frames.push(new_frame);

        Ok(StackValue::Nil)
    }

    fn binary_op<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: Fn(&StackValue, &StackValue, &mut Heap) -> Result<StackValue, RuntimeError>,
    {
        let Some(b) = self.stack.pop() else {
            return Err(RuntimeError::generic("stack underflow"));
        };
        let Some(a) = self.stack.last_mut() else {
            return Err(RuntimeError::generic("stack underflow"));
        };
        let result = f(a, &b, &mut self.heap)?;
        *a = result;
        Ok(())
    }

    fn compare_op<F>(&mut self, pred: F, keep_rhs: bool)
    where
        F: Fn(Option<Ordering>) -> bool,
    {
        let b = self.stack.pop().unwrap_or(StackValue::Nil);
        if keep_rhs {
            let a = self.stack.pop().unwrap_or(StackValue::Nil);
            let result = StackValue::Primitive(Primitive::Bool(pred(compare(&a, &b, &self.heap))));
            self.stack.push(b);
            self.stack.push(result);
        } else {
            let result = match self.stack.last() {
                Some(a) => StackValue::Primitive(Primitive::Bool(pred(compare(a, &b, &self.heap)))),
                None => StackValue::Primitive(Primitive::Bool(pred(compare(
                    &StackValue::Nil,
                    &b,
                    &self.heap,
                )))),
            };
            if let Some(top) = self.stack.last_mut() {
                *top = result;
            } else {
                self.stack.push(result);
            }
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
