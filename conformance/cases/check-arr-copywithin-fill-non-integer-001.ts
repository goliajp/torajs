// ES §23.1.3.4 (Array.copyWithin) and §23.1.3.7 (Array.fill) —
// `target`, `start`, `end` all route through ToIntegerOrInfinity
// before normalisation. Pre-fix tora panicked the codegen backend
// with "GPR materialization can't hold an f64" because the shared
// `relative_to_len` helper consumed the raw f64 literal directly
// in its ICmp/Add/Store chain. Fix routes through coerce_to_i64
// at the helper entry, treating every callsite uniformly. Same
// root cause family as Array.at non-integer (commit 41ceb22e) and
// split limit (commit cd9f6af3).

// copyWithin — 0/1/2/3-arg + fractional / negative-fractional
console.log([1,2,3,4,5].copyWithin(0.5, 2.5, 4.5));    // [3,4,3,4,5]
console.log([1,2,3,4,5].copyWithin(-1.5, 1.5));         // [1,2,3,4,2]   (target=-1→4, start=1)
console.log([1,2,3,4,5].copyWithin(0, -2.5, -1.5));    // [4,2,3,4,5]   (start=-2→3, end=-1→4)

// fill — fractional start / end
console.log([1,2,3,4,5].fill(7, 1.5, 3.5));    // [1,7,7,4,5]
console.log([1,2,3,4,5].fill(0, -3.5, -1.5));  // [1,2,0,0,5]   (start=-3→2, end=-1→4)
console.log([1,2,3,4,5].fill(0, NaN, 3));       // [0,0,0,4,5]   (NaN→0)
console.log([1,2,3,4,5].fill(0, 0, NaN));       // [1,2,3,4,5]   (end NaN→0, range empty)

// Integer args unchanged (sanity).
console.log([1,2,3,4,5].copyWithin(0, 3));   // [4,5,3,4,5]
console.log([1,2,3,4,5].fill(7));             // [7,7,7,7,7]
