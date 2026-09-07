// `o["k"]` IS `o.k` (§13.3.2), and the parser folds it so a literal
// key reaches the class IC, accessors and the static field layout.
// What the fold used to erase is the SPELLING — so a Map, a Set, a
// RegExp or a String got a nominal reject for a name its builtin
// table happens not to list, while the SAME key spelled dynamically
// answered undefined off the runtime probe. One key, two answers.
const m = new Map()
const q = new Set()
const re = /ab/
const s = "ab"

// a miss is a miss, whichever way it is spelled
let kd = "a"
kd += ""
console.log(m["a"], m[kd])
console.log(q["a"], q[kd])
console.log(re["a"], re[kd])
console.log(s["a"], s[kd])

// what the fold buys is untouched: known names still resolve
// statically, methods included
m["set"](1, 2)
console.log(m["size"], m["get"](1), m["has"](9))
console.log(s["length"], re["source"])

// a struct's literal keys, both spellings of them
const o = { a: 1 }
console.log(o["a"], o["zz"], o[0])

// a class instance keeps field, accessor and method lookup nominal
class Box {
  v = 3
  get g() {
    return this.v * 2
  }
  m() {
    return "m"
  }
}
const b = new Box()
console.log(b["v"], b["g"], b["m"](), b["zz"])

// the chain spelling is the same fold and answers the same way
const on: { a: number } | null = { a: 1 }
console.log(on?.["a"], on?.["zz"])
console.log(m?.["a"], m?.["size"])
