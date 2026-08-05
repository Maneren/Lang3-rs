use std::fmt::{Arguments, Display, Write as _};

use crate::*;

fn append(out: &mut String, args: Arguments<'_>) {
    out.write_fmt(args)
        .expect("Writing to string should not fail.");
}

#[must_use]
pub fn format_ast(program: &Program) -> String {
    let mut out = String::new();
    AstPrinter::new().print_block(program, &mut out);
    out
}

struct AstPrinter {
    depth: usize,
}

impl AstPrinter {
    const fn new() -> Self {
        Self { depth: 0 }
    }

    fn indent(&self, out: &mut String) {
        for _ in 0..self.depth {
            out.push_str("▏ ");
        }
    }

    fn line(&self, out: &mut String, text: impl Display) {
        self.indent(out);
        append(out, format_args!("{text}\n"));
    }

    fn print_block(&mut self, block: &Block, out: &mut String) {
        self.line(out, "Block");
        self.depth += 1;
        for stmt in &block.statements {
            self.print_statement(stmt, out);
        }
        if let Some(ref last) = block.last_statement {
            self.line(out, "LastStatement");
            self.depth += 1;
            self.print_last_statement(last, out);
            self.depth -= 1;
        }
        self.depth -= 1;
    }

    fn print_statement(&mut self, stmt: &Statement, out: &mut String) {
        match stmt {
            Statement::Declaration(d) => self.print_declaration(d, out),
            Statement::ForLoop(f) => self.print_for_loop(f, out),
            Statement::FunctionCall(f) => self.print_function_call(f, out),
            Statement::IfStatement(i) => self.print_if_statement(i, out),
            Statement::NameAssignment(a) => self.print_name_assignment(a, out),
            Statement::NamedFunction(f) => self.print_named_function(f, out),
            Statement::OperatorAssignment(a) => self.print_operator_assignment(a, out),
            Statement::RangeForLoop(r) => self.print_range_for_loop(r, out),
            Statement::While(w) => self.print_while(w, out),
        }
    }

    fn print_expression(&mut self, expr: &Expression, out: &mut String) {
        match expr {
            Expression::AnonymousFunction(f) => self.print_anonymous_function(f, out),
            Expression::BinaryExpression(b) => self.print_binary_expression(b, out),
            Expression::Comparison(c) => self.print_comparison(c, out),
            Expression::FunctionCall(f) => self.print_function_call(f, out),
            Expression::IfExpression(i) => self.print_if_expression(i, out),
            Expression::Literal(l) => self.print_literal(l, out),
            Expression::LogicalExpression(l) => self.print_logical_expression(l, out),
            Expression::UnaryExpression(u) => self.print_unary_expression(u, out),
            Expression::Variable(v) => self.print_variable(v, out),
        }
    }

    fn print_literal(&mut self, lit: &Literal, out: &mut String) {
        match lit {
            Literal::Nil(_) => self.line(out, "Nil"),
            Literal::Boolean(b) => self.line(out, format!("Boolean {}", b.value)),
            Literal::Number(n) => self.line(out, format!("Number {}", n.value)),
            Literal::Float(f) => self.line(out, format!("Float {}", f.value)),
            Literal::String(s) => self.line(out, format!("String \"{}\"", s.value)),
            Literal::Array(a) => {
                self.line(out, "Array");
                self.depth += 1;
                for elem in &a.elements {
                    self.print_expression(elem, out);
                }
                self.depth -= 1;
            },
        }
    }

    fn print_variable(&mut self, var: &Variable, out: &mut String) {
        match var {
            Variable::Identifier(id) => self.line(out, format!("Identifier '{}'", id.name)),
            Variable::IndexExpression(idx) => {
                self.line(out, "IndexExpression");
                self.depth += 1;
                self.print_variable(&idx.base, out);
                self.print_expression(&idx.index, out);
                self.depth -= 1;
            },
        }
    }

    fn print_unary_expression(&mut self, expr: &UnaryExpression, out: &mut String) {
        let op = match expr.op {
            UnaryOperator::Plus => "Plus",
            UnaryOperator::Minus => "Minus",
            UnaryOperator::Not => "Not",
        };
        self.line(out, format!("UnaryExpression {op}"));
        self.depth += 1;
        self.print_expression(&expr.expression, out);
        self.depth -= 1;
    }

