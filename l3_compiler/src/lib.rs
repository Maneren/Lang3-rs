use error::CompileError;
use l3_ast::Program;
use l3_bytecode::{Instruction, ProgramBytecode};
use l3_location::Location;

mod error;

mod alias;
mod compile;
mod context;
mod fold;
pub mod optimizer;

use context::{Context, LoopContext};

pub struct Compiler {
    program: ProgramBytecode,
    contexts: Vec<Context>,
    loop_contexts: Vec<LoopContext>,
    synthetic_counter: usize,
    location_stack: Vec<Location>,
}

impl Compiler {
    #[must_use]
    pub const fn new() -> Self {
        let program = ProgramBytecode::new();
        Self {
            program,
            contexts: Vec::new(),
            loop_contexts: Vec::new(),
            synthetic_counter: 0,
            location_stack: Vec::new(),
        }
    }

    pub fn compile(&mut self, ast: &Program) -> Result<&ProgramBytecode, CompileError> {
        self.push_context();
        self.compile_block(ast)?;
        self.emit(Instruction::Return);
        Ok(&self.program)
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use l3_parser::parse_program;

    use super::Compiler;

    fn compile_error(source: &str) -> String {
        let program = parse_program(source, "<test>").expect("source parses");
        Compiler::new()
            .compile(&program)
            .expect_err("compilation must fail")
            .to_string()
    }

    fn assert_compile_error(source: &str, expected: &str) {
        assert_eq!(
            compile_error(source),
            expected,
            "wrong compile error for: {source}"
        );
    }

    #[test]
    fn break_outside_loop_is_a_compile_error() {
        assert_compile_error(
            "break\n",
            "CompileError: break outside of a loop is not allowed",
        );
    }

    #[test]
    fn continue_outside_loop_is_a_compile_error() {
        assert_compile_error(
            "continue\n",
            "CompileError: continue outside of a loop is not allowed",
        );
    }

    #[test]
    fn break_inside_function_defined_in_loop_is_a_compile_error() {
        assert_compile_error(
            "while true do\n  let f = fn() break end\nend\n",
            "CompileError: break outside of a loop is not allowed",
        );
    }

    #[test]
    fn continue_inside_function_defined_in_loop_is_a_compile_error() {
        assert_compile_error(
            "while true do\n  let f = fn() continue end\nend\n",
            "CompileError: continue outside of a loop is not allowed",
        );
    }

    #[test]
    fn assignment_to_immutable_binding_is_a_compile_error() {
        assert_compile_error(
            "let x = 1\nx = 2\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn op_assignment_to_immutable_binding_is_a_compile_error() {
        assert_compile_error(
            "let x = 1\nx += 2\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn assignment_to_immutable_loop_variable_is_a_compile_error() {
        assert_compile_error(
            "for x in [1, 2] do\n  x = 1\nend\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn assignment_to_immutable_captured_binding_is_a_compile_error() {
        assert_compile_error(
            "let x = 1\nlet f = fn() x = 2 end\n",
            "CompileError: cannot assign to immutable binding `x`",
        );
    }

    #[test]
    fn mutable_bindings_compile() {
        for source in [
            "let mut x = 1\nx = 2\n",
            "let mut x = 1\nx += 2\n",
            "for mut x in [1, 2] do\n  x = 1\nend\n",
            "for mut i in 0..3 do\n  i += 1\nend\n",
        ] {
            let program = parse_program(source, "<test>").expect("source parses");
            Compiler::new()
                .compile(&program)
                .unwrap_or_else(|e| panic!("mutable binding rejected: {source}: {e}"));
        }
    }
}
