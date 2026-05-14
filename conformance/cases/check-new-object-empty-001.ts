// P1 — `new Object()` 0-arg form per ES spec §20.1.1.1.
// Returns a fresh empty object literal `{}`. Pre-fix tora bailed
// at 'unknown identifier `__new_Object`' since the class-lowering
// desugar synthesizes `__new_C` factories for user classes only
// — built-in Object has no factory. Test262 uses `new Object()`
// pervasively in array-method fixtures (~12+ cases blocked
// across the broader sample under built-ins/Array/prototype/*).
//
// The 1-arg form (`new Object(value)`) is the wrap-or-return-as-
// is shape and is deferred — it requires runtime type
// discrimination and overlaps with the property-bag substrate
// (P3).
//
// Implementation: ast.rs `desugar_builtin_new` Pass — for each
// `Expr::New { class_name = "Object", args.is_empty() }`, rewrite
// in place to `Expr::ObjectLit { fields: vec![] }`. The empty-
// struct downstream typecheck and ssa-lower both accept the
// shape (Type::Struct with no fields).

let o = new Object()
console.log(typeof o)                        // object

// Used as a marker / unique reference.
let a: any = new Object()
let b: any = new Object()
console.log(a === b)                         // false (different refs)
console.log(a === a)                         // true (same ref)
