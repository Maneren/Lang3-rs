# Lang3-rs

A Rust port of the L3 language — a dynamically-typed, Lua/Python-inspired scripting language with first-class functions, closures, currying, and GC.

## Code Structure

Workspace with 8 crates (topologically sorted):

| Crate         | Purpose                                                                      |
| ------------- | ---------------------------------------------------------------------------- |
| `l3_location` | Source location types (`Position`, `Location`)                               |
| `l3_ast`      | AST node definitions + pretty-printer                                        |
| `l3_runtime`  | Runtime types: `Heap`, `RuntimeEnv` (stdio + rng), `StackValue`, `Primitive`, `Function`, `RuntimeError` |
| `l3_bytecode` | Bytecode `Instruction` enum + formatter + optimizer                          |
| `l3_parser`   | Lexer (logos) + LALRPOP grammar                                              |
| `l3_compiler` | AST → Bytecode compiler                                                      |
| `l3_vm`       | Bytecode VM loop + builtin functions                                         |
| `l3`          | Top-level binary + lib (pipeline: parse → compile → execute)                 |

Dependency flow: `l3_location → l3_ast → l3_runtime → l3_bytecode → (l3_parser, l3_compiler, l3_vm) → l3`

Key files: `l3_parser/src/grammar.lalrpop` (815 lines of grammar), `l3_vm/src/builtins.rs` (20 builtins), `l3_vm/src/lib.rs` (dispatch loop; `stack.rs` holds `VmStack`/`CallStack`/`CallFrame`), `l3_compiler/src/{lib.rs, context.rs, alias.rs, fold.rs, compile.rs}` (compiler split by concern), `plan/` directory has bytecode optimization docs.

## Tests

- **Snapshot tests** in `l3/tests/snapshot_tests.rs` using a custom `snapshot_tests!` macro.
- 18 test inputs in `l3/tests/snapshot/inputs/*.l3`, expected outputs in `l3/tests/snapshot/expected/<name>/{output,ast,bytecode}.txt`.
- Each input generates 3 test cases: output, AST, and bytecode.
- Run: `cargo test`
- Update snapshots: `L3_UPDATE_SNAPSHOTS=1 cargo test`

## Benchmarks

- Criterion benchmarks in `l3/benches/pipeline.rs`, benchmarking the full L3
  pipeline on `bench/*.l3` scripts.
- Run: `cargo bench` (or `cargo bench -- --quick` for a fast pass during
  iteration or non-performance-critical changes).

## Coding Practices

- **Formatting:** `cargo fmt` (default rustfmt, no config file).
- **Clippy:** `cargo clippy` — workspace lints in `Cargo.toml` enable `clippy::pedantic` with selective allows (doc warnings, casting, wildcard imports, module name repetitions).
- **Commits:** Single-line imperative mood, lowercase, no trailing period. e.g. `fold negate of numeric literals at compile time`.
- **No build scripts** — standard Cargo workflows only.
- **Rust edition 2024**, workspace resolver `"2"`.
- **`release-debug` profile:** inherits release with `debug = true`.
- **Code style:**
  - Prefer functional style over imperative style.
  - Prefer `if let` over `match` with only two arms.
  - Prefer methods on `Option` and `Result` over `if let` and `match`.
  - Make plenty of reusable helpers - strictly adhere to DRY.
  - Best kind of code change is one that is deleting lines rather than adding lines.
- **No CI** configured.
