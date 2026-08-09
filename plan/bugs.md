# Confirmed Bugs

Write-up of confirmed interpreter defects. Each entry has a repro (also saved under
`/tmp/opencode/*.l3`), expected vs. actual output, root cause with `file:line`, and a
fix approach. None of these are implemented yet — this document is the work plan.

Severity legend: **C** = corruption (wrong result / stack corruption), **H** = hang /
infinite loop, **L** = latent / low impact, **F** = feature-affecting.

---

## 1. `continue` in a collection `for` loop jumps to the loop *condition*, not the index increment — infinite loop **[H]**

Repro (`t14_continue_for.l3` / `t53_for_continue_after.l3`):

```l3
for x in [1, 2, 3] do
  if x == 2 then continue end
  println(x)
end
```

- **Expected:** prints `1`, `3` (continue skips the body after the condition-check point).
- **Actual:** prints `1`, `2`, `3`, then hangs forever (`exit=124`).

**Root cause:** In `compile_for_loop` (l3_compiler/src/lib.rs:796-799) the collection
for-loop patches `continue` jumps to `loop_start` (l3_compiler/src/lib.rs:728), which is
the `index < length` comparison — *before* the `index++` instructions (emitted at
lib.rs:765-774). Each continue re-enters the comparison with an unchanged index, so the
loop never advances. (Range-for `continue` works because `ForLoop` is a single VM
instruction with an inherent step.)

**Fix approach:** In `compile_for_loop`, record the bytecode offset of the `index++`
sequence (the first `GetLocal idx`) as the continue target, and patch `continue_jumps`
to that offset instead of `loop_start`. The collection-for continue must skip the
`GetIndex`/`SetLocal var` reload and the body, but not the increment.

---

## 2. `break` leaks loop-body locals onto the stack — subsequent `let` bindings read/write wrong slots **[C]**

Repro (`t31_break_nonconst.l3`):

```l3
let i = 0
let y = 0
while i < 5 do
  let a = i * 100
  if a > 9 then break end
  i += 1
end
let y = i * 100
println(y)
```

- **Expected:** `400`.
- **Actual:** `12` (the second `y` reads the leaked `a` slot).

Also reproduced with:
- collection for-loop (`t16b.l3`), bare `break` in while body (`t7_for_break_body_local.l3` → prints `8` instead of `100`),
- range-for (`t51_range_break.l3` → `40` instead of `3000`),
- nested block inside a loop (`t52_nested_break.l3` → `103` instead of `15`).

**Root cause:** `LastStatement::Break` emits only `Jump { offset: 0 }` and records it in
the loop context (l3_compiler/src/lib.rs:1089-1097). The enclosing block's `end_scope`
`Pop { count }` is emitted *after* the jump and is dead at runtime. Locals declared in
the loop body therefore never leave the stack, shifting every later frame-relative
`LocalIndex` by the leaked count. `continue` (lib.rs:1098-1114) already emits the
correct `Pop`; `break` is missing the identical handling.

**Fix approach:** In the `Break` arm of `compile_last_statement_inner`, mirror the
`Continue` arm: when a loop context exists, emit `Pop { count: body_locals }` (computed
the same way, `locals.len() - body_locals_snapshot`) before the jump. The dead
`end_scope` Pop that remains after the jump is harmless and already removed by the
optimizer's DCE.

---

## 3. Upvalue mutation is invisible to the enclosing scope; `SetUpvalue`/`GetLocal` are asymmetric **[C]**

Repro (`t5_opt_closure_mut.l3`):

```l3
let x = 5
let f = fn() x += 1 end
f()
println(x)
```

- **Expected:** `6`.
- **Actual:** `5`, with and without `-O`.

**Root cause:** A captured local is shared through an `UpvalueCell` kept in the frame's
`captured_locals` map. `SetLocal` writes back into the cell when the local is captured
(l3_vm/src/lib.rs:451-459), but the owner's `GetLocal` reads only the plain stack slot
(l3_vm/src/lib.rs:440-444). When the closure mutates the capture via `SetUpvalue`, only
the cell is updated (l3_vm/src/lib.rs:772-787) — the stack slot keeps the stale value,
so the enclosing scope never observes the change.

