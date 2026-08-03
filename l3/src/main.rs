use clap::Parser;
use l3_ast::{ast_printer, dot_printer};
use l3_bytecode::format as bytecode_fmt;
use l3_bytecode::optimizer::Optimizer;
use l3_cli::Cli;
use l3_compiler::Compiler;
use l3_parser::parse_program;
use l3_vm::BytecodeVM;
use std::fs;
use std::io::Read;
use std::process;
use std::time::Instant;

struct Debug {
    lexer: bool,
    parser: bool,
    ast: bool,
    ast_graph: Option<String>,
    vm: bool,
    bytecode: bool,
    timings: bool,
}

fn read_input(cli: &Cli) -> (String, String) {
    if cli.files.is_empty() || cli.files[0] == "-" {
        let mut source = String::new();
        if std::io::stdin().read_to_string(&mut source).is_err() {
            eprintln!("Error reading from stdin");
            process::exit(1);
        }
        (source, "<stdin>".to_string())
    } else {
        let filename = &cli.files[0];
        match fs::read_to_string(filename) {
            Ok(source) => (source, filename.clone()),
            Err(e) => {
                eprintln!("Error reading file '{filename}': {e}");
                process::exit(1);
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();

    if cli.files.len() > 1 {
        eprintln!("Ignoring extra input files: {:?}", &cli.files[1..]);
    }

    let debug = Debug {
        lexer: cli.debug || cli.debug_lexer,
        parser: cli.debug || cli.debug_parser,
        ast: cli.debug || cli.debug_ast,
        ast_graph: cli.debug_ast_graph.clone(),
        vm: cli.debug || cli.debug_vm,
        bytecode: cli.debug || cli.debug_bytecode,
        timings: cli.debug || cli.timings,
    };

    let (source, filename) = read_input(&cli);
    let start_time = Instant::now();

    if debug.lexer && debug.parser {
        eprintln!("=== Lexer + Parser ===");
    } else if debug.lexer {
        eprintln!("=== Lexer ===");
    } else if debug.parser {
        eprintln!("=== Parser ===");
    }

    let program = match parse_program(&source, &filename) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };

    if debug.timings {
        eprintln!("Parsed to AST in {}μs", start_time.elapsed().as_micros());
    }

    if debug.ast {
        eprintln!("=== AST ===");
        print!("{}", ast_printer::format_ast(&program));
    }

    if let Some(ref path) = debug.ast_graph {
        match fs::write(path, dot_printer::format_ast_graph(&program)) {
            Ok(()) => eprintln!("AST graph written to {path}"),
            Err(e) => {
                eprintln!("Error writing AST graph to '{path}': {e}");
                process::exit(1);
            }
        }
    }

    if (debug.lexer || debug.parser || debug.ast || debug.ast_graph.is_some()) && !debug.vm {
        process::exit(0);
    }

    if debug.vm {
        eprintln!("=== VM ===");
    }

    let compile_start = Instant::now();
    let mut compiler = Compiler::new();
    let bytecode = match compiler.compile(&program) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Internal compiler error: {e}");
            process::exit(1);
        }
    };

    let bytecode = if cli.optimize {
        Optimizer::new().optimize(bytecode.clone())
    } else {
        bytecode.clone()
    };

    if debug.timings {
        eprintln!(
            "Compiled to bytecode in {}μs",
            compile_start.elapsed().as_micros()
        );
    }

    if debug.bytecode {
        eprint!("{}", bytecode_fmt::format_bytecode(&bytecode));

        if !debug.vm {
            process::exit(0);
        }
    }

    let exec_start = Instant::now();
    let mut stdout = std::io::stdout();
    let mut stdin = std::io::stdin();
    let mut vm = BytecodeVM::new(&mut stdout, &mut stdin, debug.vm);
    let result = vm.execute(&bytecode);
    vm.heap.flush_print();
    if let Err(e) = result {
        eprintln!("{e}");
        process::exit(1);
    }

    if debug.timings {
        eprintln!("Executed in {}ms", exec_start.elapsed().as_millis());
    }
}
