// V3-18 wedge — Array.concat accepts any number of array
// args per JS spec §22.1.3.2:
//   xs.concat()            ≡ fresh shallow copy of xs
//   xs.concat(a)           ≡ xs + a (already supported)
//   xs.concat(a, b, ..., z)≡ xs + a + b + ... + z
// Pre-fix tora declared `concat` with a fixed 1-arg signature
// (`Function([Array<T>], Array<T>)`) so 0-arg and multi-arg
// calls failed at the unified arity check with 'expected 1
// argument(s), got N'.
//
// Implementation:
// * check.rs adds a Member-call special-case before the
//   generic arity check that accepts 0..N array args (all of
//   the same element type as the receiver) on Array<T>
//   receivers, returning Array<T>.
// * ssa_lower::concat now folds N-arg calls left-to-right
//   through the existing 1-arg `arr_concat` intrinsic. The
//   0-arg form lowers to `arr_slice(recv, 0, len)`, which is
//   the canonical shallow-copy path; refcount-inc fires once
//   on the final array's full length so non-Copy elements
//   stay drop-balanced regardless of the fold depth.
//
// Subset constraint kept (separate substrate item): every
// additional arg must be an Array<T> of the same element
// type. Scalar args (the spec's "values are added") would
// require the heterogeneous-element substrate.

// Multi-arg form on numeric arrays.
let xs: number[] = [1, 2]
let ys: number[] = [3, 4]
let zs: number[] = [5, 6]
console.log(xs.concat(ys, zs))                 // [ 1, 2, 3, 4, 5, 6 ]
console.log(xs)                                // [ 1, 2 ] — unchanged
console.log(ys)                                // [ 3, 4 ]
console.log(zs)                                // [ 5, 6 ]

// Multi-arg on string arrays (exercises the refcount-aware
// element path).
let a: string[] = ["alpha", "beta"]
let b: string[] = ["gamma"]
let c: string[] = ["delta", "epsilon"]
console.log(a.concat(b, c))                    // [alpha, beta, gamma, delta, epsilon]
console.log(a.concat(b, c).length)             // 5

// 0-arg form — fresh shallow copy.
let v: number[] = [10, 20, 30]
let copy = v.concat()
console.log(copy)                              // [ 10, 20, 30 ]
console.log(copy.length)                       // 3
copy.push(99)
console.log(v)                                 // [ 10, 20, 30 ] — original unaffected
console.log(copy)                              // [ 10, 20, 30, 99 ]

// 1-arg form still works (backwards-compat).
console.log([1].concat([2, 3]))                // [ 1, 2, 3 ]

// 3 + 0 = 3 (an empty trailing tail is harmless).
let empty: number[] = []
console.log([1, 2].concat(empty, [3, 4]))      // [ 1, 2, 3, 4 ]
