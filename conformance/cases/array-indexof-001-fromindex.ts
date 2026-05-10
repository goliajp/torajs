// V3-18 m1.h.49 — Array.indexOf / lastIndexOf / includes accept
// an optional 2nd `fromIndex` arg per JS spec §22.1.3.13 /
// §22.1.3.16. Negative fromIndex counts from end (clamped to 0).
// Pre-fix tora declared the methods with 1 fixed param so 2-arg
// calls hit the arity check.

let a = [10, 20, 30, 20, 40]
console.log(a.indexOf(20))           // 1 (no regression)
console.log(a.indexOf(20, 2))        // 3 (skip the first hit)
console.log(a.indexOf(20, 4))        // -1
console.log(a.indexOf(20, -3))       // 3 (negative — from end)
console.log(a.indexOf(99))           // -1

console.log(a.lastIndexOf(20))       // 3 (default starts at end)
console.log(a.includes(20))           // true
console.log(a.includes(20, 4))        // false
console.log(a.includes(99))           // false

// String[] same path.
let s = ["a", "b", "a", "c"]
console.log(s.indexOf("a"))          // 0
console.log(s.indexOf("a", 1))       // 2
console.log(s.includes("c", 0))      // true
console.log(s.includes("c", 3))      // true
console.log(s.includes("c", 4))      // false
