// A dynamic-key write on a `Record<string, number>` stores the value
// it is given, whatever the declared value type says: a borrowed
// `any` holding a string, an object or a split piece keeps its own
// reference, a number over a string and a string over a number both
// read back, a double, -0 and a value past int32 keep their bits, and
// a write through globalThis leaves a module's top-level `var` alone
// (§9.1.1.5: a module keeps its bindings in its own environment).
const ks: string[] = ["a", "b", "c", "v"]
const d: Record<string, number> = { a: 1, b: 2, c: 3 }
const s: any = "xy"
const big: any = "a string long enough to live in a heap cell"
const o: any = { n: 1 }
d[ks[0]] = s
d[ks[1]] = big
d[ks[2]] = o
console.log(d[ks[0]], d[ks[1]], JSON.stringify(d[ks[2]]), s, big, o.n)
o.n = 2
console.log(JSON.stringify(d[ks[2]]), (d[ks[0]] ?? 0) + 1)
d[ks[0]] = 4
d[ks[1]] = 5
d[ks[2]] = 6
console.log(d[ks[0]] + d[ks[1]] + d[ks[2]], big.length, o.n)
const parts: any = "p,q,r".split(",")
d[ks[0]] = parts[1]
console.log(d[ks[0]], parts.join("|"))
d[ks[1]] = 2147483648
d[ks[2]] = -0
console.log(d[ks[1]], Object.is(d[ks[2]], -0), d[ks[1]] - 1)
d[ks[1]] = 0.25
console.log(d[ks[1]] * 4, JSON.stringify(d))
var v = 1
const g: Record<string, number> = globalThis as any
g[ks[3]] = 7
console.log(v, g[ks[3]])
