# Missing Test Coverage

Map of behavior currently **not** exercised by the test suite. Existing coverage:

- **Snapshot tests** (`l3/tests/snapshot_tests.rs`, `snapshot_tests!` macro) generate
  output / AST / bytecode expectations per input in `l3/tests/snapshot/inputs/*.l3`.
  Current inputs (18): closures_recursive_factory, closures_stateful,
  comparisons_and_logic, control_flow, currying_partial_application, expressions,
  functions_closures, chained_comparison_sideeffect, indexing(_assignments|_bounds|
  _invalid_types|_negative|_nested_error), mutable_references, range_for,
  recursion_direct, recursion_indirect.
- **Optimizer tests** (`l3/tests/optimizer_tests.rs`, 9 unit tests) call
  `run_pipeline_optimized` and inspect bytecode.

Gaps below are grouped by area and reference the bug/feature in `bugs.md` / `missing.md`
where applicable. Suggested new snapshot inputs live in `l3/tests/snapshot/inputs/`
(or new unit tests for optimizer/error cases).

---

## Control flow (bugs #1, #2, #6)

Only a *while-continue* with no body locals is covered (`control_flow.l3:12-23`).
Nothing covers:

- `continue` in a **collection for** loop (bug #1; `t14`/`t53`).
- `break` with **body-scope locals** in while / collection-for / range-for
  (bug #2; `t31`, `t7`, `t16b`, `t51`, `t52`). This is the class of bug most likely to
  regress silently, since it depends on stack-height bookkeeping.
- `break`/`continue` in **range-for** (only `range_for.l3` with no break/continue).
- `continue` followed by a `let` after the loop (stack height after continue).
- **`break`/`continue` outside a loop** → compile error (bug #6).
- `break`/`continue` inside a **function defined in a loop** → compile error (bug #6).
- Nested loops where `continue`/`break` target the *inner* loop only.
- `break`/`continue` inside nested `if` blocks within a loop.

Error-path tests for bug #6 should assert a `CompileError` (not a runtime hang) — the
snapshot macro may need an error-variant input or a unit test in `l3_compiler`.

---

## Values, strings, and arithmetic (bug #4, missing #9)

- **String equality** by value: `"a" == "a"`, literal vs. local, and `!=`
  (bug #4; `t1`). Also equality of same-cell vs. distinct-cell strings.
- **Vector equality** by content and mixed (string/vector) `==`.
- Equality of **functions** (reference equality) — document chosen semantics.
- **Non-ASCII strings**: `len("héllo")` == 5 (chars, missing #9.1), indexing a
  multi-byte char (`"héllo"[1]` == `"é"`), `head`/`tail`/`drop`/`take` on multi-byte
  strings, iteration over such a string.
- **Mixed-type `%`** (`5 % 2.0` → `1.0`, missing #9.2) and **negative `^`**
  (`2 ^ -1` → `0.5`, `2 ^ -1.0`). Verify optimizer folding matches the runtime.
- **Float edge cases**: `1.0 / 0.0` → `inf`, `5.0 % 0.0` → `NaN` (documented IEEE),
  `5 / 0` error, `0.5` formatting, `42.` print (missing #10).
- **Big-int overflow** wrap behavior (`9223372036854775807 + 1`, `t46`) — document the
  choice (bugs.md #8.4).
- Float equality (e.g. `0.1 + 0.2 == 0.3` → `false`); make sure no accidental
  integer promotion path.

---

## Closures and upvalues (bug #3, missing #1)

Existing inputs cover capture + read (`closures_stateful`, `closures_recursive_factory`)
but never **mutate-and-read-back**:

- Closure writes to a captured local, enclosing scope then reads it (bug #3; `t5`,
  `t37`-`t39`). Cover while/for bodies and nested closure-in-closure.
- **Three-level** (and deeper) upvalue nesting (missing #1; `t3`).
- Captured local re-assigned by the owner *and* the closure (both `SetLocal` and
  `SetUpvalue` paths to the same cell).
- `let mut` captured variable mutated from a closure.
- Loop variable captured by a closure created in the loop (aliasing hazards).
- Closure stored in a vector, then invoked.

Optimizer-specific (see bug #3 fix):
- Optimizer must **not** fold `GetLocal` across a `Call` (mutation visible). Current
  `preserves_closure_capture_semantics` covers `x = x + 1` then `f()`; add the inverse
  where `f()` mutates `x` before the owner reads it.
- `-O` and non-`-O` parity for every closure/upvalue case above.

---

## Declarations and assignment (bug #5, missing #3)

- **Multi-name `let`** destructuring: `let a, b = [1, 2]`, nested destructuring, and
  the error paths (non-vector RHS → `TypeError`, short vector → `ValueError`)
  (bug #5; `t2`).
- **Mutability enforcement**: assignment to `let x` → `CompileError`; `let mut x`
  works; assignment to non-`mut` loop var (`for x in ...`) → `CompileError`; `for mut`
  works; assignment to an immutable *captured* variable from a closure →
  `CompileError` (missing #3). These should be unit tests asserting `CompileError`.
- Vector element assignment on a non-mutable vector local.
- `OperatorAssignment` (`+=`, `-=`, `*=`, etc.) on immutable vs. mutable bindings.

---

## Builtins (missing #2, #8, #9.3)

Currently untested builtins: `map`/`count`/`any`/`all`/`filter` with **user functions**
(missing #2), `head`/`tail`/`drop`/`take` on empty and negative-arg cases (bugs.md
#8.1), `range`, `id`, `random`, `sleep`, `error`, `assert`, `input`.

Strictness (missing #9.3):
- `len(5)`, `len(nil)`, `len()` → `TypeError`; `len("héllo")` → `5`.
- `sum([1, "x", 2])` → `TypeError`; `sum([])` → `0`; `sum()` → `TypeError`.
- Collection `for` over non-container (`for x in 5`) → error.
- `head([])`/`tail([])` → `ValueError` (already covered for head via `t33`, add tail,
  drop, take, and the negative-count error).

GC:
- Live data survives GC; unreachable cells collected (once `__trigger_gc` or GC
  triggering is fixed, bugs.md #7). Stress: many small allocations then GC, verify heap
  shrinks. No test currently observes heap state.

---

## Optimizer (bugs #2, #3, #4, missing #9.2)

Current 9 tests cover basic folding, chained comparison, and two upvalue/loop
interaction cases. Add:

- **Dead `Pop` DCE after `break`** — loop body locals must not affect the optimized
  result (bug #2; `t31` under `-O`).
- **Closure mutation across `Call`** (bug #3) — see closures section.
- **Constant folding of string/vector equality** must match runtime (bug #4) — `-O`
  and non-`-O` must agree.
- Folding of mixed `%` and negative `^` (missing #9.2).
- Loop-with-locals: `break`/`continue` inside a loop that declares locals, under `-O`
  and without.
- `-O` on a program that redefines the global `len` (missing #7).

---

## Interpreter robustness

- **Deep recursion** — stack depth limit behavior (missing #10); recursion_direct/
  indirect inputs are shallow.
- **Stack underflow** error path — the `1.1` location display on the 3-level closure
  repro is suspicious (`l3_vm` prints `at {}` with an odd location); add a regression
  test that the location is sensible.
- Error-message stability for the newly-added `CompileError`s (break/continue outside
  loop, immutability, destructuring).
- Statement-position calls that discard returns in various contexts (as a loop body
  statement, inside if branches, etc.).

---

## How to add

- **Snapshot inputs**: new files in `l3/tests/snapshot/inputs/`; regenerate with
  `L3_UPDATE_SNAPSHOTS=1 cargo test`. One input per gap (or per group).
- **Compile-error cases** (`break` outside loop, immutability, destructuring errors):
  unit tests in `l3_compiler` asserting `Err(CompileError)` with the expected message,
  since the snapshot macro may not have an error-input variant.
- **Optimizer cases**: unit tests in `l3/tests/optimizer_tests.rs` asserting both the
  `-O` result and (for soundness checks) that the folded bytecode matches the runtime.
- After each fix in `bugs.md`, add the corresponding regression test **before**
  regenerating snapshots, and re-run `cargo fmt` + `cargo clippy` + `cargo test`.
