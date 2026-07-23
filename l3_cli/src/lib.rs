use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "l3", about = "L3 programming language interpreter")]
pub struct Cli {
    /// Input file to execute
    pub file: Option<String>,

    /// Print debug information about AST
    #[arg(long)]
    pub debug_ast: bool,

    /// Print debug information about bytecode
    #[arg(long)]
    pub debug_bytecode: bool,

    /// Print debug information about VM execution
    #[arg(long)]
    pub debug_vm: bool,

    /// Print execution timing
    #[arg(long)]
    pub time: bool,

    /// Output AST as Graphviz DOT
    #[arg(long)]
    pub dot: Option<String>,
}
