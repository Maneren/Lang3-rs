use std::io::{Read, Write};

use l3_ast::{ast_printer, dot_printer};
use l3_bytecode::{format as bytecode_fmt, optimizer::Optimizer};
use l3_compiler::Compiler;
use l3_parser::parse_program;
use l3_vm::BytecodeVM;

fn run(
    source: &str,
    filename: &str,
    writer: &mut impl Write,
    reader: &mut impl Read,
    optimize: bool,
) -> Result<(), String> {
    let program = parse_program(source, filename).map_err(|e| format!("Parse error: {e}"))?;

    let mut compiler = Compiler::new();
    let bytecode = compiler
        .compile(&program)
        .map_err(|e| format!("Compile error: {e}"))?;

    let mut vm = BytecodeVM::new(writer, reader, false);
    let result = if optimize {
        vm.execute(&Optimizer::new().optimize(bytecode.clone()))
    } else {
        vm.execute(bytecode)
    };
    if let Err(e) = result {
        let _ = writeln!(vm.heap.output, "{e}");
    }
    Ok(())
}

pub fn run_pipeline(
    source: &str,
    filename: &str,
    writer: &mut impl Write,
    reader: &mut impl Read,
) -> Result<(), String> {
    run(source, filename, writer, reader, false)
}

pub fn run_pipeline_optimized(
    source: &str,
    filename: &str,
    writer: &mut impl Write,
    reader: &mut impl Read,
) -> Result<(), String> {
    run(source, filename, writer, reader, true)
}

pub fn format_ast(source: &str, filename: &str) -> Result<String, String> {
    let program = parse_program(source, filename).map_err(|e| format!("Parse error: {e}"))?;
    Ok(ast_printer::format_ast(&program))
}

pub fn format_bytecode(source: &str, filename: &str) -> Result<String, String> {
    let program = parse_program(source, filename).map_err(|e| format!("Parse error: {e}"))?;
    let mut compiler = Compiler::new();
    let bytecode = compiler
        .compile(&program)
        .map_err(|e| format!("Compile error: {e}"))?;
    Ok(bytecode_fmt::format_bytecode(bytecode))
}

pub fn format_bytecode_optimized(source: &str, filename: &str) -> Result<String, String> {
    let program = parse_program(source, filename).map_err(|e| format!("Parse error: {e}"))?;
    let mut compiler = Compiler::new();
    let bytecode = compiler
        .compile(&program)
        .map_err(|e| format!("Compile error: {e}"))?;
    Ok(bytecode_fmt::format_bytecode(
        &Optimizer::new().optimize(bytecode.clone()),
    ))
}

pub fn format_ast_graph(source: &str, filename: &str) -> Result<String, String> {
    let program = parse_program(source, filename).map_err(|e| format!("Parse error: {e}"))?;
    Ok(dot_printer::format_ast_graph(&program))
}
