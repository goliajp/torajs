// named functions into a method name unrelated classes share: through
// `any`, and forwarded from a fn-typed param
class K { m(g: () => number) { return g() + 1 } }
class L { m(g: () => number) { return g() * 3 } }
function seven(): number { return 7 }
const a: any = new K()
const b: any = new L()
console.log(a.m(seven), b.m(seven))
const k = new K()
const l = new L()
function o(g: () => number) { return k.m(g) }
function p(g: () => number) { return l.m(g) }
console.log(o(seven), p(seven), o(() => 2), p(() => 2))
const n = 4
console.log(o(() => n), p(() => n))
