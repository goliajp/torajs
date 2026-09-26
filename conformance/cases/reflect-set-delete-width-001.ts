// Reflect.set is the target's [[Set]] and Reflect.deleteProperty its
// [[Delete]] (§28.1.12, §28.1.4): on an array they write an element and
// leave a hole exactly as t[k] = v and delete t[k] do, including the hole
// a callback punches into the array its loop walks.
const ys = [1, 2, 3]
console.log(Reflect.set(ys, 1, 0.25), ys.join(","))
const a: number[] = [1, 2, 3, 4, 5]
let seen: number[] = []
a.forEach((v, i) => {
  if (i === 0) Reflect.deleteProperty(a, 3)
  seen.push(v)
})
console.log(seen.join(","), 3 in a, a.length)
const b = [10, 20, 30]
console.log(Reflect.deleteProperty(b, 1), 1 in b, b[1])
for (const v of b) console.log(typeof v)
const s: string[] = ["p", "q", "r"]
console.log(s.filter((x, i) => { if (i === 0) Reflect.deleteProperty(s, "1"); return true }).join("|"))
