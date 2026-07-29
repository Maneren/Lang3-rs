use crate::token::Token;
use logos::Logos;

pub struct Lexer<'input> {
    inner: logos::Lexer<'input, Token<'input>>,
}

impl<'input> Lexer<'input> {
    #[must_use]
    pub fn new(input: &'input str) -> Self {
        Self {
            inner: Token::lexer(input),
        }
    }
}

impl<'input> Iterator for Lexer<'input> {
    type Item = Result<(usize, Token<'input>, usize), &'static str>;

    fn next(&mut self) -> Option<Self::Item> {
        let tok = self.inner.next()?;
        let span = self.inner.span();
        match tok {
            Ok(Token::Error) => Some(Err("unexpected character")),
            Ok(tok) => Some(Ok((span.start, tok, span.end))),
            Err(()) => Some(Err("lexer error")),
        }
    }
}
