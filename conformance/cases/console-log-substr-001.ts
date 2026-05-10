// V3-18 m1.h.34 — `console.log(substr)` for a single Substr
// operand (the result of split[i] / slice / substring / etc).
// Pre-fix Substr fell through to the catch-all print_i64 in
// console_print_target, so a Substr arg printed the pointer-as-
// integer (or nothing visible for an empty length-0 case).
//
// Fix: dedicated __torajs_substr_print runtime helper that walks
// the {hdr@0, len@8, parent@16, offset@24} layout. console_print_target
// dispatches it for Type::Substr.

console.log("a-b-c".split("-")[0])         // a
console.log("a-b-c".split("-")[1])         // b
console.log("hello".slice(1, 3))           // el
console.log("abc".substring(0, 2))         // ab

let parts = "x,y,z".split(",")
console.log(parts[0])                       // x
console.log(parts[1])                       // y
console.log(parts[2])                       // z

// Empty substr (slice with start==end).
console.log("xyz".slice(1, 1))              // ""
console.log("after")
