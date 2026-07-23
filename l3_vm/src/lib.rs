pub mod builtins;

use l3_bytecode::*;
use l3_runtime::*;
use l3_runtime::heap_data::{add, sub, mul, div, modulo, pow, negative, not_op, index, index_mut, compare};
use l3_location::Location;
use std::collections::HashMap;

pub struct BytecodeVM {
    pub heap: Heap,
    pub stack: Vec<StackValue>,
    pub global_symbols: HashMap<String, StackValue>,
    program: Option<ProgramBytecode>,
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

impl BytecodeVM {
    pub fn new(debug: bool) -> Self {
        let mut vm = Self {
            heap: Heap::new(),
            stack: Vec::new(),
            global_symbols: HashMap::new(),
            program: None,
            frames: Vec::new(),
            debug,
        };

        // Register builtins
        for (name, body) in builtins::builtins() {
            let func = vm.heap.alloc_function(Function::Builtin(
                BuiltinFunction::new(
                    l3_ast::Identifier::new(name.to_string(), Location::default()),
                    body,
                )
            ));
            vm.global_symbols.insert(name.to_string(), func);
        }

        vm
    }

    pub fn execute(&mut self, program: &ProgramBytecode) -> Result<(), RuntimeError> {
        let saved = program.clone();
        self.program = Some(saved);
        let chunks = self.program.as_ref().map(|p| p.chunks.clone()).unwrap_or_default();

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
            self.heap.maybe_gc();

            let chunk_id = self.frames.last().unwrap().chunk_id;
            let ip = self.frames.last().unwrap().ip;

            if chunk_id >= chunks.len() || ip >= chunks[chunk_id].code.len() {
                break;
            }

            let instruction = chunks[chunk_id].code[ip].clone();
            self.frames.last_mut().unwrap().ip += 1;

            self.dispatch(&instruction)?;

            if self.frames.is_empty() {
                break;
            }
        }

        self.program = None;
        Ok(())
    }

