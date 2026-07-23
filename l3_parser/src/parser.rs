use l3_ast::*;
use l3_location::{Location, Position};
use crate::token::Token;
use crate::lexer::Lexer;

pub struct Parser<'input> {
    lexer: Lexer<'input>,
    peek: Option<(usize, Token<'input>, usize)>,
    filename: String,
    eof: bool,
}

impl<'input> Parser<'input> {
    pub fn new(source: &'input str, filename: &str) -> Self {
        let mut lexer = Lexer::new(source);
        let peek = lexer.next().and_then(|r| r.ok());
        Self {
            lexer,
            peek,
            filename: filename.to_string(),
            eof: false,
        }
    }

    fn advance(&mut self) -> Option<(usize, Token<'input>, usize)> {
        let current = self.peek.take();
        self.peek = self.lexer.next().and_then(|r| r.ok());
        if current.is_none() {
            self.eof = true;
        }
        current
    }

    fn peek_token(&self) -> Option<&Token<'input>> {
        self.peek.as_ref().map(|(_, t, _)| t)
    }

    fn expect(&mut self, expected: Token<'input>) -> Result<(), String> {
        if let Some((_, ref t, _)) = self.peek {
            if *t == expected {
                self.advance();
                return Ok(());
            }
        }
        Err(format!("expected {:?}, got {:?}", expected, self.peek_token()))
    }

    #[allow(dead_code)]
    fn expect_any(&mut self, expected: &[Token<'input>]) -> Result<(), String> {
        if let Some((_, ref t, _)) = self.peek {
            if expected.contains(t) {
                self.advance();
                return Ok(());
            }
        }
        Err(format!("expected one of {:?}, got {:?}", expected, self.peek_token()))
    }

    fn loc(&self) -> Location {
        Location::new(
            Position::new(Some(self.filename.clone()), 1, 1),
            Position::new(Some(self.filename.clone()), 1, 1),
        )
    }

    fn mk_id(&self, name: &str) -> Identifier {
        Identifier::new(name.to_string(), self.loc())
    }

    // -----------------------------------------------------------------------
    // Entry point
    // -----------------------------------------------------------------------

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let block = self.parse_block()?;
        Ok(block)
    }

    // -----------------------------------------------------------------------
    // Expression parsing (precedence climbing)
    // -----------------------------------------------------------------------

    fn parse_expr(&mut self) -> Result<Box<Expression>, String> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Box<Expression>, String> {
        let mut left = self.parse_and_expr()?;
        while matches!(self.peek_token(), Some(Token::Or)) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Box::new(Expression::LogicalExpression(
                LogicalExpression::new(*left, LogicalOperator::Or, *right, self.loc())
            ));
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Box<Expression>, String> {
        let mut left = self.parse_cmp_expr()?;
        while matches!(self.peek_token(), Some(Token::And)) {
            self.advance();
            let right = self.parse_cmp_expr()?;
            left = Box::new(Expression::LogicalExpression(
                LogicalExpression::new(*left, LogicalOperator::And, *right, self.loc())
            ));
        }
        Ok(left)
    }

    fn parse_cmp_expr(&mut self) -> Result<Box<Expression>, String> {
        let left = self.parse_add_expr()?;

        let op = match self.peek_token() {
            Some(Token::EqualEqual) => {
                self.advance();
                ComparisonOperator::Equal
            }
            Some(Token::NotEqual) => {
                self.advance();
                ComparisonOperator::NotEqual
            }
            Some(Token::Less) => {
                self.advance();
                ComparisonOperator::Less
            }
            Some(Token::LessEqual) => {
                self.advance();
                ComparisonOperator::LessEqual
            }
            Some(Token::Greater) => {
                self.advance();
                ComparisonOperator::Greater
            }
            Some(Token::GreaterEqual) => {
                self.advance();
                ComparisonOperator::GreaterEqual
            }
            _ => return Ok(left),
        };

        let right = self.parse_add_expr()?;
        let mut cmp = Comparison::new(*left, op, *right, self.loc());

        // Chained comparisons
        loop {
            let next_op = match self.peek_token() {
                Some(Token::EqualEqual) => {
                    self.advance();
                    ComparisonOperator::Equal
                }
                Some(Token::NotEqual) => {
                    self.advance();
                    ComparisonOperator::NotEqual
                }
                Some(Token::Less) => {
                    self.advance();
                    ComparisonOperator::Less
                }
                Some(Token::LessEqual) => {
                    self.advance();
                    ComparisonOperator::LessEqual
                }
                Some(Token::Greater) => {
                    self.advance();
                    ComparisonOperator::Greater
                }
                Some(Token::GreaterEqual) => {
                    self.advance();
                    ComparisonOperator::GreaterEqual
                }
                _ => break,
            };
            let next_right = self.parse_add_expr()?;
            cmp.add_comparison(next_op, *next_right);
        }

        Ok(Box::new(Expression::Comparison(cmp)))
    }

    fn parse_add_expr(&mut self) -> Result<Box<Expression>, String> {
        let mut left = self.parse_mul_expr()?;
        loop {
            let op = match self.peek_token() {
                Some(Token::Plus) => {
                    self.advance();
                    BinaryOperator::Plus
                }
                Some(Token::Minus) => {
                    self.advance();
                    BinaryOperator::Minus
                }
                _ => break,
            };
            let right = self.parse_mul_expr()?;
            left = Box::new(Expression::BinaryExpression(
                BinaryExpression::new(*left, op, *right, self.loc())
            ));
        }
        Ok(left)
    }

    fn parse_mul_expr(&mut self) -> Result<Box<Expression>, String> {
        let mut left = self.parse_pow_expr()?;
        loop {
            let op = match self.peek_token() {
                Some(Token::Star) => {
                    self.advance();
                    BinaryOperator::Multiply
                }
                Some(Token::Slash) => {
                    self.advance();
                    BinaryOperator::Divide
                }
                Some(Token::Percent) => {
                    self.advance();
                    BinaryOperator::Modulo
                }
                _ => break,
            };
            let right = self.parse_pow_expr()?;
            left = Box::new(Expression::BinaryExpression(
                BinaryExpression::new(*left, op, *right, self.loc())
            ));
        }
        Ok(left)
    }

    fn parse_pow_expr(&mut self) -> Result<Box<Expression>, String> {
        let left = self.parse_unary_expr()?;
        if matches!(self.peek_token(), Some(Token::Caret)) {
            self.advance();
            let right = self.parse_pow_expr()?; // right-associative
            Ok(Box::new(Expression::BinaryExpression(
                BinaryExpression::new(*left, BinaryOperator::Power, *right, self.loc())
            )))
        } else {
            Ok(left)
        }
    }

    fn parse_unary_expr(&mut self) -> Result<Box<Expression>, String> {
        match self.peek_token() {
            Some(Token::Minus) => {
                self.advance();
                let expr = self.parse_unary_expr()?;
                Ok(Box::new(Expression::UnaryExpression(
                    UnaryExpression::new(UnaryOperator::Minus, *expr, self.loc())
                )))
            }
            Some(Token::Plus) => {
                self.advance();
                let expr = self.parse_unary_expr()?;
                Ok(Box::new(Expression::UnaryExpression(
                    UnaryExpression::new(UnaryOperator::Plus, *expr, self.loc())
                )))
            }
            Some(Token::Not) => {
                self.advance();
                let expr = self.parse_unary_expr()?;
                Ok(Box::new(Expression::UnaryExpression(
                    UnaryExpression::new(UnaryOperator::Not, *expr, self.loc())
                )))
            }
            _ => self.parse_primary_expr(),
        }
    }

    fn parse_variable(&mut self) -> Result<Variable, String> {
        let name = if let Some(Token::Ident(name)) = self.peek_token().cloned() {
            self.advance();
            name
        } else {
            return Err("expected identifier".to_string());
        };
        let mut var = Variable::Identifier(self.mk_id(name));
        while matches!(self.peek_token(), Some(Token::LBracket)) {
            self.advance();
            let index = *self.parse_expr()?;
            self.expect(Token::RBracket)?;
            var = Variable::IndexExpression(
                IndexExpression::new(var, index, self.loc())
            );
        }
        Ok(var)
    }

    fn parse_primary_expr(&mut self) -> Result<Box<Expression>, String> {
        match self.peek_token().cloned() {
            Some(Token::If) => self.parse_if_expr(),
            Some(Token::Fn) => self.parse_anon_fn(),
            Some(Token::LParen) => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            Some(Token::LBracket) => self.parse_array_lit(),
            Some(Token::Nil) => {
                self.advance();
                Ok(Box::new(Expression::Literal(
                    Literal::Nil(Nil::new(self.loc()))
                )))
            }
            Some(Token::True) => {
                self.advance();
                Ok(Box::new(Expression::Literal(
                    Literal::Boolean(Boolean::new(true, self.loc()))
                )))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Box::new(Expression::Literal(
                    Literal::Boolean(Boolean::new(false, self.loc()))
                )))
            }
            Some(Token::Number(n)) => {
                self.advance();
                if matches!(self.peek_token(), Some(Token::Dot)) {
                    self.advance();
                    if let Some(Token::Number(f)) = self.peek_token().cloned() {
                        self.advance();
                        let val = format!("{}.{}", n, f).parse::<f64>().unwrap();
                        Ok(Box::new(Expression::Literal(
                            Literal::Float(Float::new(val, self.loc()))
                        )))
                    } else {
                        Ok(Box::new(Expression::Literal(
                            Literal::Float(Float::new(n as f64, self.loc()))
                        )))
                    }
                } else {
                    Ok(Box::new(Expression::Literal(
                        Literal::Number(Number::new(n, self.loc()))
                    )))
                }
            }
            Some(Token::Str(s)) => {
                let unescaped = unescape_string(s);
                self.advance();
                Ok(Box::new(Expression::Literal(
                    Literal::String(StringLiteral::new(unescaped, self.loc()))
                )))
            }
            Some(Token::Ident(_)) => {
                let var = self.parse_variable()?;
                if matches!(self.peek_token(), Some(Token::LParen)) {
                    let (name, _) = match &var {
                        Variable::Identifier(id) => (id.name.clone(), true),
                        _ => (String::new(), false),
                    };
                    if matches!(var, Variable::Identifier(_)) {
                        let args = self.parse_call_args()?;
                        Ok(Box::new(Expression::FunctionCall(
                            FunctionCall::new(self.mk_id(&name), args, self.loc())
                        )))
                    } else {
                        Err("cannot call indexed expression".to_string())
                    }
                } else {
                    Ok(Box::new(Expression::Variable(var)))
                }
            }
            Some(Token::Return) => {
                Err("unexpected return in expression context".to_string())
            }
            _ => Err(format!(
                "unexpected token {:?} in expression",
                self.peek_token()
            )),
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expression>, String> {
        self.expect(Token::LParen)?;
        let mut args = Vec::new();
        if !matches!(self.peek_token(), Some(Token::RParen)) {
            args.push(*self.parse_expr()?);
            while matches!(self.peek_token(), Some(Token::Comma)) {
                self.advance();
                if matches!(self.peek_token(), Some(Token::RParen)) {
                    break;
                }
                args.push(*self.parse_expr()?);
            }
        }
        self.expect(Token::RParen)?;
        Ok(args)
    }

    fn parse_array_lit(&mut self) -> Result<Box<Expression>, String> {
        self.expect(Token::LBracket)?;
        let mut elements = Vec::new();
        if !matches!(self.peek_token(), Some(Token::RBracket)) {
            elements.push(*self.parse_expr()?);
            while matches!(self.peek_token(), Some(Token::Comma)) {
                self.advance();
                if matches!(self.peek_token(), Some(Token::RBracket)) {
                    break;
                }
                elements.push(*self.parse_expr()?);
            }
        }
        self.expect(Token::RBracket)?;
        Ok(Box::new(Expression::Literal(
            Literal::Array(Array::new(elements, self.loc()))
        )))
    }

    fn parse_if_expr(&mut self) -> Result<Box<Expression>, String> {
        self.expect(Token::If)?;
        let cond = *self.parse_expr()?;
        self.expect(Token::Then)?;
        let body = self.parse_block()?;

        let mut elifs = Vec::new();
        while matches!(self.peek_token(), Some(Token::Elif)) {
            self.advance();
            let elif_cond = *self.parse_expr()?;
            self.expect(Token::Then)?;
            let elif_body = self.parse_block()?;
            elifs.push(IfBase::new(elif_cond, elif_body, self.loc()));
        }

        self.expect(Token::Else)?;
        let else_block = self.parse_block()?;
        self.expect(Token::End)?;

        Ok(Box::new(Expression::IfExpression(
            IfExpression::new(
                IfBase::new(cond, body, self.loc()),
                elifs,
                else_block,
                self.loc(),
            )
        )))
    }

    fn parse_anon_fn(&mut self) -> Result<Box<Expression>, String> {
        self.expect(Token::Fn)?;
        let params = self.parse_params()?;
        let body = self.parse_block()?;
        self.expect(Token::End)?;
        Ok(Box::new(Expression::AnonymousFunction(
            AnonymousFunction::new(
                FunctionBody::new(params, body, self.loc()),
                self.loc(),
            )
        )))
    }

    fn parse_params(&mut self) -> Result<Vec<Identifier>, String> {
        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if matches!(self.peek_token(), Some(Token::Ident(_))) {
            if let Some(Token::Ident(name)) = self.peek_token().cloned() {
                self.advance();
                params.push(self.mk_id(name));
                while matches!(self.peek_token(), Some(Token::Comma)) {
                    self.advance();
                    if let Some(Token::Ident(name)) = self.peek_token().cloned() {
                        self.advance();
                        params.push(self.mk_id(name));
                    } else {
                        return Err("expected identifier".to_string());
                    }
                }
            }
        }
        self.expect(Token::RParen)?;
        Ok(params)
    }

    // -----------------------------------------------------------------------
    // Statement parsing
    // -----------------------------------------------------------------------

    fn parse_statement(&mut self) -> Result<Statement, String> {
        match self.peek_token().cloned() {
            Some(Token::Let) => self.parse_declaration(),
            Some(Token::If) => self.parse_if_stmt(),
            Some(Token::While) => self.parse_while(),
            Some(Token::For) => self.parse_for(),
            Some(Token::Fn) => self.parse_fn_def(),
            Some(Token::Return) => unreachable!(), // handled by parse_last_statement
            Some(Token::Break) => unreachable!(),
            Some(Token::Continue) => unreachable!(),
            Some(Token::Ident(_)) => {
                // Could be assignment or function call
                self.parse_ident_stmt()
            }
            _ => Err(format!(
                "unexpected token {:?} starting statement",
                self.peek_token()
            )),
        }
    }

    fn parse_declaration(&mut self) -> Result<Statement, String> {
        self.expect(Token::Let)?;
        let mutability = if matches!(self.peek_token(), Some(Token::Mut)) {
            self.advance();
            Mutability::Mutable
        } else {
            Mutability::Immutable
        };

        let mut names = Vec::new();
        if let Some(Token::Ident(name)) = self.peek_token().cloned() {
            self.advance();
            names.push(self.mk_id(name));
            while matches!(self.peek_token(), Some(Token::Comma)) {
                self.advance();
                if let Some(Token::Ident(name)) = self.peek_token().cloned() {
                    self.advance();
                    names.push(self.mk_id(name));
                } else {
                    return Err("expected identifier in name list".to_string());
                }
            }
        } else {
            return Err("expected identifier after let".to_string());
        }

        let init = if matches!(self.peek_token(), Some(Token::Equal)) {
            self.advance();
            Some(*self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Declaration(
            Declaration::new(names, init, mutability, self.loc())
        ))
    }

    fn parse_ident_stmt(&mut self) -> Result<Statement, String> {
        let var = self.parse_variable()?;
        let is_simple_ident = matches!(var, Variable::Identifier(_));

        match self.peek_token() {
            Some(Token::Equal)
            | Some(Token::PlusEqual)
            | Some(Token::MinusEqual)
            | Some(Token::StarEqual)
            | Some(Token::SlashEqual)
            | Some(Token::PercentEqual)
            | Some(Token::CaretEqual) => {
                let op = match self.peek_token().unwrap() {
                    Token::Equal => AssignmentOperator::Assign,
                    Token::PlusEqual => AssignmentOperator::Plus,
                    Token::MinusEqual => AssignmentOperator::Minus,
                    Token::StarEqual => AssignmentOperator::Multiply,
                    Token::SlashEqual => AssignmentOperator::Divide,
                    Token::PercentEqual => AssignmentOperator::Modulo,
                    Token::CaretEqual => AssignmentOperator::Power,
                    _ => unreachable!(),
                };
                self.advance();
                let expr = *self.parse_expr()?;
                if op == AssignmentOperator::Assign && is_simple_ident {
                    let name = match var {
                        Variable::Identifier(id) => id,
                        _ => unreachable!(),
                    };
                    Ok(Statement::NameAssignment(
                        NameAssignment::new(vec![name], expr, self.loc())
                    ))
                } else {
                    Ok(Statement::OperatorAssignment(
                        OperatorAssignment::new(var, op, expr, self.loc())
                    ))
                }
            }
            Some(Token::LParen) => {
                if is_simple_ident {
                    let name = match var {
                        Variable::Identifier(id) => id,
                        _ => unreachable!(),
                    };
                    let args = self.parse_call_args()?;
                    Ok(Statement::FunctionCall(
                        FunctionCall::new(name, args, self.loc())
                    ))
                } else {
                    Err("cannot call indexed expression".to_string())
                }
            }
            _ => {
                Err(format!("unexpected token after identifier: {:?}", self.peek_token()))
            }
        }
    }

    fn parse_if_stmt(&mut self) -> Result<Statement, String> {
        self.expect(Token::If)?;
        let cond = *self.parse_expr()?;
        self.expect(Token::Then)?;
        let body = self.parse_block()?;

        let mut elifs = Vec::new();
        while matches!(self.peek_token(), Some(Token::Elif)) {
            self.advance();
            let elif_cond = *self.parse_expr()?;
            self.expect(Token::Then)?;
            let elif_body = self.parse_block()?;
            elifs.push(IfBase::new(elif_cond, elif_body, self.loc()));
        }

        let else_block = if matches!(self.peek_token(), Some(Token::Else)) {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };

        self.expect(Token::End)?;

        Ok(Statement::IfStatement(
            IfStatement::new(
                IfBase::new(cond, body, self.loc()),
                elifs,
                else_block,
                self.loc(),
            )
        ))
    }

    fn parse_while(&mut self) -> Result<Statement, String> {
        self.expect(Token::While)?;
        let cond = *self.parse_expr()?;
        self.expect(Token::Do)?;
        let body = self.parse_block()?;
        self.expect(Token::End)?;
        Ok(Statement::While(
            While::new(cond, body, self.loc())
        ))
    }

    fn parse_for(&mut self) -> Result<Statement, String> {
        self.expect(Token::For)?;
        let mutability = if matches!(self.peek_token(), Some(Token::Mut)) {
            self.advance();
            Mutability::Mutable
        } else {
            Mutability::Immutable
        };

        let var = if let Some(Token::Ident(name)) = self.peek_token().cloned() {
            self.advance();
            self.mk_id(name)
        } else {
            return Err("expected identifier after for".to_string());
        };

        self.expect(Token::In)?;

        // Check if it's a range for loop (start .. end) or for-in loop
        let start = *self.parse_expr()?;

        if matches!(self.peek_token(), Some(Token::DotDot) | Some(Token::DotDotEqual)) {
            let range_op = match self.peek_token() {
                Some(Token::DotDot) => { self.advance(); RangeOperator::Exclusive }
                Some(Token::DotDotEqual) => { self.advance(); RangeOperator::Inclusive }
                _ => unreachable!(),
            };
            let end = *self.parse_expr()?;
            let step = if matches!(self.peek_token(), Some(Token::Step)) {
                self.advance();
                Some(*self.parse_expr()?)
            } else {
                None
            };
            self.expect(Token::Do)?;
            let body = self.parse_block()?;
            self.expect(Token::End)?;
            Ok(Statement::RangeForLoop(
                RangeForLoop::new(
                    var, start, end, step, body, range_op, mutability, self.loc(),
                )
            ))
        } else {
            self.expect(Token::Do)?;
            let body = self.parse_block()?;
            self.expect(Token::End)?;
            Ok(Statement::ForLoop(
                ForLoop::new(var, start, body, mutability, self.loc())
            ))
        }
    }

    fn parse_fn_def(&mut self) -> Result<Statement, String> {
        self.expect(Token::Fn)?;
        let name = if let Some(Token::Ident(name)) = self.peek_token().cloned() {
            self.advance();
            self.mk_id(name)
        } else {
            return Err("expected function name after fn".to_string());
        };

        let params = self.parse_params()?;
        let body = self.parse_block()?;
        self.expect(Token::End)?;

        Ok(Statement::NamedFunction(
            NamedFunction::new(
                name,
                FunctionBody::new(params, body, self.loc()),
                self.loc(),
            )
        ))
    }

    // -----------------------------------------------------------------------
    // Last statement (return, break, continue)
    // -----------------------------------------------------------------------

    fn parse_last_statement(&mut self) -> Result<LastStatement, String> {
        match self.peek_token() {
            Some(Token::Return) => {
                self.advance();
                let expr = if !matches!(self.peek_token(), Some(Token::End) | Some(Token::Else) | Some(Token::Elif)) {
                    Some(*self.parse_expr()?)
                } else {
                    None
                };
                Ok(LastStatement::Return(
                    ReturnStatement::new(expr, self.loc())
                ))
            }
            Some(Token::Break) => {
                self.advance();
                Ok(LastStatement::Break(
                    BreakStatement::new(self.loc())
                ))
            }
            Some(Token::Continue) => {
                self.advance();
                Ok(LastStatement::Continue(
                    ContinueStatement::new(self.loc())
                ))
            }
            _ => Err("expected return, break, or continue".to_string()),
        }
    }

    // -----------------------------------------------------------------------
    // Block parsing
    // -----------------------------------------------------------------------

    fn parse_block(&mut self) -> Result<Block, String> {
        let mut block = Block::default();

        loop {
            match self.peek_token() {
                Some(Token::End) | None => break,
                Some(Token::Else) | Some(Token::Elif) => break,
                Some(Token::Return) | Some(Token::Break) | Some(Token::Continue) => {
                    let last = self.parse_last_statement()?;
                    block = block.with_last(last);
                    // Optional semicolon
                    if matches!(self.peek_token(), Some(Token::Semi)) {
                        self.advance();
                    }
                    break;
                }
                _ => {
                    let stmt = self.parse_statement()?;
                    block = block.with_statement(stmt);
                    // Optional semicolon
                    if matches!(self.peek_token(), Some(Token::Semi)) {
                        self.advance();
                    }
                }
            }
        }

        Ok(block)
    }
}

fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(code) = u8::from_str_radix(&hex, 16) {
                        result.push(code as char);
                    }
                }
                Some(c) => result.push(c),
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}
