use l3_ast::{
    BinaryExpression, BinaryOperator, Expression, Literal, UnaryExpression, UnaryOperator,
};
use l3_bytecode::{ConstantIndex, idx};
use l3_runtime::{HeapData, Primitive};

use crate::Compiler;

impl Compiler {
    pub(crate) fn make_constant(&mut self, value: HeapData) -> ConstantIndex {
        if let Some(i) = self
            .program
            .constants
            .iter()
            .position(|data| *data == value)
        {
            return idx(i);
        }
        self.program.constants.push(value)
    }

    pub(crate) fn make_string_constant(&mut self, s: &str) -> ConstantIndex {
        self.make_constant(HeapData::String(s.to_string()))
    }

    // -----------------------------------------------------------------------
    // Compile blocks
    // -----------------------------------------------------------------------

    pub(crate) fn try_fold_expression(&mut self, expr: &Expression) -> Option<HeapData> {
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

    pub(crate) fn try_fold_unary(&mut self, ue: &UnaryExpression) -> Option<HeapData> {
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

    pub(crate) fn try_fold_binary(&mut self, be: &BinaryExpression) -> Option<HeapData> {
        let lhs = self.try_fold_expression(&be.lhs)?;
        let rhs = self.try_fold_expression(&be.rhs)?;
        match (lhs, rhs) {
            (HeapData::Primitive(a), HeapData::Primitive(b)) => {
                let result = match be.op {
                    BinaryOperator::Plus => (a + b).ok(),
                    BinaryOperator::Minus => (a - b).ok(),
                    BinaryOperator::Multiply => (a * b).ok(),
                    BinaryOperator::Divide => (a / b).ok(),
                    BinaryOperator::Modulo => (a % b).ok(),
                    BinaryOperator::Power => a.pow(b).ok(),
                };
                result.map(HeapData::Primitive)
            },
            _ => None,
        }
    }
}
