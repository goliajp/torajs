// A callback that deletes from the typed array it is walking: the
// deleted index is not visited (§23.1.3 HasProperty before each
// visit), even though the loop read the header before it started.
const a: number[] = [1, 2, 3, 4, 5]
let seen: number[] = []
a.forEach((v, i) => {
  if (i === 0) delete a[3]
  seen.push(v)
})
console.log(seen.join(","), 3 in a, a.length)

const b: number[] = [10, 20, 30, 40]
const m = b.map((v, i) => {
  if (i === 1) delete b[2]
  return v * 2
})
console.log(m.length, 2 in m, m.join("|"))

const c: string[] = ["x", "y", "z"]
const f = c.filter((s, i) => {
  if (i === 0) delete c[1]
  return true
})
console.log(f.join("|"), f.length)

const d: number[] = [1, 2, 3, 4]
let calls = 0
const sum = d.reduce((acc, v, i) => {
  calls++
  if (i === 1) delete d[3]
  return acc + v
}, 0)
console.log(sum, calls)

const e: number[] = [5, 6, 7]
let hits = 0
e.some((v, i) => {
  hits++
  if (i === 0) delete e[1]
  return false
})
console.log(hits)

