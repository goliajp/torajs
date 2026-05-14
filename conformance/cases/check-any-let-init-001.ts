// P0.1 (extension) — `let x: any = expr` accepts any concrete-typed
// initializer per TS spec. Pre-fix tora's check.rs strict
// is_assignable_to_resolved rejected with 'type mismatch on x:
// declared Any, init has Number' because the lattice was missing
// the "anything → Any" rule (only had "Any → anything").
//
// Implementation:
// * check.rs: is_assignable_to_resolved gains a `to == Any` arm
//   (mirror of the existing `from == Any`). Per TS spec everything
//   is assignable to Any.
// * ssa_lower: when a Type::Any slot is being initialised with a
//   concrete-typed operand, route through the new box_to_any
//   helper. The helper extracts the runtime tag (ANY_NULL=0,
//   ANY_BOOL=1, ANY_I64=2, ANY_F64=3, ANY_HEAP=4) from the
//   operand's SSA type, packs the payload (with bitcast for f64,
//   zext for bool, raw for ptr / i64), and calls
//   __torajs_any_box(tag, value) — the existing 24-byte heap
//   Any-box helper that's been the runtime substrate since T-10.
//
// This is the foundational sub-item of the P0 phase: every later
// Any-aware operation (typeof, ===, BinOp, Member) will assume
// Any-typed bindings carry a real boxed pointer at the SSA layer.
//
// Subset notes:
// * typeof / === / BinOp on Any operands aren't yet wired (P0.3 /
//   P0.6 / P0.7); fixture exercises only print of Any-typed
//   bindings. Array / Obj boxed into Any prints as "[object]"
//   placeholder — full pretty-print arrives with P0.4 (Member /
//   Index access on Any).
// * The boxing path goes through the heap Any-box (24 bytes per
//   value); the v4 trunk's "16-byte tagged stack slot" idea was
//   superseded by adopting the existing Any-box runtime so we
//   don't churn the layout under existing T-10 fixtures.

let x: any = 5
console.log(x)                               // 5

let s: any = "hello"
console.log(s)                               // hello

let b: any = true
console.log(b)                               // true

let bf: any = false
console.log(bf)                              // false

let n: any = null
console.log(n)                               // null

let f: any = 3.14
console.log(f)                               // 3.14

// Heap-typed values (Array, Obj, Closure) box correctly at
// let-init but their print falls back to "[object]" placeholder
// in tora vs bun's structured rendering — substrate diff that
// closes once P0.4 lands Any-aware Member / Index dispatch on
// the boxed payload. Not exercised here to keep the fixture
// byte-equal with bun.
