// V3-18 m2.g — runtime-key hasOwnProperty / propertyIsEnumerable
// on class instances. Pre-fix m2.d only handled literal-string
// keys (compile-time fold); runtime keys (`let key = "x";
// f.hasOwnProperty(key)`) defaulted to false. Now we emit an
// inline str_eq chain over the struct's known field names —
// each name interned as a literal Str (zero-alloc per compare).

class Foo {
  x: number
  y: string
  z: boolean
  constructor(x: number, y: string, z: boolean) {
    this.x = x; this.y = y; this.z = z
  }
}

let f = new Foo(5, "hi", true)

// Literal key (m2.d path).
console.log(f.hasOwnProperty("x"))      // true
console.log(f.hasOwnProperty("notAField"))  // false

// Runtime key (m2.g path).
let k1 = "x"
console.log(f.hasOwnProperty(k1))       // true
let k2 = "z"
console.log(f.hasOwnProperty(k2))       // true
let k3 = "missing"
console.log(f.hasOwnProperty(k3))       // false

// Substr key (split result).
let parts = "y,bar".split(",")
console.log(f.hasOwnProperty(parts[0])) // true (key "y")

// propertyIsEnumerable shares the same path.
console.log(f.propertyIsEnumerable(k1)) // true
console.log(f.propertyIsEnumerable(k3)) // false
