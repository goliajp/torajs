// S129-3 — Array<Any>.flat() depth-1 regression guard. Per ES
// §23.1.3.13 flat returns a new array with array-typed elements
// (own enumerable, recursive up to depth) spread one level in.
// Pre-fix tora bailed at check.rs with "Array.flat requires
// Array<Array<T>>, receiver is Array<Any>" — typed-Array<Any>
// (NaN-box AnyValue slots) needs an Any-aware runtime path
// instead of the typed memcpy fastpath.
//
// Two-tier fix mirror of S129-1 ternary / S128-3 fill:
// check.rs Any-elem escape (result stays Array<Any>); runtime
// `__torajs_arr_flat_any` decodes each outer slot's tag,
// extends inner Array<Any> via arr_extend_any (rc-safe), and
// passes non-array slots through as a single push_any.

function getNested(): any[] {
  return [[1, 2], [3, [4, 5]], 6];
}
const xs: any[] = getNested();

// depth=1 default — outer Array<Any>, inner Array<Any> unwrap
// one level; nested [4, 5] stays as an inner Array<Any> in the
// output, scalar 6 passes through unchanged.
const a = xs.flat();
console.log(a.length);    // 5 — [1,2,3,[4,5],6]
console.log(a[0]);         // 1
console.log(a[1]);         // 2
console.log(a[2]);         // 3
console.log(a[4]);         // 6

// depth=2 — unrolls to two flat-1 calls; the nested [4,5]
// unwraps as well: [1,2,3,4,5,6].
const b = xs.flat(2);
console.log(b.length);    // 6
console.log(b[0]);         // 1
console.log(b[2]);         // 3
console.log(b[3]);         // 4
console.log(b[4]);         // 5
console.log(b[5]);         // 6

// regression guard for typed Array<Array<T>> path — pre-fix
// behavior unchanged.
const typed: number[][] = [[10, 20], [30, 40]];
const t = typed.flat();
console.log(t.length);    // 4
console.log(t[0]);         // 10
console.log(t[3]);         // 40

// non-array elements at outer position pass through unchanged
// (string, boolean, number all preserved).
function makeMixed(): any[] {
  return ["a", [10, 20], true, [30], 99];
}
const m = makeMixed().flat();
console.log(m.length);    // 6 — ["a", 10, 20, true, 30, 99]
console.log(m[0]);         // a
console.log(m[1]);         // 10
console.log(m[3]);         // true
console.log(m[5]);         // 99
