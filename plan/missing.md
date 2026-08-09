# Missing Features & Limitations

Summary of unfinished features, known limitations, and semantic gaps. Each item notes
the affected component and, where relevant, the semantics already chosen by the user
(see notes in each section). This complements `bugs.md` (confirmed defects) and
`tests.md` (coverage gaps). Nothing here is implemented yet.

---

## 1. Upvalue capture is limited to one level of nesting

Only locals of the immediately enclosing scope can be captured as upvalues.
`resolve_upvalue` (l3_compiler/src/lib.rs:184-213) walks exactly one enclosing context;
the loop that would search outer *upvalues* (lib.rs:208-211) is a stub that
deliberately stops at one level.

Repro (`t3_three_level.l3`):

```l3
fn outer() do
  let x = 1
  let mid = fn() do
    let inner = fn() return x end
    return inner
  end
  return mid
end
```

- **Actual:** `RuntimeError: UndefinedVariable: x` when calling the middle function.
- **Fix:** implement transitive upvalue resolution — when the name is not found in the
  immediate enclosing context, propagate the *outer context's* upvalue descriptor
  (mapping an outer `UpvalueDesc` to a local of the current context).

---

## 2. `map` / `count` accept only builtin functions

`map` (l3_vm/src/builtins.rs:271-282) and `count` (builtins.rs:296-309) require
`Function::Builtin` and raise `TypeError: map currently only supports builtin functions`
for user-defined closures or named functions.

Repro (`t18_map_userfn.l3`):

```l3
let double = fn(x) return x * 2 end
map(double, [1, 2, 3])
```

- **Actual:** `TypeError: map currently only supports builtin functions`.
- **Fix:** invoke arbitrary functions via the VM (call a `Function::Bytecode` value),
  which also requires deciding whether `map`'s callback can be a `FunctionCall`
  expression rather than a value. `any`/`all`/`filter` share the same limitation.

---

## 3. Mutability is parsed but never enforced

