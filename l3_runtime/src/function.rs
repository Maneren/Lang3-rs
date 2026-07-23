use crate::heap::UpvalueCell;
use crate::stack_value::StackValue;
use l3_ast::Identifier;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

pub type L3Args = Vec<StackValue>;

pub type BuiltinBody =
    Rc<dyn Fn(L3Args, &mut crate::heap::Heap) -> Result<StackValue, crate::error::RuntimeError>>;

#[derive(Clone)]
pub struct BuiltinFunction {
    pub name: Identifier,
    pub body: BuiltinBody,
}

impl BuiltinFunction {
    pub fn new(name: Identifier, body: BuiltinBody) -> Self {
        Self { name, body }
    }

    pub fn invoke(
        &self,
        args: L3Args,
        heap: &mut crate::heap::Heap,
    ) -> Result<StackValue, crate::error::RuntimeError> {
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
    pub id: usize,
    pub name: String,
    pub arity: usize,
    pub curried_args: Vec<StackValue>,
    pub captured_upvalues: Vec<Rc<RefCell<UpvalueCell>>>,
}

#[derive(Debug, Clone)]
pub enum Function {
    Builtin(BuiltinFunction),
    Bytecode(BytecodeFunction),
}

impl Function {
    #[must_use]
    pub fn as_builtin(&self) -> Option<&BuiltinFunction> {
        if let Function::Builtin(b) = self {
            Some(b)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_bytecode(&self) -> Option<&BytecodeFunction> {
        if let Function::Bytecode(b) = self {
            Some(b)
        } else {
            None
        }
    }

    pub fn as_mut_bytecode(&mut self) -> Option<&mut BytecodeFunction> {
        if let Function::Bytecode(b) = self {
            Some(b)
        } else {
            None
        }
    }
}