    fn print_binary_expression(&mut self, expr: &BinaryExpression, out: &mut String) {
        let op = match expr.op {
            BinaryOperator::Plus => "Plus",
            BinaryOperator::Minus => "Minus",
            BinaryOperator::Multiply => "Multiply",
            BinaryOperator::Divide => "Divide",
            BinaryOperator::Modulo => "Modulo",
            BinaryOperator::Power => "Power",
        };
        self.line(out, format!("BinaryExpression {op}"));
        self.depth += 1;
        self.print_expression(&expr.lhs, out);
        self.print_expression(&expr.rhs, out);
        self.depth -= 1;
    }

    fn print_logical_expression(&mut self, expr: &LogicalExpression, out: &mut String) {
        let op = match expr.op {
            LogicalOperator::And => "And",
            LogicalOperator::Or => "Or",
        };
        self.line(out, format!("LogicalExpression {op}"));
        self.depth += 1;
        self.print_expression(&expr.lhs, out);
        self.print_expression(&expr.rhs, out);
        self.depth -= 1;
    }

    fn print_comparison(&mut self, comp: &Comparison, out: &mut String) {
        self.line(out, "ChainedComparison");
        self.depth += 1;
        self.print_expression(&comp.start, out);
        for (op, rhs) in &comp.comparisons {
            let op_str = match op {
                ComparisonOperator::Equal => "Equal",
                ComparisonOperator::NotEqual => "NotEqual",
                ComparisonOperator::Less => "Less",
                ComparisonOperator::LessEqual => "LessEqual",
                ComparisonOperator::Greater => "Greater",
                ComparisonOperator::GreaterEqual => "GreaterEqual",
            };
            self.line(out, op_str.to_string());
            self.depth += 1;
            self.print_expression(rhs, out);
            self.depth -= 1;
        }
        self.depth -= 1;
    }

    fn print_function_call(&mut self, call: &FunctionCall, out: &mut String) {
        self.line(out, "FunctionCall");
        self.depth += 1;
        self.line(out, format!("Identifier '{}'", call.name.name));
        self.line(out, "Arguments");
        self.depth += 1;
        for arg in &call.arguments {
            self.print_expression(arg, out);
        }
        self.depth -= 2;
    }

    fn print_function_body(&mut self, body: &FunctionBody, out: &mut String) {
        self.line(out, "Parameters");
        self.depth += 1;
        for param in &body.parameters {
            self.line(out, format!("Identifier '{}'", param.name));
        }
        self.depth -= 1;
        self.print_block(&body.block, out);
    }

    fn print_anonymous_function(&mut self, func: &AnonymousFunction, out: &mut String) {
        self.line(out, "AnonymousFunction");
        self.depth += 1;
        self.print_function_body(&func.body, out);
        self.depth -= 1;
    }

    fn print_named_function(&mut self, func: &NamedFunction, out: &mut String) {
        self.line(out, "NamedFunction");
        self.depth += 1;
        self.line(out, format!("Identifier '{}'", func.name.name));
        self.print_function_body(&func.body, out);
        self.depth -= 1;
    }

    fn print_if_base(&mut self, base: &IfBase, out: &mut String) {
        self.line(out, "Condition");
        self.depth += 1;
        self.print_expression(&base.condition, out);
        self.depth -= 1;
        self.print_block(&base.block, out);
    }

    fn print_if_statement(&mut self, stmt: &IfStatement, out: &mut String) {
        self.line(out, "IfStatement");
        self.depth += 1;
        self.print_if_base(&stmt.base_if, out);
        for elseif in &stmt.elseif {
            self.print_if_base(elseif, out);
        }
        if let Some(ref else_block) = stmt.else_block {
            self.line(out, "Else");
            self.depth += 1;
            self.print_block(else_block, out);
            self.depth -= 1;
        }
        self.depth -= 1;
    }

    fn print_if_expression(&mut self, expr: &IfExpression, out: &mut String) {
        self.line(out, "IfExpression");
        self.depth += 1;
        self.print_if_base(&expr.base_if, out);
        for elseif in &expr.elseif {
            self.print_if_base(elseif, out);
        }
        self.line(out, "Else");
        self.depth += 1;
        self.print_block(&expr.else_block, out);
        self.depth -= 2;
    }

