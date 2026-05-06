// T-10.c (v0.4.0) — heterogeneous Array literal with `any[]` slot.
// `[1, 'a', true]` — three different literal kinds — routes through
// the new tagged-slot Array<Any> codegen path. T-10.c covers alloc +
// per-element tagged push + length read; xs[i] indexed read /
// console.log(xs[i]) Any-dispatch / for-of-Any iteration land with
// T-10.d, so this fixture only exercises length and the alloc/drop
// code path.
let xs: any[] = [1, 'hello', true]
console.log(xs.length)

let ys: any[] = []
console.log(ys.length)

// Larger heterogeneous literal — exercises the 2x grow path of
// __torajs_arr_push_any (initial cap is 4; this needs 5 pushes).
let many: any[] = [1, 'a', true, 2, 'b']
console.log(many.length)
