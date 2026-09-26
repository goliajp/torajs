// the callback loop reads length once and asks HasProperty before each
// visit (§23.1.3.15); when the loop's receiver is an alias of the array
// the callback shortens or punches, the alias sees the same cell
const c: number[] = [1, 2, 3, 4]
const d = c
let seen = 0
d.forEach((v) => { seen++; if (v === 1) c.length = 2 })
console.log(seen, c.length)
const e: number[] = [1, 2, 3, 4]
const f = e
let seen2 = 0
f.forEach((v) => { seen2++; if (v === 1) delete e[2] })
console.log(seen2)
const g: string[] = ["a", "b", "c"]
let h: string[] = []
h = g
const out: string[] = []
h.forEach((v) => { out.push(v); if (v === "a") g.pop() })
console.log(out.join(","))
