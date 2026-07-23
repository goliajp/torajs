// `Array<T>.flatMap(cb)` scalar callback return per ES §23.1.3.11
// step 8.d — a non-Array cb result acts like `[U]` (single value
// pushed into the accumulator). Sister wedge to
// check_type_of_call_arr_flat_map_hetero (Array-return cb).

// Number scalar cb — cb result acts like `[n * 10]`.
const a1 = [1, 2, 3];
const b1 = a1.flatMap(n => n * 10);
console.log(JSON.stringify(b1));
// [10,20,30]

// String scalar cb — cb result acts like `[String(n)]`.
const a2 = [1, 2, 3];
const b2 = a2.flatMap(n => "x" + n);
console.log(JSON.stringify(b2));
// ["x1","x2","x3"]

// Boolean scalar cb — cb result acts like `[n % 2 === 0]`.
const a3 = [1, 2, 3, 4];
const b3 = a3.flatMap(n => n % 2 === 0);
console.log(JSON.stringify(b3));
// [false,true,false,true]

// Any scalar cb (via annotation) — cb result acts like `[k]`.
const a4 = [1, 2, 3];
const b4 = a4.flatMap((n): any => (n % 2 === 0 ? ("even" as any) : (n as any)));
console.log(JSON.stringify(b4));
// [1,"even",3]

// Empty src — empty result.
const a5: number[] = [];
const b5 = a5.flatMap(n => n + 1);
console.log(JSON.stringify(b5));
// []

// Single-element src.
const a6 = [42];
const b6 = a6.flatMap(n => n * 2);
console.log(JSON.stringify(b6));
// [84]

// Chained flat-Map: transform then reduce.
const a7 = [1, 2, 3];
const b7 = a7.flatMap(n => n + 100).reduce((s, n) => s + n, 0);
console.log(b7);
// 306

// String array with scalar String cb.
const a8 = ["a", "b", "c"];
const b8 = a8.flatMap(s => s + s);
console.log(JSON.stringify(b8));
// ["aa","bb","cc"]
