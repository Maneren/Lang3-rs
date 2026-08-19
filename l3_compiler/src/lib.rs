use std::{ops::ControlFlow, rc::Rc};

use l3_ast::{
    AnonymousFunction, AssignmentOperator, BinaryExpression, BinaryOperator, Block, Comparison,
    ComparisonOperator, Declaration, Expression, ForLoop, FunctionBody, FunctionCall, IfBase,
    IfExpression, IfStatement, LastStatement, Literal, LogicalExpression, LogicalOperator,
    Mutability, NameAssignment, NamedFunction, OperatorAssignment, Program, RangeForLoop,
    RangeOperator, Statement, UnaryExpression, UnaryOperator, Variable, While,
};
use l3_bytecode::{
    Chunk, ChunkId, CodeOffset, ConstantIndex, Instruction, LocalIndex, ProgramBytecode,
    UpvalueDesc, UpvalueIndex, Upvalues, idx, indexed_vec,
};
use l3_location::Location;
use l3_runtime::{BytecodeFunction, CompileError, Function, HeapCell, HeapData, Primitive};

indexed_vec! {
    /// The locals of a single compilation context, indexed by `LocalIndex`.
    Locals,
    LocalIndex,
    Local
}

pub struct Compiler {
    program: ProgramBytecode,
    contexts: Vec<Context>,
    loop_contexts: Vec<LoopContext>,
    synthetic_counter: usize,
    location_stack: Vec<Location>,
}

#[derive(Debug, Clone)]
struct Local {
    name: String,
    depth: i32,
    /// Whether the binding may be reassigned. `let`/`fn`/loop vars without
    /// `mut` are immutable; assignment to an immutable binding is a compile
    /// error.
    mutable: bool,
    /// Set once the local's heap value may be shared with other references
    /// (assignment copy, closure capture, function argument, container
    /// element). Disables the exclusive-ownership `VectorAppend` optimization.
    possibly_aliased: bool,
}

struct Context {
    locals: Locals,
    upvalues: Upvalues,
    chunk_id: ChunkId,
    scope_depth: i32,
}

struct LoopContext {
    break_jumps: Vec<CodeOffset>,
    continue_jumps: Vec<CodeOffset>,
    body_locals_snapshot: LocalIndex,
    /// The chunk the loop belongs to. `break`/`continue` are only valid in the
    /// same chunk: a nested function body compiles into its own chunk and must
    /// not inherit an enclosing loop's control flow.
    chunk_id: ChunkId,
}

enum VarType {
    Local(LocalIndex),
    Upvalue(UpvalueIndex),
    Global(String),
}

impl Compiler {
    #[must_use]
    pub const fn new() -> Self {
        let program = ProgramBytecode::new();
        Self {
            program,
            contexts: Vec::new(),
            loop_contexts: Vec::new(),
            synthetic_counter: 0,
            location_stack: Vec::new(),
        }
    }

    pub fn compile(&mut self, ast: &Program) -> Result<&ProgramBytecode, CompileError> {
        self.push_context();
        self.compile_block(ast)?;
        self.emit(Instruction::Return);
        Ok(&self.program)
    }

    // -----------------------------------------------------------------------
    // Instruction emission with source locations
    // -----------------------------------------------------------------------

    fn current_location(&self) -> Location {
        self.location_stack.last().cloned().unwrap_or_default()
    }

    /// Run `f` with `loc` as the innermost source location. Mirrors the C++
    /// `LocationScope` RAII guard: every `emit` inside inherits this location.
    fn with_location<T>(&mut self, loc: &Location, f: impl FnOnce(&mut Self) -> T) -> T {
        self.location_stack.push(loc.clone());
        let result = f(self);
        self.location_stack.pop();
        result
    }

