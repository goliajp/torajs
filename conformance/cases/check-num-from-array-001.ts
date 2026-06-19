// `Number(Array<T>)` pre-fix surfaced as the V3-18 m1.h.9 wedge
// "Number(Array(T)) coercion not yet supported" — check.rs rejected
// every non-{Number/Bool/Null/Undefined/String/Any} arg. Per ES
// §7.1.4 ToNumber(Array) is ToNumber(ToString(Array)) = ToNumber(
// arr.join(",")), and `String(Array<T>)` already lowers through
// the arr_join_* dispatch table. Fix accepts `Type::Array(_)` at
// check.rs and adds a `Type::Arr` arm in ssa_lower's Number ctor
// that joins via the same elem-typed intrinsic the String(Arr)
// path uses, then forwards the join result to `str_to_number`
// (dropping the intermediate Str). Bun parity: empty array → 0,
// single-elem numeric → that number, multi-elem → NaN.

console.log(Number([]));
console.log(Number([1]));
console.log(Number([42]));
console.log(Number([1, 2]));
console.log(Number([1.5]));
console.log(Number(['3']));
console.log(Number(['a']));
console.log(Number([true]));

// `Number([null])` is L3b — the typed `Array<Null>` arr_join_*
// dispatch SIGSEGVs on the null elem path independently of the
// Number ctor change; the join-→str-to-number pipeline is correct
// once a join intrinsic for null-elem arrays lands.

console.log(Number.isNaN(Number([1, 2])));
console.log(Number.isNaN(Number(['x'])));
console.log(Number([0]) === 0);
console.log(Number([]) === 0);
console.log(typeof Number([1]));
