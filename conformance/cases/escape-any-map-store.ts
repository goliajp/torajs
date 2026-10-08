// A `Map<K, number>` that is stored an element of a typed array whose
// alias escaped into the `any` world, or the argument of a function
// value called through `any`, holds the value as it is: `get` answers
// what was stored.

const d: number[] = [1, 2, 3]
const x: any = d
x[1] = "s"

const m = new Map<string, number>()
m.set("a", d[0])
m.set("b", d[1])
console.log(m.get("a"), m.get("b"), typeof m.get("b"), m.size)
let total = 0
total = total + m.get("a")! + m.get("b")!
console.log(total)

const seen = new Map<number, number>()
const note: any = (k: number, v: number) => {
  seen.set(k, v)
}
note(1, "one")
note(2, 2)
console.log(seen.get(1), seen.get(2), JSON.stringify([...seen.values()]))

// a map no such value reaches keeps its number lane
const counts = new Map<string, number>()
counts.set("x", 1)
counts.set("x", counts.get("x")! + 1)
console.log(counts.get("x"), typeof counts.get("x"), counts.get("y"))
