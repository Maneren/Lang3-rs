# Lang3 Performance Bottleneck Exploration

Date: 2026-08-08
Worktree: `/tmp/opencode/lang3-perf` (branch `perf-explore`, HEAD `37cb118 perf: store bytecode indices as u32 to shrink instruction to 24 bytes`)

## 1. Benchmark

```
hyperfine -N "$CARGO_TARGET_DIR/release/l3 examples/game_of_life.l3" --warmup=5

Benchmark 1: /home/maneren/.cache/cargo/target/release/l3 examples/game_of_life.l3
  Time (mean ± σ):     338.3 ms ±   5.2 ms   [User: 333.2 ms, System: 2.0 ms]
  Range (min … max):   332.2 ms … 350.8 ms   10 runs
```

Confirmed on a fresh build: **~336 ms mean**. `CARGO_TARGET_DIR=/home/maneren/.cache/cargo/target`.

### Results after the first round of fixes

Three targets are implemented (commits `4c466fb`, `0d6be3b`, `37cb118`):

| Commit | Change | Mean (hyperfine, warmup 5) |
|--------|--------|------:|
| `185864d` baseline | — | 336 ms |
| `4c466fb` | cache `ip`; sync frame only on Call/Return/error | 301 ms |
| `0d6be3b` | GC check only at allocation sites | 287 ms |
| `37cb118` | `u32` indices, `Box<[UpvalueDesc]>` (enum 48→24 B) | 286 ms |

**Total: ~336 → ~286 ms (~15%).** A boxed-`ForLoop` alternative (enum → 32 B) *regressed* to 297 ms — the pointer deref on the hot loop path outweighs the cache win — and is preserved as `stash@{0}` rather than deleted.

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

## 7. Remaining recommended fixes, ranked by effort/reward

Implemented so far (see §1): cache `ip` (was #1), GC check at allocation sites (was #3), `u32` instruction indices. Remaining:

| # | Change | Est. win | Scope |
|---|--------|----------|-------|
| 1 | Global slot indices instead of string-hash globals | ~12% | compiler + VM |
| 2 | Capture read-only locals by value (drop Rc/RefCell) | ~11% | compiler + VM |
| 3 | Special-case primitive arithmetic before heap `resolve` | ~10% | runtime |
| 4 | Pre-size VM stack; reduce array-append allocations | ~5-11% | VM / builtins |

Item 1 is the classic "symbol resolution" win. Item 2 requires a compiler "captures-are-not-mutated" analysis.

Section 9 shows the post-fix measurements: comparison chain ~25%, StackValue copies ~20%, dispatch+bounds ~10%, GetGlobal ~4.5%.

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

## 9. Post-fix profile (frame pointers enabled)

Built `release-debug` with `RUSTFLAGS='-C force-frame-pointers=yes'` (config-based `[build] rustflags` did not apply — the shell exports `CARGO_ENCODED_RUSTFLAGS=""` which overrides config; env var works). 1512 resolved samples at `-F 5000`.

### Call-graph (children %) — now reliable

| Children | Self | Function |
|--------:|-----:|----------|
| 94.72 | 78.00 | `BytecodeVM::execute_loop` |
| 13.67 | — | `pop_value` → `Vec<StackValue>::pop` → `ptr::read` |
| 9.42 | — | `l3_runtime::heap_data::compare` |
| 8.36 | — | `compare_op` closure #5 |
| 8.23 | — | `compare_primitives` |
| 7.51 | — | `heap_data::add` |
| 7.46 | — | `Vec<StackValue>::push` → `push_mut` → `ptr::write` |
| 7.26 | — | `BytecodeVM::call_function` |
| 6.21 | — | `Ordering::eq` (result-of-compare check in `compare_op` closures) |
| 5.43 | 5.43 | `l3_runtime::heap_data::compare` (self) |
| 4.49 | — | `HashMap<String, StackValue>::get` (GetGlobal) |
| 4.07 | — | `slice::get` `Instruction` (bounds check in `code.get(ip)`) |
| 3.81 | — | `heap_data::resolve` |
| 3.42 | — | `compare_op` closure #4 |

### Source lines (self %)

| Self % | Line | Code |
|-------:|------|------|
| 12.8 | core `ptr/mod.rs:1755` | `ptr::read<StackValue>` — 16-byte pop off the VM stack |
| 9.1 | `l3_vm/src/lib.rs:0` | dispatch prologue / instruction fetch |
| 5.6 | `lib.rs:168` | `match instruction { … }` jump-table dispatch |
| 5.2 | core `cmp.rs:2031` | `Ord::cmp` (int compare inside `compare_primitives`) |
| 4.7 | core `ptr/mod.rs:1963` | `ptr::write<StackValue>` — 16-byte push |
| 3.6 | `lib.rs:549` | GetIndex: `*container = result` |
| 3.4 | core `slice/index.rs:184` | `code.get(ip)` bounds check |
| 3.0 | `l3_runtime/src/primitive.rs:168` | `compare_primitives` match |
| 2.7 | `heap_data.rs:170` | `resolve()`: `StackValue::Heap` → `cells.get` |
| 1.7 | alloc `vec/mod.rs:1038` | `Vec::push` capacity check |
| 1.5 | `heap_data.rs:169` | `resolve()`: `StackValue::Primitive` arm |

### What this changes vs. the baseline profile (§4)

- The three fixes moved ~13% out of `execute_loop` self-time into the operations it calls: `resolve`, `compare`, `add`, `call_function`, and StackValue push/pop are now visible as separate attributed frames instead of being smeared into `execute_loop` (82% self before → 78% now; previously that 82% was partly an artifact of the call-graph being garbage).
- The **remaining** cost is now dominated by the 16-byte `StackValue` copies through the VM stack: `ptr::read` 12.3% + `ptr::write` 4.5% + `Vec::pop`/`push` capacity checks ~4% ≈ **~20% total**, plus the dispatch/bounds-check ~10% (`lib.rs:0` + `lib.rs:168` + `slice::get`).
- The **comparison chain** (`compare` 9.4 + `compare_primitives` 8.2 + `Ordering::eq` 6.2 + `compare_op` closures ~16 total ≈ **~25%**) is the single biggest cluster now — driven by `Less`/`LessEqual`/`Equal` in the hot `count_neighbors` loop plus the chained-comparison codegen (`LessEqual` inside `0 < i && i < H`-style bounds checks). These are cheap per-op but the *closure-based `compare_op` helper* allocates a closure env and compares `Ordering` values, and the compiler inlines all of it.
- `GetGlobal` via `HashMap<String, _>` still shows ~4.5% — global slot indices (section 7 #1) remain unclaimed.
- `resolve` dropped from 9.9% self (baseline, without reliable frames) to 3.8% — the profile is now accurately attributing its heap `cells.get` cost instead of folding it into `execute_loop`.

### Remaining opportunities, by current measured cost

1. **Comparison chain ~25%** — replace the `compare_op(Ordering::X)` closure pattern with direct integer/primitive branch codegen; `Ordering::eq` after every compare is pure overhead when the codegen already knows the expected direction.
2. **StackValue 16-byte copy ~20%** — shrink `StackValue` (e.g. `Primitive` 8B + tag in one word) or keep the VM stack in parallel typed arrays; also `Vec::push` capacity re-check per op.
3. **Dispatch+bounds ~10%** — `code.get(ip)` is a checked slice index every instruction; an unsafe `get_unchecked` (ip always < len, maintained by the loop invariant + Call/Return jumps) would cut the bounds check ~3-4%.
4. **`GetGlobal` hashmap 4.5%** — global slot indices (section 7 #1).
