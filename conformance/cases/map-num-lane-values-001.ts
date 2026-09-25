// for-of over values() of a Map<K, number> lane binding steps f64s;
// a stored undefined still reads as undefined, deletes during the
// walk are skipped, and a non-lane receiver answers the same

const m = new Map<string, number>()
const xs: number[] = [1]
m.set("a", 1)
m.set("b", 2.5)
m.set("hole", xs[4])
m.set("c", -3)
let total = 0
let holes = 0
for (const v of m.values()) {
  if (v === undefined) {
    holes = holes + 1
  } else {
    total = total + v
  }
}
console.log(total, holes)

let kinds = ""
for (const v of m.values()) {
  kinds = kinds + typeof v + ","
}
console.log(kinds)

// a delete mid-walk: the later entry is not visited
let seen = 0
for (const v of m.values()) {
  if (v === 1) m.delete("c")
  seen = seen + 1
}
console.log(seen, m.size)

// the iterator bound by hand, then walked
const it = m.values()
let sum = 0
for (const v of it) {
  sum = sum + (v ?? 0)
}
console.log(sum)

// NaN sums through the undefined entry the way bun's does
let raw = 0
for (const v of m.values()) {
  raw = raw + (v as number)
}
console.log(raw)

// integers only: the accumulator still reads every value
const counts = new Map<number, number>()
for (let i = 0; i < 10; i = i + 1) {
  counts.set(i % 3, (counts.get(i % 3) ?? 0) + i)
}
let all = 0
for (const c of counts.values()) {
  all = all + c
}
console.log(all, counts.get(0), counts.get(2))

// a parameter's Map keeps the any lane and agrees
function sumOf(p: Map<string, number>): number {
  let s = 0
  for (const v of p.values()) {
    if (v !== undefined) s = s + v
  }
  return s
}
console.log(sumOf(m))
