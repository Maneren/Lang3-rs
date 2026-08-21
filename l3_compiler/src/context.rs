use l3_ast::AssignmentOperator;
use l3_bytecode::{
    Chunk, ChunkId, CodeOffset, Instruction, LocalIndex, UpvalueDesc, UpvalueIndex, Upvalues, idx,
    indexed_vec,
};
use l3_location::Location;

use crate::{CompileError, Compiler};

indexed_vec! {
    /// The locals of a single compilation context, indexed by `LocalIndex`.
    pub(crate) Locals,
    LocalIndex,
    Local
}

#[derive(Debug, Clone)]
pub struct Local {
    pub(crate) name: String,
    pub(crate) depth: i32,
    /// Whether the binding may be reassigned. `let`/`fn`/loop vars without
    /// `mut` are immutable; assignment to an immutable binding is a compile
    /// error.
    pub(crate) mutable: bool,
    /// Set once the local's heap value may be shared with other references
    /// (assignment copy, closure capture, function argument, container
    /// element). Disables the exclusive-ownership `VectorAppend` optimization.
    pub(crate) possibly_aliased: bool,
}

pub struct Context {
    pub(crate) locals: Locals,
    pub(crate) upvalues: Upvalues,
    pub(crate) chunk_id: ChunkId,
    pub(crate) scope_depth: i32,
}

pub struct LoopContext {
    pub(crate) break_jumps: Vec<CodeOffset>,
    pub(crate) continue_jumps: Vec<CodeOffset>,
    pub(crate) body_locals_snapshot: LocalIndex,
    /// The chunk the loop belongs to. `break`/`continue` are only valid in the
    /// same chunk: a nested function body compiles into its own chunk and must
    /// not inherit an enclosing loop's control flow.
    pub(crate) chunk_id: ChunkId,
}

pub enum VarType {
    Local(LocalIndex),
    Upvalue(UpvalueIndex),
    Global(String),
}

impl Compiler {
    // -----------------------------------------------------------------------
    // Instruction emission with source locations
    // -----------------------------------------------------------------------

    pub(crate) fn current_location(&self) -> Location {
        self.location_stack.last().cloned().unwrap_or_default()
    }

    /// Run `f` with `loc` as the innermost source location. Mirrors the C++
    /// `LocationScope` RAII guard: every `emit` inside inherits this location.
    pub(crate) fn with_location<T>(&mut self, loc: &Location, f: impl FnOnce(&mut Self) -> T) -> T {
        self.location_stack.push(loc.clone());
        let result = f(self);
        self.location_stack.pop();
        result
    }

    pub(crate) fn push_context(&mut self) -> ChunkId {
        let chunk_id = self.program.chunks.push(Chunk::default());
        self.contexts.push(Context {
            locals: Locals::new(),
            upvalues: Upvalues::new(),
            chunk_id,
            scope_depth: 0,
        });
        chunk_id
    }

    #[inline]
    pub(crate) fn pop_context(&mut self) {
        self.contexts
            .pop()
            .expect("the context stack shouldn't be empty");
    }

    #[inline]
    pub(crate) fn current_context(&self) -> &Context {
        self.contexts
            .last()
            .expect("a context is always active while compiling")
    }

    #[inline]
    pub(crate) fn current_context_mut(&mut self) -> &mut Context {
        self.contexts
            .last_mut()
            .expect("a context is always active while compiling")
    }

    #[inline]
    pub(crate) fn current_chunk(&mut self) -> &mut Chunk {
        let id = self.current_context().chunk_id;
        self.program
            .chunks
            .get_mut(id)
            .expect("context references a chunk pushed during compilation")
    }

    pub(crate) fn emit(&mut self, inst: Instruction) {
        let loc = self.current_location();
        self.current_chunk().write(inst, loc);
    }

    pub(crate) fn begin_scope(&mut self) {
        self.current_context_mut().scope_depth += 1;
    }