`let`/`let mut`, `for`/`for mut`, and the `Mutability` AST field exist, but the compiler
and VM never check them. Assigning to a `let` binding or a non-`mut` loop variable
silently works (see bugs.md #3 for the related upvalue write).

```l3
let x = 5
x = 6          // no error today
println(x)     // 6
```

- **Fix (per user decision):** a `Local` records `mutable: bool` (`let` → false,
  `let mut`/params/loop control vars → true). Assignments — `NameAssignment`,
  `OperatorAssignment` (incl. `VectorAppend`), and `SetUpvalue` for captured
  immutables — become `CompileError`s. Loop variables honor `for mut` / `for`.
- **Deliberate loosening to keep:** function *parameters* are mutable (current
  behavior); `fn`-named functions are immutable bindings.

---

## 4. Calls and indexing are restricted to bare identifiers

The grammar's `Primary` (l3_parser/src/grammar.lalrpop:702-728) requires a bare
`"ident"` for function calls and indexed variables. Calling or indexing an arbitrary
expression is a parse error:

```l3
f()()          // parse error: can't call a call's result
"abc"[0]       // parse error: can't index a string literal
[1, 2, 3][0]   // parse error
(getVector())[0] // parse error
```

- **Fix:** generalize `Primary` to allow postfix `( args )` and `[ idx ]` on any
  expression, which also enables method-style chaining once slicing exists.

---

## 5. Slicing is unimplemented (`Slice` is dead code)

`l3_runtime/src/stack_value.rs:8` defines `Slice { start, end }`, but no builtin
produces or consumes it. There is no `a[start:end]` syntax and no `slice(...)` builtin.
Either implement slicing or remove the dead type.

---

## 6. `deduplicate_constants` is a no-op stub

`deduplicate_constants` (l3_compiler/src/lib.rs:1495) is an empty const fn. Constant
*deduplication* happens opportunistically in `make_constant`, but the separate
post-pass is unimplemented.

---

## 7. For-loops depend on the global `len`

Collection `for` loops lower to `len(collection)` resolved via `GetGlobal "len"`
(l3_compiler/src/lib.rs:713-723). If a program redefines the global `len`, all
collection loops break. Fix: bind `len` to the builtin explicitly at compile time (or
give the compiler a builtin constant) rather than a mutable global lookup.

---

## 8. No user-visible GC trigger

`__trigger_gc` exists as a builtin (`l3_vm/src/builtins.rs:398`) but is not a valid
identifier in the lexer/grammar, so it is a parse error to call (see bugs.md #7 for the
deeper defect it would hit). Either lex `__trigger_gc` as a callable builtin name (and
fix its collection bug) or drop it.

---

## 9. Semantic gaps (decisions made, not yet implemented)

These have agreed-upon semantics from the earlier analysis but are unimplemented:

### 9.1 `len` uses bytes, not characters **[user decision: char count]**

`builtin_len` (l3_vm/src/builtins.rs:140) returns `s.len()` (UTF-8 byte length), while
indexing/`head`/`tail`/`drop`/`take` operate on `chars()`. `len("héllo")` → `6` but
`"héllo"[1]` → `'é'` (`t9_unicode_len.l3`). Fix: `s.chars().count()`.

### 9.2 `%` and `^` do not promote like `+ - * /` **[user decision: promote uniformly]**

`5 % 2.0` raises `TypeError` while `5 + 2.0` promotes to double; `2 ^ -1` errors while
`2 ^ -1.0` → `0.5` (`t8_mixed_mod.l3`, `t40`). Fix: `modulo` (l3_runtime/src/
heap_data.rs:248) accepts mixed int/double → double; `pow` (heap_data.rs:255) promotes
negative *integer* exponents to double. Mirror in the optimizer's folding
(l3_bytecode/src/optimizer.rs, `constant_propagation`).

### 9.3 `len`/`sum`/`for` are lenient instead of strict **[user decision: make strict]**

- `len(5)`, `len(nil)`, `len()` → `0` (`t9`); `for x in 5` iterates 0 times (`t22`);
  `sum([1, "x", 2])` → `3`, silently skipping non-numbers (`t25`).
- Fix: `builtin_len` returns `TypeError` for non-container/no argument; `builtin_sum`
  errors on non-number elements; collection `for` over a non-container errors (falls
  out of strict `len`). IEEE float div/mod-zero behavior (inf/NaN) is kept.

---

## 10. Other known limitations

- **No forward-reference hoisting** — calling `f()` before `fn f()` fails with
  `UndefinedVariable: f` (`t30_fwd.l3`).
- **Assignment auto-creates globals** — `y = 99` inside any scope silently creates a
  global; no "assignment before declaration" error.
- **`break`/`continue` not validated** — outside a loop / across function boundary it
  miscompiles (see bugs.md #6; decision: CompileError).
- **Double formatting drops `.0`** — `println(42.)` → `42`, `println(1.0 + 2)` → `3`
  (`t24_floats.l3`). Cosmetic but confusing for float-typed results.
- **Deep recursion** — no stack-depth limit; very deep recursion can exhaust memory
  before any graceful error. No test covers the boundary.
- **`Mutability` on `for` loop vars** — parsed (`for mut x in ...`) but not enforced.
- **Currying arity errors** — partial application exists, but over-application /
  arity mismatch paths are untested and may misbehave (see tests.md).

---

## Reference: line map

| Item | Primary location(s) |
| --- | --- |
| #1 upvalue depth | l3_compiler/src/lib.rs:184-213 |
| #2 map/count | l3_vm/src/builtins.rs:271-282, 296-309 |
| #3 mutability | l3_ast `Mutability`; l3_compiler local/assign paths |
| #4 grammar restrictions | l3_parser/src/grammar.lalrpop:702-728 |
| #5 Slice | l3_runtime/src/stack_value.rs:8 |
| #6 dedup stub | l3_compiler/src/lib.rs:1495 |
| #7 for/len coupling | l3_compiler/src/lib.rs:713-723 |
| #8 __trigger_gc | l3_vm/src/builtins.rs:371-374, 398 |
| #9.1 len bytes | l3_vm/src/builtins.rs:140 |
| #9.2 %/^ promotion | l3_runtime/src/heap_data.rs:248, 255 |
| #9.3 strictness | l3_vm/src/builtins.rs:135-147, 347-369 |
