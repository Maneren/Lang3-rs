pub mod ast_printer;
pub mod dot_printer;

use std::fmt;

use l3_location::Location;

// ---------------------------------------------------------------------------
// Operators
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOperator {
    Assign,
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    Immutable,
    Mutable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOperator {
    Inclusive,
    Exclusive,
}

// ---------------------------------------------------------------------------
// Identifier
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier {
    pub name: String,
    pub location: Location,
}

impl Identifier {
    pub fn new(name: impl Into<String>, location: Location) -> Self {
        Self {
            name: name.into(),
            location,
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

// ---------------------------------------------------------------------------
// Literals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Nil {
    pub location: Location,
}

impl Nil {
    #[must_use]
    pub const fn new(location: Location) -> Self {
        Self { location }
    }
}

#[derive(Debug, Clone)]
pub struct Boolean {
    pub value: bool,
    pub location: Location,
}

impl Boolean {
    #[must_use]
    pub const fn new(value: bool, location: Location) -> Self {
        Self { value, location }
    }
}

#[derive(Debug, Clone)]
pub struct Number {
    pub value: i64,
    pub location: Location,
}

impl Number {
    #[must_use]
    pub const fn new(value: i64, location: Location) -> Self {
        Self { value, location }
    }
}

#[derive(Debug, Clone)]
pub struct Float {
    pub value: f64,
    pub location: Location,
}

impl Float {
    #[must_use]
    pub const fn new(value: f64, location: Location) -> Self {
        Self { value, location }
    }
}

#[derive(Debug, Clone)]
pub struct StringLiteral {
    pub value: String,
    pub location: Location,
}

impl StringLiteral {
    pub fn new(value: impl Into<String>, location: Location) -> Self {
        Self {
            value: value.into(),
            location,
        }
    }
}

// ---------------------------------------------------------------------------
// Forward declarations for recursive types
// ---------------------------------------------------------------------------

pub type ExpressionList = Vec<Expression>;
pub type NameList = Vec<Identifier>;

// ---------------------------------------------------------------------------
// Expression types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Array {
    pub elements: ExpressionList,
    pub location: Location,
}

impl Array {
    #[must_use]
    pub const fn new(elements: ExpressionList, location: Location) -> Self {
        Self { elements, location }
    }
}

#[derive(Debug, Clone)]
pub enum Literal {
    Nil(Nil),
    Boolean(Boolean),
    Number(Number),
    Float(Float),
    String(StringLiteral),
    Array(Array),
}

impl Literal {
    #[must_use]
    pub const fn location(&self) -> &Location {
        match self {
            Self::Nil(n) => &n.location,
            Self::Boolean(b) => &b.location,
            Self::Number(n) => &n.location,
            Self::Float(f) => &f.location,
            Self::String(s) => &s.location,
            Self::Array(a) => &a.location,
        }
    }
}

// ---------------------------------------------------------------------------
// Variables
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct IndexExpression {
    pub base: Box<Variable>,
    pub index: Box<Expression>,
    pub location: Location,
}

impl IndexExpression {
    #[must_use]
    pub fn new(base: Variable, index: Expression, location: Location) -> Self {
        Self {
            base: Box::new(base),
            index: Box::new(index),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Variable {
    Identifier(Identifier),
    IndexExpression(IndexExpression),
}

impl Variable {
    #[must_use]
    pub const fn location(&self) -> &Location {
        match self {
            Self::Identifier(i) => &i.location,
            Self::IndexExpression(i) => &i.location,
        }
    }
}

// ---------------------------------------------------------------------------
// Complex Expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct UnaryExpression {
    pub op: UnaryOperator,
    pub expression: Box<Expression>,
    pub location: Location,
}

impl UnaryExpression {
    #[must_use]
    pub fn new(op: UnaryOperator, expression: Expression, location: Location) -> Self {
        Self {
            op,
            expression: Box::new(expression),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BinaryExpression {
    pub lhs: Box<Expression>,
    pub op: BinaryOperator,
    pub rhs: Box<Expression>,
    pub location: Location,
}

impl BinaryExpression {
    #[must_use]
    pub fn new(lhs: Expression, op: BinaryOperator, rhs: Expression, location: Location) -> Self {
        Self {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogicalExpression {
    pub lhs: Box<Expression>,
    pub op: LogicalOperator,
    pub rhs: Box<Expression>,
    pub location: Location,
}

impl LogicalExpression {
    #[must_use]
    pub fn new(lhs: Expression, op: LogicalOperator, rhs: Expression, location: Location) -> Self {
        Self {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Comparison {
    pub start: Box<Expression>,
    pub comparisons: Vec<(ComparisonOperator, Expression)>,
    pub location: Location,
}

impl Comparison {
    #[must_use]
    pub fn new(
        start: Expression,
        op: ComparisonOperator,
        rhs: Expression,
        location: Location,
    ) -> Self {
        Self {
            start: Box::new(start),
            comparisons: vec![(op, rhs)],
            location,
        }
    }

    /// Add another comparison. Returns false if types are mixed (equality vs
    /// inequality).
    pub fn add_comparison(&mut self, op: ComparisonOperator, expr: Expression) -> bool {
        self.comparisons.push((op, expr));
        true
    }
}

#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub name: Identifier,
    pub arguments: ExpressionList,
    pub location: Location,
}

impl FunctionCall {
    #[must_use]
    pub const fn new(name: Identifier, arguments: ExpressionList, location: Location) -> Self {
        Self {
            name,
            arguments,
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FunctionBody {
    pub parameters: NameList,
    pub block: Block,
    pub location: Location,
}

impl FunctionBody {
    #[must_use]
    pub const fn new(parameters: NameList, block: Block, location: Location) -> Self {
        Self {
            parameters,
            block,
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnonymousFunction {
    pub body: FunctionBody,
    pub location: Location,
}

impl AnonymousFunction {
    #[must_use]
    pub const fn new(body: FunctionBody, location: Location) -> Self {
        Self { body, location }
    }
}

#[derive(Debug, Clone)]
pub struct NamedFunction {
    pub name: Identifier,
    pub body: FunctionBody,
    pub location: Location,
}

impl NamedFunction {
    #[must_use]
    pub const fn new(name: Identifier, body: FunctionBody, location: Location) -> Self {
        Self {
            name,
            body,
            location,
        }
    }
}

// ---------------------------------------------------------------------------
// If expressions/statements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct IfBase {
    pub condition: Box<Expression>,
    pub block: Block,
    pub location: Location,
}

impl IfBase {
    #[must_use]
    pub fn new(condition: Expression, block: Block, location: Location) -> Self {
        Self {
            condition: Box::new(condition),
            block,
            location,
        }
    }
}

pub type ElseIfList = Vec<IfBase>;

#[derive(Debug, Clone)]
pub struct IfExpression {
    pub base_if: IfBase,
    pub elseif: ElseIfList,
    pub else_block: Block,
    pub location: Location,
}

impl IfExpression {
    #[must_use]
    pub const fn new(
        base_if: IfBase,
        elseif: ElseIfList,
        else_block: Block,
        location: Location,
    ) -> Self {
        Self {
            base_if,
            elseif,
            else_block,
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IfStatement {
    pub base_if: IfBase,
    pub elseif: ElseIfList,
    pub else_block: Option<Block>,
    pub location: Location,
}

impl IfStatement {
    #[must_use]
    pub const fn new(
        base_if: IfBase,
        elseif: ElseIfList,
        else_block: Option<Block>,
        location: Location,
    ) -> Self {
        Self {
            base_if,
            elseif,
            else_block,
            location,
        }
    }
}

// ---------------------------------------------------------------------------
// Loops
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct While {
    pub condition: Box<Expression>,
    pub body: Block,
    pub location: Location,
}

impl While {
    #[must_use]
    pub fn new(condition: Expression, body: Block, location: Location) -> Self {
        Self {
            condition: Box::new(condition),
            body,
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForLoop {
    pub variable: Identifier,
    pub collection: Box<Expression>,
    pub body: Block,
    pub mutability: Mutability,
    pub location: Location,
}

impl ForLoop {
    #[must_use]
    pub fn new(
        variable: Identifier,
        collection: Expression,
        body: Block,
        mutability: Mutability,
        location: Location,
    ) -> Self {
        Self {
            variable,
            collection: Box::new(collection),
            body,
            mutability,
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RangeForLoop {
    pub variable: Identifier,
    pub start: Box<Expression>,
    pub end: Box<Expression>,
    pub step: Option<Box<Expression>>,
    pub body: Block,
    pub range_type: RangeOperator,
    pub mutability: Mutability,
    pub location: Location,
}

impl RangeForLoop {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        variable: Identifier,
        start: Expression,
        end: Expression,
        step: Option<Expression>,
        body: Block,
        range_type: RangeOperator,
        mutability: Mutability,
        location: Location,
    ) -> Self {
        Self {
            variable,
            start: Box::new(start),
            end: Box::new(end),
            step: step.map(Box::new),
            body,
            range_type,
            mutability,
            location,
        }
    }
}

// ---------------------------------------------------------------------------
// Last statements (return, break, continue)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ReturnStatement {
    pub expression: Option<Box<Expression>>,
    pub location: Location,
}

impl ReturnStatement {
    pub fn new(expression: Option<Expression>, location: Location) -> Self {
        Self {
            expression: expression.map(Box::new),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BreakStatement {
    pub location: Location,
}

impl BreakStatement {
    #[must_use]
    pub const fn new(location: Location) -> Self {
        Self { location }
    }
}

#[derive(Debug, Clone)]
pub struct ContinueStatement {
    pub location: Location,
}

impl ContinueStatement {
    #[must_use]
    pub const fn new(location: Location) -> Self {
        Self { location }
    }
}

#[derive(Debug, Clone)]
pub enum LastStatement {
    Return(ReturnStatement),
    Break(BreakStatement),
    Continue(ContinueStatement),
}

// ---------------------------------------------------------------------------
// Assignments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OperatorAssignment {
    pub variable: Variable,
    pub op: AssignmentOperator,
    pub expression: Box<Expression>,
    pub location: Location,
}

impl OperatorAssignment {
    #[must_use]
    pub fn new(
        variable: Variable,
        op: AssignmentOperator,
        expression: Expression,
        location: Location,
    ) -> Self {
        Self {
            variable,
            op,
            expression: Box::new(expression),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NameAssignment {
    pub names: NameList,
    pub expression: Box<Expression>,
    pub location: Location,
}

impl NameAssignment {
    #[must_use]
    pub fn new(names: NameList, expression: Expression, location: Location) -> Self {
        Self {
            names,
            expression: Box::new(expression),
            location,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Assignment {
    OperatorAssignment(OperatorAssignment),
    NameAssignment(NameAssignment),
}

// ---------------------------------------------------------------------------
// Declaration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Declaration {
    pub names: NameList,
    pub expression: Option<Box<Expression>>,
    pub mutability: Mutability,
    pub location: Location,
}

impl Declaration {
    pub fn new(
        names: NameList,
        expression: Option<Expression>,
        mutability: Mutability,
        location: Location,
    ) -> Self {
        Self {
            names,
            expression: expression.map(Box::new),
            mutability,
            location,
        }
    }

    #[must_use]
    pub const fn is_const(&self) -> bool {
        matches!(self.mutability, Mutability::Immutable)
    }

    #[must_use]
    pub const fn is_mutable(&self) -> bool {
        matches!(self.mutability, Mutability::Mutable)
    }
}

// ---------------------------------------------------------------------------
// Block
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub last_statement: Option<LastStatement>,
}

impl Block {
    #[must_use]
    pub fn with_statement(mut self, stmt: Statement) -> Self {
        self.statements.push(stmt);
        self
    }

    #[must_use]
    pub fn with_last(mut self, last: LastStatement) -> Self {
        self.last_statement = Some(last);
        self
    }
}

// ---------------------------------------------------------------------------
// Program = top-level block
// ---------------------------------------------------------------------------

pub type Program = Block;

// ---------------------------------------------------------------------------
// Expression enum (central)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Expression {
    AnonymousFunction(AnonymousFunction),
    BinaryExpression(BinaryExpression),
    Comparison(Comparison),
    FunctionCall(FunctionCall),
    IfExpression(IfExpression),
    Literal(Literal),
    LogicalExpression(LogicalExpression),
    UnaryExpression(UnaryExpression),
    Variable(Variable),
}

// ---------------------------------------------------------------------------
// Statement enum (central)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Statement {
    Declaration(Declaration),
    ForLoop(ForLoop),
    FunctionCall(FunctionCall),
    IfStatement(IfStatement),
    NameAssignment(NameAssignment),
    NamedFunction(NamedFunction),
    OperatorAssignment(OperatorAssignment),
    RangeForLoop(RangeForLoop),
    While(While),
}

// ---------------------------------------------------------------------------
// Display implementations
// ---------------------------------------------------------------------------

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plus => write!(f, "+"),
            Self::Minus => write!(f, "-"),
            Self::Not => write!(f, "not"),
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plus => write!(f, "+"),
            Self::Minus => write!(f, "-"),
            Self::Multiply => write!(f, "*"),
            Self::Divide => write!(f, "/"),
            Self::Modulo => write!(f, "%"),
            Self::Power => write!(f, "^"),
        }
    }
}

impl fmt::Display for ComparisonOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Equal => write!(f, "=="),
            Self::NotEqual => write!(f, "!="),
            Self::Less => write!(f, "<"),
            Self::LessEqual => write!(f, "<="),
            Self::Greater => write!(f, ">"),
            Self::GreaterEqual => write!(f, ">="),
        }
    }
}

impl fmt::Display for LogicalOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::And => write!(f, "and"),
            Self::Or => write!(f, "or"),
        }
    }
}
