// S236 — `String.{padStart,padEnd}(N, undefined)` per ES §22.1.3.{16,17}
// step 6.a: when fillString is undefined, set fillString to " ". The
// check.rs S236 widens the arg-1 type gate to String|Undefined, and
// ssa_lower_str's undef_space_at_arg1_pad substitutes the interned " "
// literal so the helper's (Str, I64, Str) ABI receives a concrete
// pointer — same shape as the V3-18 m1.h.45 1-arg default-fill path.
console.log("5".padStart(3, undefined));
console.log("5".padEnd(3, undefined));
console.log("hello".padStart(8, undefined));
console.log("hello".padEnd(8, undefined));
console.log("ab".padStart(2, undefined));
console.log("ab".padEnd(2, undefined));
