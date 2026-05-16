// T-45 — JS binary `in` operator: `<key> in <obj>` returns true if
// `key` (number) is a property of `obj`. ECMAScript §13.10.2 — for
// arrays this means index ∈ [0, length). 7+ test262 cases under
// built-ins/Array/prototype/* blocked pre-fix on parser rejecting
// `in` as a binary operator (treated as bare ident, hit "expected
// `)`, got Ident("in")").
//
// Implementation: parser detects `<expr> in <expr>` at relational
// precedence and emits a synthetic Call to `__torajs_in_op(key,
// obj)` (avoiding a new AST variant that every recursive walker
// would need to handle). check.rs intercepts by name → returns
// Boolean. ssa_lower intercepts → dispatches on obj's static SSA
// type. For Type::Arr emits inline bounds check; for Type::Any
// routes to dynobj_has via the box's value@16 dynobj ptr.
//
// Out of scope (subset): Type::Struct (compile-time field-name
// check); Type::Closure / FnSig (fnprops_has); Type::String
// (character-index check). Other rhs types panic with clear msg.

let arr = [10, 20, 30];
console.log(0 in arr);    // true
console.log(1 in arr);    // true
console.log(2 in arr);    // true
console.log(3 in arr);    // false
console.log(-1 in arr);   // false

// Empty array.
let empty: number[] = [];
console.log(0 in empty);  // false

// Larger index past length.
console.log(100 in arr);  // false
