use std::{cell::RefCell, fmt, rc::Rc};

use l3_ast::Identifier;

use crate::{
    error::RuntimeResult,
    heap::{Heap, UpvalueCell},
    stack_value::StackValue,
};

pub type L3Args = Vec<StackValue>;

pub type BuiltinBody =
    Rc<dyn for<'h, 'r> Fn(&[StackValue], &'r mut Heap<'h>) -> RuntimeResult<StackValue>>;

#[derive(Clone)]
pub struct BuiltinFunction {
    pub name: Identifier,
    pub body: BuiltinBody,
}

impl BuiltinFunction {
    pub fn new(name: Identifier, body: BuiltinBody) -> Self {
        Self { name, body }
    }

    pub fn invoke(&self, args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
        (self.body)(args, heap)
    }
}

impl fmt::Debug for BuiltinFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuiltinFunction")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct BytecodeFunction {
    pub id: u32,
    pub arity: u32,
    pub name: Rc<str>,
    pub curried_args: Vec<StackValue>,
    pub captured_upvalues: Rc<Box<[Rc<RefCell<UpvalueCell>>]>>,
}

#[derive(Debug, Clone)]
pub enum Function {
    Builtin(BuiltinFunction),
    Bytecode(Box<BytecodeFunction>),
}

impl Function {
    #[must_use]
    pub const fn as_builtin(&self) -> Option<&BuiltinFunction> {
        if let Self::Builtin(b) = self {
            Some(b)
        } else {
            None
        }
    }

    pub const fn as_mut_builtin(&mut self) -> Option<&mut BuiltinFunction> {
        if let Self::Builtin(b) = self {
            Some(b)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_bytecode(&self) -> Option<&BytecodeFunction> {
        if let Self::Bytecode(b) = self {
            Some(b)
        } else {
            None
        }
    }

    pub fn as_mut_bytecode(&mut self) -> Option<&mut BytecodeFunction> {
        if let Self::Bytecode(b) = self {
            Some(b)
        } else {
            None
        }
    }
}
