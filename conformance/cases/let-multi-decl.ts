// V3-18 m1.h.5 — JS spec §14.3.1 multi-decl `let`. A single
// `let` can declare several bindings separated by commas, each
// with its own optional type annotation and initializer.
let a = 10, b = 20, c = 30;
console.log(a + b + c)

let x: number = 1, y: number = 2;
console.log(x * y)

const p = 7, q = 11;
console.log(p + q)
