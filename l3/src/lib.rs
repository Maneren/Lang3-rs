use l3_parser::parse_program;
use l3_compiler::Compiler;
use l3_vm::BytecodeVM;

pub fn run_pipeline(source: &str, filename: &str) -> Result<Vec<String>, String> {
    let program = parse_program(source, filename).map_err(|e| format!("Parse error: {}", e))?;

    let mut compiler = Compiler::new();
    let bytecode = compiler
        .compile(&program)
        .map_err(|e| format!("Compile error: {}", e))?;

    let mut vm = BytecodeVM::new(false);
    if let Err(e) = vm.execute(bytecode) {
        let mut lines = std::mem::take(&mut vm.heap.output_lines);
        lines.push(format!("RuntimeError: {}", e));
        return Ok(lines);
    }

    Ok(std::mem::take(&mut vm.heap.output_lines))
}
