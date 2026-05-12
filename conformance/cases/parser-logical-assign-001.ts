// V3-18 wedge — ES2021 logical assignment operators: `??=`,
// `||=`, `&&=`. Each is a compound-assign whose RHS only
// evaluates when the LHS predicate matches:
//   x ??= y → assign y to x only if x is null/undefined
//   x ||= y → assign y to x only if x is falsy
//   x &&= y → assign y to x only if x is truthy
// Pre-fix tora's parser bailed at the `=` token because `??`,
// `||`, `&&` were consumed first as binary operators leaving
// a bare `=` start-of-expr.
//
// Implementation: parse_assign peeks the two-token sequence
// (??Eq, ||Eq, &&Eq); parse_nullish / parse_logical_or /
// parse_logical_and decline to consume their op when an `=`
// follows so the sequence falls through to parse_assign. The
// rhs is wrapped in Expr::Nullish / Expr::BinOp{LOr|LAnd}, the
// outer wrap is Expr::Assign.

// ??= — nullish coalescing assign.
let s: string | null = null
s ??= "default"
console.log(s)                         // default
s ??= "ignored"                        // already non-null, no-op
console.log(s)                         // default

// ||= — falsy-OR assign.
let s2 = ""
s2 ||= "fallback"
console.log(s2)                        // fallback
s2 ||= "ignored"
console.log(s2)                        // fallback

// &&= — truthy-AND assign.
let n = 5
n &&= n * 2
console.log(n)                         // 10
let n2 = 0
n2 &&= 99                              // 0 is falsy → no-op
console.log(n2)                        // 0

// Member-target form (lhs is a member access — must single-eval).
type Box = { count: number | null }
let b: Box = { count: null }
b.count ??= 42
console.log(b.count)                   // 42
b.count ??= 999
console.log(b.count)                   // 42

// String fallback via ||=.
type Doc = { title: string }
let d: Doc = { title: "" }
d.title ||= "untitled"
console.log(d.title)                   // untitled