**Compiler-side consequence:** the optimizer folds `GetLocal x` to its last known
constant even across a `Call` boundary (l3_bytecode/src/optimizer.rs:471 and
`constant_propagation`), turning the closure call into a no-op and masking the bug
(verified: `-O` also prints `5`).

**Fix approach:** Make the cell authoritative. In `GetLocal`, if `index` is in the
frame's `captured_locals`, read from the cell instead of the stack slot (the reverse of
the existing `SetLocal` write-back). In the optimizer, invalidate all local abstract
values at every `Call` (conservative), so closure side effects cannot be folded away.

---

## 4. String and vector equality always compares unequal **[C]**

Repro (`t1_string_eq.l3`):

```l3
let s = "a"
println(s == s)   // false
println("a" == "a") // false
println("a" == "b") // false
```

- **Expected:** `true`, `true`, `false`.
- **Actual:** `false`, `false`, `false`.

**Root cause:** `compare` (l3_runtime/src/heap_data.rs:284-291) only handles
`Primitive` vs `Primitive` and `Nil` vs `Nil`; any heap-backed operand falls through to
`None`, and `Equal`/`NotEqual` treat `None` as unequal. Same for the optimizer's
constant folding, which routes through `compare` (l3_bytecode/src/optimizer.rs:655-662)
and therefore folds `"a" == "a"` to `false` at compile time.

**Fix approach:** Extend `compare` to compare `String` by value and `Vector` by content
(cycle-safe — track visited heap keys); fall back to reference equality (same heap key)
for other heap types such as functions. Do the same where `HeapData::PartialEq` is used
(heap_data.rs:27 already compares strings by value, but the runtime path does not use it).

---

## 5. `let a, b = expr` binds garbage for the second and later names **[C]**

Repro (`t2_multibind.l3`):

```l3
let a, b = 5
println(a)
println(b)
```

- **Expected:** `5`, `5` (all names bound to the one value) — or, per the chosen
  destructuring semantics, a `TypeError`/`ValueError` for a non-vector RHS.
- **Actual:** `5` then `<builtin println>` (a stale stack slot reused from the constant
  pool).

**Root cause:** `compile_declaration` (l3_compiler/src/lib.rs:497-516) pushes exactly
one value for the whole declaration but registers one `Local` per name. The i-th name
binds `locals.len()` slots above the pushed value, so all but the first read unrelated
stack memory.

**Fix approach:** Per the chosen semantics (destructuring), after the RHS is evaluated,
emit `Constant i; GetIndex` for the 2nd..nth name so each binds an element of the RHS
vector. Single-name `let` stays as-is. Decide error behavior for non-vector RHS
(`TypeError`) and too-few-elements (`ValueError`).

---

## 6. `break`/`continue` outside a loop, or inside a function defined in a loop, miscompile into infinite loops **[H]**

Repro (`t16b.l3`, `t47_break_fn.l3`):

```l3
break              // t16b: unconditioned Jump { offset: 0 } never patched → jumps to itself
```

```l3
while true do
  let f = fn() break end   // t47: break is patched into the *while* context
  f()
end
```

- **Expected:** a compile error for both (break/continue must occur lexically inside a
  loop).
- **Actual:** `break` with no loop context emits `Jump { offset: 0 }` that is never
  patched, jumping to itself — an infinite loop. A `break` inside a nested function
  body is patched against the *enclosing chunk's* `loop_contexts` because
  `loop_contexts` is a single compiler-level `Vec` (l3_compiler/src/lib.rs:24), not
  scoped per chunk/function; at runtime it behaves as a loop-level break at best and
  corrupts the loop at worst.

**Root cause:** `compile_last_statement_inner` (l3_compiler/src/lib.rs:1089-1114)
silently falls back to `Jump { offset: 0 }` when `loop_contexts` is empty or when the
enclosing loop belongs to a different chunk. Nothing validates that the jump will be
patched.