    fn push_context(&mut self) -> ChunkId {
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
    fn pop_context(&mut self) {
        self.contexts
            .pop()
            .expect("the context stack shouldn't be empty");
    }

    #[inline]
    fn current_context(&self) -> &Context {
        self.contexts
            .last()
            .expect("a context is always active while compiling")
    }

    #[inline]
    fn current_context_mut(&mut self) -> &mut Context {
        self.contexts
            .last_mut()
            .expect("a context is always active while compiling")
    }

    #[inline]
    fn current_chunk(&mut self) -> &mut Chunk {
        let id = self.current_context().chunk_id;
        self.program
            .chunks
            .get_mut(id)
            .expect("context references a chunk pushed during compilation")
    }

    fn emit(&mut self, inst: Instruction) {
        let loc = self.current_location();
        self.current_chunk().write(inst, loc);
    }

    fn begin_scope(&mut self) {
        self.current_context_mut().scope_depth += 1;
    }

    fn end_scope(&mut self) {
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

    fn add_local(&mut self, name: &str) -> LocalIndex {
        self.add_local_with_mutability(name, false)
    }

    fn add_mutable_local(&mut self, name: &str) -> LocalIndex {
        self.add_local_with_mutability(name, true)
    }

    fn add_local_with_mutability(&mut self, name: &str, mutable: bool) -> LocalIndex {
        let ctx = self.current_context_mut();
        ctx.locals.push(Local {
            name: name.to_string(),
            depth: ctx.scope_depth,
            mutable,
            possibly_aliased: false,
        })
    }

    fn resolve_local(&self, name: &str) -> Option<LocalIndex> {
        let ctx = self.contexts.last()?;
        ctx.locals
            .iter()
            .enumerate()
            .rev()
            .find(|(_, local)| local.name == name)
            .map(|(i, _)| idx(i))
    }

    fn resolve_upvalue(&mut self, name: &str) -> Option<UpvalueIndex> {
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

    fn add_upvalue(&mut self, is_local: bool, index: usize) -> UpvalueIndex {
        let ctx = self.current_context_mut();
        ctx.upvalues.push(UpvalueDesc {
            is_local,
            index: idx(index),
        })
    }

    fn resolve_variable(&mut self, name: &str) -> VarType {
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
    fn binding_mutability(&self, name: &str) -> Option<bool> {
        for ctx in self.contexts.iter().rev() {
            if let Some(local) = ctx.locals.iter().rev().find(|local| local.name == name) {
                return Some(local.mutable);
            }
        }
        None
    }

    /// Reject assignment to an immutable binding (a local or upvalue).
    fn ensure_mutable_binding(&self, name: &str) -> Result<(), CompileError> {
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
    fn mark_referenced_aliased(&mut self, name: &str) {
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
    fn set_local_aliased(&mut self, name: &str, aliased: bool) {
        if let Some(idx) = self.resolve_local(name)
            && let Some(local) = self.current_context_mut().locals.get_mut(idx)
        {
            local.possibly_aliased = aliased;
        }
    }

    /// Conservatively mark every variable referenced within `expr` as
    /// possibly aliased. Over-marking is always safe; it only skips the
    /// `VectorAppend` optimization.
    fn mark_expression_aliased(&mut self, expr: &Expression) {
        match expr {
            Expression::Variable(var) => self.mark_variable_aliased(var),
            Expression::FunctionCall(fc) => {
                self.mark_referenced_aliased(&fc.name.name);
                for arg in &fc.arguments {
                    self.mark_expression_aliased(arg);
                }
            },
            Expression::BinaryExpression(be) => {
                self.mark_expression_aliased(&be.lhs);
                self.mark_expression_aliased(&be.rhs);
            },
            Expression::UnaryExpression(ue) => self.mark_expression_aliased(&ue.expression),
            Expression::LogicalExpression(le) => {
                self.mark_expression_aliased(&le.lhs);
                self.mark_expression_aliased(&le.rhs);
            },
            Expression::Comparison(cmp) => {
                self.mark_expression_aliased(&cmp.start);
                for (_, rhs) in &cmp.comparisons {
                    self.mark_expression_aliased(rhs);
                }
            },
            Expression::IfExpression(ife) => {
                self.mark_if_base_aliased(&ife.base_if);
                for elif in &ife.elseif {
                    self.mark_if_base_aliased(elif);
                }
                self.mark_block_aliased(&ife.else_block);
            },
            // A closure's captures are marked when its body is compiled.
            Expression::AnonymousFunction(_) => {},
            Expression::Literal(lit) => {
                if let Literal::Array(arr) = lit {
                    for elem in &arr.elements {
                        self.mark_expression_aliased(elem);
                    }
                }
            },
        }
    }

    fn mark_if_base_aliased(&mut self, if_base: &IfBase) {
        self.mark_expression_aliased(&if_base.condition);
        self.mark_block_aliased(&if_base.block);
    }

    fn mark_variable_aliased(&mut self, var: &Variable) {
        match var {
            Variable::Identifier(id) => self.mark_referenced_aliased(&id.name),
            Variable::IndexExpression(ie) => {
                self.mark_variable_aliased(&ie.base);
                self.mark_expression_aliased(&ie.index);
            },
        }
    }

    fn variable_references_name(&self, var: &Variable, name: &str) -> bool {
        match var {
            Variable::Identifier(id) => id.name == name,
            Variable::IndexExpression(ie) => {
                self.variable_references_name(&ie.base, name)
                    || self.expression_references_name(&ie.index, name)
            },
        }
    }

    fn mark_block_aliased(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.mark_statement_aliased(stmt);
        }
        if let Some(LastStatement::Return(ret)) = block.last_statement.as_ref()
            && let Some(expr) = &ret.expression
        {
            self.mark_expression_aliased(expr);
        }
    }

    fn mark_statement_aliased(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Declaration(decl) => {
                if let Some(expr) = &decl.expression {
                    self.mark_expression_aliased(expr);
                }
            },
            Statement::FunctionCall(fc) => {
                for arg in &fc.arguments {
                    self.mark_expression_aliased(arg);
                }
            },
            Statement::IfStatement(ifs) => {
                self.mark_if_base_aliased(&ifs.base_if);
                for elif in &ifs.elseif {
                    self.mark_if_base_aliased(elif);
                }
                if let Some(else_block) = &ifs.else_block {
                    self.mark_block_aliased(else_block);
                }
            },
            Statement::While(w) => {
                self.mark_expression_aliased(&w.condition);
                self.mark_block_aliased(&w.body);
            },
            Statement::ForLoop(fl) => {
                self.mark_expression_aliased(&fl.collection);
                self.mark_block_aliased(&fl.body);
            },
            Statement::RangeForLoop(rfl) => {
                self.mark_expression_aliased(&rfl.start);
                self.mark_expression_aliased(&rfl.end);
                if let Some(step) = &rfl.step {
                    self.mark_expression_aliased(step);
                }
                self.mark_block_aliased(&rfl.body);
            },
            Statement::NameAssignment(na) => self.mark_expression_aliased(&na.expression),
            Statement::NamedFunction(_) => {},
            Statement::OperatorAssignment(oa) => self.mark_expression_aliased(&oa.expression),
        }
    }

    /// Whether any variable referenced in `expr` (transitively) is `name`.
    /// Conservative: complex expressions that could capture `name` report true.
    fn expression_references_name(&self, expr: &Expression, name: &str) -> bool {
        match expr {
            Expression::Variable(var) => self.variable_references_name(var, name),
            Expression::FunctionCall(fc) => {
                fc.name.name == name
                    || fc
                        .arguments
                        .iter()
                        .any(|arg| self.expression_references_name(arg, name))
            },
            Expression::BinaryExpression(be) => {
                self.expression_references_name(&be.lhs, name)
                    || self.expression_references_name(&be.rhs, name)
            },
            Expression::UnaryExpression(ue) => {
                self.expression_references_name(&ue.expression, name)
            },
            Expression::LogicalExpression(le) => {
                self.expression_references_name(&le.lhs, name)
                    || self.expression_references_name(&le.rhs, name)
            },
            Expression::Comparison(cmp) => {
                self.expression_references_name(&cmp.start, name)
                    || cmp
                        .comparisons
                        .iter()
                        .any(|(_, rhs)| self.expression_references_name(rhs, name))
            },
            Expression::IfExpression(_) | Expression::AnonymousFunction(_) => true,
            Expression::Literal(lit) => match lit {
                Literal::Array(arr) => arr
                    .elements
                    .iter()
                    .any(|elem| self.expression_references_name(elem, name)),
                _ => false,
            },
        }
    }

    fn make_constant(&mut self, value: HeapData) -> ConstantIndex {
        if let Some(i) = self
            .program
            .constants
            .iter()
            .position(|cell| cell.value == value)
        {
            return idx(i);
        }
        self.program.constants.push(HeapCell::new(value))
    }

    fn make_string_constant(&mut self, s: &str) -> ConstantIndex {
        self.make_constant(HeapData::String(s.to_string()))
    }

    // -----------------------------------------------------------------------
    // Compile blocks
    // -----------------------------------------------------------------------

    fn compile_block(&mut self, block: &Block) -> Result<(), CompileError> {
        self.begin_scope();
        for stmt in &block.statements {
            self.compile_statement(stmt)?;
        }
        let is_return = matches!(&block.last_statement, Some(LastStatement::Return(_)));
        if let Some(ref last) = block.last_statement {
            self.compile_last_statement(last)?;
        }
        if !is_return {
            self.end_scope();
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Compile statements
    // -----------------------------------------------------------------------

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        self.with_location(statement_location(stmt), |c| {
            c.compile_statement_inner(stmt)
        })
    }

    fn compile_statement_inner(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        match stmt {
            Statement::Declaration(d) => self.compile_declaration(d)?,
            Statement::FunctionCall(fc) => self.compile_fcall(fc, true)?,
            Statement::NamedFunction(nf) => self.compile_named_function(nf)?,
            Statement::IfStatement(is) => self.compile_if_statement(is)?,
            Statement::While(w) => self.compile_while(w)?,
            Statement::ForLoop(fl) => self.compile_for_loop(fl)?,
            Statement::RangeForLoop(rfl) => self.compile_range_for_loop(rfl)?,
            Statement::NameAssignment(na) => self.compile_name_assign(na)?,
            Statement::OperatorAssignment(oa) => self.compile_op_assign(oa)?,
        }
        Ok(())
    }

    fn compile_declaration(&mut self, decl: &Declaration) -> Result<(), CompileError> {
        if let Some(ref expr) = decl.expression {
            self.mark_expression_aliased(expr);
            self.compile_expression(expr)?;
        } else {
            let nil_idx = self.make_constant(HeapData::Nil);
            self.emit(Instruction::Constant { index: nil_idx });
        }
        if decl.names.len() > 1 {
            // Destructuring: bind name i to element i of the RHS. The source is
            // kept alive in a hidden local so the extractions do not strand it
            // on the stack (it is reclaimed by the enclosing scope's end).
            let source_name = format!("<destructure_{}>", self.synthetic_counter);
            self.synthetic_counter += 1;
            let source_idx = self.add_local(&source_name);
            self.set_local_aliased(&source_name, true);
            for (i, name) in decl.names.iter().enumerate() {
                self.emit(Instruction::GetLocal { index: source_idx });
                let idx = self.make_constant(HeapData::Primitive(Primitive::Integer(idx(i))));
                self.emit(Instruction::Constant { index: idx });
                self.emit(Instruction::GetIndex);
                self.add_local_with_mutability(
                    &name.name,
                    matches!(decl.mutability, Mutability::Mutable),
                );
                // Extracted elements share heap cells with the source container.
                self.set_local_aliased(&name.name, true);
            }
            return Ok(());
        }
        let fresh = decl
            .expression
            .as_deref()
            .is_some_and(|expr| matches!(expr, Expression::Literal(_)));
        let aliased = !fresh;
        for name in &decl.names {
            self.add_local_with_mutability(
                &name.name,
                matches!(decl.mutability, Mutability::Mutable),
            );
            if aliased {
                self.set_local_aliased(&name.name, true);
            }
        }
        Ok(())
    }

    fn compile_fcall(
        &mut self,
        fcall: &FunctionCall,
        pop_result: bool,
    ) -> Result<(), CompileError> {
        self.compile_fcall_expr(fcall, !pop_result)
    }

    fn compile_function_body(
        &mut self,
        body: &FunctionBody,
    ) -> Result<(ChunkId, Upvalues), CompileError> {
        self.with_location(&body.location, |c| {
            let chunk_id = c.push_context();
            for param in &body.parameters {
                c.add_mutable_local(&param.name);
                // A parameter may be aliased by the caller's own reference.
                c.set_local_aliased(&param.name, true);
            }
            let ended_with_return =
                matches!(&body.block.last_statement, Some(LastStatement::Return(_)));
            c.compile_block(&body.block)?;
            if !ended_with_return {
                let nil_idx = c.make_constant(HeapData::Nil);
                c.emit(Instruction::Constant { index: nil_idx });
            }
            c.emit(Instruction::Return);
            let upvalues = c.current_context().upvalues.clone();
            c.pop_context();
            Ok::<_, CompileError>((chunk_id, upvalues))
        })
    }

    fn compile_named_function(&mut self, nf: &NamedFunction) -> Result<(), CompileError> {
        let is_top_level = self.contexts.len() == 1;
        let arity = nf.body.parameters.len();

        let nil_idx = self.make_constant(HeapData::Nil);
        self.emit(Instruction::Constant { index: nil_idx });
        let local_idx = self.add_local(&nf.name.name);

        let (chunk_id, upvalues) = self.compile_function_body(&nf.body)?;

        let func_data = HeapData::Function(Function::Bytecode(Box::new(BytecodeFunction {
            id: chunk_id.0,
            name: Rc::from(nf.name.name.clone()),
            arity: idx(arity),
            curried_args: Vec::new(),
            captured_upvalues: Rc::default(),
        })));
        let func_idx = self.make_constant(func_data);

        if upvalues.is_empty() {
            self.emit(Instruction::Constant { index: func_idx });
        } else {
            self.emit(Instruction::Closure {
                function_index: func_idx,
                upvalues,
            });
        }

        self.emit(Instruction::SetLocal { index: local_idx });
        if is_top_level {
            let name_idx = self.make_string_constant(&nf.name.name);
            self.emit(Instruction::GetLocal { index: local_idx });
            self.emit(Instruction::SetGlobal {
                name_index: name_idx,
            });
        }

        Ok(())
    }

    fn compile_if_statement(&mut self, if_stmt: &IfStatement) -> Result<(), CompileError> {
        let mut end_jumps = Vec::new();

        // If branch
        self.compile_expression(&if_stmt.base_if.condition)?;
        let else_jump = self.current_chunk().code.len();
        self.emit(Instruction::JumpIf {
            offset: CodeOffset(0),
            expected: false,
            keep_stay: true,
            keep_jump: false,
        });
        self.emit(Instruction::Pop { count: 1 });

        self.compile_block(&if_stmt.base_if.block)?;
        let end_jump = self.current_chunk().code.len();
        self.emit(Instruction::Jump {
            offset: CodeOffset(0),
        });
        end_jumps.push(end_jump);

        // Patch the else jump
        let else_patch = self.current_chunk().code.len();
        if let Some(Instruction::JumpIf { offset, .. }) =
            self.current_chunk().code.get_mut(else_jump)
        {
            *offset = else_patch;
        }

        if let Some(ref else_block) = if_stmt.else_block {
            self.compile_block(else_block)?;
        }

        // Patch end jumps
        let end_patch = self.current_chunk().code.len();
        for jump in &end_jumps {
            if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(*jump) {
                *offset = end_patch;
            }
        }

        Ok(())
    }

    fn compile_while(&mut self, w: &While) -> Result<(), CompileError> {
        let loop_start = self.current_chunk().code.len();
        self.loop_contexts.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_locals_snapshot: self.current_context().locals.len(),
            chunk_id: self.current_context().chunk_id,
        });

        self.compile_expression(&w.condition)?;
        let exit_jump = self.current_chunk().code.len();
        self.emit(Instruction::JumpIf {
            offset: CodeOffset(0),
            expected: false,
            keep_stay: true,
            keep_jump: false,
        });
        self.emit(Instruction::Pop { count: 1 });

        self.compile_block(&w.body)?;

        // Jump back to condition
        self.emit(Instruction::Jump { offset: loop_start });

        // Patch exit jump
        let exit_patch = self.current_chunk().code.len();
        if let Some(Instruction::JumpIf { offset, .. }) =
            self.current_chunk().code.get_mut(exit_jump)
        {
            *offset = exit_patch;
        }

        // Patch break/continue
        let lc = self
            .loop_contexts
            .pop()
            .expect("loop context was pushed when entering the loop");
        for jump in &lc.break_jumps {
            if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(*jump) {
                *offset = exit_patch;
            }
        }
        for jump in &lc.continue_jumps {
            if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(*jump) {
                *offset = loop_start;
            }
        }

        Ok(())
    }

    fn compile_for_loop(&mut self, fl: &ForLoop) -> Result<(), CompileError> {
        let nil_idx = self.make_constant(HeapData::Nil);
        self.emit(Instruction::Constant { index: nil_idx });
        let var_idx = self.add_local_with_mutability(
            &fl.variable.name,
            matches!(fl.mutability, Mutability::Mutable),
        );
        self.set_local_aliased(&fl.variable.name, true);

        // The collection's value is stored into the synthetic __collection__
        // slot for the loop's whole scope — a second holder of the key.
        self.mark_expression_aliased(&fl.collection);
        self.compile_expression(&fl.collection)?;
        let coll_idx = self.add_mutable_local("__collection__");
        self.set_local_aliased("__collection__", true);

        let zero_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(0)));
        self.emit(Instruction::Constant { index: zero_idx });
        let idx_idx = self.add_mutable_local("__index__");

        // Call len(collection)
        let len_name_idx = self.make_string_constant("len");
        self.emit(Instruction::GetGlobal {
            name_index: len_name_idx,
        });
        self.emit(Instruction::GetLocal { index: coll_idx });
        self.emit(Instruction::Call {
            arg_count: 1,
            keep_return_value: true,
        });
        let len_idx = self.add_mutable_local("__length__");

        let body_locals_snapshot = self.current_context().locals.len();
        let loop_start = self.current_chunk().code.len();
        self.loop_contexts.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_locals_snapshot,
            chunk_id: self.current_context().chunk_id,
        });

