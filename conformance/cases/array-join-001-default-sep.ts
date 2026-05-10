// V3-18 m1.h.42 — Array<String|Substr>.join() with no separator
// argument defaults to "," per JS spec §22.1.3.13. Pre-fix tora
// declared join with 1 fixed param so `xs.join()` failed at the
// arity check.

let a = ["a", "b", "c"]
console.log(a.join())          // a,b,c
console.log(a.join("-"))       // a-b-c (no regression)
console.log(a.join(""))        // abc

// Substr (split result) — same path with default sep.
console.log("x.y.z".split(".").join())   // x,y,z

// Empty array still works.
let empty: string[] = []
console.log(empty.join())      // ""
console.log("done")
