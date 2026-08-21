# Default recipe: list available commands
default:
    @just --list

# Build all crates
build:
    cargo build --all

# Run all tests
test:
    cargo test --package l3

# Run tests for a specific crate
test-pkg pkg:
    cargo test --package {{pkg}}

# Update snapshot tests
snapshots:
    L3_UPDATE_SNAPSHOTS=1 cargo test --package l3

# Format code
fmt:
    cargo fmt

# Check formatting without modifying
fmt-check:
    cargo fmt -- --check

# Run clippy lints
clippy:
    cargo clippy --all-targets

# Run benchmarks (full)
bench:
    cargo bench --package l3 --bench pipeline

# Run benchmarks (quick pass)
bench-quick:
    cargo bench --package l3 --bench pipeline -- --quick

# Format, lint, and test
check: fmt clippy test

# Run the L3 interpreter on a file
run file:
    cargo run --release -- {{file}}

# Build a static musl binary without AVX instructions
musl:
    RUSTFLAGS="-C target-cpu=x86-64 -C target-feature=-avx" \
    cargo build --release --target x86_64-unknown-linux-musl --bin l3
