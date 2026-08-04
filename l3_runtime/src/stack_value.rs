use std::fmt;

use slotmap::DefaultKey;

use crate::{heap::Heap, heap_data::HeapData, primitive::Primitive};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slice {
    pub start: Option<i64>,
    pub end: Option<i64>,
}

#[derive(Debug, Clone, Default, Copy)]
pub enum StackValue {
    #[default]
    Nil,
    Primitive(Primitive),
    Heap(DefaultKey),
}

impl StackValue {
    #[must_use]
    pub const fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    #[must_use]
    pub const fn is_primitive(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }

    #[must_use]
    pub const fn as_primitive(&self) -> Option<Primitive> {
        if let Self::Primitive(p) = self {
            Some(*p)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn holds_heap_cell(&self) -> bool {
        matches!(self, Self::Heap(_))
    }

    #[must_use]
    pub const fn get_heap_key(&self) -> Option<DefaultKey> {
        if let Self::Heap(k) = self {
            Some(*k)
        } else {
            None
        }
    }

    #[must_use]
    pub fn is_truthy(&self, heap: &Heap) -> bool {
        match self {
            Self::Nil => false,
            Self::Primitive(p) => p.is_truthy(),
            Self::Heap(key) => heap
                .cells
                .get(*key)
                .is_some_and(|cell| cell.value.is_truthy(heap)),
        }
    }

    #[must_use]
    pub fn type_name(&self, heap: &Heap) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Primitive(p) => p.type_name(),
            Self::Heap(key) => heap
                .cells
                .get(*key)
                .map_or("invalid", |cell| cell.value.type_name(heap)),
        }
    }

    #[must_use]
    pub fn as_heap_ref<'a>(&self, heap: &'a Heap) -> Option<&'a HeapData> {
        if let Self::Heap(key) = self {
            heap.cells.get(*key).map(|c| &c.value)
        } else {
            None
        }
    }

    pub fn as_heap_mut<'a>(&mut self, heap: &'a mut Heap) -> Option<&'a mut HeapData> {
        if let Self::Heap(key) = self {
            heap.cells.get_mut(*key).map(|c| &mut c.value)
        } else {
            None
        }
    }
}

impl From<Primitive> for StackValue {
    fn from(p: Primitive) -> Self {
        Self::Primitive(p)
    }
}

impl fmt::Display for StackValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "nil"),
            Self::Primitive(p) => write!(f, "{p}"),
            Self::Heap(_) => write!(f, "<heap>"),
        }
    }
}
