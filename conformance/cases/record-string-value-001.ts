// `Record<string, V>` and `{ [k: string]: V }` bindings: writes by a
// literal, a dynamic and a dotted key, Object.entries / keys, for-in,
// `in`, `delete`, a counter and a nested array value.

const obj: Record<string, number> = {}
const o2: { [k: string]: number } = {}
obj["a"] = 1; obj.b = 2; o2["x"] = 3
for (const [k, v] of Object.entries(obj)) console.log(k, v)
for (const k in o2) console.log(k, o2[k])
const counts: Record<string, number> = {}
for (const w of "a b a c b a".split(" ")) counts[w] = (counts[w] ?? 0) + 1
console.log(JSON.stringify(counts))
const groups: Record<string, string[]> = {}
for (const w of ["apple", "avocado", "banana"]) { const g = w[0]; if (!groups[g]) groups[g] = []; groups[g].push(w) }
console.log(JSON.stringify(groups), Object.keys(groups).length)
console.log(obj["zz"], "zz" in obj, delete obj.a, Object.keys(obj).join(","))
