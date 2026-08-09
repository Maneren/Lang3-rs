use crate::{
    Upvalues,
    indices::{CodeOffset, ConstantIndex, LocalIndex, UpvalueIndex},
};

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
        upvalues: Upvalues,
    },

    GetUpvalue {
        index: UpvalueIndex,
    },
    SetUpvalue {
        index: UpvalueIndex,
    },
}
