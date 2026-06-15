// S127-5 — Set.add / Map.set return self per spec §24.2.3.1 / §23.1.3.9
// to enable chained `s.add(v1).add(v2)` / `m.set(k,v).set(k2,v2)` idiom.
// Pre-fix typecheck declared both as returning Void so the chain pattern
// rejected with "no member `.add` on type Void". ssa-lower emit now
// captures receiver before the runtime call, emit_rc_inc's it so the
// returned chain value owns its own ref, and returns it.
const s = new Set<number>()
s.add(1).add(2).add(3).add(2)
console.log("set.size =", s.size)
console.log("has 1 =", s.has(1))
console.log("has 2 =", s.has(2))
console.log("has 3 =", s.has(3))
console.log("has 99 =", s.has(99))

const m = new Map<string, number>()
m.set("a", 1).set("b", 2).set("c", 3).set("a", 99)
console.log("map.size =", m.size)
console.log("a =", m.get("a"))
console.log("b =", m.get("b"))
console.log("c =", m.get("c"))

// chain return value used as binding
const t = new Set<string>()
const u = t.add("x").add("y")
console.log("u.size =", u.size)
console.log("t.size =", t.size)
// Both bindings point at the same Set; modification through one shows
// in the other.
u.add("z")
console.log("t.size after u.add z =", t.size)
console.log("u.size after u.add z =", u.size)
