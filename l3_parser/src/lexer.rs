use crate::token::Token;

pub struct Lexer<'input> {
    input: &'input str,
    pos: usize,
}

impl<'input> Lexer<'input> {
    #[must_use]
    pub fn new(input: &'input str) -> Self {
        Self { input, pos: 0 }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl<'input> Iterator for Lexer<'input> {
    type Item = Result<(usize, Token<'input>, usize), &'static str>;

    fn next(&mut self) -> Option<Self::Item> {
        // Skip whitespace and comments
        loop {
            let remaining = &self.input[self.pos..];
            if remaining.is_empty() {
                return None;
            }
            let c = remaining.chars().next().unwrap();
            if c.is_whitespace() {
                self.pos += c.len_utf8();
                continue;
            }
            if c == '#' {
                // skip to end of line
                self.pos += 1;
                while self.pos < self.input.len() {
                    let c = self.input[self.pos..].chars().next().unwrap();
                    if c == '\n' {
                        self.pos += 1;
                        break;
                    }
                    self.pos += c.len_utf8();
                }
                continue;
            }
            break;
        }

        let start = self.pos;
        let remaining = &self.input[self.pos..];

        // Identifiers and keywords
        if let Some(c) = remaining.chars().next() {
            if is_ident_start(c) {
                self.pos += c.len_utf8();
                while self.pos < self.input.len() {
                    let c = self.input[self.pos..].chars().next().unwrap();
                    if is_ident_continue(c) {
                        self.pos += c.len_utf8();
                    } else {
                        break;
                    }
                }
                let word = &self.input[start..self.pos];
                let token = match word {
                    "if" => Token::If,
                    "else" => Token::Else,
                    "elif" => Token::Elif,
                    "then" => Token::Then,
                    "end" => Token::End,
                    "while" => Token::While,
                    "do" => Token::Do,
                    "for" => Token::For,
                    "in" => Token::In,
                    "step" => Token::Step,
                    "return" => Token::Return,
                    "break" => Token::Break,
                    "continue" => Token::Continue,
                    "fn" => Token::Fn,
                    "let" => Token::Let,
                    "mut" => Token::Mut,
                    "true" => Token::True,
                    "false" => Token::False,
                    "nil" => Token::Nil,
                    "not" => Token::Not,
                    "and" => Token::And,
                    "or" => Token::Or,
                    _ => Token::Ident(word),
                };
                return Some(Ok((start, token, self.pos)));
            }
        }

        // Numbers
        if let Some(c) = remaining.chars().next() {
            if c.is_ascii_digit() {
                self.pos += c.len_utf8();
                while self.pos < self.input.len() {
                    let c = self.input[self.pos..].chars().next().unwrap();
                    if c.is_ascii_digit() {
                        self.pos += c.len_utf8();
                    } else {
                        break;
                    }
                }
                let num: i64 = self.input[start..self.pos].parse().unwrap();
                return Some(Ok((start, Token::Number(num), self.pos)));
            }
        }

        // Strings
        if remaining.starts_with('"') {
            self.pos += 1;
            while self.pos < self.input.len() {
                let c = self.input[self.pos..].chars().next().unwrap();
                self.pos += c.len_utf8();
                if c == '\\' {
                    // skip escaped character
                    if self.pos < self.input.len() {
                        let c = self.input[self.pos..].chars().next().unwrap();
                        self.pos += c.len_utf8();
                    }
                } else if c == '"' {
                    let inner = &self.input[start + 1..self.pos - 1];
                    return Some(Ok((start, Token::Str(inner), self.pos)));
                }
            }
            return Some(Err("unterminated string"));
        }

        // Multi-character operators
        let two = if remaining.len() >= 2 {
            &remaining[..2]
        } else {
            ""
        };
        let token = match two {
            ".." => {
                // Check for ..=
                if remaining.len() >= 3 && &remaining[..3] == "..=" {
                    self.pos += 3;
                    Token::DotDotEqual
                } else {
                    self.pos += 2;
                    Token::DotDot
                }
            }
            "==" => {
                self.pos += 2;
                Token::EqualEqual
            }
            "!=" => {
                self.pos += 2;
                Token::NotEqual
            }
            "<=" => {
                self.pos += 2;
                Token::LessEqual
            }
            ">=" => {
                self.pos += 2;
                Token::GreaterEqual
            }
            "+=" => {
                self.pos += 2;
                Token::PlusEqual
            }
            "-=" => {
                self.pos += 2;
                Token::MinusEqual
            }
            "*=" => {
                self.pos += 2;
                Token::StarEqual
            }
            "/=" => {
                self.pos += 2;
                Token::SlashEqual
            }
            "%=" => {
                self.pos += 2;
                Token::PercentEqual
            }
            "^=" => {
                self.pos += 2;
                Token::CaretEqual
            }
            _ => {
                // Single-character operators and punctuation
                let c = remaining.chars().next().unwrap();
                self.pos += c.len_utf8();
                match c {
                    '(' => Token::LParen,
                    ')' => Token::RParen,
                    '{' => Token::LBrace,
                    '}' => Token::RBrace,
                    '[' => Token::LBracket,
                    ']' => Token::RBracket,
                    ',' => Token::Comma,
                    ';' => Token::Semi,
                    '.' => Token::Dot,
                    '+' => Token::Plus,
                    '-' => Token::Minus,
                    '*' => Token::Star,
                    '/' => Token::Slash,
                    '%' => Token::Percent,
                    '^' => Token::Caret,
                    '~' => Token::Concat,
                    '<' => Token::Less,
                    '>' => Token::Greater,
                    '=' => Token::Equal,
                    _ => {
                        self.pos += 1;
                        Token::Error
                    }
                }
            }
        };

        Some(Ok((start, token, self.pos)))
    }
}
