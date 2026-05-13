// V3-18 wedge — Array.prototype.toString / toLocaleString.
// Per JS spec §22.1.3.30, equivalent to `arr.join(",")` —
// elements ToString'd inline, comma-separated. Pre-fix tora's
// check.rs rejected with 'no member .toString on type
// Array(Number)' (and similar for Array(String) /
// Array(Boolean)) since the arm wasn't wired.
//
// Implementation: check.rs adds an Array.toString /
// toLocaleString arm gated on element type ∈ { String,
// Number, Boolean } — the same constraint as Array.join. ssa_lower's
// Type::Arr method dispatch routes both names to the
// existing __torajs_arr_join_* intrinsic with sep=",".

let arr: number[] = [1, 2, 3]
console.log(arr.toString())            // 1,2,3
console.log(arr.toLocaleString())      // 1,2,3 (POSIX-ish — no real locale)

let strs: string[] = ["a", "b", "c"]
console.log(strs.toString())           // a,b,c

let bools: boolean[] = [true, false, true]
console.log(bools.toString())          // true,false,true

// Empty array → empty string.
let empty: number[] = []
console.log("[" + empty.toString() + "]")  // []

// Single-element array → no separator.
let one: string[] = ["solo"]
console.log(one.toString())            // solo

// In an expression — implicit ToString via the explicit call.
let xs: number[] = [10, 20, 30]
console.log("xs is " + xs.toString())  // xs is 10,20,30
