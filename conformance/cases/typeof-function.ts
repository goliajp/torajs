// V3-18 m1.h.7 — JS spec §13.5.3 typeof returns "function" for
// any callable. Tora previously bucketed Closure / FnSig under
// "object" — divergent from bun on every typeof check of a
// function value.
let f = function() { return 5 }
console.log(typeof f)

function g() { return 10 }
console.log(typeof g)

let arr = (a: number, b: number) => a + b
console.log(typeof arr)