    pub(crate) fn end_scope(&mut self) {
        let ctx = self.current_context_mut();
        ctx.scope_depth -= 1;
        let mut pop_count = 0;
        while let Some(local) = ctx.locals.last() {
            if local.depth > ctx.scope_depth {
                ctx.locals.pop();
                pop_count += 1;
            } else {
                break;
            }
        }
        if pop_count > 0 {
            self.emit(Instruction::Pop {
                count: idx(pop_count),
            });
        }
    }

    pub(crate) fn add_local(&mut self, name: &str) -> LocalIndex {
        self.add_local_with_mutability(name, false)
    }

    pub(crate) fn add_mutable_local(&mut self, name: &str) -> LocalIndex {
        self.add_local_with_mutability(name, true)
    }

    pub(crate) fn add_local_with_mutability(&mut self, name: &str, mutable: bool) -> LocalIndex {
        let ctx = self.current_context_mut();
        ctx.locals.push(Local {
            name: name.to_string(),
            depth: ctx.scope_depth,
            mutable,
            possibly_aliased: false,
        })
    }

    pub(crate) fn resolve_local(&self, name: &str) -> Option<LocalIndex> {
        let ctx = self.contexts.last()?;
        ctx.locals
            .iter()
            .enumerate()
            .rev()
            .find(|(_, local)| local.name == name)
            .map(|(i, _)| idx(i))
    }

    pub(crate) fn resolve_upvalue(&mut self, name: &str) -> Option<UpvalueIndex> {
        let outer_index = self.contexts.len().checked_sub(2)?;
        {
            let outer = self.contexts.get(outer_index)?;
            let cur = self.contexts.last()?;
            // Check if this context already captures the given name from the outer context
            for (j, existing) in cur.upvalues.iter().enumerate() {
                if existing.is_local
                    && let Some(l) = outer.locals.get(LocalIndex(existing.index))
                    && l.name == name
                {
                    return Some(idx(j));
                }
            }
        }
        let outer = self.contexts.get(outer_index)?;
        if let Some(i) = outer.locals.iter().position(|local| local.name == name) {
            // A closure captures this local: its slot now has another holder.
            let outer = self.contexts.get_mut(outer_index)?;
            if let Some(local) = outer.locals.get_mut(idx(i)) {
                local.possibly_aliased = true;
            }
            return Some(self.add_upvalue(true, i));
        }
        // Check outer's upvalues
        for _uv in &self.contexts.get(outer_index)?.upvalues {
            // We need to find if the outer captures this name
            // For MVP, we only handle one level of upvalue nesting
        }
        None
    }

    pub(crate) fn add_upvalue(&mut self, is_local: bool, index: usize) -> UpvalueIndex {
        let ctx = self.current_context_mut();
        ctx.upvalues.push(UpvalueDesc {
            is_local,
            index: idx(index),
        })
    }

    pub(crate) fn resolve_variable(&mut self, name: &str) -> VarType {
        if let Some(idx) = self.resolve_local(name) {
            return VarType::Local(idx);
        }
        if let Some(idx) = self.resolve_upvalue(name) {
            return VarType::Upvalue(idx);
        }
        VarType::Global(name.to_string())
    }

    /// Whether the binding `name` resolves to is mutable. `None` for globals or
    /// unresolved names, which are always assignable.
    pub(crate) fn binding_mutability(&self, name: &str) -> Option<bool> {
        for ctx in self.contexts.iter().rev() {
            if let Some(local) = ctx.locals.iter().rev().find(|local| local.name == name) {
                return Some(local.mutable);
            }
        }
        None
    }

