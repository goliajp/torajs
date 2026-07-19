// `<key> in <fn>` with a statically Function-typed rhs — no `any`
// on the path. The checker used to reject the shape outright
// ("`in` rhs must be Array, Struct, or any"); bun runs it. The
// lowering boxes the closure cell and takes the Any kernels' full
// HasProperty face (own + prototype chain).

function g(a: number): number {
  return a * 2;
}

console.log("call" in g); // true (Function.prototype)
console.log("apply" in g); // true
console.log("bind" in g); // true
console.log("name" in g); // true (own)
console.log("length" in g); // true (own)
console.log("toString" in g); // true (chain root)
console.log("hasOwnProperty" in g); // true
console.log("constructor" in g); // true
console.log("nope" in g); // false
console.log(0 in g); // false

// the value-use mark must not break the direct call path
console.log(g(21)); // 42
