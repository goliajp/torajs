// V3-18 m1.h.50 — String.indexOf / lastIndexOf accept an optional
// 2nd `fromIndex` arg per JS spec §21.1.3.7 / §21.1.3.10.
// Negative fromIndex clamps to 0; large fromIndex clamps to len.
// Pre-fix tora declared with 1 fixed param so 2-arg calls hit
// the arity check.

let s = "hello world"
console.log(s.indexOf("o"))            // 4 (no regression)
console.log(s.indexOf("o", 5))         // 7
console.log(s.indexOf("o", 8))         // -1
console.log(s.indexOf("o", -100))      // 4 (negative clamps to 0)
console.log(s.indexOf("xyz"))          // -1

console.log(s.lastIndexOf("o"))        // 7
console.log(s.lastIndexOf("o", 5))     // 4
console.log(s.lastIndexOf("o", 0))     // -1
console.log(s.lastIndexOf("o", -1))    // -1 (negative clamps to 0, no o at 0)

// Empty needle still works.
console.log(s.indexOf(""))             // 0
console.log(s.indexOf("", 3))          // 3 — clamped to in-range start
