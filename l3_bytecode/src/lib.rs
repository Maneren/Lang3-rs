pub mod format;
pub mod indices;
pub mod instructions;
pub mod optimizer;

use std::{
    fmt::Debug,
    ops::{Index, IndexMut},
    slice::{Iter, IterMut},
    vec::IntoIter,
};

pub use indices::{ChunkId, CodeOffset, ConstantIndex, LocalIndex, StackIndex, UpvalueIndex, idx};
pub use instructions::{Instruction, UpvalueDesc};
use l3_location::Location;
use l3_runtime::{BytecodeFunction, Function, HeapCell, HeapData};

/// A `Vec<T>` whose `len`/`push` return the strongly typed index into the
/// collection, removing the `usize` ↔ typed-index conversions at every use
/// site.
#[macro_export]
macro_rules! indexed_vec {
    ($(#[$meta:meta])* $vis:vis $name:ident, $index:ty, $item:ty) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default)]
        $vis struct $name {
            items: Vec<$item>,
        }

        impl $name {
            #[must_use]
            pub const fn new() -> Self {
                Self { items: Vec::new() }
            }

            /// Append an item and return the index of the newly pushed slot.
            pub fn push(&mut self, item: $item) -> $index {
                let index = $crate::idx(self.items.len());
                self.items.push(item);
                index
            }

            #[must_use]
            pub fn len(&self) -> $index {
                $crate::idx(self.items.len())
            }

            #[must_use]
            pub const fn is_empty(&self) -> bool {
                self.items.is_empty()
            }

            #[must_use]
            pub fn get(&self, index: $index) -> Option<&$item> {
                self.items.get(index.as_index())
            }

            pub fn get_mut(&mut self, index: $index) -> Option<&mut $item> {
                self.items.get_mut(index.as_index())
            }

            #[must_use]
            pub fn last(&self) -> Option<&$item> {
                self.items.last()
            }

            pub fn pop(&mut self) -> Option<$item> {
                self.items.pop()
            }

            #[must_use]
            pub fn as_slice(&self) -> &[$item] {
                &self.items
            }

            pub fn as_mut_slice(&mut self) -> &mut [$item] {
                &mut self.items
            }

            pub fn iter(&self) -> std::slice::Iter<'_, $item> {
                self.items.iter()
            }

            pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, $item> {
                self.items.iter_mut()
            }
        }

        impl From<Vec<$item>> for $name {
            fn from(items: Vec<$item>) -> Self {
                Self { items }
            }
        }

        impl std::ops::Index<$index> for $name {
            type Output = $item;

            #[expect(
                clippy::indexing_slicing,
                reason = "typed index is within bounds by construction"
            )]
            fn index(&self, index: $index) -> &Self::Output {
                &self.items[index.as_index()]
            }
        }

        impl std::ops::IndexMut<$index> for $name {
            #[expect(
                clippy::indexing_slicing,
                reason = "typed index is within bounds by construction"
            )]
            fn index_mut(&mut self, index: $index) -> &mut Self::Output {
                &mut self.items[index.as_index()]
            }
        }

        impl IntoIterator for $name {
            type Item = $item;
            type IntoIter = std::vec::IntoIter<$item>;

            fn into_iter(self) -> Self::IntoIter {
                self.items.into_iter()
            }
        }

        impl<'a> IntoIterator for &'a $name {
            type Item = &'a $item;
            type IntoIter = std::slice::Iter<'a, $item>;

            fn into_iter(self) -> Self::IntoIter {
                self.items.iter()
            }
        }

        impl<'a> IntoIterator for &'a mut $name {
            type Item = &'a mut $item;
            type IntoIter = std::slice::IterMut<'a, $item>;

            fn into_iter(self) -> Self::IntoIter {
                self.items.iter_mut()
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Chunk -- a sequence of instructions with source locations
// ---------------------------------------------------------------------------

indexed_vec! {
    /// A chunk's instruction stream, indexed by `CodeOffset`.
    pub Code,
    CodeOffset,
    Instruction
}

indexed_vec! {
    /// The captured upvalue descriptors of a closure.
    pub Upvalues,
    UpvalueIndex,
    UpvalueDesc
}

#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub code: Code,
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
        &self.code[offset]
    }
}

impl IndexMut<CodeOffset> for Chunk {
    fn index_mut(&mut self, offset: CodeOffset) -> &mut Self::Output {
        &mut self.code[offset]
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
        let index = idx(self.constants.len());
        self.constants.push(cell);
        index
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

indexed_vec! {
    /// The program's chunks, indexed by `ChunkId`.
    pub Chunks,
    ChunkId,
    Chunk
}

#[derive(Debug, Clone)]
pub struct ProgramBytecode {
    pub chunks: Chunks,
    pub constants: ConstantPool,
}

impl ProgramBytecode {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chunks: Chunks::new(),
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
        &self.chunks[id]
    }
}

impl IndexMut<ChunkId> for ProgramBytecode {
    fn index_mut(&mut self, id: ChunkId) -> &mut Self::Output {
        &mut self.chunks[id]
    }
}
