// `d[k] = (d[k] ?? 0) + 1` on a `Record<string, number>`, the read and
// the write of one update, against every way the entry the read found
// can stop being the one the write must replace (§10.1.9.2
// OrdinarySet): a right-hand side that calls code adding a thousand
// keys or redefining the entry, a `valueOf` that does the same, a key
// expression that rebinds the dictionary between the target and the
// read, a read of another key, a non-writable or frozen entry, an
// inherited entry of the same name, and integer-like keys.
"use strict"

const ks: string[] = ["a", "b", "c", "7", "3", "x"]

const d: Record<string, number> = {}
for (let i = 0; i < 12; i = i + 1) {
  const k = ks[i % 6]
  d[k] = (d[k] ?? 0) + 1
}
console.log(JSON.stringify(d), Object.keys(d).join(","))

let added = 0
function grow(): number {
  for (let j = 0; j < 1000; j = j + 1) {
    d["g" + added] = added
    added = added + 1
  }
  return 1
}
for (let i = 0; i < 3; i = i + 1) {
  d[ks[0]] = (d[ks[0]] ?? 0) + grow()
}
console.log(d[ks[0]], Object.keys(d).length, d["g2999"])

const v = {
  valueOf(): number {
    for (let j = 0; j < 1000; j = j + 1) d["v" + j] = j
    Object.defineProperty(d, ks[1], { value: 50, writable: false })
    return 5
  },
}
try {
  d[ks[1]] = (d[ks[1]] ?? 0) + (v as any)
  console.log("wrote", d[ks[1]])
} catch (e) {
  console.log("refused", e instanceof TypeError, d[ks[1]])
}
try {
  d[ks[1]] = (d[ks[1]] ?? 0) + 1
  console.log("wrote", d[ks[1]])
} catch (e) {
  console.log("refused", e instanceof TypeError, d[ks[1]])
}

let cur: Record<string, number> = { a: 1 }
const old = cur
const other: Record<string, number> = { a: 40 }
function swap(): string {
  cur = other
  return ks[0]
}
cur[swap()] = (cur[ks[0]] ?? 0) + 1
console.log(old[ks[0]], other[ks[0]], cur === other)

const e: Record<string, number> = { a: 1, b: 20 }
e[ks[0]] = (e[ks[1]] ?? 0) + 1
e[ks[1]] = -(e[ks[1]] ?? 0)
const w = 3
e[ks[1]] = (e[ks[1]] ?? 0) * w
e[ks[0]] += 0.5
console.log(JSON.stringify(e))

const f: Record<string, number> = Object.freeze({ a: 1 })
try {
  f[ks[0]] = (f[ks[0]] ?? 0) + 1
} catch (err) {
  console.log("frozen", err instanceof TypeError, f[ks[0]])
}

const proto: Record<string, number> = { a: 100 }
const child: Record<string, number> = Object.create(proto)
for (let i = 0; i < 3; i = i + 1) child[ks[0]] = (child[ks[0]] ?? 0) + 1
console.log(child[ks[0]], proto[ks[0]], Object.keys(child).join(","))

const acc: Record<string, number> = {}
let sets = 0
Object.defineProperty(acc, ks[2], {
  get() {
    return 7
  },
  set(_x: number) {
    sets = sets + 1
  },
  configurable: true,
})
for (let i = 0; i < 4; i = i + 1) acc[ks[2]] = (acc[ks[2]] ?? 0) + 1
console.log(acc[ks[2]], sets)
