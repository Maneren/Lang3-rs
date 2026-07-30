pub mod builtins;

use l3_bytecode::*;
use l3_location::Location;
use l3_runtime::heap_data::{
    add, compare, div, index, index_mut, modulo, mul, negative, not_op, pow, sub,
};
use l3_runtime::*;
use std::collections::HashMap;

pub struct BytecodeVM {
    pub heap: Heap,
    pub stack: Vec<StackValue>,
    pub global_symbols: HashMap<String, StackValue>,
    program: Option<ProgramBytecode>,
    constant_keys: Vec<slotmap::DefaultKey>,
    frames: Vec<CallFrame>,
    debug: bool,
}

struct CallFrame {
    chunk_id: usize,
    ip: usize,
    frame_pointer: usize,
    closure_info: Option<(BytecodeFunction, StackValue)>,
    upvalues: Vec<std::rc::Rc<std::cell::RefCell<UpvalueCell>>>,
    captured_locals: HashMap<usize, std::rc::Rc<std::cell::RefCell<UpvalueCell>>>,
}

macro_rules! debug_println {
    ($self:expr, $($arg:tt)*) => {
        if $self.debug {
            eprintln!($($arg)*);
        }
    };
}

impl BytecodeVM {
    #[must_use]
    pub fn new(debug: bool) -> Self {
        let mut vm = Self {
            heap: Heap::new(),
            stack: Vec::new(),
            global_symbols: HashMap::new(),
            program: None,
            constant_keys: Vec::new(),
            frames: Vec::new(),
            debug,
        };

        // Register builtins
        for (name, body) in builtins::builtins() {
            let func = vm
                .heap
                .alloc_function(Function::Builtin(BuiltinFunction::new(
                    l3_ast::Identifier::new(name.to_string(), Location::default()),
                    body,
                )));
            vm.global_symbols.insert(name.to_string(), func);
        }

        vm
    }

    pub fn execute(&mut self, program: &ProgramBytecode) -> Result<(), RuntimeError> {
        self.program = Some(program.clone());
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
            upvalues: Vec::new(),
            captured_locals: HashMap::new(),
        };
        self.frames.push(frame);

        loop {
            self.maybe_gc();

            let chunk_id = self.frames.last().unwrap().chunk_id;
            let ip = self.frames.last().unwrap().ip;

            if chunk_id >= chunks.len() || ip >= chunks[chunk_id].code.len() {
                break;
            }

            let instruction = &chunks[chunk_id].code[ip];
            self.frames.last_mut().unwrap().ip += 1;

            debug_println!(self, "  IP={} {:?}", ip, instruction);

            self.dispatch(instruction)?;

            if self.frames.is_empty() {
                break;
            }
        }

