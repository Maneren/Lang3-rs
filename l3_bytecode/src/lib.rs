pub mod format;
pub mod optimizer;

use std::{
    fmt::{self, Display, Formatter},
    ops::{Index, IndexMut},
    slice::{Iter, IterMut},
    vec::IntoIter,
};

use l3_location::Location;
use l3_runtime::{BytecodeFunction, Function, HeapCell, HeapData};

// ---------------------------------------------------------------------------
// Strongly typed domain handles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ConstantIndex(pub u32);

impl ConstantIndex {
    pub fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for ConstantIndex {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl Display for ConstantIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LocalIndex(pub u32);

impl LocalIndex {
    pub fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for LocalIndex {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl Display for LocalIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UpvalueIndex(pub u32);

impl UpvalueIndex {
    pub fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for UpvalueIndex {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl Display for UpvalueIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ChunkId(pub u32);

impl ChunkId {
    pub fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for ChunkId {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl Display for ChunkId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CodeOffset(pub u32);

impl CodeOffset {
    pub fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for CodeOffset {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl Display for CodeOffset {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Upvalue descriptor (used in OpClosure)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpvalueDesc {
    pub is_local: bool,
    pub index: u32,
}

// ---------------------------------------------------------------------------
// All opcodes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Instruction {
    Return,

    Constant {
        index: ConstantIndex,
    },

    Pop {
        count: u32,
    },

    Duplicate {
        index: u32,
    },

    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    Negate,

    Equal {
        keep_rhs: bool,
    },
    NotEqual {
        keep_rhs: bool,
    },
    Greater {
        keep_rhs: bool,
    },
    GreaterEqual {
        keep_rhs: bool,
    },
    Less {
        keep_rhs: bool,
    },
    LessEqual {
        keep_rhs: bool,
    },
    Not,

    GetGlobal {
        name_index: ConstantIndex,
    },
    SetGlobal {
        name_index: ConstantIndex,
    },

    GetLocal {
        index: LocalIndex,
    },
    SetLocal {
        index: LocalIndex,
    },

    ForLoop {
        control_index: LocalIndex,
        limit_index: LocalIndex,
        body_offset: CodeOffset,
        inclusive: bool,
        step_index: Option<LocalIndex>,
    },

    Jump {
        offset: CodeOffset,
    },

    JumpIf {
        offset: CodeOffset,
        expected: bool,
        keep_stay: bool,
        keep_jump: bool,
    },

    Call {
        arg_count: u32,
        keep_return_value: bool,
    },

    MakeArray {
        count: u32,
    },

    /// In-place append of the top `count` stack values onto the heap vector
    /// referenced by the value below them. Only emitted when the compiler can
    /// prove exclusive ownership of the vector.
    VectorAppend {
        count: u32,
    },

    GetIndex,
    SetIndex,

    Closure {
        function_index: ConstantIndex,
        upvalues: Vec<UpvalueDesc>,
    },

    GetUpvalue {
        index: UpvalueIndex,
    },
    SetUpvalue {
        index: UpvalueIndex,
    },
}

// ---------------------------------------------------------------------------
// Chunk -- a sequence of instructions with source locations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub locations: Vec<Location>,
}

impl Chunk {
    pub fn write(&mut self, instruction: Instruction, location: Location) {
        self.code.push(instruction);
        self.locations.push(location);
    }
}

impl Index<CodeOffset> for Chunk {
    type Output = Instruction;

    fn index(&self, offset: CodeOffset) -> &Self::Output {
        &self.code[offset.as_index()]
    }
}

impl IndexMut<CodeOffset> for Chunk {
    fn index_mut(&mut self, offset: CodeOffset) -> &mut Self::Output {
        &mut self.code[offset.as_index()]
    }
}

// ---------------------------------------------------------------------------
// ConstantPool -- type-aware constant table wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ConstantPool {
    pub constants: Vec<HeapCell>,
}

impl ConstantPool {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            constants: Vec::new(),
        }
    }

    #[must_use]
    pub fn get(&self, index: ConstantIndex) -> Option<&HeapCell> {
        self.constants.get(index.as_index())
    }

    /// Invariant: Compiler emits a valid string constant index for global
    /// names.
    #[must_use]
    pub fn string(&self, index: ConstantIndex) -> &str {
        self.constants[index.as_index()]
            .value
            .as_string()
            .expect("compiler invariant: constant is string")
    }

    /// Invariant: Compiler emits a valid bytecode function constant index for
    /// closures.
    #[must_use]
    pub fn bytecode_function(&self, index: ConstantIndex) -> &BytecodeFunction {
        match &self.constants[index.as_index()].value {
            HeapData::Function(Function::Bytecode(bc)) => bc,
            _ => unreachable!("compiler invariant: constant is BytecodeFunction"),
        }
    }

    pub fn push(&mut self, cell: HeapCell) -> ConstantIndex {
        let idx = ConstantIndex(u32::try_from(self.constants.len()).expect("constants fit in u32"));
        self.constants.push(cell);
        idx
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.constants.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.constants.is_empty()
    }

    pub fn iter(&self) -> Iter<'_, HeapCell> {
        self.constants.iter()
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, HeapCell> {
        self.constants.iter_mut()
    }
}

impl Index<ConstantIndex> for ConstantPool {
    type Output = HeapCell;

    fn index(&self, index: ConstantIndex) -> &Self::Output {
        &self.constants[index.as_index()]
    }
}

impl IntoIterator for ConstantPool {
    type Item = HeapCell;
    type IntoIter = IntoIter<HeapCell>;

    fn into_iter(self) -> Self::IntoIter {
        self.constants.into_iter()
    }
}

impl<'a> IntoIterator for &'a ConstantPool {
    type Item = &'a HeapCell;
    type IntoIter = Iter<'a, HeapCell>;

    fn into_iter(self) -> Self::IntoIter {
        self.constants.iter()
    }
}

impl<'a> IntoIterator for &'a mut ConstantPool {
    type Item = &'a mut l3_runtime::HeapCell;
    type IntoIter = IterMut<'a, l3_runtime::HeapCell>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

// ---------------------------------------------------------------------------
// ProgramBytecode -- the compiled output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProgramBytecode {
    pub chunks: Vec<Chunk>,
    pub constants: ConstantPool,
}

impl ProgramBytecode {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chunks: Vec::new(),
            constants: ConstantPool::new(),
        }
    }
}

impl Default for ProgramBytecode {
    fn default() -> Self {
        Self::new()
    }
}

impl Index<ChunkId> for ProgramBytecode {
    type Output = Chunk;

    fn index(&self, id: ChunkId) -> &Self::Output {
        &self.chunks[id.as_index()]
    }
}

impl IndexMut<ChunkId> for ProgramBytecode {
    fn index_mut(&mut self, id: ChunkId) -> &mut Self::Output {
        &mut self.chunks[id.as_index()]
    }
}
