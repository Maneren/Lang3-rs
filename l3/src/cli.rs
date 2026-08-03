use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "l3", about = "A Lang3 programming language interpreter.")]
pub struct Cli {
    /// Input file to execute ('-' for stdin)
    pub files: Vec<String>,

    /// Enable all debug options
    #[arg(short = 'd', long)]
    pub debug: bool,

    /// Enable optimizations
    #[arg(short = 'O', long)]
    pub optimize: bool,

    /// Debug the lexer
    #[arg(long)]
    pub debug_lexer: bool,

    /// Debug the parser
    #[arg(long)]
    pub debug_parser: bool,

    /// Debug the AST
    #[arg(long)]
    pub debug_ast: bool,

    /// Output AST graph to a DOT file
    #[arg(long)]
    pub debug_ast_graph: Option<String>,

    /// Debug the VM
    #[arg(long)]
    pub debug_vm: bool,

    /// Debug the bytecode
    #[arg(long)]
    pub debug_bytecode: bool,

    /// Show execution timings
    #[arg(long)]
    pub timings: bool,
}
