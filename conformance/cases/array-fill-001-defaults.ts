// V3-18 m1.h.53 — Array.fill with optional start / end args
// per JS spec §22.1.3.6:
//   xs.fill(v)            = xs.fill(v, 0, len)
//   xs.fill(v, start)     = xs.fill(v, start, len)
//
// Pre-fix tora declared with 3 fixed params so 1 / 2 -arg calls
// hit the arity check.

let a = [0, 0, 0, 0, 0]
a.fill(7)
console.log(a)                       // [ 7, 7, 7, 7, 7 ]

let b = [0, 0, 0, 0, 0]
b.fill(7, 2)
console.log(b)                       // [ 0, 0, 7, 7, 7 ]

let c = [0, 0, 0, 0, 0]
c.fill(7, 1, 3)
console.log(c)                       // [ 0, 7, 7, 0, 0 ]

// 0-elem arr no-op.
let e: number[] = []
e.fill(99)
console.log(e.length)                // 0
