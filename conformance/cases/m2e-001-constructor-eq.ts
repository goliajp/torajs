// V3-18 m2.e — `<prim>.constructor === <Ctor>` compile-time fold.
// Tora has no first-class function ref for namespace ctors
// (bare `Number` / `String` etc can't lower as a value), so the
// canonical test262 idiom `n.constructor === Number` is folded
// at AST level by comparing the prim type tag to the ctor name.

let n = 5
console.log(n.constructor === Number)        // true
console.log(n.constructor === String)         // false
console.log(n.constructor === Boolean)        // false
console.log(n.constructor !== Number)         // false
console.log(n.constructor !== String)         // true

let s = "hi"
console.log(s.constructor === String)         // true
console.log(s.constructor === Number)         // false

let b = true
console.log(b.constructor === Boolean)        // true

let bi = 100n
console.log(bi.constructor === BigInt)        // true

let sym = Symbol("x")
console.log(sym.constructor === Symbol)       // true

// Reverse argument order also folds.
console.log(Number === n.constructor)         // true
console.log(String === s.constructor)         // true
