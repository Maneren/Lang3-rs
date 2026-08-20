use l3_ast::{Block, Expression, IfBase, LastStatement, Literal, Statement, Variable};

use crate::Compiler;

impl Compiler {
    /// Conservatively mark every variable referenced within `expr` as
    /// possibly aliased. Over-marking is always safe; it only skips the
    /// `VectorAppend` optimization.
    pub(crate) fn mark_expression_aliased(&mut self, expr: &Expression) {
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

    pub(crate) fn mark_if_base_aliased(&mut self, if_base: &IfBase) {
        self.mark_expression_aliased(&if_base.condition);
        self.mark_block_aliased(&if_base.block);
    }

    pub(crate) fn mark_variable_aliased(&mut self, var: &Variable) {
        match var {
            Variable::Identifier(id) => self.mark_referenced_aliased(&id.name),
            Variable::IndexExpression(ie) => {
                self.mark_variable_aliased(&ie.base);
                self.mark_expression_aliased(&ie.index);
            },
        }
    }

    pub(crate) fn variable_references_name(&self, var: &Variable, name: &str) -> bool {
        match var {
            Variable::Identifier(id) => id.name == name,
            Variable::IndexExpression(ie) => {
                self.variable_references_name(&ie.base, name)
                    || self.expression_references_name(&ie.index, name)
            },
        }
    }

    pub(crate) fn mark_block_aliased(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.mark_statement_aliased(stmt);
        }
        if let Some(LastStatement::Return(ret)) = block.last_statement.as_ref()
            && let Some(expr) = &ret.expression
        {
            self.mark_expression_aliased(expr);
        }
    }

    pub(crate) fn mark_statement_aliased(&mut self, stmt: &Statement) {
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
    pub(crate) fn expression_references_name(&self, expr: &Expression, name: &str) -> bool {
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
}