        // Loop condition: index < length
        self.emit(Instruction::GetLocal { index: idx_idx });
        self.emit(Instruction::GetLocal { index: len_idx });
        self.emit(Instruction::Less { keep_rhs: false });
        let exit_jump = self.current_chunk().code.len();
        self.emit(Instruction::JumpIf {
            offset: CodeOffset(0),
            expected: false,
            keep_stay: false,
            keep_jump: false,
        });

        // collection[index] → assign to loop variable
        self.emit(Instruction::GetLocal { index: coll_idx });
        self.emit(Instruction::GetLocal { index: idx_idx });
        self.emit(Instruction::GetIndex);
        self.emit(Instruction::SetLocal { index: var_idx });

        self.compile_block(&fl.body)?;

        // index++
        let increment_start = self.current_chunk().code.len();
        self.emit(Instruction::GetLocal { index: idx_idx });
        let one_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(1)));
        self.emit(Instruction::Constant { index: one_idx });
        self.emit(Instruction::Add);
        self.emit(Instruction::SetLocal { index: idx_idx });

        self.emit(Instruction::Jump { offset: loop_start });

        let exit_patch = self.current_chunk().code.len();
        if let Some(Instruction::JumpIf { offset, .. }) =
            self.current_chunk().code.get_mut(exit_jump)
        {
            *offset = exit_patch;
        }

        let lc = self
            .loop_contexts
            .pop()
            .expect("loop context was pushed when entering the loop");
        for jump in &lc.break_jumps {
            if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(*jump) {
                *offset = exit_patch;
            }
        }
        for jump in &lc.continue_jumps {
            if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(*jump) {
                *offset = increment_start;
            }
        }

        Ok(())
    }

    fn compile_range_for_loop(&mut self, rfl: &RangeForLoop) -> Result<(), CompileError> {
        let nil_idx = self.make_constant(HeapData::Nil);
        self.emit(Instruction::Constant { index: nil_idx });
        let control_idx = self.add_local_with_mutability(
            &rfl.variable.name,
            matches!(rfl.mutability, Mutability::Mutable),
        );
        self.set_local_aliased(&rfl.variable.name, true);

        self.compile_expression(&rfl.start)?;
        if let Some(ref step_expr) = rfl.step {
            self.compile_expression(step_expr)?;
        } else {
            let one_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(1)));
            self.emit(Instruction::Constant { index: one_idx });
        }
        self.emit(Instruction::Subtract);
        self.emit(Instruction::SetLocal { index: control_idx });

        self.compile_expression(&rfl.end)?;
        let limit_idx = self.add_mutable_local("__limit__");
        self.set_local_aliased("__limit__", true);

        if let Some(ref step_expr) = rfl.step {
            self.compile_expression(step_expr)?;
        } else {
            let one_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(1)));
            self.emit(Instruction::Constant { index: one_idx });
        }
        let step_idx = self.add_mutable_local("__step__");
        self.set_local_aliased("__step__", true);

        let body_locals_snapshot = self.current_context().locals.len();
        let for_idx = self.current_chunk().code.len();
        self.loop_contexts.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_locals_snapshot,
            chunk_id: self.current_context().chunk_id,
        });

        self.emit(Instruction::ForLoop {
            control_index: control_idx,
            limit_index: limit_idx,
            body_offset: CodeOffset(0),
            inclusive: matches!(rfl.range_type, RangeOperator::Inclusive),
            step_index: Some(step_idx),
        });

        let exit_jump_idx = self.current_chunk().code.len();
        self.emit(Instruction::Jump {
            offset: CodeOffset(0),
        });

        let body_start = self.current_chunk().code.len();
        self.compile_block(&rfl.body)?;
        self.emit(Instruction::Jump { offset: for_idx });

        if let Some(Instruction::ForLoop { body_offset, .. }) =
            self.current_chunk().code.get_mut(for_idx)
        {
            *body_offset = body_start;
        }

        let exit_patch = self.current_chunk().code.len();
        if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(exit_jump_idx)
        {
            *offset = exit_patch;
        }

        let lc = self
            .loop_contexts
            .pop()
            .expect("loop context was pushed when entering the loop");

        let code = &mut self.current_chunk().code;
        for jump in &lc.break_jumps {
            if let Some(Instruction::Jump { offset }) = code.get_mut(*jump) {
                *offset = exit_patch;
            } else {
                return Err(CompileError::new("Invalid break target"));
            }
        }
        for jump in &lc.continue_jumps {
            if let Some(Instruction::Jump { offset }) = code.get_mut(*jump) {
                *offset = for_idx;
            } else {
                return Err(CompileError::new("Invalid continue target"));
            }
        }

        Ok(())
    }

    fn compile_name_assign(&mut self, na: &NameAssignment) -> Result<(), CompileError> {
        if let Some(name) = na.names.first() {
            self.ensure_mutable_binding(&name.name)?;
        }
        self.mark_expression_aliased(&na.expression);
        let fresh = matches!(&*na.expression, Expression::Literal(_));
        self.compile_expression(&na.expression)?;
        if let Some(name) = na.names.first() {
            let name = &name.name;
            match self.resolve_variable(name) {
                VarType::Local(idx) => {
                    if !fresh {
                        self.set_local_aliased(name, true);
                    }
                    self.emit(Instruction::SetLocal { index: idx });
                },
                VarType::Upvalue(uv) => {
                    self.emit(Instruction::SetUpvalue { index: uv });
                },
                VarType::Global(_) => {
                    let name_idx = self.make_string_constant(name);
                    self.emit(Instruction::SetGlobal {
                        name_index: name_idx,
                    });
                },
            }
        }
        Ok(())
    }

    fn compile_op_assign(&mut self, oa: &OperatorAssignment) -> Result<(), CompileError> {
        match &oa.variable {
            Variable::Identifier(id) => {
                let name = &id.name;
                self.ensure_mutable_binding(name)?;

                // `v += [elems]` on an exclusively-owned local vector: append
                // in place, avoiding the temp array, the full clone and a new
                // heap allocation.
                if oa.op == AssignmentOperator::Plus
                    && let Expression::Literal(Literal::Array(arr)) = &*oa.expression
                    && let VarType::Local(slot) = self.resolve_variable(name)
                    && self
                        .current_context()
                        .locals
                        .get(slot)
                        .is_some_and(|local| !local.possibly_aliased)
                    && !arr
                        .elements
                        .iter()
                        .any(|elem| self.expression_references_name(elem, name))
                {
                    self.compile_variable(&oa.variable)?;
                    for elem in &arr.elements {
                        self.mark_expression_aliased(elem);
                        self.compile_expression(elem)?;
                    }
                    self.emit(Instruction::VectorAppend {
                        count: idx(arr.elements.len()),
                    });
                    self.emit(Instruction::SetLocal { index: slot });
                    return Ok(());
                }

                self.mark_expression_aliased(&oa.expression);
                let fresh = matches!(&*oa.expression, Expression::Literal(_));
                self.compile_variable(&oa.variable)?;
                self.compile_expression(&oa.expression)?;

                match oa.op {
                    AssignmentOperator::Plus => self.emit(Instruction::Add),
                    AssignmentOperator::Minus => {
                        self.emit(Instruction::Subtract);
                    },
                    AssignmentOperator::Multiply => {
                        self.emit(Instruction::Multiply);
                    },
                    AssignmentOperator::Divide => {
                        self.emit(Instruction::Divide);
                    },
                    AssignmentOperator::Modulo => {
                        self.emit(Instruction::Modulo);
                    },
                    AssignmentOperator::Power => self.emit(Instruction::Power),
                    AssignmentOperator::Assign => {
                        self.emit(Instruction::Pop { count: 1 });
                        if !fresh {
                            self.set_local_aliased(name, true);
                        }
                        match self.resolve_variable(name) {
                            VarType::Local(idx) => {
                                self.emit(Instruction::SetLocal { index: idx });
                            },
                            VarType::Upvalue(uv) => {
                                self.emit(Instruction::SetUpvalue { index: uv });
                            },
                            VarType::Global(_) => {
                                let name_idx = self.make_string_constant(name);
                                self.emit(Instruction::SetGlobal {
                                    name_index: name_idx,
                                });
                            },
                        }
                        return Ok(());
                    },
                }

                match self.resolve_variable(name) {
                    VarType::Local(idx) => {
                        self.emit(Instruction::SetLocal { index: idx });
                    },
                    VarType::Upvalue(uv) => {
                        self.emit(Instruction::SetUpvalue { index: uv });
                    },
                    VarType::Global(_) => {
                        let name_idx = self.make_string_constant(name);
                        self.emit(Instruction::SetGlobal {
                            name_index: name_idx,
                        });
                    },
                }
                Ok(())
            },
            Variable::IndexExpression(ie) => {
                self.mark_expression_aliased(&oa.expression);
                self.compile_variable(&ie.base)?;
                self.compile_expression(&ie.index)?;

                let compound = match oa.op {
                    AssignmentOperator::Plus => Some(Instruction::Add),
                    AssignmentOperator::Minus => Some(Instruction::Subtract),
                    AssignmentOperator::Multiply => Some(Instruction::Multiply),
                    AssignmentOperator::Divide => Some(Instruction::Divide),
                    AssignmentOperator::Modulo => Some(Instruction::Modulo),
                    AssignmentOperator::Power => Some(Instruction::Power),
                    AssignmentOperator::Assign => None,
                };
                if let Some(op) = compound {
                    self.emit(Instruction::Duplicate { index: 1 });
                    self.emit(Instruction::Duplicate { index: 1 });
                    self.emit(Instruction::GetIndex);
                    self.compile_expression(&oa.expression)?;
                    self.emit(op);
                } else {
                    self.compile_expression(&oa.expression)?;
                }

                self.emit(Instruction::SetIndex);
                self.emit(Instruction::Pop { count: 1 });
                Ok(())
            },
        }
    }

    // -----------------------------------------------------------------------
    // Compile last statements
    // -----------------------------------------------------------------------

    fn compile_last_statement(&mut self, last: &LastStatement) -> Result<(), CompileError> {
        self.with_location(last_statement_location(last), |c| {
            c.compile_last_statement_inner(last)
        })
    }

    fn compile_last_statement_inner(&mut self, last: &LastStatement) -> Result<(), CompileError> {
        match last {
            LastStatement::Return(ret) => {
                if let Some(ref expr) = ret.expression {
                    self.compile_expression(expr)?;
                } else {
                    let nil_idx = self.make_constant(HeapData::Nil);
                    self.emit(Instruction::Constant { index: nil_idx });
                }
                if self.contexts.len() > 1 {
                    self.emit(Instruction::Return);
                }
            },
            LastStatement::Break(_) => self.compile_loop_control(ControlFlow::Break(()))?,
            LastStatement::Continue(_) => self.compile_loop_control(ControlFlow::Continue(()))?,
        }
        Ok(())
    }

    /// Emit the stack restore + jump for a `break`/`continue`. Only an
    /// enclosing loop in the *same chunk* is a valid target; anything else is
    /// a compile error (a `break` inside a nested function body, or outside any
    /// loop, would otherwise compile into an unpatched `Jump { offset: 0 }`).
    fn compile_loop_control(&mut self, keyword: ControlFlow<()>) -> Result<(), CompileError> {
        let context = self.current_context();
        let chunk_id = context.chunk_id;
        let Some(lc_pos) = self
            .loop_contexts
            .iter()
            .rposition(|lc| lc.chunk_id == chunk_id)
        else {
            let keyword = match keyword {
                ControlFlow::Break(()) => "break",
                ControlFlow::Continue(()) => "continue",
            };

            return Err(CompileError::new(format!(
                "{keyword} outside of a loop is not allowed"
            )));
        };
        {
            let lc = &self.loop_contexts[lc_pos];
            let body_locals = context.locals.len() - lc.body_locals_snapshot;
            if body_locals.0 > 0 {
                self.emit(Instruction::Pop {
                    count: body_locals.0,
                });
            }
        }
        let jump = self.current_chunk().code.len();
        self.emit(Instruction::Jump {
            offset: CodeOffset(0),
        });
        let lc = &mut self.loop_contexts[lc_pos];
        match keyword {
            ControlFlow::Break(()) => lc.break_jumps.push(jump),
            ControlFlow::Continue(()) => lc.continue_jumps.push(jump),
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Compile expressions
    // -----------------------------------------------------------------------

    fn compile_expression(&mut self, expr: &Expression) -> Result<(), CompileError> {
        self.with_location(expression_location(expr), |c| {
            c.compile_expression_inner(expr)
        })
    }

    fn compile_expression_inner(&mut self, expr: &Expression) -> Result<(), CompileError> {
        match expr {
            Expression::Literal(lit) => self.compile_literal(lit),
            Expression::Variable(var) => self.compile_variable(var),
            Expression::FunctionCall(fc) => self.compile_fcall_expr(fc, true),
            Expression::BinaryExpression(be) => self.compile_binary(be),
            Expression::UnaryExpression(ue) => self.compile_unary(ue),
            Expression::LogicalExpression(le) => self.compile_logical(le),
            Expression::Comparison(cmp) => self.compile_comparison(cmp),
            Expression::IfExpression(ife) => self.compile_if_expr(ife),
            Expression::AnonymousFunction(af) => self.compile_anon_fn(af),
        }
    }

    fn compile_literal(&mut self, lit: &Literal) -> Result<(), CompileError> {
        let heap_data = match lit {
            Literal::Nil(_) => HeapData::Nil,
            Literal::Boolean(b) => HeapData::Primitive(Primitive::Bool(b.value)),
            Literal::Number(n) => HeapData::Primitive(Primitive::Integer(n.value)),
            Literal::Float(f) => HeapData::Primitive(Primitive::Double(f.value)),
            Literal::String(s) => HeapData::String(s.value.clone()),
            Literal::Array(arr) => {
                for elem in &arr.elements {
                    self.mark_expression_aliased(elem);
                    self.compile_expression(elem)?;
                }
                self.emit(Instruction::MakeArray {
                    count: idx(arr.elements.len()),
                });
                return Ok(());
            },
        };
        let idx = self.make_constant(heap_data);
        self.emit(Instruction::Constant { index: idx });
        Ok(())
    }

    fn compile_variable(&mut self, var: &Variable) -> Result<(), CompileError> {
        match var {
            Variable::Identifier(id) => match self.resolve_variable(&id.name) {
                VarType::Local(idx) => {
                    self.emit(Instruction::GetLocal { index: idx });
                },
                VarType::Upvalue(uv) => {
                    self.emit(Instruction::GetUpvalue { index: uv });
                },
                VarType::Global(name) => {
                    let name_idx = self.make_string_constant(&name);
                    self.emit(Instruction::GetGlobal {
                        name_index: name_idx,
                    });
                },
            },
            Variable::IndexExpression(ie) => {
                self.compile_variable(&ie.base)?;
                self.compile_expression(&ie.index)?;
                self.emit(Instruction::GetIndex);
            },
        }
        Ok(())
    }

    fn compile_fcall_expr(
        &mut self,
        fc: &FunctionCall,
        keep_return_value: bool,
    ) -> Result<(), CompileError> {
        match self.resolve_variable(&fc.name.name) {
            VarType::Local(idx) => {
                self.emit(Instruction::GetLocal { index: idx });
            },
            VarType::Upvalue(uv) => {
                self.emit(Instruction::GetUpvalue { index: uv });
            },
            VarType::Global(_) => {
                let name_idx = self.make_string_constant(&fc.name.name);
                self.emit(Instruction::GetGlobal {
                    name_index: name_idx,
                });
            },
        }

        for arg in &fc.arguments {
            self.mark_expression_aliased(arg);
            self.compile_expression(arg)?;
        }

        self.emit(Instruction::Call {
            arg_count: idx(fc.arguments.len()),
            keep_return_value,
        });
        Ok(())
    }

    fn compile_binary(&mut self, be: &BinaryExpression) -> Result<(), CompileError> {
        if let Some(folded) = self.try_fold_binary(be) {
            let idx = self.make_constant(folded);
            self.emit(Instruction::Constant { index: idx });
            return Ok(());
        }
        self.compile_expression(&be.lhs)?;
        self.compile_expression(&be.rhs)?;
        let inst = match be.op {
            BinaryOperator::Plus => Instruction::Add,
            BinaryOperator::Minus => Instruction::Subtract,
            BinaryOperator::Multiply => Instruction::Multiply,
            BinaryOperator::Divide => Instruction::Divide,
            BinaryOperator::Modulo => Instruction::Modulo,
            BinaryOperator::Power => Instruction::Power,
        };
        self.emit(inst);
        Ok(())
    }

    fn try_fold_expression(&mut self, expr: &Expression) -> Option<HeapData> {
        match expr {
            Expression::Literal(lit) => match lit {
                Literal::Nil(_) => Some(HeapData::Nil),
                Literal::Boolean(b) => Some(HeapData::Primitive(Primitive::Bool(b.value))),
                Literal::Number(n) => Some(HeapData::Primitive(Primitive::Integer(n.value))),
                Literal::Float(f) => Some(HeapData::Primitive(Primitive::Double(f.value))),
                _ => None,
            },
            Expression::UnaryExpression(ue) => self.try_fold_unary(ue),
            Expression::BinaryExpression(be) => self.try_fold_binary(be),
            _ => None,
        }
    }

    fn try_fold_unary(&mut self, ue: &UnaryExpression) -> Option<HeapData> {
        match ue.op {
            UnaryOperator::Minus => {
                let inner = self.try_fold_expression(&ue.expression)?;
                match inner {
                    HeapData::Primitive(p) => Some(HeapData::Primitive(-p)),
                    _ => None,
                }
            },
            UnaryOperator::Plus => self.try_fold_expression(&ue.expression),
            UnaryOperator::Not => {
                let inner = self.try_fold_expression(&ue.expression)?;
                match inner {
                    HeapData::Primitive(p) => {
                        Some(HeapData::Primitive(Primitive::Bool(!p.is_truthy())))
                    },
                    HeapData::Nil => Some(HeapData::Primitive(Primitive::Bool(true))),
                    _ => None,
                }
            },
        }
    }

    fn try_fold_binary(&mut self, be: &BinaryExpression) -> Option<HeapData> {
        let lhs = self.try_fold_expression(&be.lhs)?;
        let rhs = self.try_fold_expression(&be.rhs)?;
        match (lhs, rhs) {
            (HeapData::Primitive(a), HeapData::Primitive(b)) => {
                let result = match be.op {
                    BinaryOperator::Plus => a + b,
                    BinaryOperator::Minus => a - b,
                    BinaryOperator::Multiply => a * b,
                    BinaryOperator::Divide => a / b,
                    BinaryOperator::Modulo => a % b,
                    BinaryOperator::Power => match (a, b) {
                        (Primitive::Integer(a), Primitive::Integer(b)) => u32::try_from(b)
                            .ok()
                            .ok_or("exponent must be a non-negative 32-bit integer")
                            .map(|exp| Primitive::Integer(a.wrapping_pow(exp))),
                        (Primitive::Double(a), Primitive::Double(b)) => {
                            Ok(Primitive::Double(a.powf(b)))
                        },
                        (Primitive::Integer(a), Primitive::Double(b)) => {
                            Ok(Primitive::Double((a as f64).powf(b)))
                        },
                        (Primitive::Double(a), Primitive::Integer(b)) => {
                            Ok(Primitive::Double(a.powi(b as i32)))
                        },
                        _ => Err("unsupported operand types for ^"),
                    },
                };
                result.ok().map(HeapData::Primitive)
            },
            _ => None,
        }
    }

    fn compile_unary(&mut self, ue: &UnaryExpression) -> Result<(), CompileError> {
        if let Some(folded) = self.try_fold_unary(ue) {
            let idx = self.make_constant(folded);
            self.emit(Instruction::Constant { index: idx });
            return Ok(());
        }
        self.compile_expression(&ue.expression)?;
        let inst = match ue.op {
            UnaryOperator::Minus => Instruction::Negate,
            UnaryOperator::Not => Instruction::Not,
            UnaryOperator::Plus => {
                return Ok(());
            },
        };
        self.emit(inst);
        Ok(())
    }

    fn compile_logical(&mut self, le: &LogicalExpression) -> Result<(), CompileError> {
        self.compile_expression(&le.lhs)?;

        match le.op {
            LogicalOperator::And => {
                // Short-circuit: if lhs is falsy, keep it as result and jump past rhs
                let jump = self.current_chunk().code.len();
                self.emit(Instruction::JumpIf {
                    offset: CodeOffset(0),
                    expected: false,
                    keep_stay: false,
                    keep_jump: true,
                });
                self.compile_expression(&le.rhs)?;
                let patch = self.current_chunk().code.len();
                if let Some(Instruction::JumpIf { offset, .. }) =
                    self.current_chunk().code.get_mut(jump)
                {
                    *offset = patch;
                }
            },
            LogicalOperator::Or => {
                // Short-circuit: if lhs is truthy, keep it as result and jump past rhs
                let jump = self.current_chunk().code.len();
                self.emit(Instruction::JumpIf {
                    offset: CodeOffset(0),
                    expected: true,
                    keep_stay: false,
                    keep_jump: true,
                });
                self.compile_expression(&le.rhs)?;
                let patch = self.current_chunk().code.len();
                if let Some(Instruction::JumpIf { offset, .. }) =
                    self.current_chunk().code.get_mut(jump)
                {
                    *offset = patch;
                }
            },
        }
        Ok(())
    }

    fn compile_comparison(&mut self, cmp: &Comparison) -> Result<(), CompileError> {
        let comparisons = &cmp.comparisons;
        let len = comparisons.len();
        let mut false_jumps = Vec::new();

        self.compile_expression(&cmp.start)?;

        for (i, (op, rhs)) in comparisons.iter().enumerate() {
            let is_last = i == len - 1;

            self.compile_expression(rhs)?;
            self.emit(match_comparison_op(*op, !is_last));

            if !is_last {
                let jump = self.current_chunk().code.len();
                self.emit(Instruction::JumpIf {
                    offset: CodeOffset(0),
                    expected: false,
                    keep_stay: false,
                    keep_jump: true,
                });
                false_jumps.push(jump);
            }
        }

        let end_jump = self.current_chunk().code.len();
        self.emit(Instruction::Jump {
            offset: CodeOffset(0),
        });

        let cleanup = self.current_chunk().code.len();
        for jump in &false_jumps {
            if let Some(Instruction::JumpIf { offset, .. }) =
                self.current_chunk().code.get_mut(*jump)
            {
                *offset = cleanup;
            }
        }
        self.emit(Instruction::Pop { count: 2 });
        let false_idx = self.make_constant(HeapData::Primitive(Primitive::Bool(false)));
        self.emit(Instruction::Constant { index: false_idx });

        let end = self.current_chunk().code.len();
        if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(end_jump) {
            *offset = end;
        }

        Ok(())
    }

    fn compile_if_expr(&mut self, ife: &IfExpression) -> Result<(), CompileError> {
        self.compile_expression(&ife.base_if.condition)?;
        let else_jump = self.current_chunk().code.len();
        self.emit(Instruction::JumpIf {
            offset: CodeOffset(0),
            expected: false,
            keep_stay: true,
            keep_jump: false,
        });
        self.emit(Instruction::Pop { count: 1 });

        self.compile_block(&ife.base_if.block)?;
        let end_jump = self.current_chunk().code.len();
        self.emit(Instruction::Jump {
            offset: CodeOffset(0),
        });

        let else_patch = self.current_chunk().code.len();
        if let Some(Instruction::JumpIf { offset, .. }) =
            self.current_chunk().code.get_mut(else_jump)
        {
            *offset = else_patch;
        }

        self.compile_block(&ife.else_block)?;

        let end_patch = self.current_chunk().code.len();
        if let Some(Instruction::Jump { offset }) = self.current_chunk().code.get_mut(end_jump) {
            *offset = end_patch;
        }

        Ok(())
    }

    fn compile_anon_fn(&mut self, af: &AnonymousFunction) -> Result<(), CompileError> {
        let arity = af.body.parameters.len();
        let name = format!("<anon_{}>", self.synthetic_counter);
        self.synthetic_counter += 1;

        let (chunk_id, upvalues) = self.compile_function_body(&af.body)?;

        let func_data = HeapData::Function(Function::Bytecode(Box::new(BytecodeFunction {
            id: chunk_id.0,
            name: Rc::from(name),
            arity: idx(arity),
            curried_args: Vec::new(),
            captured_upvalues: Rc::default(),
        })));
        let func_idx = self.make_constant(func_data);

        if upvalues.is_empty() {
            self.emit(Instruction::Constant { index: func_idx });
        } else {
            self.emit(Instruction::Closure {
                function_index: func_idx,
                upvalues,
            });
        }

        Ok(())
    }
}

const fn statement_location(stmt: &Statement) -> &Location {
    match stmt {
        Statement::Declaration(d) => &d.location,
        Statement::ForLoop(fl) => &fl.location,
        Statement::FunctionCall(fc) => &fc.location,
        Statement::IfStatement(is) => &is.location,
        Statement::NameAssignment(na) => &na.location,
        Statement::NamedFunction(nf) => &nf.location,
        Statement::OperatorAssignment(oa) => &oa.location,
        Statement::RangeForLoop(rfl) => &rfl.location,
        Statement::While(w) => &w.location,
    }
}

const fn expression_location(expr: &Expression) -> &Location {
    match expr {
        Expression::AnonymousFunction(af) => &af.location,
        Expression::BinaryExpression(be) => &be.location,
        Expression::Comparison(c) => &c.location,
        Expression::FunctionCall(fc) => &fc.location,
        Expression::IfExpression(ife) => &ife.location,
        Expression::Literal(lit) => lit.location(),
        Expression::LogicalExpression(le) => &le.location,
        Expression::UnaryExpression(ue) => &ue.location,
        Expression::Variable(var) => var.location(),
    }
}

const fn last_statement_location(last: &LastStatement) -> &Location {
    match last {
        LastStatement::Return(r) => &r.location,
        LastStatement::Break(b) => &b.location,
        LastStatement::Continue(c) => &c.location,
    }
}

const fn match_comparison_op(op: ComparisonOperator, keep_rhs: bool) -> Instruction {
    match op {
        ComparisonOperator::Equal => Instruction::Equal { keep_rhs },
        ComparisonOperator::NotEqual => Instruction::NotEqual { keep_rhs },
        ComparisonOperator::Less => Instruction::Less { keep_rhs },
        ComparisonOperator::LessEqual => Instruction::LessEqual { keep_rhs },
        ComparisonOperator::Greater => Instruction::Greater { keep_rhs },
        ComparisonOperator::GreaterEqual => Instruction::GreaterEqual { keep_rhs },
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use l3_parser::parse_program;

    use super::Compiler;

    fn compile_error(source: &str) -> String {
        let program = parse_program(source, "<test>").expect("source parses");
        Compiler::new()
            .compile(&program)
            .expect_err("compilation must fail")
            .to_string()
    }

    fn assert_compile_error(source: &str, expected: &str) {
        assert_eq!(
            compile_error(source),
            expected,
            "wrong compile error for: {source}"
        );
    }

    #[test]
    fn break_outside_loop_is_a_compile_error() {
        assert_compile_error(
            "break\n",
            "CompileError: break outside of a loop is not allowed",
        );
    }

    #[test]
    fn continue_outside_loop_is_a_compile_error() {
        assert_compile_error(
            "continue\n",
            "CompileError: continue outside of a loop is not allowed",
        );
    }

    #[test]
    fn break_inside_function_defined_in_loop_is_a_compile_error() {
        assert_compile_error(
            "while true do\n  let f = fn() break end\nend\n",
            "CompileError: break outside of a loop is not allowed",
        );
    }

    #[test]
    fn continue_inside_function_defined_in_loop_is_a_compile_error() {
        assert_compile_error(
            "while true do\n  let f = fn() continue end\nend\n",
            "CompileError: continue outside of a loop is not allowed",
        );
    }

    #[test]
    fn assignment_to_immutable_binding_is_a_compile_error() {
        assert_compile_error(
            "let x = 1\nx = 2\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn op_assignment_to_immutable_binding_is_a_compile_error() {
        assert_compile_error(
            "let x = 1\nx += 2\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn assignment_to_immutable_loop_variable_is_a_compile_error() {
        assert_compile_error(
            "for x in [1, 2] do\n  x = 1\nend\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn assignment_to_immutable_captured_binding_is_a_compile_error() {
        assert_compile_error(
            "let x = 1\nlet f = fn() x = 2 end\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn mutable_bindings_compile() {
        for source in [
            "let mut x = 1\nx = 2\n",
            "let mut x = 1\nx += 2\n",
            "for mut x in [1, 2] do\n  x = 1\nend\n",
            "for mut i in 0..3 do\n  i += 1\nend\n",
        ] {
            let program = parse_program(source, "<test>").expect("source parses");
            Compiler::new()
                .compile(&program)
                .unwrap_or_else(|e| panic!("mutable binding rejected: {source}: {e}"));
        }
    }
}
