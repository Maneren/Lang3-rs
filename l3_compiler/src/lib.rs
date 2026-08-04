use l3_ast::{
    AnonymousFunction, AssignmentOperator, BinaryExpression, BinaryOperator, Block, Comparison,
    ComparisonOperator, Declaration, Expression, ForLoop, FunctionCall, IfExpression, IfStatement,
    LastStatement, Literal, LogicalExpression, LogicalOperator, NameAssignment, NamedFunction,
    OperatorAssignment, Program, RangeForLoop, RangeOperator, Statement, UnaryExpression,
    UnaryOperator, Variable, While,
};
use l3_bytecode::{Chunk, Instruction, ProgramBytecode, UpvalueDesc};
use l3_location::Location;
use l3_runtime::{BytecodeFunction, CompileError, Function, HeapCell, HeapData, Primitive};

pub struct Compiler {
    program: ProgramBytecode,
    contexts: Vec<Context>,
    loop_contexts: Vec<LoopContext>,
    synthetic_counter: usize,
}

struct Local {
    name: String,
    depth: i32,
}

struct Context {
    locals: Vec<Local>,
    upvalues: Vec<UpvalueDesc>,
    chunk_id: usize,
    scope_depth: i32,
}

struct LoopContext {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
    body_locals_snapshot: usize,
}

