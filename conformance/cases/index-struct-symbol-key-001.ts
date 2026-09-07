// A STRUCT receiver's symbol key. §7.1.19 step 2 hands a symbol on
// uncoerced, so it reaches the property probe as its own cell — the
// arm the any-receiver lane next door has had since it was written,
// and the struct lane did not, so this type-checked (the gate admits
// Struct x Symbol) and then died coercing a Symbol to i64.
const sk = Symbol("s")
const o = { a: 1 }
console.log(o[sk])
console.log(o[Symbol.iterator])

// a symbol key stored dynamically reads back under the same cell,
// and does not collide with the string spelling of its description
const d: any = {}
d[sk] = 7
d["s"] = 8
console.log(d[sk], d["s"])

// a well-known symbol on a struct that really carries one
const it = {
  [Symbol.iterator]() {
    return [1, 2][Symbol.iterator]()
  },
}
console.log(typeof it[Symbol.iterator])
console.log([...it])

// a class instance answers the same way through its shape
class Box {
  v = 5
}
const b = new Box()
console.log(b[sk])
