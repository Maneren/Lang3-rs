# Lang3 Performance Bottleneck Exploration

Date: 2026-08-08
Worktree: `/tmp/opencode/lang3-perf` (branch `perf-explore`, HEAD `185864d wip refactor`)

## 1. Benchmark

```
hyperfine -N "$CARGO_TARGET_DIR/release/l3 examples/game_of_life.l3" --warmup=5

Benchmark 1: /home/maneren/.cache/cargo/target/release/l3 examples/game_of_life.l3
  Time (mean ± σ):     338.3 ms ±   5.2 ms   [User: 333.2 ms, System: 2.0 ms]
  Range (min … max):   332.2 ms … 350.8 ms   10 runs
```

Confirmed on a fresh build: **~336 ms mean**. `CARGO_TARGET_DIR=/home/maneren/.cache/cargo/target`.

### Workload (examples/game_of_life.l3)
- 40×40 board, 100 generations.
- `count_neighbors` (chunk 5) is the hot function: 1600 cells/gen × 100 gens = **160k calls**, each with a 9-iteration nested loop (~25 bytecode instructions per inner iteration).
- `create_board()` re-runs every generation: 1600 `row += [false]` array-appends + 40 `board += [row]` appends → ~164k array allocations.
- `display_board` runs 100×, printing 40 rows of 42 cells.

## 2. Methodology

1. **Phase split** via `--timings`: parse = 588µs, compile = 80µs. **The VM loop is the entire 336ms.**
2. **`perf record -g -F 5000 -- release/l3 examples/game_of_life.l3`** → `perf report` / `perf annotate`.
3. **`release-debug` profile** (release + `debug = true`) for DWARF source attribution via `llvm-addr2line`. Note GNU `addr2line` fails on rustc CGU paths; `llvm-addr2line` works.
4. **`scripts/perf-profile.py`** — reusable helper: aggregates perf samples by function / source line using the MMAP2 file-offset base (kernel samples counted as unresolved).
5. **Dynamic instruction counter** — temporary instrumentation of `execute_loop` incrementing per-opcode counters, printed at end of `execute()`. Reverted after use.

## 3. perf report (release, cycles)

```
Children   Self   Symbol
93.29%   82.37%   <l3_vm::BytecodeVM>::execute_loop
 5.01%    3.12%   l3_runtime::heap_data::compare
 1.72%    1.34%   l3_runtime::heap_data::sub
 1.66%    1.09%   l3_runtime::heap_data::add
```

The remaining ~7% of total time is spread across allocation (`__libc_malloc`/`free`), `Heap::alloc`, `CallFrame` drop glue, and `hashbrown` global-map traffic.

Note: perf call-chains show `execute_loop → [unknown 0x7f…5010] → execute_loop` self-edges. The binary is built **without frame pointers**, so perf's frame-pointer unwinding walks garbage; the `[unknown]` address is a stack-region artifact, not a real caller. **Treat the call-graph as unreliable; the self-time attribution is sound.**

## 4. Source-attributed self-time (release-debug, 1742 resolved samples)

### Functions (demangled)

| Self % | Function |
|-------:|----------|
| 9.9 | `l3_runtime::heap_data::resolve` |
| 5.7 | `hashbrown::map::make_hash` (String, global lookups) |
| 5.3 | `BytecodeVM::execute_loop` (dispatch prologue itself) |
| 4.2 | `alloc::alloc::dealloc` |
| 4.1 | `core::ptr::copy_nonoverlapping` |
| 3.6 | `execute_loop` closure (Closure opcode upvalue mapping) |
| 3.4 | `hashbrown::RawTableInner::set_ctrl` |
| 3.3 | `HashMap::insert` (String→StackValue) |
| 2.8 | `drop_glue` `Rc<RefCell<UpvalueCell>>` |
| 2.7 | `core::ptr::read` `StackValue` |
| 2.6 | `hashbrown::Tag::copy` |
| 2.4 | `Vec<StackValue>::pop` |
| 2.4 | `core::num::from_ascii_bytes_radix_impl` (integer formatting) |

### Source lines (l3_vm/src/lib.rs)

| Self % | Line | Code |
|-------:|------|------|
| 3.6 | 561 | Closure opcode: `captured_locals.entry(index).or_insert_with(Rc::new(RefCell::new(...)))` |
| 1.7 | 397 | ForLoop opcode: integer extraction + step handling |
| 0.5 | 369 | JumpIf truthiness |

### Source lines (l3_runtime/src/heap_data.rs)

