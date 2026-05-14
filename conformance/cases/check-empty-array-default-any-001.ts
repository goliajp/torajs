// P0.10 — bare empty `[]` literal without an annotation defaults
// to `Array<Any>` per TS spec (untyped `[]` is `any[]`). Pre-fix
// tora demanded an explicit `let xs: T[] = []` annotation; test262
// uses bare `let arr = []` pervasively (160+ cases blocked on this
// single shape across the broader sample).
//
// Implementation:
// * check.rs LetDecl arm — when init is empty `[]` and no
//   annotation, default the inferred type to `Array<Any>` instead
//   of erroring with "needs an explicit type annotation".
// * ssa_lower.rs LetDecl arm — when init is empty `[]` and no
//   annotation, intern the `Array<Any>` layout and use the
//   resulting ArrId directly. Routes through the existing empty-
//   `[]` branch (which was already gated on `Type::Arr(_)`) without
//   needing further changes.
//
// Mutating the bare `[]` (push / [] assign) writes to the Array<Any>
// pool slots — that path matches `let xs: any[] = []` and shares
// its substrate (a separate work item if it has remaining gaps).
// This fixture exercises only the read-side: bare empty `[]`,
// .length, iteration over empty.

let a = []
console.log(a.length)                        // 0

// Empty array passed to fn that reads .length — fn param can stay
// any[] / Array<any>.
function readLen(xs: any[]): number { return xs.length; }
let b = []
console.log(readLen(b))                      // 0

// Iteration over empty bare array — body never runs.
let c = []
let count: number = 0
for (let i: number = 0; i < c.length; i = i + 1) { count = count + 1; }
console.log(count)                           // 0

// Multiple bare empties in same scope (each gets its own slot).
let d = []
let e = []
console.log(d.length + e.length)             // 0
