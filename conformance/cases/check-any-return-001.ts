// P0.9 — `function f(...): any { return ... }` accepts any
// concrete return shape per TS spec. Pre-fix the
// desugar_implicit_generics path treated explicit `: any`
// return as a generic-T placeholder (allocating __T<n>
// TypeVar) which then either:
//   (a) attempted body-return sniff to constrain the TypeVar
//       to a single concrete type (worked for the trivial
//       `return x` shape from P0.1's first commit but failed
//       on multi-return shapes like `if (b) return 5; return
//       "hi"`), or
//   (b) left __T<n> unbound, requiring per-call-site mono
//       inference which can't pick when the body's returns
//       disagree.
// Either way, multi-return Any-typed functions hit 'could
// not infer type parameter __T1 for f' at every call site.
//
// Implementation:
// * ast.rs desugar_implicit_generics: explicit `: any`
//   return now stays as literal "any" (Type::Any). User who
//   wants per-call-site mono writes `function id<T>(x: T): T`
//   explicitly. Note: P0.1's earlier change for the
//   `function f(x: any): any { return x }` single-return
//   trivial case keeps working because the body ends up
//   compatible with both generic and Any return paths.
// * check.rs: return-type assignability check now goes through
//   is_assignable_to_resolved instead of strict equality so
//   Any-typed return accepts concrete returned values.
// * ssa_lower: Stmt::Return for an Any-declared return slot
//   boxes the concrete returned operand via box_to_any
//   before the SSA Ret. Without this the calling-ABI would
//   see a raw primitive in an Any-shaped slot and segfault
//   on first deref.

// Multi-return Any function — the canonical test262 assert
// shape that motivated this commit.
function maybe(b: boolean): any {
  if (b) return 5
  return "hi"
}
console.log(maybe(true))                     // 5
console.log(maybe(false))                    // hi

// Trivial passthrough — works through the same Any-return
// path now.
function passthrough(x: any): any {
  return x
}
console.log(passthrough(42))                 // 42
console.log(passthrough("ok"))               // ok
console.log(passthrough(true))               // true

// Conditional with mixed types in branches.
function classify(n: number): any {
  if (n < 0) return "negative"
  if (n === 0) return null
  return n
}
console.log(classify(-5))                    // negative
console.log(classify(0))                     // null
console.log(classify(99))                    // 99

// Returning an Any-typed local — already-boxed value passes
// through unchanged.
function echo(x: any): any {
  let y: any = x
  return y
}
console.log(echo(7))                         // 7
console.log(echo("here"))                    // here

// Computed Any return (boxed via P0.6 +).
function build(prefix: string, n: number): any {
  return prefix + n
}
console.log(build("v=", 42))                 // v=42
