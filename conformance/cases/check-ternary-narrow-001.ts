// V3-18 wedge — flow narrowing on Nullable<T> applies to
// `cond ? then : else` ternary expressions, not just `if (...)`
// statements. Pre-fix tora's check.rs only narrowed inside
// Stmt::If; the same null-narrow info was thrown away on
// Expr::Ternary, so the canonical TS pattern
// `s ? s.length : 0` rejected with 'no member .length on type
// Nullable(String)' even though the if-stmt rewrite
// `if (s) return s.length; return 0` typechecked just fine.
//
// Implementation:
// * Mirror the Stmt::If narrow protocol in Expr::Ternary's
//   type_of: collect_null_narrow on the cond, narrow the
//   matching branch (then if polarity=true, else if
//   polarity=false), restore the binding's declared type
//   between the two branches' typecheck calls. The narrow
//   helper covers all four shapes: `s !== null` / `s === null`
//   (legacy) and bare `s` / `!s` (truthy-narrow wedge from
//   the previous commit).
// * No ssa_lower change needed — the runtime path for
//   Ternary already evaluates the cond and dispatches to the
//   matching branch; only the type-system narrow that
//   unblocks the field access in the then-branch was missing.

// Truthy ternary on Nullable<String>.
function lenOr(s: string | null, d: number): number {
  return s ? s.length : d
}
console.log(lenOr("hello", -1))                // 5
console.log(lenOr(null, -1))                   // -1
console.log(lenOr("", -1))                     // -1   "" is falsy

// `!s` ternary — else-branch narrows.
function ezNeg(s: string | null): number {
  return !s ? -1 : s.length
}
console.log(ezNeg("hi"))                       // 2
console.log(ezNeg(null))                       // -1

// Nullable struct in ternary.
type Box = { v: number }
function getOr(b: Box | null, d: number): number {
  return b ? b.v : d
}
console.log(getOr({ v: 7 }, -1))               // 7
console.log(getOr(null, -1))                   // -1

// Explicit `!== null` in ternary still narrows (regression
// check — the legacy BinOp narrow shape goes through the
// same collect_null_narrow path).
function explicit(s: string | null): number {
  return s !== null ? s.length : 0
}
console.log(explicit("abc"))                   // 3
console.log(explicit(null))                    // 0

// Nested ternary — outer truthy narrow exposes b.v inside the
// inner ternary so b.v > 0 typechecks against Number.
function inner(b: Box | null): string {
  return b ? (b.v > 0 ? "pos" : "non-pos") : "none"
}
console.log(inner({ v: 5 }))                   // pos
console.log(inner({ v: -1 }))                  // non-pos
console.log(inner(null))                       // none

// Nullable<Array> in ternary.
function firstOr(xs: number[] | null, d: number): number {
  return xs ? xs[0] : d
}
console.log(firstOr([10, 20, 30], -1))         // 10
console.log(firstOr(null, -1))                 // -1
