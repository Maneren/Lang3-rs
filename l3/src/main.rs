use clap::Parser;
use l3_cli::Cli;
use l3_compiler::Compiler;
use l3_parser::parse_program;
use l3_vm::BytecodeVM;
use std::fs;
use std::io::Read;
use std::process;

fn main() {
    let cli = Cli::parse();

    let source = if let Some(ref file) = cli.file {
        match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading file '{file}': {e}");
                process::exit(1);
            }
        }
    } else {
        // Read from stdin
        let mut source = String::new();
        if std::io::stdin().read_to_string(&mut source).is_err() {
            eprintln!("Error reading from stdin");
            process::exit(1);
        }
        source
    };

    let filename = cli.file.clone().unwrap_or_else(|| "<stdin>".to_string());

    // Parse
    let program = match parse_program(&source, &filename) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Parse error: {e}");
            process::exit(1);
        }
    };

    if cli.debug_ast {
        println!("=== AST ===");
        println!("{program:#?}");
    }

    // Compile
    let mut compiler = Compiler::new();
    let bytecode = match compiler.compile(&program) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Compile error: {e}");
            process::exit(1);
        }
    };

    if cli.debug_bytecode {
        println!("=== Bytecode ===");
        for (chunk_id, chunk) in bytecode.chunks.iter().enumerate() {
            println!("Chunk {chunk_id}:");
            for (offset, inst) in chunk.code.iter().enumerate() {
                println!("  {offset:>4}: {inst:?}");
            }
        }
    }

    // Execute
    let mut vm = BytecodeVM::new(cli.debug_vm);
    let result = vm.execute(bytecode);
    vm.heap.flush_print();
    if let Err(e) = result {
        for line in &vm.heap.output_lines {
            println!("{line}");
        }
        eprintln!("Runtime error: {e}");
        process::exit(1);
    }
    for line in &vm.heap.output_lines {
        println!("{line}");
    }
}
