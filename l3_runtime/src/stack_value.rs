use std::fmt;

use slotmap::DefaultKey;

use crate::{heap::Heap, primitive::Primitive};

#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub fn is_nil(&self) -> bool {
        matches!(self, StackValue::Nil)
    }

    #[must_use]
    pub fn is_primitive(&self) -> bool {
        matches!(self, StackValue::Primitive(_))
    }

    #[must_use]
    pub fn as_primitive(&self) -> Option<Primitive> {
        if let StackValue::Primitive(p) = self {
            Some(*p)
        } else {
            None
        }
    }

    #[must_use]
    pub fn holds_heap_cell(&self) -> bool {
        matches!(self, StackValue::Heap(_))
    }

    #[must_use]
    pub fn get_heap_key(&self) -> Option<DefaultKey> {
        if let StackValue::Heap(k) = self {
            Some(*k)
        } else {
            None
        }
    }

    #[must_use]
    pub fn is_truthy(&self, heap: &Heap) -> bool {
        match self {
            StackValue::Nil => false,
            StackValue::Primitive(p) => p.is_truthy(),
            StackValue::Heap(key) => {
                if let Some(cell) = heap.cells.get(*key) {
                    cell.value.is_truthy(heap)
                } else {
                    false
                }
            },
        }
    }

    #[must_use]
    pub fn type_name(&self, heap: &Heap) -> &'static str {
        match self {
            StackValue::Nil => "nil",
            StackValue::Primitive(p) => p.type_name(),
            StackValue::Heap(key) => {
                if let Some(cell) = heap.cells.get(*key) {
                    cell.value.type_name(heap)
                } else {
                    "invalid"
                }
            },
        }
    }

    #[must_use]
    pub fn as_heap_ref<'a>(&self, heap: &'a Heap) -> Option<&'a crate::heap_data::HeapData> {
        if let StackValue::Heap(key) = self {
            heap.cells.get(*key).map(|c| &c.value)
        } else {
            None
        }
    }

    pub fn as_heap_mut<'a>(
        &mut self,
        heap: &'a mut Heap,
    ) -> Option<&'a mut crate::heap_data::HeapData> {
        if let StackValue::Heap(key) = self {
            heap.cells.get_mut(*key).map(|c| &mut c.value)
        } else {
            None
        }
    }
}

impl From<Primitive> for StackValue {
    fn from(p: Primitive) -> Self {
        StackValue::Primitive(p)
    }
}

impl fmt::Display for StackValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StackValue::Nil => write!(f, "nil"),
            StackValue::Primitive(p) => write!(f, "{p}"),
            StackValue::Heap(_) => write!(f, "<heap>"),
        }
    }
}
