// `<key> in <typed-struct>` per ES §13.10.1 — pre-fix tora rejected
// typed-struct rhs at typecheck with "in rhs must be Array or any
// (subset stub); got Struct([...])". Typed struct fields are statically
// known from the layout, so a literal-string key folds to a compile-
// time boolean (no runtime dispatch). Non-literal keys aren't sensible
// against a typed struct (no dynamic field set) and stay rejected at
// ssa-lower time.
//
// Implementation:
//   * check.rs `__torajs_in_op` arm now accepts Struct(_) rhs (and
//     returns Boolean).
//   * ssa_lower.rs `__torajs_in_op` dispatch adds a Type::Obj(sid) arm
//     ahead of the existing Type::Any branch. Looks up the struct
//     layout, then folds `Expr::String(s)` keys to Operand::ConstBool
//     (s present in the field list). Non-literal keys panic at lower
//     time with a clear message.

const o = { a: 1, b: 2, c: "hello" };
console.log("a" in o);   // true
console.log("b" in o);   // true
console.log("c" in o);   // true
console.log("d" in o);   // false
console.log("" in o);    // false

// Boolean operator forms.
if ("a" in o) console.log("has-a");
if (!("z" in o)) console.log("no-z");

// Mixed field types resolve the same — membership only looks at names.
type Box = { x: number; y: string; z: boolean };
const b: Box = { x: 1, y: "yy", z: true };
console.log("x" in b);   // true
console.log("y" in b);   // true
console.log("w" in b);   // false
