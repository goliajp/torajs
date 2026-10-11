// a function union bound without an annotation: a generic member, reassignment,
// a typeof test, a callback and a capture all call the function held
function id<T>(x: T): T { return x }
function inc(n: number) { return n + 1 }
function label(n: number) { return "#" + n }
let c = process.argv.length > 5
console.log((c ? id : inc)(4))
let g = c ? inc : label
console.log(g(1))
g = inc
console.log(g(2))
if (typeof g === "function") console.log(g(3))
console.log([1, 2].map(g).join(","))
const h = () => g(9)
console.log(h())
