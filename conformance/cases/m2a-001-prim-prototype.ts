// V3-18 m2.a — Object.prototype methods on primitives via JS
// auto-boxing rules. Subset semantics:
//   .valueOf()              → identity (returns the primitive)
//   .hasOwnProperty(k)       → false  (primitives have no own
//                              enumerable properties in the subset;
//                              real spec returns true for indexed
//                              string slots — deferred to dynamic-
//                              substrate phase T-27)
//   .propertyIsEnumerable(k) → false  (same)
//
// Object.getPrototypeOf returns null (no real prototype chain in
// tora's nominal class system). Most test262 cases that exercise
// these are checking the call doesn't throw + the result has a
// known shape; the stub returns null which works as the bottom
// case.

let n = 5
console.log(n.valueOf())                      // 5
console.log(n.hasOwnProperty("foo"))          // false
console.log(n.propertyIsEnumerable("foo"))    // false

let s = "hello"
console.log(s.valueOf())                      // hello
console.log(s.hasOwnProperty("notAKey"))      // false (subset)

let b = true
console.log(b.valueOf())                      // true
console.log(b.hasOwnProperty("a"))            // false

let bi = 100n
console.log(bi.valueOf().toString())          // 100
console.log(bi.hasOwnProperty("a"))           // false

// Object.getPrototypeOf doesn't throw — called for side-effect-
// gated cases in test262. Returns null in tora's subset (no real
// prototype chain); bun returns the actual prototype object.
// We only test the call-doesn't-throw shape.
let arr = [1, 2, 3]
let proto = Object.getPrototypeOf(arr)
console.log(arr.length)                       // 3 (proto call doesn't disturb arr)