    /// Reject assignment to an immutable binding (a local or upvalue).
    pub(crate) fn ensure_mutable_binding(&self, name: &str) -> Result<(), CompileError> {
        if self.binding_mutability(name) == Some(false) {
            return Err(CompileError::new(format!(
                "cannot assign to immutable binding `{name}`"
            )));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Alias tracking: marks locals whose heap value may be shared, disabling
    // the in-place `VectorAppend` optimization (which is only sound on
    // exclusively-owned vectors).
    // -----------------------------------------------------------------------

    /// Record that `name`'s value may have gained another holder. The local may
    /// live in this context or in the immediately enclosing one (upvalue).
    pub(crate) fn mark_referenced_aliased(&mut self, name: &str) {
        if let Some(idx) = self.resolve_local(name) {
            if let Some(local) = self.current_context_mut().locals.get_mut(idx) {
                local.possibly_aliased = true;
            }
            return;
        }
        if self.contexts.len() >= 2 {
            let outer = self.contexts.len() - 2;
            if let Some(outer_ctx) = self.contexts.get_mut(outer)
                && let Some(local) = outer_ctx.locals.iter_mut().find(|local| local.name == name)
            {
                local.possibly_aliased = true;
            }
        }
    }

    /// Set the aliasing state of a local in the current context. Used by
    /// plain assignments: a fresh literal result stays exclusive.
    pub(crate) fn set_local_aliased(&mut self, name: &str, aliased: bool) {
        if let Some(idx) = self.resolve_local(name)
            && let Some(local) = self.current_context_mut().locals.get_mut(idx)
        {
            local.possibly_aliased = aliased;
        }
    }

    // -----------------------------------------------------------------------
    // Variable load / store helpers
    // -----------------------------------------------------------------------

    /// Emit `GetLocal`, `GetUpvalue`, or `GetGlobal` for the named variable.
    pub(crate) fn emit_variable_get(&mut self, name: &str) {
        match self.resolve_variable(name) {
            VarType::Local(idx) => self.emit(Instruction::GetLocal { index: idx }),
            VarType::Upvalue(uv) => self.emit(Instruction::GetUpvalue { index: uv }),
            VarType::Global(ref n) => {
                let name_idx = self.make_string_constant(n);
                self.emit(Instruction::GetGlobal {
                    name_index: name_idx,
                });
            },
        }
    }

    /// Emit `SetLocal`, `SetUpvalue`, or `SetGlobal` for the named variable.
    pub(crate) fn emit_variable_set(&mut self, name: &str) {
        match self.resolve_variable(name) {
            VarType::Local(idx) => self.emit(Instruction::SetLocal { index: idx }),
            VarType::Upvalue(uv) => self.emit(Instruction::SetUpvalue { index: uv }),
            VarType::Global(ref n) => {
                let name_idx = self.make_string_constant(n);
                self.emit(Instruction::SetGlobal {
                    name_index: name_idx,
                });
            },
        }
    }

    // -----------------------------------------------------------------------
    // Loop context helpers
    // -----------------------------------------------------------------------

    /// Push a fresh `LoopContext` for the current chunk.
    pub(crate) fn push_loop_context(&mut self) {
        self.loop_contexts.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_locals_snapshot: self.current_context().locals.len(),
            chunk_id: self.current_context().chunk_id,
        });
    }

    /// Pop the innermost loop context and patch all its `break` jumps to
    /// `exit_patch` and all `continue` jumps to `continue_target`.
    pub(crate) fn pop_loop_context(&mut self, exit_patch: CodeOffset, continue_target: CodeOffset) {
        let lc = self
            .loop_contexts
            .pop()
            .expect("loop context was pushed when entering the loop");
        let code = &mut self.current_chunk().code;
        for jump in &lc.break_jumps {
            if let Some(Instruction::Jump { offset }) = code.get_mut(*jump) {
                *offset = exit_patch;
            }
        }
        for jump in &lc.continue_jumps {
            if let Some(Instruction::Jump { offset }) = code.get_mut(*jump) {
                *offset = continue_target;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Assignment operator helper
    // -----------------------------------------------------------------------

    /// Emit the bytecode instruction corresponding to an `AssignmentOperator`
    /// applied as a compound assignment (e.g. `+=` → `Add`).
    pub(crate) fn emit_compound_op(&mut self, op: AssignmentOperator) {
        let inst = match op {
            AssignmentOperator::Plus => Instruction::Add,
            AssignmentOperator::Minus => Instruction::Subtract,
            AssignmentOperator::Multiply => Instruction::Multiply,
            AssignmentOperator::Divide => Instruction::Divide,
            AssignmentOperator::Modulo => Instruction::Modulo,
            AssignmentOperator::Power => Instruction::Power,
            AssignmentOperator::Assign => return,
        };
        self.emit(inst);
    }
}
