// V3-18 m1.h.47 — Symbol.prototype.toString() and
// Symbol.prototype.description per JS spec §20.4.3.3 / §20.4.3.2.
// toString → "Symbol(<desc>)" / "Symbol()". description → the desc
// string passed to Symbol(), or null when Symbol() was called with
// no arg.
//
// Pre-fix tora rejected with "no member `.toString` on type Symbol"
// — runtime print helper had the format but it wasn't wired
// through the method-call surface.

let s1 = Symbol("hello")
console.log(s1.toString())     // Symbol(hello)
console.log(s1.description)     // hello

let s2 = Symbol("foo bar")
console.log(s2.toString())     // Symbol(foo bar)
console.log(s2.description)     // foo bar

// Symbol identity via toString shape (not by-value: each Symbol is unique).
let s3 = Symbol("dup")
let s4 = Symbol("dup")
console.log(s3.toString() === s4.toString())  // true
console.log(s3 === s4)                          // false (identity)

// Used in concat — toString called implicitly when forced.
console.log("got: " + s1.toString())