**Fix approach (per user decision):** make it a `CompileError`. When compiling a
`Break`/`Continue` with no loop context in the *current chunk* (compare the
`loop_contexts` entry's chunk against the active chunk, or clear `loop_contexts` when
entering a function body), return `CompileError`. This also fixes the nested-function
case, since a function body has its own chunk with no enclosing loop.

---

## 7. `__trigger_gc` sweeps the heap without marking roots — collects live data **[C]**

Repro (`t29_gc.l3`):

```l3
let v = [1, 2, 3]
println(v)
__trigger_gc()
println(v)
```

- **Expected:** `[1, 2, 3]` twice (GC frees only unreachable cells).
- **Actual:** the builtin itself is currently unreachable from the language (it is a
  parse error — see missing.md #8), but the function `builtin_trigger_gc`
  (l3_vm/src/builtins.rs:371-374) calls `heap.sweep()` directly with no mark phase, so
  if it were callable it would free every live cell, including the vector `v` and the
  builtin table entries.

**Root cause:** `Heap::sweep` is invoked in isolation; the full mark-and-sweep
(`collect_garbage`) that marks the stack, constants, and builtins as roots is never
called from this path.

**Fix approach:** Either call the proper mark-and-sweep (marking VM stack, call frames,
upvalues, constant pool, builtin function table) instead of a bare `sweep`, or remove
the builtin. Whichever path, add a snapshot test that triggers GC with live data.

---

## 8. Latent / minor defects

### 8.1 Negative `drop`/`take` underflows to `usize::MAX` **[L]**

`builtin_drop` (l3_vm/src/builtins.rs:195) does
`usize::try_from(count).unwrap_or(usize::MAX)`, so `drop([1,2,3], -1)` returns `[]` and
`take(-1)` (similarly) can return the whole container — no error for a nonsensical
negative count. Fix: error (or clamp to `0`) on negative counts.

### 8.2 GC runs only at allocation sites — unbounded growth in tight loops **[L]**

The VM triggers GC only at allocation points; a loop that repeatedly allocates strings
or vectors without allocating in between (e.g. `concat`-free transformations, or many
small allocations below the trigger threshold) grows the heap without bound. Fix:
also check heap growth on a fixed interval (e.g. every N VM steps) or raise allocation
pressure checks.

### 8.3 Optimizer's abstract-stack model can diverge from the runtime around `break`/`continue` **[L]**

Because `break` leaves locals on the stack (bug #2), the optimizer's stack-height model
disagrees with the runtime. Once bug #2 is fixed, re-verify optimizer folding around
loop exits and re-run the optimizer test suite with the new `-O` snapshots.

### 8.4 Integer arithmetic wraps silently **[L]**

`9223372036854775807 + 1` → `-9223372036854775808` with no overflow detection
(`t46.l3`). Decide whether wrapping (current) or an error is desired; document the
choice.

### 8.5 `5.0 / 0.0` = `inf` while `5 / 0` errors — inconsistent IEEE/error semantics **[L]**

Float division/modulo by zero follow IEEE (`1.0/0.0` → `inf`, `5.0%0.0` → `NaN`),
while integer division/modulo by zero raise errors. This is currently documented as
intended; confirm and keep, or unify.

---

## Reference: line map

| Bug | Primary location(s) |
| --- | --- |
| #1 continue | l3_compiler/src/lib.rs:796-799, 728 |
| #2 break leak | l3_compiler/src/lib.rs:1089-1097 (vs. 1098-1114) |
| #3 upvalue asymmetry | l3_vm/src/lib.rs:440-444, 451-459, 772-787; l3_bytecode/src/optimizer.rs:471 |
| #4 equality | l3_runtime/src/heap_data.rs:284-291; l3_bytecode/src/optimizer.rs:655-662 |
| #5 multi-name let | l3_compiler/src/lib.rs:497-516 |
| #6 break outside loop | l3_compiler/src/lib.rs:24, 1089-1114 |
| #7 __trigger_gc | l3_vm/src/builtins.rs:371-374 |
| #8 latent | l3_vm/src/builtins.rs:195; l3_vm GC call sites |
