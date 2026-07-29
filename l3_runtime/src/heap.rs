use crate::function::Function;
use crate::heap_data::HeapData;
use crate::stack_value::StackValue;
use slotmap::{DefaultKey, SlotMap};
use std::cell::Cell;
use std::collections::VecDeque;
use std::io::Write;

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
    pub fn new(value: HeapData) -> Self {
        Self {
            value,
            marked: Cell::new(false),
        }
    }

    /// Recursively mark this cell and all cells it references.
    pub fn mark(&self, heap: &SlotMap<DefaultKey, HeapCell>) {
        if self.marked.get() {
            return;
        }
        self.marked.set(true);

        match &self.value {
            HeapData::Vector(vec) => {
                for sv in vec {
                    mark_stack_value(sv, heap);
                }
            }
            HeapData::Function(Function::Bytecode(bc)) => {
                for arg in &bc.curried_args {
                    mark_stack_value(arg, heap);
                }
                for uv in &bc.captured_upvalues {
                    if let Ok(uv) = uv.try_borrow() {
                        if let Some(key) = uv.value.get_heap_key() {
                            if let Some(cell) = heap.get(key) {
                                cell.mark(heap);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn mark_stack_value(sv: &StackValue, heap: &SlotMap<DefaultKey, HeapCell>) {
    if let StackValue::Heap(key) = sv {
        if let Some(cell) = heap.get(*key) {
            cell.mark(heap);
        }
    }
}

/// An upvalue cell captured by a closure. Stored as Rc<RefCell<..>> for
/// shared mutable access between the GC heap and active closures.
#[derive(Debug, Clone)]
pub struct UpvalueCell {
    pub value: StackValue,
    pub marked: Cell<bool>,
}

impl UpvalueCell {
    #[must_use]
    pub fn new(value: StackValue) -> Self {
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
#[derive(Debug, Clone)]
pub struct Heap {
    pub cells: SlotMap<DefaultKey, HeapCell>,
    pub size: usize,
    pub added_since_last_sweep: usize,
    pub next_gc_threshold: usize,
    pub sweep_count: usize,
    pub output_lines: Vec<String>,
    pub current_line: String,
    pub stream_output: bool,
    pub input_queue: VecDeque<String>,
    pub rng_state: u64,
}

impl Heap {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cells: SlotMap::with_capacity(1024),
            size: 0,
            added_since_last_sweep: 0,
            next_gc_threshold: 1024,
            sweep_count: 0,
            output_lines: Vec::new(),
            current_line: String::new(),
            stream_output: false,
            input_queue: VecDeque::new(),
            rng_state: 42,
        }
    }

    pub fn alloc(&mut self, value: HeapData) -> StackValue {
        match &value {
            HeapData::Nil => StackValue::Nil,
            HeapData::Primitive(p) => StackValue::Primitive(*p),
            _ => {
                self.size += 1;
                self.added_since_last_sweep += 1;
                let key = self.cells.insert(HeapCell::new(value));
                StackValue::Heap(key)
            }
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
        self.cells.retain(|_, cell| {
            let marked = cell.marked.get();
            cell.marked.set(false); // unmark for next cycle
            marked
        });
        let erased = before - self.cells.len();
        self.size = self.cells.len();
        self.added_since_last_sweep = 0;
        self.next_gc_threshold = (self.size * 2).max(1024);
        erased
    }

    pub fn flush_print(&mut self) {
        if !self.current_line.is_empty() {
            let line = std::mem::take(&mut self.current_line);
            if self.stream_output {
                println!("{line}");
            }
            self.output_lines.push(line);
        } else if self.stream_output {
            std::io::stdout().flush().ok();
        }
    }

    pub fn maybe_gc(&mut self) {
        if self.added_since_last_sweep >= self.next_gc_threshold {
            self.sweep();
        }
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}
