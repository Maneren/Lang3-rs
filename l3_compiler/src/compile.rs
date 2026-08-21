use std::{ops::ControlFlow, rc::Rc};

use l3_ast::{
    AnonymousFunction, AssignmentOperator, BinaryExpression, BinaryOperator, Block, Comparison,
    ComparisonOperator, Declaration, Expression, ForLoop, FunctionBody, FunctionCall, IfExpression,
    IfStatement, IndexExpression, LastStatement, Literal, LogicalExpression, LogicalOperator,
    Mutability, NameAssignment, NamedFunction, OperatorAssignment, RangeForLoop, RangeOperator,
    Statement, UnaryExpression, UnaryOperator, Variable, While,
};
use l3_bytecode::{ChunkId, CodeOffset, Instruction, Upvalues, idx};
use l3_location::Location;
use l3_runtime::{BytecodeFunction, Function, HeapData, Primitive};

use crate::{CompileError, Compiler, context::VarType};

impl Compiler {
    pub(crate) fn compile_block(&mut self, block: &Block) -> Result<(), CompileError> {
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

    pub(crate) fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        self.with_location(statement_location(stmt), |c| {
            c.compile_statement_inner(stmt)
        })
    }

    pub(crate) fn compile_statement_inner(&mut self, stmt: &Statement) -> Result<(), CompileError> {
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

    pub(crate) fn compile_declaration(&mut self, decl: &Declaration) -> Result<(), CompileError> {
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

    pub(crate) fn compile_fcall(
        &mut self,
        fcall: &FunctionCall,
        pop_result: bool,
    ) -> Result<(), CompileError> {
        self.compile_fcall_expr(fcall, !pop_result)
    }

    pub(crate) fn compile_function_body(
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

    pub(crate) fn compile_named_function(
        &mut self,
        nf: &NamedFunction,
    ) -> Result<(), CompileError> {
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

    pub(crate) fn compile_if_statement(
        &mut self,
        if_stmt: &IfStatement,
    ) -> Result<(), CompileError> {
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

    pub(crate) fn compile_while(&mut self, w: &While) -> Result<(), CompileError> {
        let loop_start = self.current_chunk().code.len();
        self.push_loop_context();

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

        self.pop_loop_context(exit_patch, loop_start);

        Ok(())
    }

    pub(crate) fn compile_for_loop(&mut self, fl: &ForLoop) -> Result<(), CompileError> {
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

        let loop_start = self.current_chunk().code.len();
        self.push_loop_context();

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

        self.pop_loop_context(exit_patch, increment_start);

        Ok(())
    }

    pub(crate) fn compile_range_for_loop(
        &mut self,
        rfl: &RangeForLoop,
    ) -> Result<(), CompileError> {
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

        let for_idx = self.current_chunk().code.len();
        self.push_loop_context();

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

        self.pop_loop_context(exit_patch, for_idx);

        Ok(())
    }

    pub(crate) fn compile_name_assign(&mut self, na: &NameAssignment) -> Result<(), CompileError> {
        if let Some(name) = na.names.first() {
            self.ensure_mutable_binding(&name.name)?;
        }
        self.mark_expression_aliased(&na.expression);
        let fresh = matches!(&*na.expression, Expression::Literal(_));
        self.compile_expression(&na.expression)?;
        if let Some(name) = na.names.first() {
            let name = &name.name;
            if !fresh {
                self.set_local_aliased(name, true);
            }
            self.emit_variable_set(name);
        }
        Ok(())
    }

    pub(crate) fn compile_op_assign(
        &mut self,
        oa: &OperatorAssignment,
    ) -> Result<(), CompileError> {
        match &oa.variable {
            Variable::Identifier(id) => self.compile_indentifier_assignment(oa, &id.name),
            Variable::IndexExpression(ie) => self.compile_index_expression_assignment(oa, ie),
        }
    }

    fn compile_indentifier_assignment(
        &mut self,
        oa: &OperatorAssignment,
        name: &str,
    ) -> Result<(), CompileError> {
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

        if oa.op == AssignmentOperator::Assign {
            self.emit(Instruction::Pop { count: 1 });
            if !fresh {
                self.set_local_aliased(name, true);
            }
            self.emit_variable_set(name);
            return Ok(());
        }

        self.emit_compound_op(oa.op);
        self.emit_variable_set(name);

        Ok(())
    }

    fn compile_index_expression_assignment(
        &mut self,
        oa: &OperatorAssignment,
        ie: &IndexExpression,
    ) -> Result<(), CompileError> {
        self.mark_expression_aliased(&oa.expression);
        self.compile_variable(&ie.base)?;
        self.compile_expression(&ie.index)?;

        if oa.op != AssignmentOperator::Assign {
            self.emit(Instruction::Duplicate { index: 1 });
            self.emit(Instruction::Duplicate { index: 1 });
            self.emit(Instruction::GetIndex);
            self.compile_expression(&oa.expression)?;
            self.emit_compound_op(oa.op);
        } else {
            self.compile_expression(&oa.expression)?;
        }

        self.emit(Instruction::SetIndex);
        self.emit(Instruction::Pop { count: 1 });
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Compile last statements
    // -----------------------------------------------------------------------

    pub(crate) fn compile_last_statement(
        &mut self,
        last: &LastStatement,
    ) -> Result<(), CompileError> {
        self.with_location(last_statement_location(last), |c| {
            c.compile_last_statement_inner(last)
        })
    }

    pub(crate) fn compile_last_statement_inner(
        &mut self,
        last: &LastStatement,
    ) -> Result<(), CompileError> {
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
    pub(crate) fn compile_loop_control(
        &mut self,
        keyword: ControlFlow<()>,
    ) -> Result<(), CompileError> {
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

    pub(crate) fn compile_expression(&mut self, expr: &Expression) -> Result<(), CompileError> {
        self.with_location(expression_location(expr), |c| {
            c.compile_expression_inner(expr)
        })
    }

    pub(crate) fn compile_expression_inner(
        &mut self,
        expr: &Expression,
    ) -> Result<(), CompileError> {
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

    pub(crate) fn compile_literal(&mut self, lit: &Literal) -> Result<(), CompileError> {
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

    pub(crate) fn compile_variable(&mut self, var: &Variable) -> Result<(), CompileError> {
        match var {
            Variable::Identifier(id) => self.emit_variable_get(&id.name),
            Variable::IndexExpression(ie) => {
                self.compile_variable(&ie.base)?;
                self.compile_expression(&ie.index)?;
                self.emit(Instruction::GetIndex);
            },
        }
        Ok(())
    }

    pub(crate) fn compile_fcall_expr(
        &mut self,
        fc: &FunctionCall,
        keep_return_value: bool,
    ) -> Result<(), CompileError> {
        self.emit_variable_get(&fc.name.name);

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

    pub(crate) fn compile_binary(&mut self, be: &BinaryExpression) -> Result<(), CompileError> {
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

    pub(crate) fn compile_unary(&mut self, ue: &UnaryExpression) -> Result<(), CompileError> {
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

    pub(crate) fn compile_logical(&mut self, le: &LogicalExpression) -> Result<(), CompileError> {
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

    pub(crate) fn compile_comparison(&mut self, cmp: &Comparison) -> Result<(), CompileError> {
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

    pub(crate) fn compile_if_expr(&mut self, ife: &IfExpression) -> Result<(), CompileError> {
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

    pub(crate) fn compile_anon_fn(&mut self, af: &AnonymousFunction) -> Result<(), CompileError> {
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
