// an object whose field is typed passed where the parameter is an
// optional or `| undefined` object with an `any` field
function g(o: { a: any } | undefined): string { return o ? String(o.a) + ":" + typeof o.a : "none" }
console.log(g({ a: "s" }), g({ a: 2 }), g(undefined))
function k(o?: { a: any; b: number }): string { return o ? o.a + "/" + (o.b + 1) : "none" }
const q = { a: true, b: 4 }
console.log(k(q), k(), k({ a: [1, 2], b: 0 }))
function m(o?: { a: any }): void { if (o) o.a = "w" }
const r = { a: 1 as any }
m(r)
console.log(r.a)
