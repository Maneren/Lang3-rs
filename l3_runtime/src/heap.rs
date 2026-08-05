use std::{
    cell::Cell,
    io::{self, BufRead, BufReader, LineWriter, Read, Write},
};

use slotmap::{DefaultKey, SlotMap};

use crate::{function::Function, heap_data::HeapData, stack_value::StackValue};

/// A GC-managed cell on the heap. The `marked` flag uses `Cell<bool>` so
/// the mark phase can traverse the object graph without exclusive `&mut` access
/// to every cell.
#[derive(Debug, Clone)]
pub struct HeapCell {
    pub value: HeapData,
    pub marked: Cell<bool>,
}

impl HeapCell {
    #[must_use]
    pub const fn new(value: HeapData) -> Self {
        Self {
            value,
            marked: Cell::new(false),
        }
    }

    /// Recursively mark this cell and all cells it references.
    pub fn mark(&self, heap: &SlotMap<DefaultKey, Self>) {
        if self.marked.get() {
            return;
        }
        self.marked.set(true);

        match &self.value {
            HeapData::Vector(vec) => {
                for sv in vec {
                    mark_stack_value(sv, heap);
                }
            },
            HeapData::Function(Function::Bytecode(bc)) => {
                for arg in &bc.curried_args {
                    mark_stack_value(arg, heap);
                }
                for uv in &bc.captured_upvalues {
                    if let Ok(uv) = uv.try_borrow()
                        && let Some(key) = uv.value.get_heap_key()
                        && let Some(cell) = heap.get(key)
                    {
                        cell.mark(heap);
                    }
                }
            },
            _ => {},
        }
    }
}

fn mark_stack_value(sv: &StackValue, heap: &SlotMap<DefaultKey, HeapCell>) {
    if let StackValue::Heap(key) = sv
        && let Some(cell) = heap.get(*key)
    {
        cell.mark(heap);
    }
}

/// An upvalue cell captured by a closure. Stored as `Rc<RefCell<..>>` for
/// shared mutable access between the GC heap and active closures.
#[derive(Debug, Clone)]
pub struct UpvalueCell {
    pub value: StackValue,
    pub marked: Cell<bool>,
}

impl UpvalueCell {
    #[must_use]
    pub const fn new(value: StackValue) -> Self {
        Self {
            value,
            marked: Cell::new(false),
        }
    }

    pub fn mark(&self, heap: &SlotMap<DefaultKey, HeapCell>) {
        if self.marked.get() {
            return;
        }
        self.marked.set(true);
        mark_stack_value(&self.value, heap);
    }
}

/// Garbage-collected heap using slotmap-based storage.
/// Mark-and-sweep with configurable threshold.
pub struct Heap<'a> {
    pub cells: SlotMap<DefaultKey, HeapCell>,
    pub added_since_last_sweep: usize,
    pub next_gc_threshold: usize,
    pub sweep_count: usize,
    pub input: Box<dyn BufRead + 'a>,
    pub output: Box<dyn Write + 'a>,
    pub rng_state: u64,
}

impl<'a> Heap<'a> {
    #[must_use]
    pub fn new(writer: &'a mut impl Write, reader: &'a mut impl Read) -> Self {
        Self {
            cells: SlotMap::with_capacity(1024),
            added_since_last_sweep: 0,
            next_gc_threshold: 1024,
            sweep_count: 0,
            input: Box::new(BufReader::new(reader)),
            output: Box::new(LineWriter::new(writer)),
            rng_state: 42,
        }
    }

    pub fn alloc(&mut self, value: HeapData) -> StackValue {
        match &value {
            HeapData::Nil => StackValue::Nil,
            HeapData::Primitive(p) => StackValue::Primitive(*p),
            _ => {
                self.added_since_last_sweep += 1;
                let key = self.cells.insert(HeapCell::new(value));
                StackValue::Heap(key)
            },
        }
    }

    pub fn alloc_string(&mut self, s: String) -> StackValue {
        self.alloc(HeapData::String(s))
    }

    pub fn alloc_vector(&mut self, v: Vec<StackValue>) -> StackValue {
        self.alloc(HeapData::Vector(v))
    }

    pub fn alloc_function(&mut self, f: Function) -> StackValue {
        self.alloc(HeapData::Function(f))
    }

    pub fn sweep(&mut self) -> usize {
        self.sweep_count += 1;
        let before = self.cells.len();
        self.cells.retain(|_, cell| cell.marked.replace(false));
        let erased = before - self.cells.len();
        self.added_since_last_sweep = 0;
        self.next_gc_threshold = (self.cells.len() * 2).max(1024);
        erased
    }

    pub fn flush_print(&mut self) -> Result<(), io::Error> {
        self.output.flush()
    }

    pub fn maybe_gc(&mut self) {
        if self.added_since_last_sweep >= self.next_gc_threshold {
            self.sweep();
        }
    }
}