enum VarType {
    Local(usize),
    Upvalue(UpvalueDesc),
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
        }
    }

    pub fn compile(&mut self, ast: &Program) -> Result<&ProgramBytecode, CompileError> {
        self.push_context();
        self.compile_block(ast)?;
        self.emit(Instruction::Return, Location::default());
        Self::deduplicate_constants();
        Ok(&self.program)
    }

    fn push_context(&mut self) -> usize {
        let chunk_id = self.program.chunks.len();
        self.program.chunks.push(Chunk::default());
        self.contexts.push(Context {
            locals: Vec::new(),
            upvalues: Vec::new(),
            chunk_id,
            scope_depth: 0,
        });
        chunk_id
    }

    fn pop_context(&mut self) {
        self.contexts.pop();
    }

    fn current_chunk(&mut self) -> &mut Chunk {
        let id = self.contexts.last().unwrap().chunk_id;
        &mut self.program.chunks[id]
    }

    fn emit(&mut self, inst: Instruction, loc: Location) {
        self.current_chunk().write(inst, loc);
    }

    fn begin_scope(&mut self) {
        self.contexts.last_mut().unwrap().scope_depth += 1;
    }

    fn end_scope(&mut self) {
        let ctx = self.contexts.last_mut().unwrap();
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
            self.emit(Instruction::Pop { count: pop_count }, Location::default());
        }
    }

    fn add_local(&mut self, name: &str) {
        let ctx = self.contexts.last_mut().unwrap();
        ctx.locals.push(Local {
            name: name.to_string(),
            depth: ctx.scope_depth,
        });
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        let ctx = self.contexts.last()?;
        ctx.locals
            .iter()
            .enumerate()
            .rev()
            .find(|(_, local)| local.name == name)
            .map(|(i, _)| i)
    }

    fn resolve_upvalue(&mut self, name: &str) -> Option<usize> {
        let outer_context = self.contexts.get(self.contexts.len() - 2)?;
        // Check if this context already captures the given name from the outer context
        let cur = self.contexts.last()?;
        for (j, existing) in cur.upvalues.iter().enumerate() {
            if existing.is_local
                && let Some(l) = outer_context.locals.get(existing.index)
                && l.name == name
            {
                return Some(j);
            }
        }
        for (i, local) in outer_context.locals.iter().enumerate() {
            if local.name == name {
                return Some(self.add_upvalue(true, i));
            }
        }
        // Check outer's upvalues
        for _uv in &outer_context.upvalues {
            // We need to find if the outer captures this name
            // For MVP, we only handle one level of upvalue nesting
        }
        None
    }

    fn add_upvalue(&mut self, is_local: bool, index: usize) -> usize {
        let ctx = self.contexts.last_mut().unwrap();
        let idx = ctx.upvalues.len();
        ctx.upvalues.push(UpvalueDesc { is_local, index });
        idx
    }

    fn resolve_variable(&mut self, name: &str) -> VarType {
        if let Some(idx) = self.resolve_local(name) {
            return VarType::Local(idx);
        }
        if let Some(idx) = self.resolve_upvalue(name) {
            return VarType::Upvalue(UpvalueDesc {
                is_local: false,
                index: idx,
            });
        }
        VarType::Global(name.to_string())
    }

    fn make_constant(&mut self, value: HeapData) -> usize {
        for (i, existing) in self.program.constants.iter().enumerate() {
            if existing.value == value {
                return i;
            }
        }
        let idx = self.program.constants.len();
        self.program.constants.push(HeapCell::new(value));
        idx
    }

    fn make_string_constant(&mut self, s: &str) -> usize {
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
            self.compile_expression(expr)?;
        } else {
            let nil_idx = self.make_constant(HeapData::Nil);
            self.emit(
                Instruction::Constant { index: nil_idx },
                Location::default(),
            );
        }
        for name in &decl.names {
            self.add_local(&name.name);
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

    fn compile_named_function(&mut self, nf: &NamedFunction) -> Result<(), CompileError> {
        let is_top_level = self.contexts.len() == 1;

        let nil_idx = self.make_constant(HeapData::Nil);
        self.emit(
            Instruction::Constant { index: nil_idx },
            Location::default(),
        );
        self.add_local(&nf.name.name);

        let chunk_id = self.push_context();
        let arity = nf.body.parameters.len();

        for param in &nf.body.parameters {
            self.add_local(&param.name);
        }

        self.compile_block(&nf.body.block)?;
        self.emit(Instruction::Return, Location::default());

        let upvalues = self.contexts.last().unwrap().upvalues.clone();
        self.pop_context();

        let func_data = HeapData::Function(Function::Bytecode(Box::new(BytecodeFunction {
            id: chunk_id,
            name: nf.name.name.clone(),
            arity,
            curried_args: Vec::new(),
            captured_upvalues: Vec::new(),
        })));
        let func_idx = self.make_constant(func_data);

        if upvalues.is_empty() {
            self.emit(
                Instruction::Constant { index: func_idx },
                Location::default(),
            );
        } else {
            self.emit(
                Instruction::Closure {
                    function_index: func_idx,
                    upvalues,
                },
                Location::default(),
            );
        }

        let local_idx = self.contexts.last().unwrap().locals.len() - 1;
        self.emit(
            Instruction::SetLocal { index: local_idx },
            Location::default(),
        );
        if is_top_level {
            let name_idx = self.make_string_constant(&nf.name.name);
            self.emit(
                Instruction::GetLocal { index: local_idx },
                Location::default(),
            );
            self.emit(
                Instruction::SetGlobal {
                    name_index: name_idx,
                },
                Location::default(),
            );
        }

        Ok(())
    }

    fn compile_if_statement(&mut self, if_stmt: &IfStatement) -> Result<(), CompileError> {
        let mut end_jumps = Vec::new();

        // If branch
        self.compile_expression(&if_stmt.base_if.condition)?;
        let else_jump = self.current_chunk().code.len();
        self.emit(
            Instruction::JumpIf {
                offset: 0,
                expected: false,
                keep_stay: true,
                keep_jump: false,
            },
            Location::default(),
        );
        self.emit(Instruction::Pop { count: 1 }, Location::default());

        self.compile_block(&if_stmt.base_if.block)?;
        let end_jump = self.current_chunk().code.len();
        self.emit(Instruction::Jump { offset: 0 }, Location::default());
        end_jumps.push(end_jump);

        // Patch the else jump
        let else_patch = self.current_chunk().code.len();
        if let Instruction::JumpIf { ref mut offset, .. } = self.current_chunk().code[else_jump] {
            *offset = else_patch;
        }

        if let Some(ref else_block) = if_stmt.else_block {
            self.compile_block(else_block)?;
        }

        // Patch end jumps
        let end_patch = self.current_chunk().code.len();
        for jump in &end_jumps {
            if let Instruction::Jump { ref mut offset } = self.current_chunk().code[*jump] {
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
            body_locals_snapshot: self.contexts.last().unwrap().locals.len(),
        });

        self.compile_expression(&w.condition)?;
        let exit_jump = self.current_chunk().code.len();
        self.emit(
            Instruction::JumpIf {
                offset: 0,
                expected: false,
                keep_stay: true,
                keep_jump: false,
            },
            Location::default(),
        );
        self.emit(Instruction::Pop { count: 1 }, Location::default());

        self.compile_block(&w.body)?;

        // Jump back to condition
        self.emit(
            Instruction::Jump { offset: loop_start },
            Location::default(),
        );

        // Patch exit jump
        let exit_patch = self.current_chunk().code.len();
        if let Instruction::JumpIf { ref mut offset, .. } = self.current_chunk().code[exit_jump] {
            *offset = exit_patch;
        }

        // Patch break/continue
        let lc = self.loop_contexts.pop().unwrap();
        for jump in &lc.break_jumps {
            if let Instruction::Jump { ref mut offset } = self.current_chunk().code[*jump] {
                *offset = exit_patch;
            }
        }
        for jump in &lc.continue_jumps {
            if let Instruction::Jump { ref mut offset } = self.current_chunk().code[*jump] {
                *offset = loop_start;
            }
        }

        Ok(())
    }

    fn compile_for_loop(&mut self, fl: &ForLoop) -> Result<(), CompileError> {
        let loc = Location::default();
        let nil_idx = self.make_constant(HeapData::Nil);
        self.emit(Instruction::Constant { index: nil_idx }, loc.clone());
        self.add_local(&fl.variable.name);
        let var_idx = self.contexts.last().unwrap().locals.len() - 1;

        self.compile_expression(&fl.collection)?;
        let coll_idx = self.contexts.last().unwrap().locals.len();
        self.add_local("__collection__");

        let zero_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(0)));
        self.emit(Instruction::Constant { index: zero_idx }, loc.clone());
        let idx_idx = self.contexts.last().unwrap().locals.len();
        self.add_local("__index__");

        // Call len(collection)
        let len_name_idx = self.make_string_constant("len");
        self.emit(
            Instruction::GetGlobal {
                name_index: len_name_idx,
            },
            loc.clone(),
        );
        self.emit(Instruction::GetLocal { index: coll_idx }, loc.clone());
        self.emit(
            Instruction::Call {
                arg_count: 1,
                keep_return_value: true,
            },
            loc.clone(),
        );
        let len_idx = self.contexts.last().unwrap().locals.len();
        self.add_local("__length__");

        let body_locals_snapshot = self.contexts.last().unwrap().locals.len();
        let loop_start = self.current_chunk().code.len();
        self.loop_contexts.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_locals_snapshot,
        });

        // Loop condition: index < length
        self.emit(Instruction::GetLocal { index: idx_idx }, loc.clone());
        self.emit(Instruction::GetLocal { index: len_idx }, loc.clone());
        self.emit(Instruction::Less { keep_rhs: false }, loc.clone());
        let exit_jump = self.current_chunk().code.len();
        self.emit(
            Instruction::JumpIf {
                offset: 0,
                expected: false,
                keep_stay: false,
                keep_jump: false,
            },
            loc.clone(),
        );

        // collection[index] → assign to loop variable
        self.emit(Instruction::GetLocal { index: coll_idx }, loc.clone());
        self.emit(Instruction::GetLocal { index: idx_idx }, loc.clone());
        self.emit(Instruction::GetIndex, loc.clone());
        self.emit(Instruction::SetLocal { index: var_idx }, loc.clone());

        self.compile_block(&fl.body)?;

        // index++
        self.emit(Instruction::GetLocal { index: idx_idx }, loc.clone());
        let one_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(1)));
        self.emit(Instruction::Constant { index: one_idx }, loc.clone());
        self.emit(Instruction::Add, loc.clone());
        self.emit(Instruction::SetLocal { index: idx_idx }, loc.clone());

        self.emit(Instruction::Jump { offset: loop_start }, loc);

        let exit_patch = self.current_chunk().code.len();
        if let Instruction::JumpIf { ref mut offset, .. } = self.current_chunk().code[exit_jump] {
            *offset = exit_patch;
        }

        let lc = self.loop_contexts.pop().unwrap();
        for jump in &lc.break_jumps {
            if let Instruction::Jump { ref mut offset } = self.current_chunk().code[*jump] {
                *offset = exit_patch;
            }
        }
        for jump in &lc.continue_jumps {
            if let Instruction::Jump { ref mut offset } = self.current_chunk().code[*jump] {
                *offset = loop_start;
            }
        }

        Ok(())
    }

    fn compile_range_for_loop(&mut self, rfl: &RangeForLoop) -> Result<(), CompileError> {
        let loc = Location::default();
        let nil_idx = self.make_constant(HeapData::Nil);
        self.emit(Instruction::Constant { index: nil_idx }, loc.clone());
        self.add_local(&rfl.variable.name);
        let control_idx = self.contexts.last().unwrap().locals.len() - 1;

        self.compile_expression(&rfl.start)?;
        if let Some(ref step_expr) = rfl.step {
            self.compile_expression(step_expr)?;
        } else {
            let one_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(1)));
            self.emit(Instruction::Constant { index: one_idx }, loc.clone());
        }
        self.emit(Instruction::Subtract, loc.clone());
        self.emit(Instruction::SetLocal { index: control_idx }, loc.clone());

        self.compile_expression(&rfl.end)?;
        let limit_idx = self.contexts.last().unwrap().locals.len();
        self.add_local("__limit__");

        if let Some(ref step_expr) = rfl.step {
            self.compile_expression(step_expr)?;
        } else {
            let one_idx = self.make_constant(HeapData::Primitive(Primitive::Integer(1)));
            self.emit(Instruction::Constant { index: one_idx }, loc.clone());
        }
        let step_idx = self.contexts.last().unwrap().locals.len();
        self.add_local("__step__");

        let body_locals_snapshot = self.contexts.last().unwrap().locals.len();
        let for_idx = self.current_chunk().code.len();
        self.loop_contexts.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_locals_snapshot,
        });

        self.emit(
            Instruction::ForLoop {
                control_index: control_idx,
                limit_index: limit_idx,
                body_offset: 0,
                inclusive: matches!(rfl.range_type, RangeOperator::Inclusive),
                step_index: Some(step_idx),
            },
            loc.clone(),
        );

        let exit_jump_idx = self.current_chunk().code.len();
        self.emit(Instruction::Jump { offset: 0 }, loc.clone());

        let body_start = self.current_chunk().code.len();
        self.compile_block(&rfl.body)?;
        self.emit(Instruction::Jump { offset: for_idx }, loc);

        if let Instruction::ForLoop {
            ref mut body_offset,
            ..
        } = self.current_chunk().code[for_idx]
        {
            *body_offset = body_start;
        }

        let exit_patch = self.current_chunk().code.len();
        if let Instruction::Jump { ref mut offset } = self.current_chunk().code[exit_jump_idx] {
            *offset = exit_patch;
        }

        let lc = self.loop_contexts.pop().unwrap();
        for jump in &lc.break_jumps {
            if let Instruction::Jump { ref mut offset } = self.current_chunk().code[*jump] {
                *offset = exit_patch;
            }
        }
        for jump in &lc.continue_jumps {
            if let Instruction::Jump { ref mut offset } = self.current_chunk().code[*jump] {
                *offset = for_idx;
            }
        }

        Ok(())
    }

    fn compile_name_assign(&mut self, na: &NameAssignment) -> Result<(), CompileError> {
        self.compile_expression(&na.expression)?;
        if na.names.len() == 1 {
            let name = &na.names[0].name;
            match self.resolve_variable(name) {
                VarType::Local(idx) => {
                    self.emit(Instruction::SetLocal { index: idx }, Location::default());
                },
                VarType::Upvalue(uv) => {
                    self.emit(
                        Instruction::SetUpvalue { index: uv.index },
                        Location::default(),
                    );
                },
                VarType::Global(_) => {
                    let idx = self.make_string_constant(name);
                    self.emit(
                        Instruction::SetGlobal { name_index: idx },
                        Location::default(),
                    );
                },
            }
        }
        Ok(())
    }

    fn compile_op_assign(&mut self, oa: &OperatorAssignment) -> Result<(), CompileError> {
        match &oa.variable {
            Variable::Identifier(id) => {
                let name = &id.name;
                self.compile_variable(&oa.variable)?;
                self.compile_expression(&oa.expression)?;

                match oa.op {
                    AssignmentOperator::Plus => self.emit(Instruction::Add, Location::default()),
                    AssignmentOperator::Minus => {
                        self.emit(Instruction::Subtract, Location::default());
                    },
                    AssignmentOperator::Multiply => {
                        self.emit(Instruction::Multiply, Location::default());
                    },
                    AssignmentOperator::Divide => {
                        self.emit(Instruction::Divide, Location::default());
                    },
                    AssignmentOperator::Modulo => {
                        self.emit(Instruction::Modulo, Location::default());
                    },
                    AssignmentOperator::Power => self.emit(Instruction::Power, Location::default()),
                    AssignmentOperator::Assign => {
                        self.emit(Instruction::Pop { count: 1 }, Location::default());
                        let loc = Location::default();
                        match self.resolve_variable(name) {
                            VarType::Local(idx) => {
                                self.emit(Instruction::SetLocal { index: idx }, loc);
                            },
                            VarType::Upvalue(uv) => {
                                self.emit(Instruction::SetUpvalue { index: uv.index }, loc);
                            },
                            VarType::Global(_) => {
                                let idx = self.make_string_constant(name);
                                self.emit(Instruction::SetGlobal { name_index: idx }, loc);
                            },
                        }
                        return Ok(());
                    },
                }

                let loc = Location::default();
                match self.resolve_variable(name) {
                    VarType::Local(idx) => {
                        self.emit(Instruction::SetLocal { index: idx }, loc);
                    },
                    VarType::Upvalue(uv) => {
                        self.emit(Instruction::SetUpvalue { index: uv.index }, loc);
                    },
                    VarType::Global(_) => {
                        let idx = self.make_string_constant(name);
                        self.emit(Instruction::SetGlobal { name_index: idx }, loc);
                    },
                }
                Ok(())
            },
            Variable::IndexExpression(ie) => {
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
                    self.emit(Instruction::Duplicate { index: 1 }, Location::default());
                    self.emit(Instruction::Duplicate { index: 1 }, Location::default());
                    self.emit(Instruction::GetIndex, Location::default());
                    self.compile_expression(&oa.expression)?;
                    self.emit(op, Location::default());
                } else {
                    self.compile_expression(&oa.expression)?;
                }

                self.emit(Instruction::SetIndex, Location::default());
                self.emit(Instruction::Pop { count: 1 }, Location::default());
                Ok(())
            },
        }
    }

    // -----------------------------------------------------------------------
    // Compile last statements
    // -----------------------------------------------------------------------

    fn compile_last_statement(&mut self, last: &LastStatement) -> Result<(), CompileError> {
        match last {
            LastStatement::Return(ret) => {
                if let Some(ref expr) = ret.expression {
                    self.compile_expression(expr)?;
                } else {
                    let nil_idx = self.make_constant(HeapData::Nil);
                    self.emit(
                        Instruction::Constant { index: nil_idx },
                        Location::default(),
                    );
                }
                if self.contexts.len() > 1 {
                    self.emit(Instruction::Return, Location::default());
                }
            },
            LastStatement::Break(_) => {
                let jump = self.current_chunk().code.len();
                self.emit(Instruction::Jump { offset: 0 }, Location::default());
                if let Some(lc) = self.loop_contexts.last_mut() {
                    lc.break_jumps.push(jump);
                }
            },
            LastStatement::Continue(_) => {
                if let Some(lc) = self.loop_contexts.last() {
                    let body_locals =
                        self.contexts.last().unwrap().locals.len() - lc.body_locals_snapshot;
                    if body_locals > 0 {
                        self.emit(Instruction::Pop { count: body_locals }, Location::default());
                    }
                }
                let jump = self.current_chunk().code.len();
                self.emit(Instruction::Jump { offset: 0 }, Location::default());
                if let Some(lc) = self.loop_contexts.last_mut() {
                    lc.continue_jumps.push(jump);
                }
            },
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Compile expressions
    // -----------------------------------------------------------------------

    fn compile_expression(&mut self, expr: &Expression) -> Result<(), CompileError> {
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
                    self.compile_expression(elem)?;
                }
                self.emit(
                    Instruction::MakeArray {
                        count: arr.elements.len(),
                    },
                    Location::default(),
                );
                return Ok(());
            },
        };
        let idx = self.make_constant(heap_data);
        self.emit(Instruction::Constant { index: idx }, Location::default());
        Ok(())
    }

    fn compile_variable(&mut self, var: &Variable) -> Result<(), CompileError> {
        match var {
            Variable::Identifier(id) => match self.resolve_variable(&id.name) {
                VarType::Local(idx) => {
                    self.emit(Instruction::GetLocal { index: idx }, Location::default());
                },
                VarType::Upvalue(uv) => {
                    self.emit(
                        Instruction::GetUpvalue { index: uv.index },
                        Location::default(),
                    );
                },
                VarType::Global(name) => {
                    let idx = self.make_string_constant(&name);
                    self.emit(
                        Instruction::GetGlobal { name_index: idx },
                        Location::default(),
                    );
                },
            },
            Variable::IndexExpression(ie) => {
                self.compile_variable(&ie.base)?;
                self.compile_expression(&ie.index)?;
                self.emit(Instruction::GetIndex, Location::default());
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
                self.emit(Instruction::GetLocal { index: idx }, Location::default());
            },
            VarType::Upvalue(uv) => {
                self.emit(
                    Instruction::GetUpvalue { index: uv.index },
                    Location::default(),
                );
            },
            VarType::Global(_) => {
                let name_idx = self.make_string_constant(&fc.name.name);
                self.emit(
                    Instruction::GetGlobal {
                        name_index: name_idx,
                    },
                    Location::default(),
                );
            },
        }

        for arg in &fc.arguments {
            self.compile_expression(arg)?;
        }

        self.emit(
            Instruction::Call {
                arg_count: fc.arguments.len(),
                keep_return_value,
            },
            Location::default(),
        );
        Ok(())
    }

    fn compile_binary(&mut self, be: &BinaryExpression) -> Result<(), CompileError> {
        if let Some(folded) = self.try_fold_binary(be) {
            let idx = self.make_constant(folded);
            self.emit(Instruction::Constant { index: idx }, Location::default());
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
        self.emit(inst, Location::default());
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
            self.emit(Instruction::Constant { index: idx }, Location::default());
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
        self.emit(inst, Location::default());
        Ok(())
    }

    fn compile_logical(&mut self, le: &LogicalExpression) -> Result<(), CompileError> {
        self.compile_expression(&le.lhs)?;

        match le.op {
            LogicalOperator::And => {
                // Short-circuit: if lhs is falsy, keep it as result and jump past rhs
                let jump = self.current_chunk().code.len();
                self.emit(
                    Instruction::JumpIf {
                        offset: 0,
                        expected: false,
                        keep_stay: false,
                        keep_jump: true,
                    },
                    Location::default(),
                );
                self.compile_expression(&le.rhs)?;
                let patch = self.current_chunk().code.len();
                if let Instruction::JumpIf { ref mut offset, .. } = self.current_chunk().code[jump]
                {
                    *offset = patch;
                }
            },
            LogicalOperator::Or => {
                // Short-circuit: if lhs is truthy, keep it as result and jump past rhs
                let jump = self.current_chunk().code.len();
                self.emit(
                    Instruction::JumpIf {
                        offset: 0,
                        expected: true,
                        keep_stay: false,
                        keep_jump: true,
                    },
                    Location::default(),
                );
                self.compile_expression(&le.rhs)?;
                let patch = self.current_chunk().code.len();
                if let Instruction::JumpIf { ref mut offset, .. } = self.current_chunk().code[jump]
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
            self.emit(match_comparison_op(*op, !is_last), Location::default());

            if !is_last {
                let jump = self.current_chunk().code.len();
                self.emit(
                    Instruction::JumpIf {
                        offset: 0,
                        expected: false,
                        keep_stay: false,
                        keep_jump: true,
                    },
                    Location::default(),
                );
                false_jumps.push(jump);
            }
        }

        let end_jump = self.current_chunk().code.len();
        self.emit(Instruction::Jump { offset: 0 }, Location::default());

        let cleanup = self.current_chunk().code.len();
        for jump in &false_jumps {
            if let Instruction::JumpIf { ref mut offset, .. } = self.current_chunk().code[*jump] {
                *offset = cleanup;
            }
        }
        self.emit(Instruction::Pop { count: 2 }, Location::default());
        let false_idx = self.make_constant(HeapData::Primitive(Primitive::Bool(false)));
        self.emit(
            Instruction::Constant { index: false_idx },
            Location::default(),
        );

        let end = self.current_chunk().code.len();
        if let Instruction::Jump { ref mut offset } = self.current_chunk().code[end_jump] {
            *offset = end;
        }

        Ok(())
    }

    fn compile_if_expr(&mut self, ife: &IfExpression) -> Result<(), CompileError> {
        self.compile_expression(&ife.base_if.condition)?;
        let else_jump = self.current_chunk().code.len();
        self.emit(
            Instruction::JumpIf {
                offset: 0,
                expected: false,
                keep_stay: true,
                keep_jump: false,
            },
            Location::default(),
        );
        self.emit(Instruction::Pop { count: 1 }, Location::default());

        self.compile_block(&ife.base_if.block)?;
        let end_jump = self.current_chunk().code.len();
        self.emit(Instruction::Jump { offset: 0 }, Location::default());

        let else_patch = self.current_chunk().code.len();
        if let Instruction::JumpIf { ref mut offset, .. } = self.current_chunk().code[else_jump] {
            *offset = else_patch;
        }

        self.compile_block(&ife.else_block)?;

        let end_patch = self.current_chunk().code.len();
        if let Instruction::Jump { ref mut offset } = self.current_chunk().code[end_jump] {
            *offset = end_patch;
        }

        Ok(())
    }

    fn compile_anon_fn(&mut self, af: &AnonymousFunction) -> Result<(), CompileError> {
        let chunk_id = self.push_context();
        let arity = af.body.parameters.len();
        let name = format!("<anon_{}>", self.synthetic_counter);
        self.synthetic_counter += 1;

        for param in &af.body.parameters {
            self.add_local(&param.name);
        }

        self.compile_block(&af.body.block)?;
        self.emit(Instruction::Return, Location::default());

        let upvalues = self.contexts.last().unwrap().upvalues.clone();
        self.pop_context();

        let func_data = HeapData::Function(Function::Bytecode(Box::new(BytecodeFunction {
            id: chunk_id,
            name,
            arity,
            curried_args: Vec::new(),
            captured_upvalues: Vec::new(),
        })));
        let func_idx = self.make_constant(func_data);

        if upvalues.is_empty() {
            self.emit(
                Instruction::Constant { index: func_idx },
                Location::default(),
            );
        } else {
            self.emit(
                Instruction::Closure {
                    function_index: func_idx,
                    upvalues,
                },
                Location::default(),
            );
        }

        Ok(())
    }

    const fn deduplicate_constants() {
        // For MVP, skip deduplication
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
