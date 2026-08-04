use std::fmt;

use logos::Logos;

fn extract_string<'input>(lex: &logos::Lexer<'input, Token<'input>>) -> &'input str {
    let slice = lex.slice();
    &slice[1..slice.len() - 1]
}

#[derive(Logos, Debug, Clone, Copy, PartialEq)]
pub enum Token<'input> {
    // Keywords (exact match has priority over ident regex)
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("elif")]
    Elif,
    #[token("then")]
    Then,
    #[token("end")]
    End,
    #[token("while")]
    While,
    #[token("do")]
    Do,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("step")]
    Step,
    #[token("return")]
    Return,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("fn")]
    Fn,
    #[token("let")]
    Let,
    #[token("mut")]
    Mut,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("nil")]
    Nil,
    #[token("not")]
    Not,
    #[token("and")]
    And,
    #[token("or")]
    Or,

    // Punctuation
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token(".")]
    Dot,

    // Arithmetic operators
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("^")]
    Caret,

    // Range operators (must precede ".." for longest-match)
    #[token("..=")]
    DotDotEqual,
    #[token("..")]
    DotDot,

    // Comparison
    #[token("==")]
    EqualEqual,
    #[token("!=")]
    NotEqual,
    #[token("<=")]
    LessEqual,
    #[token(">=")]
    GreaterEqual,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,

    // Assignment (single "=" must come after "==" and "<=" and ">=" for longest-match)
    #[token("=")]
    Equal,
    #[token("+=")]
    PlusEqual,
    #[token("-=")]
    MinusEqual,
    #[token("*=")]
    StarEqual,
    #[token("/=")]
    SlashEqual,
    #[token("%=")]
    PercentEqual,
    #[token("^=")]
    CaretEqual,

    // Literals
    #[regex("[0-9]+", |lex| lex.slice().parse::<i64>().unwrap())]
    Number(i64),
    #[regex("[0-9]+\\.[0-9]+", |lex| lex.slice().parse::<f64>().unwrap())]
    Float(f64),
    #[regex(r#""[^"\\]*(\\.[^"\\]*)*""#, extract_string)]
    Str(&'input str),
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident(&'input str),

    // Whitespace and comments (skipped)
    #[regex(r"[ \t\n\r\f]+", logos::skip)]
    #[regex("#[^\n]*", logos::skip)]
    Error,
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::Elif => write!(f, "elif"),
            Token::Then => write!(f, "then"),
            Token::End => write!(f, "end"),
            Token::While => write!(f, "while"),
            Token::Do => write!(f, "do"),
            Token::For => write!(f, "for"),
            Token::In => write!(f, "in"),
            Token::Step => write!(f, "step"),
            Token::Return => write!(f, "return"),
            Token::Break => write!(f, "break"),
            Token::Continue => write!(f, "continue"),
            Token::Fn => write!(f, "fn"),
            Token::Let => write!(f, "let"),
            Token::Mut => write!(f, "mut"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::Nil => write!(f, "nil"),
            Token::Not => write!(f, "not"),
            Token::And => write!(f, "and"),
            Token::Or => write!(f, "or"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Semi => write!(f, ";"),
            Token::Dot => write!(f, "."),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Caret => write!(f, "^"),
            Token::DotDot => write!(f, ".."),
            Token::DotDotEqual => write!(f, "..="),
            Token::EqualEqual => write!(f, "=="),
            Token::NotEqual => write!(f, "!="),
            Token::Less => write!(f, "<"),
            Token::LessEqual => write!(f, "<="),
            Token::Greater => write!(f, ">"),
            Token::GreaterEqual => write!(f, ">="),
            Token::Equal => write!(f, "="),
            Token::PlusEqual => write!(f, "+="),
            Token::MinusEqual => write!(f, "-="),
            Token::StarEqual => write!(f, "*="),
            Token::SlashEqual => write!(f, "/="),
            Token::PercentEqual => write!(f, "%="),
            Token::CaretEqual => write!(f, "^="),
            Token::Number(n) => write!(f, "{n}"),
            Token::Float(n) => write!(f, "{n}"),
            Token::Str(s) => write!(f, "\"{s}\""),
            Token::Ident(s) => write!(f, "{s}"),
            Token::Error => write!(f, "<error>"),
        }
    }
}
