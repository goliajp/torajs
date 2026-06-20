// S232 — `String.slice(undefined [, ...])` / `String.substring(undefined [, ...])`
// per ES §22.1.3.20 / §22.1.3.22 step 3-5:
//   ToIntegerOrInfinity(undefined) = 0 for start
//   end===undefined uses the omitted default (length)
// The check.rs S232 carve-out widens the 0/1/2-arg dispatch arms;
// ssa_lower_str's slice_subs loop substitutes ConstI64(0) for undef
// start and the lazily-loaded recv.length for undef end so neither
// helper's (Str, I64, I64) ABI sees an Undefined sentinel.
console.log("hello".slice(undefined));
console.log("hello".slice(1, undefined));
console.log("hello".slice(undefined, 3));
console.log("hello".slice(undefined, undefined));
console.log("hello".substring(undefined));
console.log("hello".substring(1, undefined));
console.log("hello".substring(undefined, 3));
console.log("hello".substring(undefined, undefined));
