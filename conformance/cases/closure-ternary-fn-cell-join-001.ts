// a destructured fn-typed field with a top-level function as its
// default joins the default and a closure read out of the field as a
// closure
function dbl(x: number): number { return x * 2 }
function o({ g = dbl }: { g?: (x: number) => number }): number { return g(1) }
const k = 10
console.log(o({}), o({ g: (x: number) => x + k }), o({ g: dbl }))
let s = 0
for (let i = 0; i < 1000; i++) s += i % 2 ? o({ g: (x: number) => x + i }) : o({})
console.log(s)
