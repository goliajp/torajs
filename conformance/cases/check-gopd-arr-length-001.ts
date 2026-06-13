// RFC C5a — Object.getOwnPropertyDescriptor(arr, "length") returns a
// real data descriptor per spec ES §10.4.2.4: `{value: arr.length,
// writable: true, enumerable: false, configurable: false}`. Pre-fix
// tora's gOPD walked dynobj entries only, so Array.length reported
// undefined.
//
// Arr<Any> coverage was added once the pre-existing drop-time crash
// was fixed (top-level `const ys: any[] = [...]` was emitted with
// `__torajs_arr_alloc` + raw i64 stores instead of `arr_alloc_any`
// + NaN-boxed push, so `arr_drop_any` decoded `10` as an Any tag and
// dereffed an invalid heap ptr in main's exit drop walker).

const xs = [1, 2, 3];
const d = Object.getOwnPropertyDescriptor(xs, "length");
console.log(d.value); // 3
console.log(d.writable); // true
console.log(d.enumerable); // false
console.log(d.configurable); // false

// 5-element typed array — exercises a larger len value through the
// helper's NaN-box pair (still fits in the I64 imm fast path).
const big = [10, 20, 30, 40, 50];
const d2 = Object.getOwnPropertyDescriptor(big, "length");
console.log(d2.value); // 5
console.log(d2.writable); // true

// String-element array — `Arr<Str>` still hits the typed `Type::Arr(_)`
// fast path; len read is the same offset.
const names = ["alice", "bob"];
const d3 = Object.getOwnPropertyDescriptor(names, "length");
console.log(d3.value); // 2
console.log(d3.configurable); // false

// `any[]` — exercise the Arr<Any> tagged-slot layout. Pre-fix this
// triggered SIGSEGV during main's exit drop walker.
const ys: any[] = [10, 20, 30];
const d4 = Object.getOwnPropertyDescriptor(ys, "length");
console.log(d4.value); // 3
console.log(d4.writable); // true
console.log(d4.enumerable); // false
console.log(d4.configurable); // false
