use std::{cell::RefCell, collections::HashMap, rc::Rc, slice::Iter};

use foldhash::fast::FixedState;
use l3_bytecode::{ChunkId, CodeOffset, LocalIndex, StackIndex};
use l3_runtime::{StackValue, UpvalueCell};

pub struct VmStack {
    values: Vec<StackValue>,
}

impl VmStack {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn push(&mut self, val: StackValue) {
        self.values.push(val);
    }

    pub(crate) fn pop(&mut self) -> StackValue {
        self.values
            .pop()
            .expect("compiler invariant: stack non-empty")
    }

    pub(crate) fn top(&self) -> StackValue {
        *self
            .values
            .last()
            .expect("compiler invariant: stack non-empty")
    }

    pub(crate) fn top_mut(&mut self) -> &mut StackValue {
        self.values
            .last_mut()
            .expect("compiler invariant: stack non-empty")
    }

    #[inline]
    pub(crate) fn get_local(&self, fp: StackIndex, index: LocalIndex) -> StackValue {
        *slice_get(&self.values, fp.as_index() + index.as_index())
    }

    #[inline]
    pub(crate) fn set_local(&mut self, fp: StackIndex, index: LocalIndex, val: StackValue) {
        let index = fp.as_index() + index.as_index();
        *slice_get_mut(&mut self.values, index) = val;
    }

    pub(crate) fn truncate(&mut self, len: StackIndex) {
        self.values.truncate(len.as_index());
    }

    pub(crate) const fn len(&self) -> StackIndex {
        StackIndex(self.values.len() as u32)
    }

    pub(crate) fn get(&self, index: StackIndex) -> Option<&StackValue> {
        self.values.get(index.as_index())
    }

    pub(crate) fn drain_from(&mut self, from: StackIndex) -> Vec<StackValue> {
        self.values.drain(from.as_index()..).collect()
    }

    pub(crate) fn get_range(&self, from: StackIndex, count: StackIndex) -> Option<&[StackValue]> {
        let from = from.as_index();
        self.values.get(from..from + count.as_index())
    }

    pub(crate) fn get_mut_from(&mut self, from: StackIndex) -> Option<&mut [StackValue]> {
        self.values.get_mut(from.as_index()..)
    }

    pub(crate) fn extend_from_slice(&mut self, other: &[StackValue]) {
        self.values.extend_from_slice(other);
    }

    pub(crate) fn iter(&self) -> Iter<'_, StackValue> {
        self.values.iter()
    }
}

impl<'a> IntoIterator for &'a VmStack {
    type Item = &'a StackValue;
    type IntoIter = Iter<'a, StackValue>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct CallFrame {
    pub(crate) chunk_id: ChunkId,
    pub(crate) ip: CodeOffset,
    pub(crate) frame_pointer: StackIndex,
    pub(crate) closure_info: Option<(Rc<str>, StackValue)>,
    pub(crate) call_site: Option<(ChunkId, CodeOffset)>,
    pub(crate) upvalues: Rc<Box<[Rc<RefCell<UpvalueCell>>]>>,
    pub(crate) captured_locals: HashMap<usize, Rc<RefCell<UpvalueCell>>, FixedState>,
    pub(crate) discard_return: bool,
}

pub struct CallStack {
    frames: Vec<CallFrame>,
}

impl CallStack {
    pub(crate) const fn new() -> Self {
        Self { frames: Vec::new() }
    }

    pub(crate) fn last_mut(&mut self) -> Option<&mut CallFrame> {
        self.frames.last_mut()
    }

    pub(crate) fn push(&mut self, frame: CallFrame) {
        self.frames.push(frame);
    }

    pub(crate) fn pop(&mut self) -> CallFrame {
        self.frames.pop().expect("active frame exists")
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub(crate) const fn len(&self) -> usize {
        self.frames.len()
    }

    pub(crate) fn iter(&self) -> Iter<'_, CallFrame> {
        self.frames.iter()
    }

    pub(crate) fn last(&self) -> Option<&CallFrame> {
        self.frames.last()
    }
}

impl<'a> IntoIterator for &'a CallStack {
    type Item = &'a CallFrame;
    type IntoIter = Iter<'a, CallFrame>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[inline]
pub fn slice_get<T>(slice: &[T], index: usize) -> &T {
    if cfg!(debug_assertions) {
        slice.get(index).expect("Index from the compiler is valid")
    } else {
        // SAFETY: All indices come from the compiler that is considered infallible by
        // the VM
        unsafe { slice.get_unchecked(index) }
    }
}

#[inline]
pub fn slice_get_mut<T>(slice: &mut [T], index: usize) -> &mut T {
    if cfg!(debug_assertions) {
        slice
            .get_mut(index)
            .expect("Index from the compiler is valid")
    } else {
        // SAFETY: All indices come from the compiler that is considered infallible by
        // the VM
        unsafe { slice.get_unchecked_mut(index) }
    }
}
