// `const a: T[] = ...` at top level registers `a` as a K.3 const-
// global (via the K.6 globals pass), and the ssa_lower receiver
// resolution for mutator methods looks the recv name up in BOTH
// `self.locals` and `self.globals`. The push K.8 path at
// ssa_lower.rs:19708 already implements both; try_arr_pop /
// try_arr_shift in ssa_lower_arr_mutators.rs also do;
// try_arr_unshift previously only checked `self.locals`, so an
// explicit-annotation top-level `const a: number[] = ...` panicked
// "unsupported member call shape: unshift" while the inferred
// `const a = [...]` form (which routes to the locals path) worked.
// try_arr_unshift now mirrors push K.8: load via GlobalRef, unshift,
// store back into the same global slot.

const ns: number[] = [1, 2, 3];
console.log(ns.unshift(0));
console.log(ns);
console.log(ns.unshift(-1));
console.log(ns);

const ss: string[] = ["b", "c"];
console.log(ss.unshift("a"));
console.log(ss);

const bs: boolean[] = [true];
console.log(bs.unshift(false));
console.log(bs);

// Inferred-type const (locals path) regression check.
const inferred = [1, 2];
console.log(inferred.unshift(0));
console.log(inferred);

// Empty typed-array unshift.
const empty: number[] = [];
console.log(empty.unshift(42));
console.log(empty);
