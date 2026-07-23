pub mod token;
pub mod lexer;
pub mod parser;

use l3_ast::Program;
use parser::Parser;
pub use token::Token;

pub fn parse_program(source: &str, filename: &str) -> Result<Program, String> {
    let mut parser = Parser::new(source, filename);
    parser.parse_program()
}
