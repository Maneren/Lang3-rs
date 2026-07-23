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

    Constant { index: usize },

    Pop { count: usize },

    Duplicate { index: usize },

    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    Negate,

    Equal { keep_rhs: bool },
    NotEqual { keep_rhs: bool },
    Greater { keep_rhs: bool },
    GreaterEqual { keep_rhs: bool },
    Less { keep_rhs: bool },
    LessEqual { keep_rhs: bool },
    Not,

    GetGlobal { name_index: usize },
    SetGlobal { name_index: usize },

    GetLocal { index: usize },
    SetLocal { index: usize },

    ForLoop {
        control_index: usize,
        limit_index: usize,
        body_offset: usize,
        inclusive: bool,
        step_index: Option<usize>,
    },

    Jump { offset: usize },

    JumpIf {
        offset: usize,
        expected: bool,
        keep_stay: bool,
        keep_jump: bool,
    },

    Call {
        arg_count: usize,
        keep_return_value: bool,
    },

    MakeArray { count: usize },

    GetIndex,
    SetIndex,

    Closure {
        function_index: usize,
        upvalues: Vec<UpvalueDesc>,
    },

    GetUpvalue { index: usize },
    SetUpvalue { index: usize },
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
    pub fn new() -> Self {
        Self { chunks: Vec::new(), constants: Vec::new() }
    }
}

impl Default for ProgramBytecode {
    fn default() -> Self {
        Self::new()
    }
}
