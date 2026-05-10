// V3-18 m1.h.51 — String.startsWith / endsWith / includes
// accept an optional 2nd `position` arg per JS spec
// §21.1.3.20 / §21.1.3.6 / §21.1.3.7. Pre-fix tora declared
// these with 1 fixed param so 2-arg calls hit the arity check.

let s = "hello world"
console.log(s.startsWith("hello"))         // true (no regression)
console.log(s.startsWith("ello", 1))       // true
console.log(s.startsWith("hello", 5))      // false
console.log(s.startsWith("world", 6))      // true
console.log(s.startsWith("hello", -100))   // true (negative clamps to 0)

console.log(s.endsWith("world"))           // true
console.log(s.endsWith("hello"))           // false
console.log(s.endsWith("hell", 4))         // true (treats slice 0..4 = "hell")
console.log(s.endsWith("hello", 5))        // true

console.log(s.includes("world"))           // true
console.log(s.includes("world", 100))      // false
console.log(s.includes("world", 6))        // true
console.log(s.includes("hello", 1))        // false
