pub mod format;
pub mod optimizer;

use l3_location::Location;
use l3_runtime::HeapCell;

// ---------------------------------------------------------------------------
// Upvalue descriptor (used in OpClosure)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpvalueDesc {
    pub is_local: bool,
    pub index: usize,
}

// ---------------------------------------------------------------------------
// All opcodes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Instruction {
    Return,

    Constant {
        index: u32,
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
        name_index: u32,
    },
    SetGlobal {
        name_index: u32,
    },

    GetLocal {
        index: u32,
    },
    SetLocal {
        index: u32,
    },

    ForLoop {
        control_index: u32,
        limit_index: u32,
        body_offset: u32,
        inclusive: bool,
        step_index: Option<u32>,
    },

    Jump {
        offset: u32,
    },

    JumpIf {
        offset: u32,
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
        function_index: u32,
        upvalues: Box<[UpvalueDesc]>,
    },

    GetUpvalue {
        index: u32,
    },
    SetUpvalue {
        index: u32,
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

// ---------------------------------------------------------------------------
// ProgramBytecode -- the compiled output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProgramBytecode {
    pub chunks: Vec<Chunk>,
    pub constants: Vec<HeapCell>,
}

impl ProgramBytecode {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chunks: Vec::new(),
            constants: Vec::new(),
        }
    }
}

impl Default for ProgramBytecode {
    fn default() -> Self {
        Self::new()
    }
}
