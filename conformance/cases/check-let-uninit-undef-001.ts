// P1.3 — `let x;` (no init) gives x the value `undefined` per
// ES spec §8.1 / §14.3.2. Pre-P1 tora returned Type::Null for
// uninit bindings (collapsed with undefined at the runtime).
// Now Type::Undefined first-class; uninit slots correctly
// typeof as "undefined", strict-eq distinguishes from null.
//
// Implementation: check.rs Expr::Uninit returns Type::Undefined
// (was Type::Null). The downstream box_to_any_from_expr (shipped
// with the P1.1+1.2+1.5 commit) already routes Type::Undefined
// sources through the ANY_UNDEF=5 tag, so the runtime-side
// behavior inherits the spec-correct distinction.

let x: any;
console.log(typeof x)                        // undefined
console.log(x === undefined)                 // true
console.log(x === null)                      // false

// Uninit binding flowing through coercion in any context.
let y: any;
console.log(typeof y)                        // undefined

// Multi-decl with one uninit, one initialized.
let p: any, q: any = 42;
console.log(typeof p)                        // undefined
console.log(typeof q)                        // number
console.log(q)                               // 42