    fn dispatch(&mut self, inst: &Instruction) -> Result<(), RuntimeError> {
        match inst {
            Instruction::Return => {
                let return_value = self.stack.pop().unwrap_or(StackValue::Nil);
                let fp = self.frames.last().map(|f| f.frame_pointer).unwrap_or(0);
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
                    _ => StackValue::Heap(
                        self.heap.cells.iter().find(|(_, c)| std::ptr::eq(&c.value, &val.value))
                            .map(|(k, _)| k)
                            .unwrap_or_else(|| {
                                let k = self.heap.cells.insert(val.clone());
                                k
                            })
                    ),
                };
                self.stack.push(sv);
            }
            Instruction::Pop { count } => {
                for _ in 0..*count {
                    self.stack.pop();
                }
            }
            Instruction::Duplicate { index } => {
                let idx = self.stack.len() - 1 - index;
                if idx < self.stack.len() {
                    self.stack.push(self.stack[idx].clone());
                }
            }
            Instruction::Add => self.binary_op(|a, b, h| add(a, b, h))?,
            Instruction::Subtract => self.binary_op_simple(|a, b, h| sub(a, b, h))?,
            Instruction::Multiply => self.binary_op(|a, b, h| mul(a, b, h))?,
            Instruction::Divide => self.binary_op_simple(|a, b, h| div(a, b, h))?,
            Instruction::Modulo => self.binary_op_simple(|a, b, h| modulo(a, b, h))?,
            Instruction::Power => self.binary_op_simple(|a, b, h| pow(a, b, h))?,
            Instruction::Negate => {
                let a = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let result = negative(&a, &self.heap)?;
                self.stack.push(result);
            }
            Instruction::Not => {
                let a = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                self.stack.push(not_op(&a, &self.heap));
            }
            Instruction::Equal { keep_rhs } => self.compare_op(|c| c == Some(std::cmp::Ordering::Equal), *keep_rhs),
            Instruction::NotEqual { keep_rhs } => self.compare_op(|c| c != Some(std::cmp::Ordering::Equal), *keep_rhs),
            Instruction::Less { keep_rhs } => self.compare_op(|c| c == Some(std::cmp::Ordering::Less), *keep_rhs),
            Instruction::LessEqual { keep_rhs } => self.compare_op(|c| matches!(c, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)), *keep_rhs),
            Instruction::Greater { keep_rhs } => self.compare_op(|c| c == Some(std::cmp::Ordering::Greater), *keep_rhs),
            Instruction::GreaterEqual { keep_rhs } => self.compare_op(|c| matches!(c, Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)), *keep_rhs),
            Instruction::GetGlobal { name_index } => {
                let name = &self.program.as_ref().unwrap().constants[*name_index];
                if let HeapData::String(s) = &name.value {
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
                let val = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                self.global_symbols.insert(name_str, val);
            }
            Instruction::GetLocal { index } => {
                let fp = self.frames.last().unwrap().frame_pointer;
                if fp + index < self.stack.len() {
                    self.stack.push(self.stack[fp + index].clone());
                }
            }
            Instruction::SetLocal { index } => {
                let fp = self.frames.last().unwrap().frame_pointer;
                let val = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                self.stack[fp + index] = val.clone();

                // Update captured locals
                if let Some(cell) = self.frames.last_mut().unwrap().captured_locals.get(index) {
                    cell.borrow_mut().value = val;
                }
            }
            Instruction::ForLoop { control_index, limit_index, body_offset, inclusive, step_index } => {
                let fp = self.frames.last().unwrap().frame_pointer;
                let control = &self.stack[fp + control_index];
                let limit = &self.stack[fp + limit_index];

                let current = match control.as_primitive() {
                    Some(Primitive::Integer(i)) => i,
                    _ => return Err(RuntimeError::type_error("for loop requires integer control")),
                };
                let limit_val = match limit.as_primitive() {
                    Some(Primitive::Integer(i)) => i,
                    _ => return Err(RuntimeError::type_error("for loop requires integer limit")),
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

                let keep_going = if *inclusive { next <= limit_val } else { next < limit_val };
                if keep_going {
                    self.frames.last_mut().unwrap().ip = *body_offset;
                }
            }
            Instruction::Jump { offset } => {
                self.frames.last_mut().unwrap().ip = *offset;
            }
            Instruction::JumpIf { offset, expected, keep_stay, keep_jump } => {
                let cond = self.stack.last().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let truthy = cond.is_truthy(&self.heap);
                let should_jump = truthy == *expected;
                let should_pop = if should_jump { !keep_jump } else { !keep_stay };
                if should_pop {
                    self.stack.pop();
                }
                if should_jump {
                    self.frames.last_mut().unwrap().ip = *offset;
                }
            }
            Instruction::Call { arg_count, keep_return_value } => {
                let base = self.stack.len() - 1 - arg_count;
                let func_sv = self.stack[base].clone();
                let args: Vec<StackValue> = if *arg_count > 0 {
                    self.stack[(base + 1)..].to_vec()
                } else {
                    Vec::new()
                };
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
                let start = self.stack.len() - count;
                let elements: Vec<StackValue> = self.stack.drain(start..).collect();
                let sv = self.heap.alloc_vector(elements);
                self.stack.push(sv);
            }
            Instruction::GetIndex => {
                let idx = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let container = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let result = index(&container, &idx, &mut self.heap)?;
                self.stack.push(result);
            }
            Instruction::SetIndex => {
                let val = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let idx = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                let container = self.stack.last_mut().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                *index_mut(container, &idx, &mut self.heap)? = val;
            }
            Instruction::Closure { function_index, upvalues } => {
                let func_data = self.program.as_ref().unwrap().constants[*function_index].clone();
                if let HeapData::Function(f) = func_data.value {
                    if let Function::Bytecode(mut bc) = f {
                        for uv_desc in upvalues {
                            if uv_desc.is_local {
                                let fp = self.frames.last().unwrap().frame_pointer;
                                let idx = uv_desc.index;
                                let cell = std::rc::Rc::new(std::cell::RefCell::new(
                                    UpvalueCell::new(self.stack[fp + idx].clone())
                                ));
                                self.frames.last_mut().unwrap().captured_locals.insert(idx, cell.clone());
                                bc.captured_upvalues.push(cell);
                            } else {
                                let uv = self.frames.last().unwrap().upvalues[uv_desc.index].clone();
                                bc.captured_upvalues.push(uv);
                            }
                        }
                        let sv = self.heap.alloc_function(Function::Bytecode(bc));
                        self.stack.push(sv);
                    }
                }
            }
            Instruction::GetUpvalue { index } => {
                if let Ok(cell) = self.frames.last().unwrap().upvalues[*index].try_borrow() {
                    self.stack.push(cell.value.clone());
                }
            }
            Instruction::SetUpvalue { index } => {
                let val = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
                if let Ok(mut cell) = self.frames.last().unwrap().upvalues[*index].try_borrow_mut() {
                    cell.value = val;
                }
            }
        }
        Ok(())
    }

    fn call_function(&mut self, func: StackValue, args: Vec<StackValue>) -> Result<StackValue, RuntimeError> {
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
                Function::Builtin(b) => {
                    b.invoke(args, &mut self.heap)
                        .map_err(|e| RuntimeError::type_error(format!("builtin error: {}", e)))
                }
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
    where F: Fn(&StackValue, &StackValue, &mut Heap) -> Result<StackValue, RuntimeError>
    {
        let b = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let a = self.stack.last_mut().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let result = f(a, &b, &mut self.heap)?;
        *a = result;
        Ok(())
    }

    fn binary_op_simple<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where F: Fn(&StackValue, &StackValue, &Heap) -> Result<StackValue, RuntimeError>
    {
        let b = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let a = self.stack.pop().ok_or_else(|| RuntimeError::generic("stack underflow"))?;
        let result = f(&a, &b, &self.heap)?;
        self.stack.push(result);
        Ok(())
    }

    fn compare_op<F>(&mut self, pred: F, keep_rhs: bool)
    where F: Fn(Option<std::cmp::Ordering>) -> bool
    {
        let b = self.stack.pop().unwrap_or(StackValue::Nil);
        if keep_rhs {
            // Swap: put b back, then result
            let a = self.stack.last().cloned().unwrap_or(StackValue::Nil);
            self.stack.pop();
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