| Self % | Line | Code |
|-------:|------|------|
| 2.1 | 170 | `resolve()`: `StackValue::Heap` → `heap.cells.get` |
| 1.1 | 169 | `resolve()`: `StackValue::Primitive` match |
| 0.8 | 91 | `Primitive::Add` (primitive.rs) |

## 5. Dynamic instruction mix (58.5M executed)

Counted by temporary instrumentation. Ranked by share of executed instructions:

| Instruction | Executed | Share | Notes |
|-------------|---------:|------:|-------|
| GetLocal | 15,584,712 | 26.6% | `stack.get(fp + index)` + bounds check |
| JumpIf | 8,624,100 | 14.7% | chained-comparison codegen |
| Jump | 7,698,323 | 13.2% | |
| Constant | 6,636,672 | 11.3% | hot loop pushes `0`, `1`, `3`, `4` repeatedly |
| GetUpvalue | 3,162,686 | 5.4% | `Rc<RefCell>::try_borrow` per read |
| GetIndex | 3,110,400 | 5.3% | `board[i][j]` slotmap lookup |
| ForLoop | 2,899,663 | 5.0% | |
| Less | 2,681,900 | 4.6% | |
| LessEqual | 2,536,000 | 4.3% | |
| SetLocal | 2,422,030 | 4.1% | |
| Add | 2,249,642 | 3.8% | |
| Equal | 2,096,820 | 3.6% | |
| Call | 1,776,206 | 3.0% | |
| GetGlobal | 1,614,202 | 2.8% | builtins resolved by string hash |
| Subtract | 1,288,283 | 2.2% | |
| Pop | 1,021,723 | 1.7% | |
| Return | 322,005 | 0.6% | |
| MakeArray | 169,781 | 0.3% | `create_board` every generation |
| SetIndex | 161,600 | 0.3% | |
| Closure | 108 | — | one per generation × 2 |
| SetGlobal | 7 | — | top-level only |

Never executed: `NotEqual, Greater, GreaterEqual, Not, Negate, Divide, Modulo, Power, Duplicate, SetUpvalue` — 10 of 32 opcodes are dead for this workload.

## 6. Root causes, in order of cost

### 1. Per-instruction frame-ip write (~20%)
`l3_vm/src/lib.rs:159-162`:
```rust
self.frames.last_mut().expect(..).ip = ip;
```
Written on **every** instruction purely to support `current_instruction_location` for error stacktraces. The compiler reloads `frames.len()` (0x1d8) and `frames.ptr` (0x1d0) each iteration and computes `frames[len-1].ip = ip` (perf: lib.rs:159 ~12% + `last_mut` inlined `slice/mod.rs:305` ~7.5% + store ~3.2% ≈ 20%). The `ip` is already live in a register inside the loop; the write-back is pure overhead.

Fix: cache `ip` in a `BytecodeVM` field (or just let the error path re-derive it), sync to the frame only on Call / Return / error.

### 2. Global lookup by string hash (~12%)
`GetGlobal`/`SetGlobal` (`lib.rs:299-326`) hash a `String` key against `global_symbols: HashMap<String, StackValue>` every time. Cost in profile: `make_hash` 5.7% + `set_ctrl` 3.4% + `insert` 3.3% + `prepare_rehash_in_place` 2.1% + `bucket_mask_to_capacity` 2.3% ≈ 12%. `GetGlobal` executes 1.6M× (`int`, `random`, `str`, `println`, `print`). `SetGlobal` also allocates a `String` key copy (`name_str.to_string()`, lib.rs:322).

Fix: compiler emits a stable **global slot index** (one pass, or resolve name→slot at compile time); VM stores globals in `Vec<StackValue>` (default `Nil`). Kills all hashbrown traffic for globals.

### 3. `maybe_gc()` after every instruction (~3-4%)
`lib.rs:598`:
```rust
self.maybe_gc();   // heap.added_since_last_sweep >= next_gc_threshold
```
Hot addresses cdb80/cdb84 (2.27% + 1.81%). The load+cmp+branch also prevents the compiler from keeping `heap.added_since_last_sweep` in a register across instructions.

Fix: call the GC check at allocation sites (heap `alloc_*` already bumps the counter) or every N instructions, not per instruction. (Also `Return`/`Call` already call it — lib.rs:194, 484 — so per-instruction call is redundant with allocation-based accounting.)

