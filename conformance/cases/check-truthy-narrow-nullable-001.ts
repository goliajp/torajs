// V3-18 wedge — truthy-narrow on Nullable<T> in `if (x)` /
// `if (!x)` cond shapes. Per JS spec §7.1.2 ToBoolean: `null`
// is falsy and `undefined` is falsy, so `if (s) ...` where
// `s: T | null` proves the then-branch sees a non-null T.
// This is the canonical TS idiom (more common than the
// explicit `if (s !== null)`); pre-fix tora's narrow tracker
// only matched literal `=== null` / `!== null` BinOp shapes,
// so `if (s) return s.length` rejected with `no member
// .length on type Nullable(String)`.
//
// Implementation:
// * check.rs's `collect_null_narrow` gains a second cond
//   shape: bare `Expr::Ident(n)` → polarity=true (then-branch
//   narrows), and `!Expr::Ident(n)` → polarity=false (else-
//   branch narrows). Both fire only when the binding's
//   declared type is Nullable<T>; non-nullable types fall
//   through unchanged.
// * ssa_lower's `coerce_to_bool` for Str / Substr previously
//   loaded `len` at offset 8 unconditionally, which segfaults
//   when the operand is NULL. Replaced with a 3-block CFG
//   that null-checks first, then loads len in the non-null
//   branch and stores the result through a slot.
// * The heap-pointer fallback in `coerce_to_bool` (Obj / Arr /
//   Closure / RegExp / Date / BigInt / ...) used to return a
//   ConstBool(true) under the assumption that these values
//   always come from `new` / literal alloc. Truthy-narrow on
//   Nullable<Obj> breaks that, so the fallback now does an
//   explicit `ptr != null` icmp — same semantics for non-null
//   inputs, correct for nullable inputs.
//
// The else-branch keeps the binding at its declared Nullable
// type — narrowing to a "null-only" branch is unimplementable
// without a `null` type and is rare in practice; the existing
// `=== null` narrow stays the way to express "the value is
// definitely null here".

// Nullable<String> — the canonical case.
function lenOrZero(s: string | null): number {
  if (s) return s.length
  return 0
}
console.log(lenOrZero("hello"))                // 5
console.log(lenOrZero(null))                   // 0
console.log(lenOrZero(""))                     // 0   "" is falsy

// `!s` form — else-branch narrows.
function notEmpty(s: string | null): string {
  if (!s) return "<empty>"
  return s
}
console.log(notEmpty("ok"))                    // ok
console.log(notEmpty(null))                    // <empty>
console.log(notEmpty(""))                      // <empty>

// Nullable<Number>.
function doubleOrMinusOne(n: number | null): number {
  if (n) return n * 2
  return -1
}
console.log(doubleOrMinusOne(5))               // 10
console.log(doubleOrMinusOne(null))            // -1
console.log(doubleOrMinusOne(0))               // -1   0 is falsy

// Nullable<Struct> — exercises the heap-pointer ToBoolean.
type Box = { v: number }
function unwrap(b: Box | null): number {
  if (b) return b.v
  return -999
}
console.log(unwrap({ v: 42 }))                 // 42
console.log(unwrap(null))                      // -999

// Explicit !== null still works (regression check).
function dual(s: string | null): number {
  if (s !== null) return s.length
  return -1
}
console.log(dual("hi"))                        // 2
console.log(dual(null))                        // -1

// Chained: truthy narrow + further field access.
type Wrap = { inner: string }
function unwrapInner(w: Wrap | null): number {
  if (w) return w.inner.length
  return 0
}
console.log(unwrapInner({ inner: "abcd" }))    // 4
console.log(unwrapInner(null))                 // 0

// Nullable<Array>.
function firstOr(xs: number[] | null): number {
  if (xs) return xs[0]
  return -1
}
console.log(firstOr([10, 20, 30]))             // 10
console.log(firstOr(null))                     // -1
