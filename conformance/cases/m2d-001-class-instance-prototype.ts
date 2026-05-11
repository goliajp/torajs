// V3-18 m2.d — class-instance Object.prototype methods. Per
// JS spec every object inherits these from Object.prototype:
//   .hasOwnProperty(k)         → true if k is a declared field
//                                  (compile-time layout lookup);
//                                  false for unknown keys.
//   .propertyIsEnumerable(k)   → same as hasOwnProperty for
//                                  instance fields (declared
//                                  fields are enumerable in the
//                                  subset).
//   .isPrototypeOf(x)          → false (no real prototype chain).
//   .valueOf()                 → identity (returns the instance).
//   .toString()                → "[object Object]" (subset stub
//                                  matching bun for non-overridden
//                                  toString).
//   typeof obj.X for X in the above → "function" (first-class
//                                       function ref).

class Foo {
  x: number
  y: string
  constructor(x: number, y: string) { this.x = x; this.y = y }
}

let f = new Foo(5, "hi")
console.log(f.hasOwnProperty("x"))           // true
console.log(f.hasOwnProperty("y"))           // true
console.log(f.hasOwnProperty("z"))           // false
console.log(f.propertyIsEnumerable("x"))     // true
console.log(f.propertyIsEnumerable("foo"))   // false
console.log(f.toString())                    // [object Object]

console.log(typeof f.toString)               // function
console.log(typeof f.hasOwnProperty)         // function
console.log(typeof f.valueOf)                // function

// valueOf identity (bun returns the same struct ref).
let g = f.valueOf()
console.log(g.x === 5)                       // true
console.log(g.y === "hi")                    // true
