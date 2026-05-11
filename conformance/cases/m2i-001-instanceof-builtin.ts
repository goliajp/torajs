// V3-18 m2.i — `<expr> instanceof <BuiltinCtor>` per JS spec.
// Static SSA-type → ConstBool fold. Per spec primitives are NOT
// instances of their boxing constructor (e.g. `5 instanceof Number
// === false`); only `new Number(5)` would be true. Tora's subset
// doesn't ship boxed primitives so they're always false.

console.log(5 instanceof Number)              // false (primitive)
console.log("hi" instanceof String)            // false
console.log(true instanceof Boolean)           // false
console.log(100n instanceof BigInt)            // false
console.log(Symbol("x") instanceof Symbol)     // false

// Array — true for any Array<T>.
console.log([1,2,3] instanceof Array)          // true
let arr: number[] = []
console.log(arr instanceof Array)              // true

// Class instance — Object always true.
class Foo { x: number; constructor(x: number) { this.x = x } }
let f = new Foo(5)
console.log(f instanceof Foo)                  // true
console.log(f instanceof Object)               // true
console.log(f instanceof Array)                // false

// Function — closures and fn-sigs are instances. Functions are
// also Objects per spec.
let g = () => 1
console.log(g instanceof Function)             // true (closure)
console.log(g instanceof Object)               // true (functions are Objects)

// Cross-tests.
console.log([1,2] instanceof Object)           // true (Arr is Object)
console.log([1,2] instanceof Number)           // false
