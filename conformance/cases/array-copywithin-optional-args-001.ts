// V3-18 wedge — Array.copyWithin accepts 1-3 args per JS
// spec §22.1.3.3:
//   xs.copyWithin(target)            ≡ (target, 0, len)
//   xs.copyWithin(target, start)     ≡ (target, start, len)
//   xs.copyWithin(target, start, end)≡ explicit 3-arg form
// Pre-fix tora declared `copyWithin` with a fixed 3-arg
// signature (`Function([Number, Number, Number], Array<T>)`)
// so the 1-arg and 2-arg shapes failed at the unified arity
// check with 'expected 3 argument(s), got 1' / 'got 2'.
//
// Implementation: in check.rs add a Member-call special-case
// before the generic arity check that accepts 1-3 args of
// type Number and returns Array<T>; in ssa_lower::copyWithin
// the args.len() guard relaxes from `== 3` to `1..=3`, with
// missing positions defaulted to start=0 and end=recv.length
// (loaded from ARR_LEN_OFF). The existing refcount-aware path
// for non-Copy element types still runs unchanged once the
// canonical 3-arg form is materialized.

// 3-arg form — explicit (was already accepted pre-fix).
let xs1: number[] = [10, 20, 30, 40, 50]
xs1.copyWithin(0, 2, 4)
console.log(xs1)                       // [ 30, 40, 30, 40, 50 ]

// 2-arg form — end defaults to len.
let xs2: number[] = [1, 2, 3, 4, 5]
xs2.copyWithin(0, 2)
console.log(xs2)                       // [ 3, 4, 5, 4, 5 ]

// 1-arg form — start defaults to 0, end to len.
// In practice this only does work when target > 0; with
// target=0 the call is a no-op (src and dst slices coincide).
let xs3: number[] = [1, 2, 3, 4, 5]
xs3.copyWithin(0)
console.log(xs3)                       // [ 1, 2, 3, 4, 5 ]

let xs4: number[] = [1, 2, 3, 4, 5]
xs4.copyWithin(2)
console.log(xs4)                       // [ 1, 2, 1, 2, 3 ]

// String arrays — exercises the refcount-aware element path.
let strs1: string[] = ["a", "b", "c", "d", "e"]
strs1.copyWithin(0, 3)
console.log(strs1)                     // [ "d", "e", "c", "d", "e" ]

let strs2: string[] = ["x", "y", "z", "w"]
strs2.copyWithin(1, 2)
console.log(strs2)                     // [ "x", "z", "w", "w" ]

// Returns the same array (for chaining).
let xs5: number[] = [1, 2, 3, 4]
let r = xs5.copyWithin(0, 2)
console.log(r === xs5)                 // true
console.log(r)                         // [ 3, 4, 3, 4 ]