        self.constant_keys.clear();
        self.program = None;
        Ok(())
    }

    fn run_gc(&mut self) {
        self.gc_mark_roots();
        self.heap.sweep();
    }

    fn maybe_gc(&mut self) {
        if self.heap.added_since_last_sweep >= self.heap.next_gc_threshold {
            self.run_gc();
        }
    }

    fn gc_mark_roots(&mut self) {
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

    fn dispatch(&mut self, inst: &Instruction) -> Result<(), RuntimeError> {
        match inst {
            Instruction::Return => {
                let return_value = self.stack.pop().unwrap_or(StackValue::Nil);
                debug_println!(self, "    RETURN value={:?}", return_value);
                let fp = self.frames.last().map_or(0, |f| f.frame_pointer);
                self.frames.pop();
                if !self.frames.is_empty() {
                    self.stack.truncate(fp);
                }
                self.stack.push(return_value);
            }
            Instruction::Constant { index } => {
                let val = &self.program.as_ref().unwrap().constants[*index];
                let sv = match &val.value {
                    HeapData::Nil => StackValue::Nil,
                    HeapData::Primitive(p) => StackValue::Primitive(*p),
                    _ => StackValue::Heap(self.constant_keys[*index]),
                };
                debug_println!(self, "    CONSTANT({}) -> {:?}", index, sv);
                self.stack.push(sv);
            }
            Instruction::Pop { count } => {
                debug_println!(self, "    POP {}", count);
                for _ in 0..*count {
                    self.stack.pop();
                }
            }
            Instruction::Duplicate { index } => {
                let idx = self.stack.len() - 1 - index;
                debug_println!(self, "    DUPLICATE({}) stack[len-1-{}]", index, index);
                if idx < self.stack.len() {
                    self.stack.push(self.stack[idx].clone());
                }
            }
            Instruction::Add => {
                debug_println!(self, "    ADD");
                self.binary_op(add)?;
            }
            Instruction::Subtract => {
                debug_println!(self, "    SUB");
                self.binary_op_simple(sub)?;
            }
            Instruction::Multiply => {
                debug_println!(self, "    MUL");
                self.binary_op(mul)?;
            }
            Instruction::Divide => {
                debug_println!(self, "    DIV");
                self.binary_op_simple(div)?;
            }
            Instruction::Modulo => {
                debug_println!(self, "    MOD");
                self.binary_op_simple(modulo)?;
            }
            Instruction::Power => {
                debug_println!(self, "    POW");
                self.binary_op_simple(pow)?;
            }
            Instruction::Negate => {
                debug_println!(self, "    NEGATE");
                let a = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let result = negative(&a, &self.heap)?;
                self.stack.push(result);
            }
            Instruction::Not => {
                debug_println!(self, "    NOT");
                let a = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                self.stack.push(not_op(&a, &self.heap));
            }
            Instruction::Equal { keep_rhs } => {
                debug_println!(self, "    EQ keep_rhs={}", keep_rhs);
                self.compare_op(|c| c == Some(std::cmp::Ordering::Equal), *keep_rhs);
            }
            Instruction::NotEqual { keep_rhs } => {
                debug_println!(self, "    NE keep_rhs={}", keep_rhs);
                self.compare_op(|c| c != Some(std::cmp::Ordering::Equal), *keep_rhs);
            }
            Instruction::Less { keep_rhs } => {
                debug_println!(self, "    LT keep_rhs={}", keep_rhs);
                self.compare_op(|c| c == Some(std::cmp::Ordering::Less), *keep_rhs);
            }
            Instruction::LessEqual { keep_rhs } => {
                debug_println!(self, "    LE keep_rhs={}", keep_rhs);
                self.compare_op(
                    |c| {
                        matches!(
                            c,
                            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                        )
                    },
                    *keep_rhs,
                );
            }
            Instruction::Greater { keep_rhs } => {
                debug_println!(self, "    GT keep_rhs={}", keep_rhs);
                self.compare_op(|c| c == Some(std::cmp::Ordering::Greater), *keep_rhs);
            }
            Instruction::GreaterEqual { keep_rhs } => {
                debug_println!(self, "    GE keep_rhs={}", keep_rhs);
                self.compare_op(
                    |c| {
                        matches!(
                            c,
                            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                        )
                    },
                    *keep_rhs,
                );
            }
            Instruction::GetGlobal { name_index } => {
                let name = &self.program.as_ref().unwrap().constants[*name_index];
                if let HeapData::String(s) = &name.value {
                    debug_println!(self, "    GET_GLOBAL {}", s);
                    if let Some(val) = self.global_symbols.get(s) {
                        self.stack.push(val.clone());
                    } else {
                        return Err(RuntimeError::undefined(s));
                    }
                }
            }
            Instruction::SetGlobal { name_index } => {
                let name = &self.program.as_ref().unwrap().constants[*name_index];
                let name_str = name.value.as_string().unwrap_or("__unknown__").to_string();
                let val = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                debug_println!(self, "    SET_GLOBAL {} = {:?}", name_str, val);
                self.global_symbols.insert(name_str, val);
            }
            Instruction::GetLocal { index } => {
                let fp = self.frames.last().unwrap().frame_pointer;
                debug_println!(self, "    GET_LOCAL {} fp={}", index, fp);
                if fp + index < self.stack.len() {
                    self.stack.push(self.stack[fp + index].clone());
                }
            }
            Instruction::SetLocal { index } => {
                let fp = self.frames.last().unwrap().frame_pointer;
                let val = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                debug_println!(self, "    SET_LOCAL {} fp={} val={:?}", index, fp, val);
                let _old = std::mem::replace(&mut self.stack[fp + index], val);

                if let Some(cell) = self.frames.last_mut().unwrap().captured_locals.get(index) {
                    cell.borrow_mut().value = self.stack[fp + index].clone();
                }
            }
            Instruction::ForLoop {
                control_index,
                limit_index,
                body_offset,
                inclusive,
                step_index,
            } => {
                let fp = self.frames.last().unwrap().frame_pointer;
                let control = &self.stack[fp + control_index];
                let limit = &self.stack[fp + limit_index];

                let Some(Primitive::Integer(current)) = control.as_primitive() else {
                    return Err(RuntimeError::type_error(
                        "for loop requires integer control",
                    ));
                };
                let Some(Primitive::Integer(limit_val)) = limit.as_primitive() else {
                    return Err(RuntimeError::type_error("for loop requires integer limit"));
                };

                let step = if let Some(si) = step_index {
                    match self.stack[fp + si].as_primitive() {
                        Some(Primitive::Integer(i)) => i,
                        _ => 1,
                    }
                } else {
                    1
                };

                let next = current + step;
                self.stack[fp + control_index] = StackValue::Primitive(Primitive::Integer(next));

                let keep_going = if *inclusive {
                    next <= limit_val
                } else {
                    next < limit_val
                };
                debug_println!(
                    self,
                    "    FOR_LOOP ctrl={} limit={} step={} next={} keep_going={}",
                    current,
                    limit_val,
                    step,
                    next,
                    keep_going
                );
                if keep_going {
                    self.frames.last_mut().unwrap().ip = *body_offset;
                }
            }
            Instruction::Jump { offset } => {
                debug_println!(self, "    JUMP -> {}", offset);
                self.frames.last_mut().unwrap().ip = *offset;
            }
            Instruction::JumpIf {
                offset,
                expected,
                keep_stay,
                keep_jump,
            } => {
                let cond = self
                    .stack
                    .last()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let truthy = cond.is_truthy(&self.heap);
                let should_jump = truthy == *expected;
                debug_println!(
                    self,
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
                    self.frames.last_mut().unwrap().ip = *offset;
                }
            }
            Instruction::Call {
                arg_count,
                keep_return_value,
            } => {
                let base = self.stack.len() - 1 - arg_count;
                let func_sv = self.stack[base].clone();
                let args: Vec<StackValue> = if *arg_count > 0 {
                    self.stack[(base + 1)..].to_vec()
                } else {
                    Vec::new()
                };
                debug_println!(
                    self,
                    "    CALL argc={} keep_ret={} func={:?}",
                    arg_count,
                    keep_return_value,
                    func_sv
                );
                self.stack.truncate(base);
                let frame_count = self.frames.len();
                let result = self.call_function(func_sv, args)?;
                if *keep_return_value {
                    let pushed_frame = self.frames.len() > frame_count;
                    if !pushed_frame {
                        self.stack.push(result);
                    }
                }
            }
            Instruction::MakeArray { count } => {
                debug_println!(self, "    MAKE_ARRAY {}", count);
                let start = self.stack.len() - count;
                let elements: Vec<StackValue> = self.stack.drain(start..).collect();
                let sv = self.heap.alloc_vector(elements);
                self.stack.push(sv);
            }
            Instruction::GetIndex => {
                debug_println!(self, "    GET_INDEX");
                let idx = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let container = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let result = index(&container, &idx, &mut self.heap)?;
                self.stack.push(result);
            }
            Instruction::SetIndex => {
                debug_println!(self, "    SET_INDEX");
                let val = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let idx = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let container = self
                    .stack
                    .last_mut()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                *index_mut(container, &idx, &mut self.heap)? = val;
            }
            Instruction::Closure {
                function_index,
                upvalues,
            } => {
                debug_println!(
                    self,
                    "    CLOSURE func={} upvalues={}",
                    function_index,
                    upvalues.len()
                );
                let func_data = self.program.as_ref().unwrap().constants[*function_index].clone();
                if let HeapData::Function(Function::Bytecode(mut bc)) = func_data.value {
                    for uv_desc in upvalues {
                        if uv_desc.is_local {
                            let fp = self.frames.last().unwrap().frame_pointer;
                            let idx = uv_desc.index;
                            let cell = std::rc::Rc::new(std::cell::RefCell::new(UpvalueCell::new(
                                self.stack[fp + idx].clone(),
                            )));
                            self.frames
                                .last_mut()
                                .unwrap()
                                .captured_locals
                                .insert(idx, cell.clone());
                            bc.captured_upvalues.push(cell);
                        } else {
                            let uv = self.frames.last().unwrap().upvalues[uv_desc.index].clone();
                            bc.captured_upvalues.push(uv);
                        }
                    }
                    let sv = self.heap.alloc_function(Function::Bytecode(bc));
                    debug_println!(self, "      -> allocated function {:?}", sv);
                    self.stack.push(sv);
                }
            }
            Instruction::GetUpvalue { index } => {
                debug_println!(self, "    GET_UPVALUE {}", index);
                if let Ok(cell) = self.frames.last().unwrap().upvalues[*index].try_borrow() {
                    self.stack.push(cell.value.clone());
                }
            }
            Instruction::SetUpvalue { index } => {
                let val = self
                    .stack
                    .pop()
                    .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                debug_println!(self, "    SET_UPVALUE {} val={:?}", index, val);
                if let Ok(mut cell) = self.frames.last().unwrap().upvalues[*index].try_borrow_mut()
                {
                    cell.value = val;
                }
            }
        }
        Ok(())
    }

    fn call_function(
        &mut self,
        func: StackValue,
        args: Vec<StackValue>,
    ) -> Result<StackValue, RuntimeError> {
        // Extract function from stack value
        let func_data = match &func {
            StackValue::Heap(key) => {
                if let Some(cell) = self.heap.cells.get(*key) {
                    cell.value.clone()
                } else {
                    return Err(RuntimeError::type_error("invalid function reference"));
                }
            }
            _ => return Err(RuntimeError::type_error("cannot call non-function")),
        };

        match func_data {
            HeapData::Function(f) => match f {
                Function::Builtin(b) => b
                    .invoke(args, &mut self.heap)
                    .map_err(|e| RuntimeError::type_error(format!("builtin error: {e}"))),
                Function::Bytecode(bc) => {
                    let total_args = bc.curried_args.len() + args.len();
                    if total_args > bc.arity {
                        return Err(RuntimeError::type_error("too many arguments"));
                    }
                    if total_args < bc.arity {
                        let mut new_bc = bc;
                        new_bc.curried_args.extend(args);
                        return Ok(self.heap.alloc_function(Function::Bytecode(new_bc)));
                    }

                    // Set up call frame
                    let frame_pointer = self.stack.len();
                    let mut all_args = bc.curried_args.clone();
                    all_args.extend(args);

                    // Push args onto stack as locals
                    for a in all_args {
                        self.stack.push(a);
                    }

                    let new_frame = CallFrame {
                        chunk_id: bc.id,
                        ip: 0,
                        frame_pointer,
                        closure_info: Some((bc.clone(), func)),
                        upvalues: bc.captured_upvalues.clone(),
                        captured_locals: HashMap::new(),
                    };
                    self.frames.push(new_frame);

                    Ok(StackValue::Nil) // placeholder, real return value comes from the stack
                }
            },
            _ => Err(RuntimeError::type_error("cannot call non-function")),
        }
    }

    fn binary_op<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: Fn(&StackValue, &StackValue, &mut Heap) -> Result<StackValue, RuntimeError>,
    {
        let b = self
            .stack
            .pop()
            .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let a = self
            .stack
            .last_mut()
            .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let result = f(a, &b, &mut self.heap)?;
        *a = result;
        Ok(())
    }

    fn binary_op_simple<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: Fn(&StackValue, &StackValue, &Heap) -> Result<StackValue, RuntimeError>,
    {
        let b = self
            .stack
            .pop()
            .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let a = self
            .stack
            .pop()
            .ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let result = f(&a, &b, &self.heap)?;
        self.stack.push(result);
        Ok(())
    }

    fn compare_op<F>(&mut self, pred: F, keep_rhs: bool)
    where
        F: Fn(Option<std::cmp::Ordering>) -> bool,
    {
        let b = self.stack.pop().unwrap_or(StackValue::Nil);
        if keep_rhs {
            let a = self.stack.pop().unwrap_or(StackValue::Nil);
            let result = StackValue::Primitive(Primitive::Bool(pred(compare(&a, &b, &self.heap))));
            self.stack.push(b);
            self.stack.push(result);
        } else {
            let a = self.stack.pop().unwrap_or(StackValue::Nil);
            let result = StackValue::Primitive(Primitive::Bool(pred(compare(&a, &b, &self.heap))));
            self.stack.push(result);
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
