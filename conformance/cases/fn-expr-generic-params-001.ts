// a generic function expression runs as the same function typed per call
const ge = function <T>(x: T): T { return x }
console.log(ge(4), ge("q"))
const named = function wrap<A, B extends A[]>(a: A, b: B): number { return b.length + (a ? 1 : 0) }
console.log(named(1, [1, 2]))
const obj = { m: function <K = string>(k: K) { return typeof k } }
console.log(obj.m(1), obj.m("s"))
const gen = function* <T>(xs: T[]) { for (const x of xs) yield x }
console.log([...gen([7, 8])].join())
