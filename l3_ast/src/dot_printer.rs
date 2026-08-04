use std::fmt::{Display, Write};

use crate::*;

#[must_use]
pub fn format_ast_graph(program: &Program) -> String {
    let mut out = String::new();
    DotPrinter::new().write_graph(program, &mut out);
    out
}

struct DotPrinter {
    node_id: usize,
}

impl DotPrinter {
    const fn new() -> Self {
        Self { node_id: 0 }
    }

    const fn next_id(&mut self) -> usize {
        let id = self.node_id;
        self.node_id += 1;
        id
    }

    fn write_node(out: &mut String, id: usize, label: impl Display) {
        writeln!(out, "  n{id} [label=\"{label}\"];").unwrap();
    }

    fn write_edge(out: &mut String, from: usize, to: usize) {
        writeln!(out, "  n{from} -> n{to};").unwrap();
    }

    fn write_edge_labeled(out: &mut String, from: usize, to: usize, label: &str) {
        writeln!(out, "  n{from} -> n{to} [label=\"{label}\"];").unwrap();
    }

    fn write_graph(&mut self, program: &Program, out: &mut String) {
        write_header(out);
        self.visit_block(program, out);
        write_footer(out);
    }

    fn visit_literal(&mut self, lit: &Literal, out: &mut String) {
        match lit {
            Literal::Nil(_) => {
                let id = self.next_id();
                Self::write_node(out, id, "Nil");
            },
            Literal::Boolean(b) => {
                let id = self.next_id();
                Self::write_node(out, id, format!("Boolean\\n{}", b.value));
            },
            Literal::Number(n) => {
                let id = self.next_id();
                Self::write_node(out, id, format!("Number\\n{}", n.value));
            },
            Literal::Float(f) => {
                let id = self.next_id();
                Self::write_node(out, id, format!("Float\\n{}", f.value));
            },
            Literal::String(s) => {
                let id = self.next_id();
                Self::write_node(out, id, format!("String\\n\"{}\"", s.value));
            },
            Literal::Array(a) => {
                let id = self.next_id();
                Self::write_node(out, id, "Array");
                for elem in &a.elements {
                    Self::write_edge(out, id, self.node_id);
                    self.visit_expression(elem, out);
                }
            },
        }
    }

    fn visit_variable(&mut self, var: &Variable, out: &mut String) {
        match var {
            Variable::Identifier(id) => {
                let node_id = self.next_id();
                Self::write_node(out, node_id, format!("Identifier\\n'{}'", id.name));
            },
            Variable::IndexExpression(idx) => {
                let id = self.next_id();
                Self::write_node(out, id, "IndexExpression");
                Self::write_edge_labeled(out, id, self.node_id, "base");
                self.visit_variable(&idx.base, out);
                Self::write_edge_labeled(out, id, self.node_id, "index");
                self.visit_expression(&idx.index, out);
            },
        }
    }

    fn visit_unary_expression(&mut self, expr: &UnaryExpression, out: &mut String) {
        let op = match expr.op {
            UnaryOperator::Plus => "Plus",
            UnaryOperator::Minus => "Minus",
            UnaryOperator::Not => "Not",
        };
        let id = self.next_id();
        Self::write_node(out, id, format!("UnaryExpression\\n{op}"));
        Self::write_edge(out, id, self.node_id);
        self.visit_expression(&expr.expression, out);
    }

    fn visit_binary_expression(&mut self, expr: &BinaryExpression, out: &mut String) {
        let op = match expr.op {
            BinaryOperator::Plus => "Plus",
            BinaryOperator::Minus => "Minus",
            BinaryOperator::Multiply => "Multiply",
            BinaryOperator::Divide => "Divide",
            BinaryOperator::Modulo => "Modulo",
            BinaryOperator::Power => "Power",
        };
        let id = self.next_id();
        Self::write_node(out, id, format!("BinaryExpression\\n{op}"));
        Self::write_edge_labeled(out, id, self.node_id, "lhs");
        self.visit_expression(&expr.lhs, out);
        Self::write_edge_labeled(out, id, self.node_id, "rhs");
        self.visit_expression(&expr.rhs, out);
    }

