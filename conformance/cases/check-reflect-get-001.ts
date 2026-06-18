// `Reflect.get(target, key)` per ES §28.1.6 — pre-fix tora rejected
// the call entirely with "no member `.get` on type Object('Reflect')".
// Reflect.has was already aliased to Object.hasOwn; this commit adds
// Reflect.get's compile-time fold for the typed-struct + literal-key
// subset (the common case where the lookup answer is statically known
// from the struct layout).
//
// Implementation:
//   * check.rs adds `(Type::Object("Reflect"), "get")` sig
//     `(Any, String) → Any`.
//   * ssa_lower.rs adds a polymorphic Reflect.get dispatch at the
//     `__torajs_in_op`-adjacent Call-level path. When args[0] is
//     `Type::Obj(sid)` and args[1] is `Expr::String(key_lit)`:
//       - Field present → struct field Load + box_to_any (rc_inc
//         refcounted fields before boxing since the caller will drop
//         the boxed Any).
//       - Field absent → ANY_UNDEF=5 box (per spec §28.1.6).
//   * Dynamic key or non-struct target panic at lower-time
//     (substrate follow-up).

const o = { a: 1, b: "hello", c: true };
console.log(Reflect.get(o, "a"));    // 1
console.log(Reflect.get(o, "b"));    // hello
console.log(Reflect.get(o, "c"));    // true
console.log(Reflect.get(o, "d"));    // undefined

// Reflect.get + named-alias struct type.
type Point = { x: number; y: number };
const p: Point = { x: 3, y: 4 };
console.log(Reflect.get(p, "x"));    // 3
console.log(Reflect.get(p, "y"));    // 4
console.log(Reflect.get(p, "z"));    // undefined

// Multi-call patterns — confirm the boxed Any survives chained ops.
const r1 = Reflect.get(o, "a");
const r2 = Reflect.get(o, "b");
console.log(r1, r2);
