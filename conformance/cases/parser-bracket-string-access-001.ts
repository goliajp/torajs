// V3-18 wedge — `obj["x"]` ≡ `obj.x` per JS spec §13.3.2 when
// "x" parses as a valid IdentifierName. Pre-fix tora rejected
// the bracket form on struct receivers with 'index must be
// number' since the Expr::Index typecheck only accepted
// Type::Number indices for Type::Array / Type::String. The
// idiom is canonical TS (often appears when the source key is
// a const-folded literal: `const KEY = "x"; obj[KEY]`) and
// shows up everywhere in TS code that copies object access
// patterns from JS.
//
// Implementation:
// * Folded at parse time. parser.rs's postfix-bracket arm
//   peeks the parsed index expression: when it's an
//   Expr::String whose content is a syntactic IdentifierName
//   (per is_identifier_name — ASCII letter/_/$ first byte,
//   alnum/_/$ rest), the node turns into Expr::Member { obj,
//   name } instead of Expr::Index { obj, index: Expr::String }.
// * Routing through Member at the AST level inherits every
//   downstream behavior for free: typecheck (struct field
//   resolution + result type), lower (struct field load /
//   refcount), write-side (`obj["x"] = v`), Member-call
//   dispatch (`obj["push"](v)` if push were a method on the
//   struct shape — currently doesn't apply but the wiring is
//   transitive), nested chaining (`grid["row"]["x"]`), and
//   the existing .name / ["name"] interchangeability that TS
//   relies on. No check.rs / ssa_lower.rs change needed.
// * Non-identifier strings (`obj[""]`, `obj["a-b"]`, `obj["1"]`)
//   stay as Index and continue to hit the existing Array /
//   String index paths — same behavior as before for everything
//   the wedge isn't meant to cover.

// Object literal — the canonical case.
let p = { x: 1, y: 2, z: 3 }
console.log(p["x"])                            // 1
console.log(p["y"])                            // 2
console.log(p["z"])                            // 3

// Class instance — same path goes through.
class Pt {
  x: number
  y: number
  constructor(x: number, y: number) { this.x = x; this.y = y }
}
let pt = new Pt(10, 20)
console.log(pt["x"])                           // 10
console.log(pt["y"])                           // 20

// Nested bracket chains.
let grid = { row: { x: 100, y: 200 } }
console.log(grid["row"]["x"])                  // 100
console.log(grid.row["y"])                     // 200

// Bracket assignment — write-side fold goes through too.
let q = { a: 1, b: 2 }
q["a"] = 99
console.log(q.a, q.b)                          // 99 2

// Mixed bracket / dot in the same chain.
let r = { foo: { bar: 42 } }
console.log(r["foo"].bar)                      // 42
console.log(r.foo["bar"])                      // 42

// Regression — numeric index on Array stays Index.
let xs = [10, 20, 30]
console.log(xs[0], xs[1], xs[2])               // 10 20 30

// Regression — numeric index on String stays Index.
console.log("hello"[0])                        // h
console.log("hello"[4])                        // o

// Identifier name with $ / _ — also valid per spec, must fold.
let dollar = { $foo: 1, _bar: 2, x_y: 3 }
console.log(dollar["$foo"])                    // 1
console.log(dollar["_bar"])                    // 2
console.log(dollar["x_y"])                     // 3
