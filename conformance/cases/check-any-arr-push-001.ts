// P0.10 — Array<Any>.push(<concrete>) routes through arr_push_any
// with proper boxing. Pre-fix tora called the regular arr_push
// intrinsic which uses 8-byte slots and corrupted the
// Array<Any>'s 16-byte tagged-slot layout (segfault on first
// push). This is the .push counterpart to the
// __torajs_arr_set_any indexed-write substrate shipped earlier;
// the same boxing scheme applies.
//
// ssa_lower.rs Array.push Ident-receiver path — when elem_ty is
// Type::Any, pack the RHS into a (tag, value) pair and dispatch
// to arr_push_any. Tags: I64=2, F64=3 (bitcast), Bool=1 (zext),
// heap-typed=4 (with rc_inc so the slot owns a balanced ref),
// Ptr disambiguates ConstPtrNull → ANY_NULL=0 from generic Ptr
// → ANY_HEAP=4. When RHS is already Type::Any, extract its
// (tag, value) from the box at offsets 16/24. Skips the
// regular arr_push (8-byte stride) entirely.
//
// Combined with the bare-`[]`-defaults-to-Array<Any> work, this
// makes the canonical `let xs = []; xs.push(...)` idiom work
// end-to-end without any annotation.

let xs: any[] = []
xs.push(1)
xs.push("hello")
xs.push(true)
xs.push(null)
console.log(xs.length)                       // 4
console.log(xs[0])                           // 1
console.log(xs[1])                           // hello
console.log(xs[2])                           // true
console.log(xs[3])                           // null

// Push more after pre-allocated initial slots — exercises the
// realloc path (initial cap 2, grows to 4 then 8 etc).
let ys: any[] = [10]
ys.push(20)
ys.push(30)
ys.push(40)
ys.push(50)
ys.push(60)
console.log(ys.length)                       // 6
console.log(ys[5])                           // 60

// Bare empty `[]` (no annotation) — defaults to Array<Any>.
// Combines with .push to give the canonical untyped-JS shape.
let zs = []
zs.push(1)
zs.push("two")
zs.push(3)
console.log(zs.length)                       // 3
console.log(zs[1])                           // two
