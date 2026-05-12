// V3-18 wedge — TS type-predicate return type:
//   function isT(v: P): v is T { ... }
// Per TS spec §3.6.5, the return is `boolean` at the value
// level; the `is T` half is a flow-narrowing hint for callers
// (so an `if (isT(x))` block treats `x` as `T`). Pre-fix tora's
// parser bailed with 'expected `{` (function body), got
// Ident("is")' because `v` was parsed as the bare return type
// and the next ident was unexpected.
//
// Implementation: parse_type_ann's entry detects the shape
// `<Ident> "is" <Type>` and accepts it as a return-type-only
// special case, discarding the predicate and returning
// "boolean". Subset limitation: callers do NOT yet see the
// narrowing — the `is T` half is type-side only and not
// plumbed into check.rs flow analysis. Most user code that
// uses type-predicates either:
//   (a) doesn't rely on the post-call narrowing (just gates
//       behavior on the bool), or
//   (b) supplements with an `as T` cast inside the branch.

function isStringy(v: string): v is string {
  return v.length > 0
}
console.log(isStringy("hi"))           // true
console.log(isStringy(""))             // false

function isPositive(n: number): n is number {
  return n > 0
}
console.log(isPositive(5))             // true
console.log(isPositive(-1))            // false

// Predicate over a struct shape — the common TS use site.
type O = { kind: string; name: string }
function isNamed(v: O): v is O {
  return v.name.length > 0
}
let a: O = { kind: "user", name: "alice" }
console.log(isNamed(a))                // true
let b: O = { kind: "user", name: "" }
console.log(isNamed(b))                // false

// Method form on a class — class method's return-type follows
// the same parse path.
class Filter {
  constructor(public min: number) {}
  matches(n: number): n is number {
    return n >= this.min
  }
}
let f = new Filter(10)
console.log(f.matches(15))             // true
console.log(f.matches(5))              // false
