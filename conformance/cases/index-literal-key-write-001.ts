// The write half of the same fold. `m["a"] = 9` was a compile error
// while `m[kd] = 9` — the same key, built at run time — landed in the
// cell's own bag, as did the `any` spelling of both. §10.1.9.2
// OrdinarySet does not care which way the key was written down.
const m = new Map()
let kd = "b"
kd += ""
m["a"] = 9
m[kd] = 8
console.log(m["a"], m["b"], m[kd])

// the bag is the object's own, and it is not the Map's entries
m.set("a", 1)
console.log(m["a"], m.get("a"), m.size)

const q = new Set()
q["a"] = 7
console.log(q["a"], q.size)

const re = /ab/
re["a"] = 6
console.log(re["a"], re.source, re.test("zab"))

const d = new Date(0)
d["a"] = 5
console.log(d["a"], d.getTime())

const mi = new Map([[1, 2]]).entries()
mi["a"] = 4
console.log(mi["a"], mi.next().value[1])

// an own property shadows the prototype's, and reads see the own one
const m2 = new Map()
m2["zz"] = 1
console.log(m2["zz"], typeof m2["get"])

// the assignment expression's value is its rhs
const r = (m["c"] = 3)
console.log(r, m["c"])

// refcounted values survive the round trip
const m3 = new Map()
m3["arr"] = [1, 2, 3]
m3["s"] = "hello"
console.log(m3["arr"][1], m3["arr"].length, m3["s"].length)