    fn print_while(&mut self, w: &While, out: &mut String) {
        self.line(out, "While");
        self.depth += 1;
        self.line(out, "Condition");
        self.depth += 1;
        self.print_expression(&w.condition, out);
        self.depth -= 1;
        self.line(out, "Block");
        self.depth += 1;
        self.print_block(&w.body, out);
        self.depth -= 2;
    }

    fn print_for_loop(&mut self, fl: &ForLoop, out: &mut String) {
        let mutability = match fl.mutability {
            Mutability::Immutable => "Immutable",
            Mutability::Mutable => "Mutable",
        };
        self.line(out, format!("ForLoop ({mutability})"));
        self.depth += 1;
        self.line(out, "Variable");
        self.depth += 1;
        self.line(out, format!("Identifier '{}'", fl.variable.name));
        self.depth -= 1;
        self.line(out, "Collection");
        self.depth += 1;
        self.print_expression(&fl.collection, out);
        self.depth -= 1;
        self.line(out, "Block");
        self.depth += 1;
        self.print_block(&fl.body, out);
        self.depth -= 2;
    }

    fn print_range_for_loop(&mut self, rfl: &RangeForLoop, out: &mut String) {
        let mutability = match rfl.mutability {
            Mutability::Immutable => "Immutable",
            Mutability::Mutable => "Mutable",
        };
        let range_type = match rfl.range_type {
            RangeOperator::Inclusive => "Inclusive",
            RangeOperator::Exclusive => "Exclusive",
        };
        self.line(out, format!("RangeForLoop ({mutability}, {range_type})"));
        self.depth += 1;
        self.line(out, "Variable");
        self.depth += 1;
        self.line(out, format!("Identifier '{}'", rfl.variable.name));
        self.depth -= 1;
        self.line(out, "Start");
        self.depth += 1;
        self.print_expression(&rfl.start, out);
        self.depth -= 1;
        self.line(out, "End");
        self.depth += 1;
        self.print_expression(&rfl.end, out);
        self.depth -= 1;
        if let Some(ref step) = rfl.step {
            self.line(out, "Step");
            self.depth += 1;
            self.print_expression(step, out);
            self.depth -= 1;
        }
        self.line(out, "Block");
        self.depth += 1;
        self.print_block(&rfl.body, out);
        self.depth -= 2;
    }

    fn print_last_statement(&mut self, last: &LastStatement, out: &mut String) {
        match last {
            LastStatement::Return(r) => {
                self.line(out, "Return");
                if let Some(ref expr) = r.expression {
                    self.depth += 1;
                    self.print_expression(expr, out);
                    self.depth -= 1;
                }
            },
            LastStatement::Break(_) => self.line(out, "Break"),
            LastStatement::Continue(_) => self.line(out, "Continue"),
        }
    }

    fn print_declaration(&mut self, decl: &Declaration, out: &mut String) {
        let mutability = match decl.mutability {
            Mutability::Immutable => "Immutable",
            Mutability::Mutable => "Mutable",
        };
        self.line(out, format!("Declaration {mutability}"));
        self.depth += 1;
        for name in &decl.names {
            self.line(out, format!("Identifier '{}'", name.name));
        }
        if let Some(ref expr) = decl.expression {
            self.print_expression(expr, out);
        }
        self.depth -= 1;
    }

    fn print_name_assignment(&mut self, na: &NameAssignment, out: &mut String) {
        self.line(out, "NameAssignment");
        self.depth += 1;
        for name in &na.names {
            self.line(out, format!("Identifier '{}'", name.name));
        }
        self.print_expression(&na.expression, out);
        self.depth -= 1;
    }

    fn print_operator_assignment(&mut self, oa: &OperatorAssignment, out: &mut String) {
        let op = match oa.op {
            AssignmentOperator::Assign => "Assign",
            AssignmentOperator::Plus => "Plus",
            AssignmentOperator::Minus => "Minus",
            AssignmentOperator::Multiply => "Multiply",
            AssignmentOperator::Divide => "Divide",
            AssignmentOperator::Modulo => "Modulo",
            AssignmentOperator::Power => "Power",
        };
        self.line(out, format!("OperatorAssignment {op}"));
        self.depth += 1;
        self.print_variable(&oa.variable, out);
        self.print_expression(&oa.expression, out);
        self.depth -= 1;
    }
}