### 4. Closure + upvalue cell overhead (~11%)
- `lib.rs:561` (3.6%): `Closure` opcode maps each upvalue descriptor through `captured_locals.entry(index).or_insert_with(|| Rc::new(RefCell::new(UpvalueCell::new(...))))` — a hashmap lookup + `Rc`/`RefCell` allocation **per closure creation** (2 closures/generation).
- `drop_glue Rc<RefCell<UpvalueCell>>` 2.8% — the cells must be freed when the enclosing frame dies.
- `GetUpvalue` 5.4%: each read does `frame.upvalues.get(index).try_borrow()` + `cell.value` copy.

The hot closures capture only read-only state (`board`, `HEIGHT`, `WIDTH` in `count_neighbors`/`for_cell_in_board`). Mutation only matters for `new_board` in `next_generation`.

Fix: capture-by-value when the compiler proves the local is never assigned; keep `Rc<RefCell>` cells only for actually-mutated captures.

### 5. Arithmetic operand resolution (~10%)
`heap_data::resolve` (`heap_data.rs:168-179`) is 9.9% self. Every `Add`/`Sub`/compare calls `resolve` on both operands, and `StackValue::Heap` operands trigger `heap.cells.get(key)`. `Add` itself is only 3.8%.

Fix: special-case `Primitive ⊕ Primitive` (a pure register operation, no heap) before the heap path; hoist the `StackValue` tag check.

### 6. Allocation churn (~11%)
`dealloc` 4.2% + `copy_nonoverlapping` 4.1% + `Vec::pop` 2.4% + `RawVec` grow paths. Two sources:
- `StackValue` is 16 bytes; every `push`/`pop`/`GetLocal`/`Call` copies it through the VM stack `Vec`.
- `create_board()` (run every generation) does 1600 `row += [false]` → `add` on arrays clones the vec and reallocates (`heap_data.rs:190-194`).

Fix candidates: pre-size the VM stack `Vec`; skip `MakeArray`+append pattern via a `Vector::push` opcode or `reserve`; reuse board memory.

### 7. Dispatch loop structure (5.3% direct + amplifier)
The `Instruction` enum is large (Closure carries a `Vec`, so the enum is 48 bytes — dispatch stride is `lea (i,i,2) shl 4`). Every iteration:
- bounds check `code.get(ip)` (~3.5%),
- reads the 48-byte instruction from memory,
- jump-table dispatch (good — `jmp *%rax`, LLVM already emits a jump table).

10/32 opcodes are never executed in this workload — the fat enum hurts I-cache for no benefit.

## 7. Recommended fixes, ranked by effort/reward

| # | Change | Est. win | Scope |
|---|--------|----------|-------|
| 1 | Cache `ip`; sync to frame only on Call/Return/error | ~20% | VM only |
| 2 | Global slot indices instead of string-hash globals | ~12% | compiler + VM |
| 3 | Move GC check off the per-instruction path | ~4% | VM only |
| 4 | Capture read-only locals by value (drop Rc/RefCell) | ~11% | compiler + VM |
| 5 | Special-case primitive arithmetic before heap `resolve` | ~10% | runtime |
| 6 | Pre-size VM stack; reduce array-append allocations | ~5-11% | VM / builtins |

Items 1 and 3 are pure VM changes with no language/bytecode-format impact and are the best first targets. Item 2 is the classic "symbol resolution" win. Item 4 requires a compiler "captures-are-not-mutated" analysis.

## 8. Tooling

### `scripts/perf-profile.py`
Reusable aggregation helper (committed in this worktree under `scripts/`).

```bash
# record (release-debug has DWARF for source attribution)
perf record -o /tmp/perf.data -g -F 5000 -- \
    $CARGO_TARGET_DIR/release-debug/l3 examples/game_of_life.l3

# aggregate
python3 scripts/perf-profile.py /tmp/perf.data \
    $CARGO_TARGET_DIR/release-debug/l3 --top 20
python3 scripts/perf-profile.py /tmp/perf.data \
    $CARGO_TARGET_DIR/release-debug/l3 --filter l3_vm/src
```

Notes:
- Requires `llvm-addr2line` (GNU `addr2line` fails on rustc CGU DWARF paths).
- Needs `--show-mmap-events` (already wired in) to recover the PIE base offset.
- Kernel / out-of-segment samples are counted as "unresolved".
- The binary must be built without frame-pointer-unwinding requirements; call-graphs from perf are unreliable here — the script deliberately uses only the leaf IP (self-time).

### Dynamic instruction counting
Temporary instrumentation (reverted): add `AtomicU64[32]` counters + a `tag()` map to `execute_loop`, print at end of `execute()`. Re-add if opcode-mix data needs refreshing.
