// V3-18 m1.h.45 — String.padStart / padEnd with no fill arg
// defaults to " " (single space) per JS spec §21.1.3.16. Pre-fix
// tora declared the methods with 2 fixed params so 1-arg calls
// failed at the arity check.

console.log("5".padStart(3))      // "  5"
console.log("5".padEnd(3))        // "5  "
console.log("5".padStart(3, "0")) // "005" (no regression)
console.log("5".padEnd(3, "."))   // "5.." (no regression)

console.log("hello".padStart(8))   // "   hello"
console.log("hello".padEnd(8))     // "hello   "

// Length <= original returns the original.
console.log("hello".padStart(3))   // "hello"
console.log("hello".padEnd(0))     // "hello"
