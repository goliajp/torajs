// P10.2-A3 — Promise.allSettled accepts inner T from {Number, String,
// Boolean} primitive set (was Number-only). Result struct value field
// tracks inner T monomorphically; runtime rc_inc's heap values so the
// settled struct co-owns the inner String ref.
//
// Mirrors async-017's await pattern (Promise<Array<Struct>>.then is
// out of scope for this sub-A — separate widening, queued as A4).

// String inner T — exercises heap value path (rc inc/dec on Str).
type SettledStr = { status: string, value: string }
let ss: Promise<string>[] = [
  Promise.resolve('hello'),
  Promise.resolve('world'),
  Promise.resolve(''),
]
let rs: SettledStr[] = await Promise.allSettled(ss)
console.log(rs.length)            // 3
console.log(rs[0].status)         // fulfilled
console.log(rs[0].value)          // hello
console.log(rs[1].status)         // fulfilled
console.log(rs[1].value)          // world
console.log(rs[2].status)         // fulfilled
console.log(rs[2].value)          // (empty)

// Boolean inner T — non-heap, value_is_heap=false path.
type SettledBool = { status: string, value: boolean }
let sb: Promise<boolean>[] = [Promise.resolve(true), Promise.resolve(false)]
let rb: SettledBool[] = await Promise.allSettled(sb)
console.log(rb.length)            // 2
console.log(rb[0].status)         // fulfilled
console.log(rb[0].value)          // true
console.log(rb[1].status)         // fulfilled
console.log(rb[1].value)          // false

// Number inner T — regression guard for the prior MVP path.
type SettledNum = { status: string, value: number }
let sn: Promise<number>[] = [Promise.resolve(100), Promise.resolve(200)]
let rn: SettledNum[] = await Promise.allSettled(sn)
console.log(rn.length)            // 2
console.log(rn[0].value)          // 100
console.log(rn[1].value)          // 200
