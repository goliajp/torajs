// Map / Set instances are ordinary objects off their entry table
// (§24.1.6 / §24.2.6): the table is internal state, so a name key is
// an ordinary own property that must land somewhere and read back.
const m: any = new Map<string, number>()
m.set("k", 1)
m.zz = 7
console.log(m.zz, m.size, m.get("k"))
console.log("zz" in m, Object.keys(m), Object.getOwnPropertyNames(m))
console.log(Object.values(m), JSON.stringify(Object.entries(m)))
delete m.zz
console.log(m.zz, "zz" in m, m.size)

const s: any = new Set<number>([1, 2])
s.tag = "here"
console.log(s.tag, s.size, s.has(2))
const seen: string[] = []
for (const k in s) seen.push(k)
console.log(seen)
console.log(JSON.stringify({ ...s }))