    fn visit_logical_expression(&mut self, expr: &LogicalExpression, out: &mut String) {
        let op = match expr.op {
            LogicalOperator::And => "And",
            LogicalOperator::Or => "Or",
        };
        let id = self.next_id();
        Self::write_node(out, id, format!("LogicalExpression\\n{op}"));
        Self::write_edge_labeled(out, id, self.node_id, "lhs");
        self.visit_expression(&expr.lhs, out);
        Self::write_edge_labeled(out, id, self.node_id, "rhs");
        self.visit_expression(&expr.rhs, out);
    }

    fn visit_comparison(&mut self, comp: &Comparison, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "ChainedComparison");
        let start_id = self.node_id;
        self.visit_expression(&comp.start, out);
        Self::write_edge_labeled(out, id, start_id, "start");
        for (op, rhs) in &comp.comparisons {
            let op_str = match op {
                ComparisonOperator::Equal => "Equal",
                ComparisonOperator::NotEqual => "NotEqual",
                ComparisonOperator::Less => "Less",
                ComparisonOperator::LessEqual => "LessEqual",
                ComparisonOperator::Greater => "Greater",
                ComparisonOperator::GreaterEqual => "GreaterEqual",
            };
            let op_id = self.next_id();
            Self::write_edge(out, id, op_id);
            Self::write_node(out, op_id, op_str);
            Self::write_edge(out, id, self.node_id);
            self.visit_expression(rhs, out);
        }
    }

    fn visit_function_call(&mut self, call: &FunctionCall, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "FunctionCall");
        Self::write_edge_labeled(out, id, self.node_id, "name");
        self.visit_identifier(&call.name, out);

        let args_id = self.next_id();
        Self::write_edge(out, id, args_id);
        Self::write_node(out, args_id, "Arguments");

        for arg in &call.arguments {
            Self::write_edge(out, id, self.node_id);
            self.visit_expression(arg, out);
        }
    }

    fn visit_function_body(&mut self, body: &FunctionBody, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "FunctionBody");

        let params_id = self.next_id();
        Self::write_edge(out, id, params_id);
        Self::write_node(out, params_id, "Parameters");

        for param in &body.parameters {
            Self::write_edge(out, id, self.node_id);
            self.visit_identifier(param, out);
        }

        Self::write_edge_labeled(out, id, self.node_id, "body");
        self.visit_block(&body.block, out);
    }

    fn visit_if_base(&mut self, base: &IfBase, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "IfBase");

        Self::write_edge_labeled(out, id, self.node_id, "condition");
        self.visit_expression(&base.condition, out);

        Self::write_edge_labeled(out, id, self.node_id, "block");
        self.visit_block(&base.block, out);
    }

    fn visit_elseif_list(&mut self, list: &ElseIfList, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "ElseIfList");
        for elseif in list {
            Self::write_edge(out, id, self.node_id);
            self.visit_if_base(elseif, out);
        }
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "IfStatement");

        Self::write_edge_labeled(out, id, self.node_id, "if");
        self.visit_if_base(&stmt.base_if, out);

        Self::write_edge_labeled(out, id, self.node_id, "elseif");
        self.visit_elseif_list(&stmt.elseif, out);

        if let Some(ref else_block) = stmt.else_block {
            Self::write_edge_labeled(out, id, self.node_id, "else");
            self.visit_block(else_block, out);
        }
    }

    fn visit_if_expression(&mut self, expr: &IfExpression, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "IfExpression");

        Self::write_edge_labeled(out, id, self.node_id, "if");
        self.visit_if_base(&expr.base_if, out);

        Self::write_edge_labeled(out, id, self.node_id, "elseif");
        self.visit_elseif_list(&expr.elseif, out);

        Self::write_edge_labeled(out, id, self.node_id, "else");
        self.visit_block(&expr.else_block, out);
    }

    fn visit_while(&mut self, w: &While, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "While");

        Self::write_edge_labeled(out, id, self.node_id, "condition");
        self.visit_expression(&w.condition, out);

        Self::write_edge_labeled(out, id, self.node_id, "body");
        self.visit_block(&w.body, out);
    }

    fn visit_for_loop(&mut self, fl: &ForLoop, out: &mut String) {
        let mutability = match fl.mutability {
            Mutability::Immutable => "Immutable",
            Mutability::Mutable => "Mutable",
        };
        let id = self.next_id();
        Self::write_node(out, id, format!("ForLoop\\n{mutability}"));

        Self::write_edge_labeled(out, id, self.node_id, "variable");
        self.visit_identifier(&fl.variable, out);

        Self::write_edge_labeled(out, id, self.node_id, "collection");
        self.visit_expression(&fl.collection, out);

        Self::write_edge_labeled(out, id, self.node_id, "body");
        self.visit_block(&fl.body, out);
    }

    fn visit_range_for_loop(&mut self, rfl: &RangeForLoop, out: &mut String) {
        let mutability = match rfl.mutability {
            Mutability::Immutable => "Immutable",
            Mutability::Mutable => "Mutable",
        };
        let range_type = match rfl.range_type {
            RangeOperator::Inclusive => "Inclusive",
            RangeOperator::Exclusive => "Exclusive",
        };
        let id = self.next_id();
        Self::write_node(
            out,
            id,
            format!("RangeForLoop\\n{mutability}\\n{range_type}"),
        );

        Self::write_edge_labeled(out, id, self.node_id, "variable");
        self.visit_identifier(&rfl.variable, out);

        Self::write_edge_labeled(out, id, self.node_id, "start");
        self.visit_expression(&rfl.start, out);

        Self::write_edge_labeled(out, id, self.node_id, "end");
        self.visit_expression(&rfl.end, out);

        if let Some(ref step) = rfl.step {
            Self::write_edge_labeled(out, id, self.node_id, "step");
            self.visit_expression(step, out);
        }

        Self::write_edge_labeled(out, id, self.node_id, "body");
        self.visit_block(&rfl.body, out);
    }

    fn visit_last_statement(&mut self, last: &LastStatement, out: &mut String) {
        match last {
            LastStatement::Return(r) => {
                let id = self.next_id();
                Self::write_node(out, id, "Return");
                if let Some(ref expr) = r.expression {
                    Self::write_edge(out, id, self.node_id);
                    self.visit_expression(expr, out);
                }
            },
            LastStatement::Break(_) => {
                let id = self.next_id();
                Self::write_node(out, id, "Break");
            },
            LastStatement::Continue(_) => {
                let id = self.next_id();
                Self::write_node(out, id, "Continue");
            },
        }
    }

    fn visit_declaration(&mut self, decl: &Declaration, out: &mut String) {
        let mutability = match decl.mutability {
            Mutability::Immutable => "Immutable",
            Mutability::Mutable => "Mutable",
        };
        let id = self.next_id();
        Self::write_node(out, id, format!("Declaration\\n{mutability}"));

        Self::write_edge_labeled(out, id, self.node_id, "names");
        self.visit_name_list(&decl.names, out);

        if let Some(ref expr) = decl.expression {
            Self::write_edge_labeled(out, id, self.node_id, "init");
            self.visit_expression(expr, out);
        }
    }

    fn visit_name_assignment(&mut self, na: &NameAssignment, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "NameAssignment");

        Self::write_edge_labeled(out, id, self.node_id, "names");
        self.visit_name_list(&na.names, out);

        Self::write_edge_labeled(out, id, self.node_id, "value");
        self.visit_expression(&na.expression, out);
    }

    fn visit_operator_assignment(&mut self, oa: &OperatorAssignment, out: &mut String) {
        let op = match oa.op {
            AssignmentOperator::Assign => "Assign",
            AssignmentOperator::Plus => "Plus",
            AssignmentOperator::Minus => "Minus",
            AssignmentOperator::Multiply => "Multiply",
            AssignmentOperator::Divide => "Divide",
            AssignmentOperator::Modulo => "Modulo",
            AssignmentOperator::Power => "Power",
        };
        let id = self.next_id();
        Self::write_node(out, id, format!("OperatorAssignment\\n{op}"));

        Self::write_edge_labeled(out, id, self.node_id, "variable");
        self.visit_variable(&oa.variable, out);

        Self::write_edge_labeled(out, id, self.node_id, "value");
        self.visit_expression(&oa.expression, out);
    }

    fn visit_identifier(&mut self, id: &Identifier, out: &mut String) {
        let node_id = self.next_id();
        Self::write_node(out, node_id, format!("Identifier\\n'{}'", id.name));
    }

    fn visit_name_list(&mut self, names: &NameList, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "NameList");
        for name in names {
            Self::write_edge(out, id, self.node_id);
            self.visit_identifier(name, out);
        }
    }

    fn visit_named_function(&mut self, func: &NamedFunction, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "NamedFunction");

        Self::write_edge_labeled(out, id, self.node_id, "name");
        self.visit_identifier(&func.name, out);

        Self::write_edge_labeled(out, id, self.node_id, "body");
        self.visit_function_body(&func.body, out);
    }

    fn visit_anonymous_function(&mut self, func: &AnonymousFunction, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "AnonymousFunction");
        Self::write_edge(out, id, self.node_id);
        self.visit_function_body(&func.body, out);
    }

    fn visit_statement(&mut self, stmt: &Statement, out: &mut String) {
        match stmt {
            Statement::Declaration(d) => self.visit_declaration(d, out),
            Statement::ForLoop(f) => self.visit_for_loop(f, out),
            Statement::FunctionCall(f) => self.visit_function_call(f, out),
            Statement::IfStatement(i) => self.visit_if_statement(i, out),
            Statement::NameAssignment(a) => self.visit_name_assignment(a, out),
            Statement::NamedFunction(f) => self.visit_named_function(f, out),
            Statement::OperatorAssignment(a) => self.visit_operator_assignment(a, out),
            Statement::RangeForLoop(r) => self.visit_range_for_loop(r, out),
            Statement::While(w) => self.visit_while(w, out),
        }
    }

    fn visit_expression(&mut self, expr: &Expression, out: &mut String) {
        match expr {
            Expression::AnonymousFunction(f) => self.visit_anonymous_function(f, out),
            Expression::BinaryExpression(b) => self.visit_binary_expression(b, out),
            Expression::Comparison(c) => self.visit_comparison(c, out),
            Expression::FunctionCall(f) => self.visit_function_call(f, out),
            Expression::IfExpression(i) => self.visit_if_expression(i, out),
            Expression::Literal(l) => self.visit_literal(l, out),
            Expression::LogicalExpression(l) => self.visit_logical_expression(l, out),
            Expression::UnaryExpression(u) => self.visit_unary_expression(u, out),
            Expression::Variable(v) => self.visit_variable(v, out),
        }
    }

    fn visit_block(&mut self, block: &Block, out: &mut String) {
        let id = self.next_id();
        Self::write_node(out, id, "Block");
        for stmt in &block.statements {
            Self::write_edge(out, id, self.node_id);
            self.visit_statement(stmt, out);
        }
        if let Some(ref last) = block.last_statement {
            Self::write_edge_labeled(out, id, self.node_id, "last");
            self.visit_last_statement(last, out);
        }
    }
}

fn write_header(out: &mut String) {
    out.push_str("digraph AST {\n");
    out.push_str(
        "node [shape=box, style=\"rounded\", ordering=out];  rankdir=TB;  nodesep=0.5;  \
         ranksep=1.0;  splines=true;fontname=\"Helvetica,Roboto,sans-serif\";node \
         [fontname=\"Helvetica,Roboto,sans-serif\"];edge \
         [fontname=\"Helvetica,Roboto,sans-serif\"];\n",
    );
    out.push('\n');
}

fn write_footer(out: &mut String) {
    out.push_str("}\n");
}
