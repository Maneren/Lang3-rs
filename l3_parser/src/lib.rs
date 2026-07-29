pub mod lexer;
pub mod loc_util;
pub mod token;

lalrpop_util::lalrpop_mod!(pub grammar);

use l3_ast::Program;

pub use token::Token;

pub fn parse_program(source: &str, filename: &str) -> Result<Program, String> {
    let lexer = lexer::Lexer::new(source);
    grammar::ProgramParser::new()
        .parse(source, filename, lexer)
        .map_err(|e| format!("Parse error: {e}"))
}
